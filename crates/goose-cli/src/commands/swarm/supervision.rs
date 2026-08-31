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
    if super::said_kind_of(text) == "error" {
        return Err(SupervisedReplyError::ProviderError(super::clip_tail(
            text, 400,
        )));
    }
    let t = text.trim_end();
    let cleaned = t
        .strip_suffix(goose::agents::MAX_TURNS_MESSAGE)
        .map(str::trim_end)
        .unwrap_or(t);
    if super::is_agent_loop_filler(cleaned) {
        return Err(SupervisedReplyError::TurnBudgetExhausted);
    }
    Ok(cleaned.to_string())
}

#[cfg(test)]
mod reply_tests {
    use super::*;

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
            super::super::parse_judge_reply(&r6a_reply).verdict,
            super::super::Verdict::Drifting,
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
        let out = super::super::parse_judge_reply(&clean);
        assert_eq!(out.verdict, super::super::Verdict::Drifting);
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
            super::super::parse_judge_reply(r2_shape).verdict,
            super::super::Verdict::Drifting,
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
}
