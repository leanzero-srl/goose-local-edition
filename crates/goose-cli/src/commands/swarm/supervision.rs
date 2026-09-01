//! SUPERVISION LANES (the last r6-blocking item; Mihai's third ask, 2026-08-30: *"we need the
//! judge generations to be captured in our window same as everything else"*). Every supervision
//! model call carries an activity key minted HERE, so the machinery that already captures worker
//! calls end to end — the digest json, the append-only `<key>.log`/`<key>.think.log` transcripts,
//! the forming sidecar armed by the `run_agent_in` hoist — captures the calls that decide
//! steering too. r5 measured the keyless state: 43 judge looks only attributable by event, and one
//! 43m52s replanner call with no lane, no digest, no think.log — the desktop could only say
//! "supervising", and the panel mislabeled it.
//!
//! Third sibling module under the incremental-split law
//! (development_gates::swarm_rs_line_count_only_decreases). The forming-sidecar cluster below
//! moved here verbatim from swarm.rs — it is the lane-capture machinery these keys arm; behavior
//! unchanged, each item keeps its own WHY.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use super::EventSink;
use goose_swarm::{JudgeOutcome, Verdict};

/// The rolling judge lane for one supervised task: ONE lane per task, never per look — 31+ looks
/// on one task would mint 31 lanes and drown the board. Each look reseeds the digest (its
/// `attempt` field carries the look number; the prior look's answer folds into `superseded`) and
/// APPENDS to the durable logs, which is exactly the cumulative story the inspector renders.
pub(super) fn judge_lane_key(task_id: &str) -> String {
    format!("judge-{task_id}")
}

/// The dynamic replanner's lane, one per replan round (`ReplanContext.round`, 0-based).
pub(super) fn replan_lane_key(round: u32) -> String {
    format!("replan-r{round}")
}

/// A completion-time pre-review's lane, one per reviewed task.
pub(super) fn prereview_lane_key(task_id: &str) -> String {
    format!("prereview-{task_id}")
}

/// The sink-tail dimension review's lane — keyed by review dimension (it reviews the whole tree,
/// not one task), matching the `tail_review` events and the `.swarm/prereview/tail-review-<dim>`
/// findings file it already writes.
pub(super) fn tail_review_lane_key(dim_id: &str) -> String {
    format!("tail-review-{dim_id}")
}

/// The SCHEDULER-side judge's semantic review (`Judge::judge`), one lane per reviewed task.
/// DELIBERATELY not `judge-<task>` (surgeon #10's warning): the omni judge already owns that key
/// for the same task, and two writers on one digest would interleave two different reviews into
/// one rolling story. Distinct class, same task suffix, so the panel can still group both onto
/// the task.
pub(super) fn schedjudge_lane_key(task_id: &str) -> String {
    format!("schedjudge-{task_id}")
}

/// One sink-review verification (`verify_finding`), keyed by the finding's index in its drained
/// batch — the fan runs these CONCURRENTLY over the fleet, so a shared key would have parallel
/// lanes fighting over one digest file. Digit-suffixed exactly (like `replan-r<n>`) so a
/// model-chosen task id such as `verify-endpoints` stays a worker lane.
pub(super) fn verify_lane_key(idx: usize) -> String {
    format!("verify-{idx}")
}

/// The mid-run operator-question answerer's lane (`answer_user_question`). One lane for the run:
/// questions are answered one at a time (the in-flight set serializes them) and each new answer
/// folds the prior into `superseded`, which is the cumulative story the inspector renders.
pub(super) const ASK_ANSWER_LANE: &str = "ask-answer";

/// The pillars distillation's lane (`distill_pillars`) — one planner call per run, at plan time.
pub(super) const PILLARS_LANE: &str = "pillars";

/// The stack-skill reflection's lane (`reflect_on_success`) — one call per successful run, at
/// the very end. Exact-match keys (`pillars`, `reflect`, `ask-answer`) carry the same accepted
/// hazard as the prefix classes below: a model-chosen task id could collide; live plans name
/// tasks after modules and none of these three is a module name any measured plan has produced.
pub(super) const REFLECT_LANE: &str = "reflect";

/// Which supervision class a lane key belongs to — None for every build/planner lane. This is the
/// ONE derivation behind both consumers: the shared digest builders stamp `"supervision": true`
/// from it (never hand-set per write site), and `run_agent_in_inner` disarms the omni judge and
/// the repeat detector for these lanes — a supervision lane is CAPTURED, never SUPERVISED, because
/// judging the judge would mint `judge-judge-…` lanes without bound, and the keyless calls these
/// keys replaced were never judged either (behavior parity).
///
/// Derivation is by the exact shapes the mint fns above produce. A MODEL-chosen task id starting
/// with `judge-`/`prereview-`/`tail-review-` would be misclassified — the same accepted hazard
/// `engine_owned_activity_keys_cannot_collide_with_a_model_chosen_task_id` documents for
/// `call_objective` (live plans name tasks after modules); the replan shape is digit-exact, so the
/// measured bonus-task fixture id `replan-extra` stays a worker lane.
pub(super) fn supervision_lane_kind(key: &str) -> Option<&'static str> {
    if key.starts_with("judge-") {
        return Some("judge");
    }
    if let Some(rest) = key.strip_prefix("replan-r") {
        if !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()) {
            return Some("replan");
        }
    }
    if key.starts_with("prereview-") {
        return Some("prereview");
    }
    if key.starts_with("tail-review-") {
        return Some("tailreview");
    }
    if key.starts_with("schedjudge-") {
        return Some("schedjudge");
    }
    // Digit-exact like replan-r<n>: `verify-endpoints` is a name a plan could give a build task.
    if let Some(rest) = key.strip_prefix("verify-") {
        if !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()) {
            return Some("verify");
        }
    }
    if key == ASK_ANSWER_LANE {
        return Some("ask");
    }
    if key == PILLARS_LANE {
        return Some("pillars");
    }
    if key == REFLECT_LANE {
        return Some("reflect");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// Every minted supervision key derives its kind, and the ids MODELS actually choose (this
    /// file's plan fixtures + live runs) stay worker lanes — a misclassified worker lane would
    /// silently lose its omni-judge supervision.
    fn minted_keys_derive_their_kind_and_model_chosen_ids_do_not() {
        assert_eq!(
            supervision_lane_kind(&judge_lane_key("open-1")),
            Some("judge")
        );
        assert_eq!(supervision_lane_kind(&replan_lane_key(0)), Some("replan"));
        assert_eq!(
            supervision_lane_kind(&prereview_lane_key("store-core")),
            Some("prereview")
        );
        assert_eq!(
            supervision_lane_kind(&tail_review_lane_key("wiring")),
            Some("tailreview")
        );
        assert_eq!(
            supervision_lane_kind(&schedjudge_lane_key("web-viz")),
            Some("schedjudge")
        );
        assert_eq!(supervision_lane_kind(&verify_lane_key(3)), Some("verify"));
        assert_eq!(supervision_lane_kind(ASK_ANSWER_LANE), Some("ask"));
        assert_eq!(supervision_lane_kind(PILLARS_LANE), Some("pillars"));
        assert_eq!(supervision_lane_kind(REFLECT_LANE), Some("reflect"));
        for worker in [
            "test-store-core",
            "review",
            "open",
            "synthesis",
            "complete-fix::r0::app/a.py",
            "replan-extra",
            "replan-r2b",
            "research-payments-q0",
            "apptest-primary-journey",
            "integrate-verify",
            // The digit-exact rule keeps a model-chosen verify task a WORKER lane: a
            // misclassification here would silently strip its omni-judge supervision.
            "verify-endpoints",
            "verify-2b",
        ] {
            assert_eq!(
                supervision_lane_kind(worker),
                None,
                "{worker} is not a supervision lane"
            );
        }
    }
}

/// II-11c: how many trailing chars of a forming call's arguments the sidecar retains, and how
/// many of those the rendered preview may carry (mirrors INFLIGHT_PREVIEW_MAX). These bound a
/// DISPLAY tail and an IO payload, never model work — the full argument body still flows to the
/// decoder untouched, so gate 5 is not in play.
pub(super) const FORMING_TAIL_KEEP: usize = 480;
pub(super) const FORMING_PREVIEW_MAX: usize = 240;

/// One live forming tool call: named by the stream's open frame, its argument bytes counted and
/// tailed as their fragments arrive (r5 measured 28,157 B/8s of live fragments during OPEN).
pub(super) struct FormingRow {
    pub(super) name: String,
    pub(super) since_ms: u64,
    pub(super) args_bytes: u64,
    pub(super) args_tail: String,
}

/// The wrapper-owned state behind the forming observer: the live map plus the write-coalesce
/// bookkeeping. `write_error` is recorded ONCE and surfaces from `FormingGuard::drop` as a named
/// `forming_write_failed` event on every exit path, aborts included (gate 1: a failed write is
/// loud, never a silent `.ok()`).
#[derive(Default)]
pub(super) struct FormingSidecar {
    pub(super) live: std::collections::BTreeMap<String, FormingRow>,
    pub(super) last_write: Option<std::time::Instant>,
    pub(super) dirty: bool,
    pub(super) write_error: Option<String>,
}

/// Trim `s` in place to its last `keep` CHARS (never bytes — a delta may split multi-byte
/// UTF-8 anywhere, and the tail must stay boundary-safe).
fn keep_tail_chars(s: &mut String, keep: usize) {
    let n = s.chars().count();
    if n > keep {
        if let Some((cut, _)) = s.char_indices().nth(n - keep) {
            s.drain(..cut);
        }
    }
}

/// II-11c (engine half): fold one provider forming event into the live map. Two measured server
/// shapes both land here honestly: the buffering shape (open frame, 161-172s of silence, one
/// terminal ArgsDelta lump) reads as a named row whose bytes jump once; the streaming shape
/// (r5's OPEN: live `function.arguments` fragments) reads as a row whose byte count and tail
/// advance as the JSON forms. Pure over the map so the fold is unit-testable without a stream.
pub(super) fn fold_forming_event(
    live: &mut std::collections::BTreeMap<String, FormingRow>,
    ev: goose_provider_types::formats::openai::ToolFormingEvent,
) {
    use goose_provider_types::formats::openai::ToolFormingEvent;
    // The observer runs synchronously at decode time, so now() IS the frame's wall time (the
    // seam's contract); elapsed renders downstream from since_ms.
    let now_ms = || {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    };
    match ev {
        ToolFormingEvent::Forming { id, name, .. } => {
            live.insert(
                id,
                FormingRow {
                    name,
                    since_ms: now_ms(),
                    args_bytes: 0,
                    args_tail: String::new(),
                },
            );
        }
        ToolFormingEvent::ArgsDelta { id, delta } => {
            // Decode order makes a delta without its open frame impossible today; if a decoder
            // change ever breaks that, the row is named by the id — bytes counted, nothing
            // fabricated (gate 1: no invented tool name).
            let row = live.entry(id.clone()).or_insert_with(|| FormingRow {
                name: id,
                since_ms: now_ms(),
                args_bytes: 0,
                args_tail: String::new(),
            });
            row.args_bytes += delta.len() as u64;
            row.args_tail.push_str(&delta);
            keep_tail_chars(&mut row.args_tail, FORMING_TAIL_KEEP);
        }
        ToolFormingEvent::Complete { id } => {
            live.remove(&id);
        }
    }
}

/// The bounded, readable window of a forming call's argument tail: the last FORMING_PREVIEW_MAX
/// chars, front-trimmed to just after the first newline (else the first sentence break) so the
/// window does not open mid-token. If trimming would empty it, the raw window stands — a short
/// tail IS the content.
pub(super) fn forming_preview(tail: &str) -> String {
    let n = tail.chars().count();
    let window: &str = if n > FORMING_PREVIEW_MAX {
        // The cut comes from char_indices, so get() cannot miss; the unreachable arm keeps the
        // full tail (still bounded by FORMING_TAIL_KEEP), never fabricates.
        tail.char_indices()
            .nth(n - FORMING_PREVIEW_MAX)
            .and_then(|(cut, _)| tail.get(cut..))
            .unwrap_or(tail)
    } else {
        tail
    };
    let trimmed = match window.split_once('\n') {
        Some((_, rest)) => rest,
        None => match window.split_once(". ") {
            Some((_, rest)) => rest,
            None => window,
        },
    };
    if trimmed.trim().is_empty() {
        window.to_string()
    } else {
        trimmed.to_string()
    }
}

/// The `<key>.forming.json` body, or None when nothing is forming (the file is then removed —
/// never written empty; an empty forming file would read as a live amber row in the panel join).
/// Sits BESIDE the digest (`<key>.json`) on purpose — the digest is rewritten on a hot 400ms
/// coalesce by a different code path, and the panel/tick JOIN the two by activity key.
/// `args_preview` is omitted while args_bytes==0 (nothing has formed; an empty preview would
/// impersonate content).
pub(super) fn render_forming_file(
    live: &std::collections::BTreeMap<String, FormingRow>,
) -> Option<String> {
    if live.is_empty() {
        return None;
    }
    let rows: Vec<serde_json::Value> = live
        .iter()
        .map(|(id, row)| {
            let mut v = serde_json::json!({
                "id": id,
                "name": row.name,
                "since_ms": row.since_ms,
                "args_bytes": row.args_bytes,
            });
            if row.args_bytes > 0 {
                v["args_preview"] = serde_json::Value::from(forming_preview(&row.args_tail));
            }
            v
        })
        .collect();
    Some(serde_json::json!({ "forming": rows }).to_string())
}

/// THE READER of what `render_forming_file` writes: argument bytes across the frames open RIGHT
/// NOW, for the one supervision question no other meter can answer — is this stream mid-delivery?
///
/// It lives beside the writer so the two cannot drift on the shape (`forming[].args_bytes`).
/// MEASURED (r6c web-viz): the lane's ONE delivery of the run — `web/viz.js`, 38,927 bytes — was
/// streamed as a single `write` tool call's arguments while `thinking_chars` sat at exactly
/// 156,267 across looks 13, 14 and 15 (18:13:49Z -> 19:00:18Z), `calls.jsonl` was frozen at its
/// 15:28:09Z entry, and the owned file did not exist. Reasoning, answer, calls and owned bytes
/// all read DEAD; the argument stream was the whole delivery, and only this sidecar saw it.
///
/// The three answers are distinct on purpose (gate 1 — an absence must not impersonate a
/// measurement):
///   * `None` — the file is ABSENT, which is the writer's own honest empty: `render_forming_file`
///     returns None when the live map is empty and the observer then UNLINKS the file. Nothing is
///     forming.
///   * `Some(n)` — at least one frame is open and `n` argument bytes have arrived across them.
///   * a present-but-unparseable body reads as `Some(0)`: a frame IS open (the writer only ever
///     creates this file for an open frame) and its size is unknown, so it can never be read as
///     GROWTH. The ambiguity resolves toward the pre-existing behavior, never toward inventing
///     progress. (The writer is tmp+rename atomic, so this arm needs a corrupt disk to reach.)
pub(super) fn forming_args_bytes(sidecar: &Path) -> Option<u64> {
    let body = std::fs::read_to_string(sidecar).ok()?;
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&body) else {
        return Some(0);
    };
    Some(
        v["forming"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|r| r["args_bytes"].as_u64())
            .sum(),
    )
}

/// `<path>.tmp` for the atomic write below — same directory, so the rename is atomic in-dir.
pub(super) fn forming_tmp(path: &Path) -> PathBuf {
    let mut os = path.as_os_str().to_os_string();
    os.push(".tmp");
    PathBuf::from(os)
}

/// tmp+rename write for the forming sidecar. main.ts reads this file on its poll and joins it
/// onto the digest; a torn read there turns into a one-poll row disappearance (forming is never
/// carried from prev), so a partially-written file must be impossible, not merely unlikely.
pub(super) fn write_forming_atomic(path: &Path, body: &str) -> std::io::Result<()> {
    let tmp = forming_tmp(path);
    std::fs::write(&tmp, body)?;
    std::fs::rename(&tmp, path)
}

/// Removes the forming sidecar (and any half-written tmp) on EVERY exit path of the wrapped
/// provider call — the ShellGroupReaper pattern: return, `?`, judge termination and cancellation
/// of the whole dispatch future all unwind through this Drop. Complete is NOT guaranteed (an
/// errored, aborted or stall-killed stream never sends it), so scope exit is the clearing point
/// of record.
///
/// `report` (Some only in the live wrapper) makes the `forming_write_failed` diagnostic ride the
/// SAME every-exit-path guarantee: the emission used to sit after the scoped `.await`, so a
/// scheduler abort that cancelled the dispatch future cleaned the files (this Drop) but dropped
/// the one named record that writes had been failing — the diagnostic died with the future.
/// Cleanup-only users (the guard-clears test) pass None.
pub(super) struct FormingReport {
    pub(super) events: Arc<dyn EventSink>,
    pub(super) key: String,
    pub(super) sidecar: Arc<Mutex<FormingSidecar>>,
}

pub(super) struct FormingGuard {
    pub(super) path: PathBuf,
    /// A fix shard's real-tree mirror (see `fix_shard_mirror_dir`). Cleared with the primary:
    /// a stale mirrored forming.json in the REAL tree would read as a live amber row in the
    /// panel join forever — worse than the shadow copy, which dies with the wave.
    pub(super) mirror: Option<PathBuf>,
    pub(super) report: Option<FormingReport>,
}

impl Drop for FormingGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
        let _ = std::fs::remove_file(forming_tmp(&self.path));
        if let Some(m) = &self.mirror {
            let _ = std::fs::remove_file(m);
            let _ = std::fs::remove_file(forming_tmp(m));
        }
        if let Some(FormingReport {
            events,
            key,
            sidecar,
        }) = self.report.take()
        {
            let (write_error, held_unflushed) = {
                // Poison-recovered so a panicking observer cannot also silence the report;
                // the sidecar holds plain data, nothing can be torn mid-update.
                let mut g = match sidecar.lock() {
                    Ok(g) => g,
                    Err(poisoned) => poisoned.into_inner(),
                };
                (g.write_error.take(), g.dirty)
            };
            if let Some(error) = write_error {
                events.write_value(serde_json::json!({
                    "event": "forming_write_failed",
                    "key": key,
                    "error": error,
                    "held_unflushed": held_unflushed,
                }));
            }
        }
    }
}

/// How many prior SAID entries a digest carries. A lane that retries more than this keeps the most
/// recent ones — the pane's superseded list is a provenance trail, not an archive (the append-only
/// `<task>.log` is the archive).
const SUPERSEDED_KEEP: usize = 4;

/// Fold the digest a PREVIOUS attempt (or previous call on this lane key) left on disk into the
/// `superseded` list the new attempt's seed will carry. The old text is marked superseded rather
/// than silently kept or erased — before this, the seed's rewrite dropped it from the digest while
/// `<task>.log` kept showing it, which is exactly how a dead attempt's transport error read as the
/// live answer. `said_kind` is RECOMPUTED from the old text so legacy digests (no provenance keys)
/// classify correctly.
pub(super) fn superseded_from_prior(prior: Option<serde_json::Value>) -> Vec<serde_json::Value> {
    let Some(prior) = prior else {
        return Vec::new();
    };
    let mut out: Vec<serde_json::Value> = prior
        .get("superseded")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let last_text = prior
        .get("last_text")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if !last_text.trim().is_empty() {
        let field = |k: &str| prior.get(k).cloned().unwrap_or(serde_json::Value::Null);
        out.push(serde_json::json!({
            "attempt": field("attempt"),
            "last_text": last_text,
            "said_kind": super::said_kind_of(last_text),
            "said_at": field("said_at"),
            "model": field("model"),
        }));
    }
    if out.len() > SUPERSEDED_KEEP {
        let drop = out.len() - SUPERSEDED_KEEP;
        out.drain(..drop);
    }
    out
}

/// THE JUDGE CONTRACT. Four fields, not three. The third — ESTABLISHED — is the whole
/// point: a redirect that throws away the useful half of a spiralling call is just a slower
/// kill. The judge extracts what the call has actually worked out, and the nudge hands it
/// back so the model resumes from there instead of restarting its own thinking.
///
/// The last line is taken verbatim from codex/salvage-engine's one genuinely good idea
/// (its pre-scheduler judge prompt): the judge may NEVER ask for termination. Only the
/// engine ends a call, and only ever by cancelling a corroborated loop.
///
/// r6d (judge-research-ledger-core-q0 look 4, seq 215/216; judge-research-ledger-core-q5 look 1):
/// the judge's OWN request carries goose's `<turn-context><turn-budget>1/1 used</turn-budget>`
/// (moim.rs `turn_budget_line`: the look runs at max_turns 1, so turns_taken 1 arms the line on
/// its first and only turn) and both looks reasoned about it as if it were the worker's — q5:
/// "The turn budget says '1/1 used'... might mean this is the last turn?... it's ambiguous";
/// q0: "turn-context says 1/1 used. This is ambiguous — it might mean this is the last turn" —
/// and q0's look 4 then ended its turn in a tool call: `judge_look_failed` "the reply was the
/// agent loop's own turn-cap filler". The worker's transcripts never carry that line (workers run
/// at max_turns 100000 and the line arms at half; grep of every r6d research-*.log/think.log:
/// none), so the contract names the block as the judge's own and the answer shape as ONE
/// message with no tool call. Moved verbatim from swarm.rs's omni-look (incremental-split law);
/// that paragraph is the one addition.
pub(super) fn judge_contract() -> String {
    format!(
        "{JUDGE_CONTRACT_HEAD}{}{JUDGE_CONTRACT_TAIL}",
        own_turn_sentence()
    )
}

/// The judge probe's turn budget — ONE derivation for four facts that used to be copies: the
/// probe's `max_turns` at dispatch, the structural probe predicate in `run_agent_in_inner`
/// (which strips the developer toolset from the judge lane — r6e E14), the `<turn-budget>
/// {n}/{n} used` sentence in the contract, and the tripwire test that pins it.
pub(super) const JUDGE_PROBE_TURNS: u32 = 1;

fn own_turn_sentence() -> String {
    format!(
        "You answer in ONE message with no tool call. Your own request carries a <turn-context> \
         block whose <turn-budget> reads {t}/{t} used: that is YOUR single turn — the one message \
         you are writing now — never the call's. Any turn-budget or turn-count text inside the \
         excerpt belongs to the call you are reading, never to you.\n",
        t = JUDGE_PROBE_TURNS
    )
}

const JUDGE_CONTRACT_HEAD: &str =
    "You supervise ONE running agent call on a shared multi-agent build. You are \
     given its goal, what it has produced so far, a measurement of how much its reasoning is \
     repeating, a sample of its reasoning from much earlier in the same call, its recent \
     commands, and — when the call owns files — the deterministic checks of what it has \
     actually written plus the head of the file itself.\n\
     THE FILE CHECKS ARE FACTS AND OUTRANK THE REASONING. A call that sounds confident \
     while its owned file does not exist, does not parse, or holds nothing but stubs is \
     not progressing, whatever its narration says.\n";

const JUDGE_CONTRACT_TAIL: &str =
    "Decide ONE thing: is this call still making meaningful progress toward its goal?\n\
     Deep, slow, or repetitive-LOOKING reasoning that is ADVANCING is OK. LOOPING means it is \
     revisiting the same analysis without adding evidence, resolving a decision, or taking the \
     next concrete step.\n\
     Say LOOPING only when ESTABLISHED quotes the claim this call has now made TWICE — \
     the sentence from the earlier span and its recurrence in the recent tail. No quote, \
     no LOOPING: a stream producing NEW content is OK or DRIFTING, whatever its pace. \
     (Your own law applied to you: a reader quotes what it acted on.)\n\
     Reply on ONE line, exactly:\n\
     VERDICT|CONFIDENCE|ESTABLISHED|NEXT\n\
     VERDICT      OK | DRIFTING | LOOPING | RESTART\n\
     CONFIDENCE   HIGH | LOW\n\
     ESTABLISHED  what this call has actually worked out that is worth keeping. Draw it from \
     what it SAID; do not invent. Fill this on an OK verdict too — one line of what the \
     call has worked out so far, so the next look can see what changed since this one. \
     Leave empty only if it has established nothing.\n\
     NEXT         the single most concrete next action toward the goal. Name the file, the \
     command, or the function. Never \"continue\" or \"proceed\".\n\
     ASK FOR THE SMALLEST ACTION THAT LEAVES A TRACE, NEVER THE WHOLE DELIVERABLE. \
     A call that has produced no action yet is usually composing the entire artefact \
     inside its reasoning and waiting until it is perfect to emit it — and it never \
     becomes perfect. MEASURED: a task owning `web/viz.js` took 13 nudges over 76 \
     minutes, every one of them asking for the complete file (\"WebGL context, orbit \
     camera, picking\"), and wrote nothing at all. So if it owns a file, name the file \
     and ask for a FIRST MINIMAL VERSION it can extend — the stub, the imports, one \
     function. If its deliverable is a structured reply, tell it to emit what it has \
     NOW and refine after. And to a call with ZERO actions so far, phrase NEXT as a \
     command about its NEXT MESSAGE — \"your next message must be a single tool call: \
     <the one write or emit>\" — never as an imperative about the artefact. MEASURED: \
     \"call the output tool NOW: emit the slice table\" bought 19,000 more characters \
     of reasoning and zero calls; \"your next message must be a tool call\" was quoted \
     back by the lane and obeyed. A thing that exists can be improved; a thing being perfected \
     in silence cannot.\n\
     This governs your VERDICT too: a call that owns files, has taken ZERO actions, and \
     whose reasoning is already writing complete file bodies is DRIFTING — working on \
     the wrong thing (perfection in silence) — however good the draft reads.\n\
     A DIRECTION THAT RESTATES THE BRIEF IS NOT A DIRECTION. If the most concrete thing \
     you can name is the job the call was already given, you have nothing to add and the \
     verdict is OK — say so and let it work. MEASURED twice on one run: the direction \
     returned was \"Check the slice list against the request section by section\", which \
     is verbatim the task. Each cost a re-stream, and a re-stream throws away everything \
     the call had reasoned so far. Redirect it only when you can tell it something it \
     does not already know.\n\
     DRIFTING = working, but on the wrong thing.\n\
     RESTART = only when the call has produced nothing usable AND a fresh start carrying what \
     you list in ESTABLISHED would beat continuing. Never for a call that is merely slow.\n\
     You may never request termination. Your job is to redirect.\n\n\
     Finally, end your reply with a token of the form ETA=<n>m — your honest estimate of \
     how many more MINUTES this call needs to finish the job it was given. Put it on its \
     OWN line, after the four-field line — never appended to that line as a fifth pipe \
     field. You are the \
     only party that can judge this: you have read what it has established, what it is \
     doing now, and how fast it is producing. Base it on the work you can see REMAINING, \
     not on how long it has already taken. ETA=0m means it is essentially done. If you \
     genuinely cannot tell, write ETA=? rather than inventing a number.";

/// WHAT THIS CALL IS FOR, in one line, for the judge.
///
/// The judge was given the reasoning tail and nothing else, so it inferred the call's purpose from what
/// the call happened to be talking about. MEASURED on a live run: it watched the plan-REVIEW call, saw it
/// discussing modules named in the plan, concluded it was a build worker falling behind, and nudged it
/// three times to "Write wordfreq/core.py implementing count_words(text)". The review must not write code
/// at all. A supervisor that does not know what it is supervising does not help — it derails.
/// (Moved verbatim from swarm.rs under the incremental-split law, paying for E14's tool gate.)
pub(super) fn call_objective(activity_key: Option<&str>) -> &'static str {
    match activity_key {
        Some(k) if k.starts_with("open-coverage") => {
            "build a COVERAGE TABLE for its part of the request: every component that part names, which \
             slice owns each, and a QUOTE from that slice's objective proving it. It must NOT write code \
             and must NOT rewrite the slices that exist.\n\n\
             THIS CALL IS SUPPOSED TO LOOK REPETITIVE, and that is the thing to understand before you \
             judge it. Its deliverable IS a table: dozens of near-identical rows, each naming a component \
             and an owner in the same shape. Structural repetition here is the call doing exactly what it \
             was asked to do. MEASURED: judging that shape as a loop re-streamed these lanes three times \
             in one run, and every re-stream threw away the whole partial table and started the \
             enumeration again from the top — which is why a phase that should take minutes took thirty. \
             It is stuck only if the rows stop ADVANCING: the same component named twice, or an owner it \
             has already given. Rows that merely look alike are progress."
        }
        Some("open") | Some("open-resplit") => {
            "split the request into balanced semantic slices, naming each slice's owned files in its \
             objective as OWNERSHIP DECLARATIONS. It must NOT write code, plan tasks, or dependencies."
        }
        Some("synthesis") => {
            "wire already-researched slices into a task DAG — ids, files and dependencies only. It \
             must NOT write code and must NOT restate the specifications."
        }
        Some(k) if k == "review" || k.starts_with("review-") => {
            "read the original request against the plan and return a small structural PATCH. It must \
             NOT write code, and must NOT rewrite any task's specification. A fanned lane (review-N) \
             holds ONE portion of the request and the whole plan, so a task that looks unrelated to its \
             portion is almost certainly owned by another portion — that is not a finding."
        }
        Some("proxy-answer") => "answer the open decisions from the request. It must NOT write code.",
        Some("rate") => "rate each defect CRITICAL or MINOR. It must NOT write code.",
        Some(k) if k.starts_with("slice-") => {
            "answer its slice's questions and then give that module's SPECIFICATION — interfaces, edge \
             cases, files — AS ITS REPLY. The specification is what it says back, not a file it puts on \
             disk, and it has no file tools: never direct it to create or edit one. It must NOT write \
             the implementation."
        }
        Some(k) if k.starts_with("research-") => {
            "answer ONE named question about the request as a HANDOFF — exact files, exact \
             key/field literals, conventions stated as conventions. It must NOT write code and \
             has no file-writing tools; its structured reply IS its deliverable. Different \
             research questions legitimately cover similar ground — it is stuck only if its OWN \
             answer stops advancing."
        }
        Some(k) if k.starts_with("apptest-") => {
            "exercise the BUILT app from one angle and report the defects it observes, with the files \
             each touches. It must NOT fix anything and must not edit a single file — a call reporting \
             bugs without writing code is doing this job exactly right."
        }
        Some(k) if k.starts_with("contract-") => {
            "emit a signature-only stub for one module. It must NOT implement anything."
        }
        Some("integrate-verify") => {
            "assemble the produced modules, run the tests, boot the app and exercise the commands the \
             request advertises."
        }
        // A dispatched build worker: the only kind that SHOULD be writing files.
        _ => "implement its assigned module — write the files it owns, then verify them.",
    }
}

/// The judge's STRUCTURED-REPLY block — the same blindness `owned_block` covers for files, for a
/// call whose deliverable is a structured reply. MEASURED, run swarm-3node-r0: `open-coverage-1`
/// reached 144,935 characters with ZERO tool calls across five nudges — the last two literally
/// "call final_output NOW" — while two of three nodes sat idle waiting on it. Every verdict was
/// DRIFTING, never LOOPING, because it genuinely produced ~4,000 FRESH characters between looks.
/// It was not looping. It was enumerating forever into a channel that is not the deliverable.
/// Empty unless the call owes a structured reply and has called nothing. (Moved verbatim from
/// swarm.rs's omni-look under the incremental-split law, paying for E7's relay wiring.)
pub(super) fn structured_reply_block(
    wants_structured_reply: bool,
    no_calls_yet: bool,
    thinking_chars: usize,
) -> String {
    if wants_structured_reply && no_calls_yet {
        format!(
            "\n\nTHIS CALL'S DELIVERABLE IS A SINGLE STRUCTURED REPLY, made by calling its \
             output tool. IT HAS NOT CALLED IT ONCE, and it has written \
             {thinking_chars} characters of reasoning instead. Reasoning is not the \
             deliverable here and no later phase can read it — if this call ends without that \
             tool call, everything it worked out is discarded and the phase gets nothing. \
             Whatever it has enumerated so far is enough to submit: tell it to call the \
             output tool NOW with what it already has. A partial table that exists beats a \
             complete one that is still being composed."
        )
    } else {
        String::new()
    }
}

/// The judge's EARLIER-SPAN block: a verbatim span from tens of thousands of characters back,
/// with the compare instruction (gate 7 — the judge is shown the WORDS across looks). Empty
/// when the meter holds no earlier span yet. (Moved verbatim from swarm.rs's omni-look.)
pub(super) fn earlier_span_block(earlier: Option<&str>) -> String {
    match earlier {
        Some(e) => format!(
            "\n\nReasoning from EARLIER in this same call (tens of thousands of characters \
             ago):\n{e}\n\
             COMPARE this earlier span with the 'Most recent reasoning' below — that \
             comparison is why you are shown both. If the call is walking the SAME items to \
             the SAME conclusions again — however coherent each sentence reads on its own — \
             it is re-emitting, not advancing: the verdict is LOOPING, and NEXT names the \
             exit (call the output tool with what it already has). MEASURED (r4b): a \
             reviewer cycled one ten-item checklist verbatim for 24 minutes and every \
             2k-char window of it read as coherent checking; only the two windows side by \
             side showed the loop.",
        ),
        None => String::new(),
    }
}

/// Keep the last `max` chars of a snippet, with a leading ellipsis when clipped. Tail, not head, because
/// the informative part of tool output (the pass/fail line, the traceback, the printed value) is at the end.
pub(super) fn clip_tail(s: &str, max: usize) -> String {
    let s = s.trim();
    let n = s.chars().count();
    if n > max {
        format!("…{}", s.chars().skip(n - max).collect::<String>())
    } else {
        s.to_string()
    }
}

/// The last `max` characters of `s` (char-wise, never a byte slice — these are model tokens).
pub(super) fn tail_chars(s: &str, max: usize) -> String {
    let n = s.chars().count();
    if n > max {
        s.chars().skip(n - max).collect()
    } else {
        s.to_string()
    }
}

/// The closing sentences of the assistant-authored ERROR texts in agent.rs's provider-error arms —
/// the refusal, the NetworkError arm, and the generic provider-error arm. These are the ONLY texts
/// that reach the answer channel without the model having said them, so "does `last_text` end with
/// one of these" is a deterministic test for "this is a transport/agent error, not the model's
/// answer". Matched as suffixes because `last_text` is a 400-char TAIL and each of these sentences
/// is what the agent appends LAST before breaking the stream.
pub(super) const AGENT_ERROR_CLOSERS: [&str; 3] = [
    "Please resend your message to try again.",
    "Please retry if you think this is a transient or recoverable error.",
    "resending this conversation is likely to be refused again.",
];

/// `said` when `last_text` is (the tail of) something the MODEL produced; `error` when it is one of
/// the agent's own provider-error texts. The distinction exists because r0's `ledger-core-tests`
/// showed attempt 0's "Network error: Stream decode error … Please resend your message" as the
/// lane's current answer for 24+ minutes while attempt 1 was running — the pane had no way to say
/// "this text is a dead attempt's transport error", because the digest never said which kind of
/// text it was carrying.
pub(super) fn said_kind_of(last_text: &str) -> &'static str {
    let t = last_text.trim_end();
    if AGENT_ERROR_CLOSERS.iter().any(|c| t.ends_with(c)) {
        "error"
    } else {
        "said"
    }
}

/// Record WHY the judge passed without a semantic review. Without this every pass looks identical in
/// the log and the one number that matters — how often the supervisor actually formed a judgement —
/// cannot be attributed to a cause. (Moved verbatim from swarm.rs under the incremental-split law,
/// paying for the E8 avoid-rank wiring.)
pub(super) fn me_events_skip(events: &Arc<dyn EventSink>, task_id: &str, reason: &str) {
    events.write_value(serde_json::json!({
        "event": "judge_skipped",
        "task_id": task_id,
        "reason": reason,
    }));
}

/// The goose core agent loop returns a FIXED meta-message when a weak worker exhausts its turn budget
/// without calling final_output ("I've reached the maximum number of actions I can do without user input.
/// Would you like me to continue?", agent.rs MAX_TURNS_MESSAGE). That filler is NOT a usable result: the
/// detailer must fall back to the skeleton brief rather than write it as a subtask spec, and a repro-author /
/// reviewer must not treat it as an authored command / verdict. True when the text is empty or is that filler.
/// Moved here from swarm.rs (incremental-split law) beside its one non-root caller, the
/// supervised-reply door below.
pub(super) fn is_agent_loop_filler(s: &str) -> bool {
    let t = s.trim().to_lowercase();
    t.is_empty()
        || t.contains("reached the maximum number of actions")
        || t.contains("would you like me to continue")
        || t.contains("continuing agent loop")
}

/// Why a supervision reply is NOT a usable reply. Both variants take the caller's failed-look
/// path; they are distinguished because the omni judge names the second with the
/// `judge_turn_budget_exhausted` vocabulary the schedjudge arm already emits.
#[derive(Debug)]
pub(super) enum SupervisedReplyError {
    /// The agent loop's own provider-error closer text (A-2, `said_kind_of == "error"`): a dead
    /// judge model arrives as `Ok` TEXT, and r2's lenient parse minted 28 `drifting` verdicts
    /// whose hint WAS gabee's 400 body. Carries the tail-clipped error for the failed event.
    ProviderError(String),
    /// The reply is — or after the strip reduces to — the agent loop's turn-cap filler
    /// (`goose::agents::MAX_TURNS_MESSAGE`): the model's single supervision turn ended in a tool
    /// call, so the ENGINE wrote the filler instead of a verdict. r6a (run.jsonl seq 58) measured
    /// the laundering: the reply was ONLY the filler and `parse_judge_reply`'s no-token fallback
    /// manufactured `drifting` with the filler as `next` — an engine sentence in the verdict
    /// channel. This state was never actually judged; a failed look, never a verdict.
    TurnBudgetExhausted,
}

impl std::fmt::Display for SupervisedReplyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ProviderError(body) => f.write_str(body),
            Self::TurnBudgetExhausted => f.write_str(
                "no verdict — the reply was the agent loop's own turn-cap filler \
                 (the supervisor's single turn ended in a tool call)",
            ),
        }
    }
}

/// The ONE door a small-turn supervision reply (judge probe, prereview, tail review, finding
/// verify, reflect, ask-answer) walks before any parser sees it:
///   1. the A-2 error-closer reclassification (absorbs the old `supervision_reply`),
///   2. a trailing `MAX_TURNS_MESSAGE` is STRIPPED — r6c (live, run.jsonl seq 312) measured a real
///      DRIFTING verdict whose `next` ended with the filler as its own trailing line, one drift-hold
///      away from being delivered to a worker as a judge direction. Engine-authored text never
///      reaches a model-read direction. Stripped by the shared constant, never a duplicated literal
///      (the JUDGE_ENDED_NEEDLE pattern: emit and matcher move together).
///   3. what remains must not BE the filler (`is_agent_loop_filler`: empty, or the filler phrases
///      anywhere — the same over-broad-but-safe contract the schedjudge arm already applies).
pub(super) fn supervised_reply_text(text: &str) -> Result<String, SupervisedReplyError> {
    if said_kind_of(text) == "error" {
        return Err(SupervisedReplyError::ProviderError(clip_tail(text, 400)));
    }
    let t = text.trim_end();
    let cleaned = t
        .strip_suffix(goose::agents::MAX_TURNS_MESSAGE)
        .map(str::trim_end)
        .unwrap_or(t);
    if is_agent_loop_filler(cleaned) {
        return Err(SupervisedReplyError::TurnBudgetExhausted);
    }
    Ok(cleaned.to_string())
}

/// #135 OMNI-JUDGE probe. Asks a model whether an IN-FLIGHT call is repeating itself, from the call's own
/// recent reasoning plus the commands it has run. This is the piece a threshold cannot do: for a plan draft,
/// healthy and pathological look IDENTICAL by volume (healthy reach 57k chars), so only reading the text
/// separates them.
///
/// Deliberately conservative — it returns true ONLY on an explicit LOOPING verdict. Anything ambiguous, any
/// parse failure, any model error reads as "keep going", because a false abort costs a planner call that has
/// NO retry path. It can never fail a task or a run: it aborts at most this one call, and every phase
/// degrades gracefully (scout -> N of 3, draft -> best-of-N, detail -> skeleton brief, worker -> the
/// existing stall path).
/// Does this verdict mean the call needs REDIRECTING, as opposed to being left alone?
///
/// RESTART belongs here and was missing. `parse_judge_reply` produces it, nothing in the planner-side
/// omni path matched on it, and the only other actuator is `drifting_now` — so a restart verdict fell
/// through to the branch that CLEARS the looping streak and was discarded. MEASURED: at 07:36:28 on run
/// swarm-3node-r0 the judge answered `restart` on the wedged REVIEW call and the engine did nothing with
/// it, then went quiet. The judge's own words were "Restart the call on a fresh connection and have it
/// produce the structured output" — which is precisely what the re-stream delivery does, so the verdict
/// had a working actuator all along and simply was not wired to it.
pub(super) fn omni_judge_says_looping(reply: &str) -> bool {
    let out = parse_judge_reply(reply);
    matches!(
        out.verdict,
        Verdict::Looping | Verdict::OverReading | Verdict::Restart
    ) && out.confidence >= 0.8
}

/// Parse the semantic judge's one-line `VERDICT|CONFIDENCE|hint` reply. Conservative: anything not a
/// clearly-flagged problem reads as OK, so a vague weak-model reply can never kill a healthy worker.
/// CONFIDENCE gates agency — the judge acts (kill + correct) only on a verdict it marks HIGH.
/// The judge's own estimate of how many more MINUTES a call needs, from an `ETA=<n>m` token.
///
/// Mihai, watching a sink spend an hour on one bug: "the estimate time needs to be updated so the judge
/// models should update the ETA because they can tell best what time is left". He is right — the repair
/// ETA is arithmetic over past round durations, which answers "how long did rounds take" and not "how much
/// is left". The judge has read what the call established, what it is doing now and how fast it is
/// producing; nothing else in the engine has that.
///
/// A labelled token rather than a fifth pipe field on purpose: the free-segment parser above earned its
/// leniency the hard way, and shifting its indices to add a field would break every measured reply shape.
/// `ETA=?` is a first-class answer — an invented number is worse than an admitted unknown.
pub(super) fn parse_judge_eta_mins(s: &str) -> Option<u64> {
    let up = s.to_uppercase();
    let at = up.find("ETA")?;
    let rest = up.get(at + 3..)?.trim_start_matches([':', '=', ' ']);
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None; // "ETA=?" and anything unparseable
    }
    digits.parse::<u64>().ok().filter(|m| *m <= 24 * 60)
}

pub(super) fn parse_judge_reply(s: &str) -> JudgeOutcome {
    // FOUR fields now: VERDICT|CONFIDENCE|ESTABLISHED|NEXT. Parsing stays deliberately LENIENT, because
    // the fleet is a 27B and the old three-field parser earned that leniency the hard way: qwen-class
    // models echo the field LABELS back (`VERDICT|CONFIDENCE|LOOPING|HIGH|<text>`), drop fields, or answer
    // in two segments. So: pick the verdict and confidence out of wherever they land, and take the free
    // text segments in order. One free segment means the old shape — treat it as NEXT, not ESTABLISHED,
    // since an action is what the nudge cannot do without.
    let is_token = |seg: &str| {
        matches!(
            seg.to_uppercase().trim(),
            "VERDICT"
                | "CONFIDENCE"
                | "CONF"
                | "HINT"
                | "ESTABLISHED"
                | "NEXT"
                | "OK"
                | "BROKEN_CODE"
                | "BROKEN CODE"
                | "LOOPING"
                | "DRIFTING"
                | "RESTART"
                | "OVER_READING"
                | "OVER READING"
                | "SPEC_DRIFT"
                | "SPEC DRIFT"
                | "HIGH"
                | "LOW"
        )
    };
    // STRIP THE ETA TOKEN BEFORE ANYTHING ELSE READS THE SEGMENTS.
    //
    // I added `ETA=<n>m` to the judge contract tonight as a labelled token, precisely so it would not
    // disturb the positional free-segment parse — and then did not remove it from the segments. It is
    // not in `is_token`, so it survived the filter, became a free segment, and therefore became
    // ESTABLISHED or NEXT. MEASURED within hours, live: nudges delivered to workers reading, in full,
    // `ETA=0m`, `ETA=5m`, `ETA=45m`, and `Complete rating all defects and deliver the final verdict
    // list\nETA=5m`. A direction that says only how long something will take is not a direction, and it
    // was going out as one.
    // EVERY occurrence, not the first. `find("ETA")` returned the FIRST match, so any earlier word
    // CONTAINING those three letters — metadata, details, theta, beta, retain — failed the `:`/`=` guard,
    // the line was kept whole, and the REAL `ETA=` at the end survived to become a free segment.
    // MEASURED live 2026-08-28, after the first fix was already shipped: workers re-streamed with a
    // direction reading, in full, `ETA=5m` and `ETA=10m`. Reproduced deterministically — "read the
    // metadata table first" leaks, "the schema is written" does not.
    // Byte-scanned against the ORIGINAL rather than an uppercased copy: `to_uppercase()` can CHANGE
    // LENGTH (ß -> SS), so an index taken from it and used to slice the original is unsound.
    let cut_eta = |l: &str| -> String {
        let b = l.as_bytes();
        for i in 0..b.len().saturating_sub(2) {
            if b[i..i + 3].eq_ignore_ascii_case(b"ETA")
                && l.is_char_boundary(i)
                && l.get(i + 3..)
                    .is_some_and(|r| r.trim_start().starts_with([':', '=']))
            {
                return l.get(..i).unwrap_or(l).trim_end().to_string();
            }
        }
        l.to_string()
    };
    let without_eta: String = s.lines().map(cut_eta).collect::<Vec<_>>().join("\n");
    let free: Vec<&str> = without_eta
        .split('|')
        .map(|h| h.trim())
        .filter(|h| !h.is_empty() && !is_token(h))
        .collect();
    let (established, next_action) = match free.len() {
        0 => (String::new(), String::new()),
        1 => (String::new(), free[0].to_string()),
        _ => (
            free[free.len() - 2].to_string(),
            free[free.len() - 1].to_string(),
        ),
    };
    // THE VERDICT COMES FROM A TOKEN SEGMENT, NEVER FROM A SUBSTRING SCAN OF THE WHOLE REPLY.
    //
    // This was `upper.contains("RESTART")` over the entire text — including the free-form hint. So the
    // judge's own good advice turned into a verdict about itself: `LOOPING|HIGH|est|stop and restart the
    // server` parsed as Restart, and `BROKEN_CODE|HIGH|the worker keeps restarting the dev server`
    // parsed as Restart at confidence 0.85, which clears the intervention bar and re-queues a live
    // worker. The mirror case lost a real verdict — a Restart the in-call loop has no branch for falls
    // to the `else` and wipes the looping streak and its corroboration tails.
    //
    // `is_token` already knows which segments are field values rather than prose, and every measured
    // reply shape puts the verdict in one of them (`LOOPING|HIGH|…` and the label-echoing
    // `VERDICT|CONFIDENCE|LOOPING|HIGH|…`). Reading only those is exact and keeps the leniency.
    let tokens: Vec<String> = s
        .split('|')
        .map(|h| h.trim().to_uppercase())
        .filter(|h| is_token(h))
        .collect();
    let said = |k: &str| tokens.iter().any(|t| t == k);
    // Same bug, same fix: a hint containing "high priority" used to raise the confidence.
    let confidence = if said("HIGH") { 0.85 } else { 0.5 };
    let verdict = if said("RESTART") {
        Verdict::Restart
    } else if said("LOOPING") {
        Verdict::Looping
    } else if said("DRIFTING") {
        Verdict::Drifting
    } else if said("BROKEN_CODE") || said("BROKEN CODE") {
        Verdict::BrokenCode
    } else if said("OVER_READING") || said("OVER READING") {
        Verdict::OverReading
    } else if said("SPEC_DRIFT") || said("SPEC DRIFT") {
        Verdict::SpecDrift
    } else {
        // No keyword. The measured qwen habit is to state a real problem as `VERDICT|HIGH|<correction>`
        // with no verdict word at all, and dropping that would make the judge inert on this fleet. Read it
        // as DRIFTING — a redirect — only when the model did NOT say OK and gave something substantive.
        // DRIFTING is the safe default now that the judge can only ever nudge: the worst case is one
        // unnecessary in-session note, not a dead worker.
        let said_ok = s.split('|').any(|p| p.trim().eq_ignore_ascii_case("ok"));
        let substantive = next_action.len() >= 16;
        if !said_ok && substantive {
            Verdict::Drifting
        } else {
            return JudgeOutcome::ok();
        }
    };
    JudgeOutcome {
        verdict,
        confidence,
        // `hint` stays the corrective one-liner every existing consumer reads; it is now the NEXT ACTION,
        // which is what a hint was always trying to be.
        hint: if next_action.is_empty() {
            "Take the next concrete action on your owned files now.".to_string()
        } else {
            next_action.clone()
        },
        established,
        next_action,
        proposed_split: None,
        // MODEL-AUTHORED. It may STEER; it may never fail a task.
        deterministic: false,
    }
}

#[cfg(test)]
mod reply_tests {
    use super::*;

    #[test]
    fn is_agent_loop_filler_catches_max_turns_message_and_empty() {
        // The exact goose core agent-loop max-turns filler (agent.rs MAX_TURNS_MESSAGE) — the string that
        // leaked into detailer specs (logstat-2, ledgr-2) and repro/verify verdicts.
        assert!(is_agent_loop_filler(
            "I've reached the maximum number of actions I can do without user input. Would you like me to continue?"
        ));
        assert!(is_agent_loop_filler(
            "  Final output tool has not been called yet. Continuing agent loop.  "
        ));
        // Empty / whitespace -> filler (subsumes the old empty-only guard).
        assert!(is_agent_loop_filler(""));
        assert!(is_agent_loop_filler("   \n\t"));
        // A REAL detailed subtask spec -> NOT filler (must be kept, not replaced by the brief).
        assert!(!is_agent_loop_filler(
            "Implement parser.py: tokenize the input into (ts, level, fields) records; skip malformed lines; \
             numeric field values parse as numbers. Files: logstat/parser.py."
        ));
        assert!(!is_agent_loop_filler(
            "def compute_ratio(a, b): guard b==0 with a clean error"
        ));
    }

    /// r6a run.jsonl seq 58 pinned: the judge probe's reply was ONLY the engine's turn-cap filler
    /// (built FROM the shared constant, exactly as agent.rs emits it), and the lenient parser
    /// really does launder it into `drifting` — which is why this gate must run BEFORE the parse.
    #[test]
    fn a_filler_only_reply_is_a_failed_look_never_a_verdict() {
        let r6a_reply = goose::agents::MAX_TURNS_MESSAGE.to_string();
        assert!(matches!(
            supervised_reply_text(&r6a_reply),
            Err(SupervisedReplyError::TurnBudgetExhausted)
        ));
        assert_eq!(
            parse_judge_reply(&r6a_reply).verdict,
            Verdict::Drifting,
            "the no-token fallback still manufactures DRIFTING from the filler, so the \
             classification must happen before parse_judge_reply ever sees it"
        );
    }

    /// r6c (live run swarm-20260831-072930517, run.jsonl seq 312) pinned: a REAL verdict whose
    /// final line is the turn-cap filler. The verdict survives; the engine's sentence does not.
    #[test]
    fn a_verdict_with_trailing_filler_parses_with_a_clean_next() {
        let r6c_shape = format!(
            "DRIFTING|HIGH|Answer is fully worked out in its reasoning: role matrix and vendor \
             payment call|Your next message must be a single tool call: invoke the output tool NOW \
             and emit the structured reply. Do not add more reasoning; refine only after it is \
             emitted.\n\n{}",
            goose::agents::MAX_TURNS_MESSAGE
        );
        let clean = supervised_reply_text(&r6c_shape).expect("a real verdict is kept");
        assert!(
            !clean.contains("maximum number of actions"),
            "the engine's filler must not survive into the verdict channel: {clean}"
        );
        let out = parse_judge_reply(&clean);
        assert_eq!(out.verdict, Verdict::Drifting);
        assert!(
            out.next_action
                .ends_with("refine only after it is emitted."),
            "next ends at the judge's own words, not the engine's: {}",
            out.next_action
        );
    }

    /// The r2 provider-error shape (moved with the `supervision_reply` absorption) and two normal
    /// verdicts, byte-untouched.
    #[test]
    fn provider_error_text_is_rejected_and_normal_replies_pass_untouched() {
        let r2_shape = "Ran into this error: Request failed with status 400 Bad Request: \
             Invalid model identifier 'qwen3-omni-30b'.\n\nPlease retry if you think this is a \
             transient or recoverable error.";
        match supervised_reply_text(r2_shape) {
            Err(SupervisedReplyError::ProviderError(body)) => assert!(
                body.contains("Invalid model identifier"),
                "the failed event carries the error body: {body}"
            ),
            _ => panic!("a provider error is never a reply"),
        }
        // parse_judge_reply on the same text is exactly the r2 laundering — pinned here so the
        // guard's necessity stays measured: lenient parsing DOES mint a Drifting verdict from it.
        assert_eq!(
            parse_judge_reply(r2_shape).verdict,
            Verdict::Drifting,
            "the parser still launders error text, so the gate must run BEFORE it"
        );
        for ok in [
            "LOOPING|HIGH|the schema is written|write app/main.py",
            "OK|HIGH|wrote store.py",
        ] {
            match supervised_reply_text(ok) {
                Ok(t) => assert_eq!(t, ok),
                Err(e) => panic!("a normal reply passes untouched: {e}"),
            }
        }
    }

    #[test]
    /// The ETA token must never become the direction.
    ///
    /// Added as a labelled token so the positional free-segment parse would be undisturbed — and then
    /// not removed from the segments, so it survived the `is_token` filter and became ESTABLISHED or
    /// NEXT. MEASURED live within hours: workers received nudges whose entire text was `ETA=0m`.
    fn the_eta_token_never_becomes_the_direction() {
        let o = parse_judge_reply(
            "LOOPING|HIGH|it has re-run one command 8 times|read the whole handler, not 25 lines\nETA=8m",
        );
        assert_eq!(parse_judge_eta_mins("ETA=8m"), Some(8));
        assert!(
            !o.next_action.contains("ETA"),
            "ETA leaked into NEXT: {:?}",
            o.next_action
        );
        assert!(
            !o.established.contains("ETA"),
            "ETA leaked into ESTABLISHED: {:?}",
            o.established
        );
        assert!(o.next_action.contains("read the whole handler"));

        // The pathological shape actually observed: the judge answered with nothing BUT an ETA.
        let bare = parse_judge_reply("LOOPING|HIGH|ETA=0m");
        assert!(
            !bare.next_action.contains("ETA"),
            "a reply carrying only an ETA must not yield 'ETA=0m' as the direction, got {:?}",
            bare.next_action
        );

        // Inline on the same segment, and the ':' spelling.
        let inline = parse_judge_reply("DRIFTING|HIGH|est|fix the do_GET routing ETA: 5m");
        assert!(
            !inline.next_action.contains("ETA"),
            "{:?}",
            inline.next_action
        );
        assert!(inline.next_action.contains("do_GET"));

        // A word merely CONTAINING eta must survive — "metadata", "beta".
        let word = parse_judge_reply("DRIFTING|HIGH|est|update the beta metadata table");
        assert!(
            word.next_action.contains("beta metadata"),
            "{:?}",
            word.next_action
        );
    }

    #[test]
    /// The judge's own words must never become a verdict about the judge.
    ///
    /// `upper.contains("RESTART")` scanned the whole reply, hint included. The hint is exactly where the
    /// word appears in normal use — the live run's best diagnosis was "stop retrying the same port ...
    /// or check if the server is already running", and a sibling shape is "the worker keeps restarting
    /// the dev server". Both parsed as Verdict::Restart at 0.85, which clears the intervention bar and
    /// re-queues a LIVE worker.
    fn a_verdict_is_read_from_a_field_not_from_the_hint() {
        // The hint mentions restarting. The verdict is LOOPING and must stay LOOPING.
        let o = parse_judge_reply(
            "LOOPING|HIGH|it has re-run one command 8 times|stop and restart the server on a new port",
        );
        assert_eq!(o.verdict, Verdict::Looping, "hint text became the verdict");

        let o = parse_judge_reply("BROKEN_CODE|HIGH|the worker keeps restarting the dev server");
        assert_eq!(o.verdict, Verdict::BrokenCode);

        // A real RESTART verdict still parses, in both measured shapes.
        assert_eq!(
            parse_judge_reply("RESTART|HIGH|nothing usable yet|start again from the spec").verdict,
            Verdict::Restart
        );
        assert_eq!(
            parse_judge_reply("VERDICT|CONFIDENCE|RESTART|HIGH|est|next").verdict,
            Verdict::Restart
        );

        // Confidence had the same bug: "high priority" in a hint used to raise it.
        let low = parse_judge_reply("DRIFTING|LOW|est|this is a high priority fix");
        assert!(low.confidence < 0.8, "hint text raised the confidence");
    }

    /// The retarget budget was env-ONLY, and the env NEVER REACHES THE ENGINE (LaunchServices hands the
    /// desktop app its own environment — proven with a probe var + `ps eww`). So the campaign's documented
    /// invariant "HELD CONSTANT every arm: ROUNDS=4" was never true on a single run: every one silently used
    /// the default 2. config.yaml is the only channel that reaches the engine.
    #[test]
    /// The FOUR-field contract, and the leniency it has to keep.
    ///
    /// ESTABLISHED is the field the whole nudge exists for — a redirect that throws away the useful half
    /// of a spiralling call is just a slower kill — so it has to survive the shapes a 27B actually emits.
    /// The old three-field replies must still parse, and when only ONE free segment arrives it is the
    /// ACTION, not the establishment: a nudge can be written without knowing what was established, but not
    /// without knowing what to do next.
    fn parse_judge_reply_reads_established_and_next() {
        let full = parse_judge_reply(
            "LOOPING|HIGH|the CSV parser needs a header row and the delimiter is a pipe|write \
             app/parse.py with parse_ledger(path: Path) -> list[Row]",
        );
        assert_eq!(full.verdict, Verdict::Looping);
        assert!(
            full.established.contains("delimiter is a pipe"),
            "established must survive: {:?}",
            full.established
        );
        assert!(
            full.next_action.contains("app/parse.py"),
            "next must be the action: {:?}",
            full.next_action
        );
        assert_eq!(full.hint, full.next_action, "hint IS the next action now");

        // Labels echoed back (the measured qwen habit) must not be mistaken for content.
        let echoed = parse_judge_reply(
            "VERDICT|CONFIDENCE|ESTABLISHED|NEXT|DRIFTING|HIGH|the schema is already written|wire it into \
             app/main.py",
        );
        assert_eq!(echoed.verdict, Verdict::Drifting);
        assert!(echoed.established.contains("schema is already written"));
        assert!(echoed.next_action.contains("app/main.py"));

        // Old three-field shape: one free segment is the ACTION, established stays empty.
        let legacy = parse_judge_reply("LOOPING|HIGH|stop re-reading and write the file now");
        assert_eq!(legacy.verdict, Verdict::Looping);
        assert!(legacy.established.is_empty(), "no establishment claimed");
        assert!(legacy.next_action.contains("write the file"));

        // AN EARLIER WORD CONTAINING "ETA" MUST NOT SHIELD THE REAL ETA TOKEN. `find` returned the
        // first match, so "metadata"/"details"/"theta" failed the `:`/`=` guard and the line survived
        // whole — workers were then re-streamed with a direction that read, in full, "ETA=5m".
        for shield in [
            "read the metadata table first",
            "check the details of the outbox",
            "theta values for the camera",
            "retain the beta flag",
        ] {
            let r = parse_judge_reply(&format!(
                "LOOPING|HIGH|{shield}|write app/main.py with parse_ledger()|ETA=5m"
            ));
            assert!(
                !r.next_action.contains("ETA"),
                "the ETA token leaked into the direction after {shield:?}: {:?}",
                r.next_action
            );
            assert!(
                r.next_action.contains("app/main.py"),
                "the real direction must survive after {shield:?}: {:?}",
                r.next_action
            );
            assert!(
                r.established.contains(shield),
                "established must survive: {:?}",
                r.established
            );
        }
        // The plain case must keep working.
        let plain =
            parse_judge_reply("LOOPING|HIGH|the schema is written|write app/main.py|ETA=45m");
        assert!(!plain.next_action.contains("ETA"));
        assert!(plain.next_action.contains("app/main.py"));

        // RESTART is recognised and is never confused with OK.
        assert_eq!(
            parse_judge_reply("RESTART|HIGH||start over from the frozen contract").verdict,
            Verdict::Restart
        );
    }

    #[test]
    fn parse_judge_reply_handles_qwen_formats() {
        // Healthy: qwen echoes the field labels and reorders OK/HIGH/LOW — all must read OK (no kill).
        for ok in [
            "VERDICT|CONFIDENCE|OK|HIGH",
            "VERDICT|OK|LOW|",
            "VERDICT|CONFIDENCE|HIGH|OK",
            "VERDICT|LOW|",
            "VERDICT|HIGH|OK|done",
        ] {
            assert_eq!(
                parse_judge_reply(ok).verdict,
                Verdict::Ok,
                "should be OK: {ok}"
            );
        }
        // A real catch with NO verdict keyword — just HIGH + a corrective hint — must become actionable
        // (this is the qwen format that was silently dropped before).
        let caught = parse_judge_reply(
            "VERDICT|HIGH|STOP retrying failing commands — write rules.py directly with a parser",
        );
        assert_ne!(
            caught.verdict,
            Verdict::Ok,
            "keyword-less HIGH+hint must act"
        );
        assert!(caught.confidence >= 0.8);
        assert!(
            caught.hint.contains("rules.py"),
            "hint must be the correction, not an echoed label"
        );
        // Explicit keyword still classifies, and the hint skips echoed labels.
        let oread = parse_judge_reply("VERDICT|CONFIDENCE|OVER_READING|HIGH|write the file now");
        assert_eq!(oread.verdict, Verdict::OverReading);
        assert_eq!(oread.hint, "write the file now");
        // HIGH but no real correction -> stays OK (a vague reply can never kill a healthy worker).
        assert_eq!(parse_judge_reply("VERDICT|HIGH|").verdict, Verdict::Ok);
    }

    /// THE READER MATCHES THE WRITER, and the three answers stay distinct (gate 1). Built from
    /// `render_forming_file`'s own output so a shape change breaks here, not silently at the
    /// delivery seam where a missed growth becomes a wiped delivery (r6c web-viz, look 14).
    #[test]
    fn forming_args_bytes_reads_what_render_forming_file_writes() {
        let dir = std::env::temp_dir().join(format!(
            "goose-forming-read-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("web-viz.forming.json");

        // Absent = the writer's honest empty: render returns None and the observer unlinks.
        assert_eq!(forming_args_bytes(&path), None);

        let mut live = std::collections::BTreeMap::new();
        fold_forming_event(
            &mut live,
            goose_provider_types::formats::openai::ToolFormingEvent::Forming {
                id: "call-1".into(),
                name: "write".into(),
                since: std::time::Instant::now(),
            },
        );
        fold_forming_event(
            &mut live,
            goose_provider_types::formats::openai::ToolFormingEvent::ArgsDelta {
                id: "call-1".into(),
                delta: "x".repeat(12_000),
            },
        );
        std::fs::write(
            &path,
            render_forming_file(&live).expect("an open frame renders"),
        )
        .unwrap();
        assert_eq!(forming_args_bytes(&path), Some(12_000));

        // The r6c growth across one look cycle: same frame, more argument bytes.
        fold_forming_event(
            &mut live,
            goose_provider_types::formats::openai::ToolFormingEvent::ArgsDelta {
                id: "call-1".into(),
                delta: "y".repeat(19_500),
            },
        );
        std::fs::write(&path, render_forming_file(&live).unwrap()).unwrap();
        assert_eq!(forming_args_bytes(&path), Some(31_500));

        // Present but unreadable can never read as GROWTH — a frame is open, its size unknown.
        std::fs::write(&path, "{not json").unwrap();
        assert_eq!(forming_args_bytes(&path), Some(0));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// E9 tripwire (r6d judge-research-ledger-core-q0 look 4 / q5 look 1): the contract must keep
    /// naming the judge's OWN `<turn-context>` turn-budget as its own single turn and the answer
    /// shape as ONE message with no tool call — beside the four-field line and the never-terminate
    /// law it moved here with. A doc-presence check: it summons a reader, it decides nothing.
    #[test]
    fn the_judge_contract_owns_its_turn_budget_and_answers_in_one_message() {
        let c = judge_contract();
        assert!(c.contains("You answer in ONE message with no tool call."));
        assert!(c.contains(&format!(
            "<turn-budget> reads {t}/{t} used: that is YOUR single turn",
            t = JUDGE_PROBE_TURNS
        )));
        assert!(c.starts_with("You supervise ONE running agent call"));
        assert!(c.contains("belongs to the call you are reading, never to you."));
        assert!(c.contains("VERDICT|CONFIDENCE|ESTABLISHED|NEXT"));
        assert!(c.contains("You may never request termination."));
    }
}
