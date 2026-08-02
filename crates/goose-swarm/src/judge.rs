//! The idle-model judge: a supervisory role that runs on a node that would otherwise sit idle while
//! other tasks are in flight. It inspects one busy worker — the files it has produced so far and its
//! live activity — and returns a [`Verdict`]. The scheduler can act on an actionable verdict by
//! killing that worker and re-dispatching its task with a corrective hint.
//!
//! This module is model-agnostic (like [`crate::dispatch`]): it defines the types, the [`Judge`]
//! trait (implemented in goose-cli by an LLM on the idle device), and a cheap [`deterministic_verdict`]
//! that needs no model. The judge is OPT-IN (`Scheduler::with_judge`) and OFF by default, so the core
//! scheduling path and every mock-based test are unaffected unless a judge is attached.

use crate::TaskId;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// What the judge concluded about an in-flight worker.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// Progressing fine — no action.
    Ok,
    /// Reading/exploring far more than writing (over-read, structure-hunt, greenfield-explore).
    OverReading,
    /// A degenerate loop — repeated identical thinking, turns burned with no new output.
    Looping,
    /// A produced file does not compile (syntax error, bad characters, broken import).
    BrokenCode,
    /// The work is drifting from the subtask spec.
    SpecDrift,
    /// The task is too big/slow for ONE worker — split it into smaller file-partitioned subtasks. This is
    /// an ACTION on healthy-but-too-large work (carries `JudgeOutcome.proposed_split`), not a worker-in-
    /// trouble signal like the others.
    Split,
}

impl Verdict {
    pub fn as_str(&self) -> &'static str {
        match self {
            Verdict::Ok => "ok",
            Verdict::OverReading => "over_reading",
            Verdict::Looping => "looping",
            Verdict::BrokenCode => "broken_code",
            Verdict::SpecDrift => "spec_drift",
            Verdict::Split => "split",
        }
    }

    /// Whether this verdict means the worker is in trouble (anything but `Ok`).
    pub fn is_problem(&self) -> bool {
        !matches!(self, Verdict::Ok)
    }
}

/// Everything the judge sees about one in-flight worker. The caller (goose-cli) gathers this from the
/// worker's owned files on disk and its live session before invoking the judge; the deterministic
/// pre-checks (`compile_errors`, `reads`/`writes`) are filled in here so both the deterministic verdict
/// and the LLM judge work from the same evidence.
#[derive(Clone)]
pub struct JudgeInput {
    pub task_id: TaskId,
    /// The subtask spec the worker is meant to satisfy.
    pub description: String,
    /// The exact files this worker is meant to produce.
    pub owned_files: Vec<String>,
    /// (path, contents) for each owned file that exists on disk so far; missing files are omitted.
    pub file_contents: Vec<(String, String)>,
    /// (path, error) for each owned file that exists but fails a syntax/compile check.
    pub compile_errors: Vec<(String, String)>,
    /// Seconds since this attempt was dispatched.
    pub elapsed_secs: u64,
    /// True once at least one owned file exists and is non-empty — the worker has produced something.
    pub any_owned_written: bool,
    /// Seconds since the most-recently-modified owned file changed; `None` if nothing is written yet. A
    /// large value (or `None`) on an old attempt means the worker is reading/looping, not producing —
    /// over-reading and a think-loop both surface here as a lack of output.
    pub secs_since_last_write: Option<u64>,
    /// How many tool calls (actions) the live worker has taken so far this attempt, if known. A
    /// behavioral progress signal independent of wall-clock: a worker that has taken MANY actions while
    /// writing NOTHING is thrashing (exploring/re-reading), and that is catchable far sooner than the
    /// elapsed-time fallback. `None` when no activity heartbeat is available (then only time-based checks
    /// apply).
    pub worker_tool_calls: Option<u32>,
    /// How many characters of REASONING the live worker has streamed this attempt, if known.
    ///
    /// This exists because every OTHER signal here is blind to a reasoning model. Qwen3.6 via LM Studio
    /// streams its deliberation as thinking content, which is neither a tool call nor text: `worker_tool_calls`
    /// stays 0, `any_owned_written` stays false, and nothing distinguishes a worker mid-generation from a
    /// hung one — while the idle watchdog, which sees the raw event flow, is reset by every chunk and so
    /// never fires either. A NON-ZERO value here is positive proof the worker is producing.
    /// `None` when no heartbeat is available, or on a digest written before this key existed.
    pub worker_thinking_chars: Option<u64>,
    /// How many times THIS task has already been split. Splitting is capped (once) so a task can never be
    /// recursively shattered; a task that has already been split is never split again.
    pub split_count: u32,
}

/// One child subtask proposed when the judge SPLITS a too-big task. It owns a DISJOINT SUBSET of the
/// original task's files; the union of all children covers the original's files. `depends_on` references
/// sibling child ids only (the children inherit the original task's external dependencies).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChildSpec {
    pub id: String,
    pub files: Vec<String>,
    #[serde(default)]
    pub depends_on: Vec<String>,
}

/// A judge's conclusion about one worker.
#[derive(Clone, Debug)]
pub struct JudgeOutcome {
    pub verdict: Verdict,
    /// 0.0–1.0. Intervention requires a high bar because the judge is itself a weak model.
    pub confidence: f32,
    /// A one-line corrective hint, prepended to the task on re-dispatch.
    pub hint: String,
    /// Set only when `verdict == Split`: the child subtasks that partition the too-big task's files.
    pub proposed_split: Option<Vec<ChildSpec>>,
    /// PROVENANCE. True only for a verdict produced by `deterministic_verdict` — a real engine fact
    /// (a compile error, an owned file never written, a measured char/tool count). False for anything the
    /// JUDGE MODEL authored.
    ///
    /// This exists because `confidence` cannot carry that distinction: the model produces its own confidence,
    /// so gating an irreversible action on `confidence >= threshold` lets a model opinion decide it. MEASURED:
    /// nf-ts-cadence's integrate-verify went `over_reading -> re_dispatch, re_dispatch, FAILED` at confidence
    /// 0.90 from the LLM path, and because integrate-verify depends on every verify::<M> under fan-verify, one
    /// model opinion took the whole run's verdict red. The standing rule is that only a DETERMINISTIC engine
    /// event may create or kill a verdict; `terminal` now requires this flag.
    pub deterministic: bool,
}

impl JudgeOutcome {
    pub fn ok() -> Self {
        Self {
            verdict: Verdict::Ok,
            confidence: 1.0,
            hint: String::new(),
            proposed_split: None,
            deterministic: false,
        }
    }

    /// A SPLIT conclusion: replace the too-big task with these file-partitioned children.
    pub fn split(children: Vec<ChildSpec>) -> Self {
        Self {
            verdict: Verdict::Split,
            confidence: 0.9,
            hint: String::new(),
            proposed_split: Some(children),
            deterministic: false,
        }
    }
}

/// Tunables for when the judge runs and when its verdict is allowed to kill a worker.
#[derive(Clone, Copy, Debug)]
pub struct JudgeConfig {
    /// Don't judge a worker until it has been running at least this long (let it get started).
    pub min_age_secs: u64,
    /// Minimum confidence for an LLM verdict to trigger a kill + re-dispatch.
    pub intervene_confidence: f32,
    /// Cap on kill+re-dispatch interventions per task, so the judge can never loop a task forever.
    pub max_interventions_per_task: u32,
    /// Minimum seconds between RE-judging the SAME in-flight task. The judge tick is ~15s; without this an
    /// OK long worker would be re-judged every tick (wasted calls queued on a busy node while another
    /// idled). 60s = at most ~1 re-judge/min/task, still catches a worker that goes bad (the idle-based
    /// worker_timeout is the hard-stall backstop). The FIRST judge is gated only by `min_age_secs`.
    pub rejudge_cooldown_secs: u64,
    /// Behavioral over-read trip: this many tool calls (actions) with NO owned file written means the
    /// worker is thrashing, regardless of the clock. A healthy worker — even a slow one composing a big
    /// file — writes within a handful of actions; this catches the "many actions, zero output" worker
    /// minutes before the elapsed-time fallback would.
    pub over_read_tool_calls: u32,
    /// A task that has exhausted its re-dispatch cap AND is still flagged is terminal-failed (not left to
    /// spin a node to worker_max_turns) — but only once its final attempt has run at least this long, so
    /// a momentary flag on a just-started final attempt can't cut a task that might still be getting going.
    pub terminal_min_secs: u64,
    /// A PRODUCTIVE task (writing, not over-reading) that has run at least this long is a candidate to be
    /// SPLIT into smaller file-partitioned subtasks instead of being left to crawl. 0 disables splitting.
    pub split_threshold_secs: u64,
    /// Master gate for task-splitting (M3): when false, `is_split_candidate` never fires and the judge
    /// never proposes a Verdict::Split, regardless of the threshold. Default false until proven live.
    pub split_enabled: bool,
    /// #134 REASONING-SPIRAL trip: a worker that has emitted MORE than this many thinking chars with ZERO
    /// tool calls and NO owned file is stuck in a reasoning spiral (measured: cli-entry streamed 20 799
    /// thinking chars then the stream died mid-token, never writing main.go). Catch it EARLY (at ~this many
    /// chars, ~60-120s) with the forceful "write the simplest version NOW" nudge instead of burning the full
    /// idle-watchdog window. 0 = OFF (byte-identical). GOOSE_SWARM_SPIRAL_THINKING_CHARS overrides.
    pub spiral_thinking_chars: u64,
}

impl Default for JudgeConfig {
    fn default() -> Self {
        Self {
            min_age_secs: 90,
            intervene_confidence: 0.8,
            // Allow the judge to help a struggling task more than once: a hard task often needs a
            // second round of "simplify your approach" guidance before it lands. Total work is still
            // bounded by max_attempts; the 420s thresholds prevent rapid re-killing.
            max_interventions_per_task: 2,
            rejudge_cooldown_secs: 60,
            over_read_tool_calls: 16,
            terminal_min_secs: 90,
            split_threshold_secs: 900,
            // Off by default: task-splitting (M3) stays dark until a live run proves the DAG mutation +
            // LLM partition safe end-to-end (M4). The scheduler logic + detection are in place and tested;
            // this is the single master switch that lets a real run produce a Verdict::Split.
            split_enabled: false,
            spiral_thinking_chars: 0,
        }
    }
}

/// Is this owned file a CODE deliverable — the kind whose absence makes "wrote nothing" a defect?
/// Docs, manifests and lockfiles are legitimate for a task to own and legitimate not to have written
/// yet, so they must never arm the over-read trip.
fn is_code_deliverable(path: &str) -> bool {
    let p = path.rsplit('/').next().unwrap_or(path).to_ascii_lowercase();
    const CODE: [&str; 10] = [
        ".py", ".rs", ".ts", ".tsx", ".js", ".jsx", ".go", ".java", ".rb", ".c",
    ];
    CODE.iter().any(|e| p.ends_with(e))
}

/// Deterministic SPLIT detection: a task is a split candidate when it has been PRODUCING (an owned file is
/// written and the worker is NOT over-reading) for at least `split_threshold_secs`, owns >= 2 files (so it
/// can actually be partitioned), and has not been split before. This is deliberately distinct from the
/// over-read/looping paths — a thrashing worker is re-dispatched, not split; splitting is for work that is
/// genuinely TOO BIG for one worker, not misbehaving.
pub fn is_split_candidate(input: &JudgeInput, cfg: &JudgeConfig) -> bool {
    cfg.split_enabled
        && cfg.split_threshold_secs > 0
        && input.elapsed_secs >= cfg.split_threshold_secs
        && input.any_owned_written
        && input.owned_files.len() >= 2
        && input.split_count == 0
        && input
            .worker_tool_calls
            .is_none_or(|n| n < cfg.over_read_tool_calls)
}

/// What the scheduler hands the judge: which in-flight worker to inspect and the spec it is meant to
/// satisfy. The implementation gathers the rest itself — files on disk, compile status, live session
/// activity — since that is IO the model-agnostic core does not perform.
pub struct JudgeRequest {
    pub task_id: TaskId,
    pub description: String,
    pub owned_files: Vec<String>,
    pub elapsed_secs: u64,
    /// The LM Link model id of a currently-idle device — the judge runs here so it never contends with
    /// the busy workers it is supervising.
    pub judge_model_id: String,
    /// The overall run goal — so the semantic judge can ask "does this worker's output move the GOAL
    /// forward?" rather than reviewing one file in a vacuum.
    pub goal: String,
    /// High-level state of the rest of the run: completed tasks with a brief of what each produced, the
    /// tasks still in flight / pending, and the tasks that have failed. Lets the judge see the big picture
    /// — spot a worker re-doing finished work, depending on something that failed, or drifting from a
    /// shape the rest of the run already established.
    pub done: Vec<(TaskId, String)>,
    pub remaining: Vec<TaskId>,
    pub failed: Vec<TaskId>,
    /// How many times THIS task has already been split (its split generation). The judge caps splitting at
    /// once, so a child born from a split (split_count >= 1) is never split again — preventing runaway
    /// shattering. The scheduler tracks this and the goose-cli judge feeds it into `is_split_candidate`.
    pub split_count: u32,
}

/// Inspects one in-flight worker and returns a verdict. Implemented in goose-cli by gathering evidence
/// (see [`JudgeInput`] / [`deterministic_verdict`]) and running an LLM on the idle device for semantic
/// review. The model-agnostic scheduler only calls this and acts on the [`JudgeOutcome`].
#[async_trait]
pub trait Judge: Send + Sync {
    async fn judge(&self, req: JudgeRequest) -> JudgeOutcome;
}

/// What the scheduler hands an idle-node PRE-REVIEWER (M5): a COMPLETED task to correctness-check while a
/// node would otherwise idle. The implementation runs the task's tests + exercises its primary feature on
/// a golden input and persists findings where integrate-verify will read them.
pub struct PreReviewRequest {
    pub task_id: TaskId,
    pub description: String,
    pub owned_files: Vec<String>,
    pub goal: String,
    /// LM Link model id of the idle device to run the review on.
    pub reviewer_model_id: String,
}

/// Outcome of a pre-review. Findings are persisted by the implementation (for integrate-verify to consume);
/// `had_findings` lets the scheduler log whether a defect was flagged.
pub struct PreReviewOutput {
    pub had_findings: bool,
    pub summary: String,
}

/// Runs on an idle node when NO in-flight worker needs judging: correctness-checks a COMPLETED task's
/// output — the deepest review finding is that passing tests hide a wrong default-path, so this exercises
/// the REAL feature — and records findings for integrate-verify. Opt-in like the judge (off by default).
#[async_trait]
pub trait PreReviewer: Send + Sync {
    async fn pre_review(&self, req: PreReviewRequest) -> PreReviewOutput;

    /// SINK IDLE-FILL (GOOSE_SWARM_SINK_REVIEW): while the integrate-verify SINK runs SOLO and pre-review is
    /// exhausted, an otherwise-idle node runs a READ-ONLY whole-tree correctness review along ONE dimension
    /// (by rotating index) and ACCUMULATES any finding inside the dispatcher for run_swarm to drain +
    /// re-verify against the FINAL tree after the sink. Read-only (no tools) so it never races the sink's
    /// writes; a stale/torn read just yields a finding the post-sink re-verify refutes. Default no-op.
    async fn idle_dimension_review(&self, _model_id: &str, _goal: &str, _dim_index: usize) {}
}

/// A verdict derivable from cheap, unambiguous signals alone — no model required. The scheduler trusts
/// this even without (or before) the LLM judge: code that won't compile and a worker that has read a
/// lot while writing nothing are not judgment calls.
pub fn deterministic_verdict(input: &JudgeInput, cfg: &JudgeConfig) -> Option<JudgeOutcome> {
    if let Some((path, err)) = input.compile_errors.first() {
        let snippet: String = err.lines().take(3).collect::<Vec<_>>().join(" ");
        return Some(JudgeOutcome {
            verdict: Verdict::BrokenCode,
            confidence: 1.0,
            hint: format!(
                "{path} does not compile ({snippet}). Fix the syntax so it parses and imports cleanly \
                 — if you are unsure how, write a SMALLER, SIMPLER version that compiles and covers the \
                 core of the spec; a working subset beats a broken whole."
            ),
            proposed_split: None,
            deterministic: true,
        });
    }
    // Behavioral over-read: the worker has TAKEN many actions (tool calls) yet produced no owned file.
    // This is thrashing — exploring/listing/re-reading instead of writing — and it is visible from the
    // worker's ACTIVITY long before the elapsed-time fallback below would trip. A healthy worker, even a
    // slow one composing a large file, makes only a handful of tool calls before its file appears; a
    // worker on its Nth action with nothing written is over-reading and should be redirected NOW, not in
    // several more minutes. A small min-age guard keeps a fast startup burst from being misread.
    // The over-read heuristic only makes sense for a worker that OWNS files it should be writing. A task
    // that owns NO files (the integrate-verify sink, a pure verifier) legitimately reads the whole program
    // and RUNS it without ever writing an owned file, so `!any_owned_written` is permanently true for it —
    // applying this gate GUARANTEES it is killed for over_reading once it makes a few tool calls (the
    // observed false-negative: integrate-verify judge_killed x3 -> run reported FAILED though the app works).
    // No-owned tasks are bounded by the idle-based worker_timeout instead; exempt them here.
    if !input.owned_files.is_empty()
        && !input.any_owned_written
        && input.elapsed_secs >= cfg.min_age_secs
        && input
            .worker_tool_calls
            .is_some_and(|n| n >= cfg.over_read_tool_calls)
    {
        return Some(JudgeOutcome {
            verdict: Verdict::OverReading,
            confidence: 0.9,
            hint: "You have taken many actions but written no file yet — you are exploring/re-reading \
                   instead of producing. STOP investigating: you already have the spec, the file layout, \
                   and the injected dependency APIs. WRITE your owned file(s) NOW — the SIMPLEST version \
                   that satisfies the spec first (a small working file), then refine. A minimal working \
                   file beats endless exploration."
                .to_string(),
            proposed_split: None,
            deterministic: true,
        });
    }
    // #134 REASONING-SPIRAL trip (OFF at default cfg.spiral_thinking_chars == 0 → byte-identical): a worker
    // that has emitted a LOT of thinking with ZERO tool calls and NO file is spiralling (cli-entry: 20 799
    // thinking chars, stream then died mid-token, never wrote main.go). Catch it EARLY — at the char cap
    // (~60-120s) — with the forceful "write the simplest version NOW" nudge, instead of burning the whole
    // idle window. Deterministic: a char count creates the verdict, never a model opinion.
    if cfg.spiral_thinking_chars > 0
        && !input.owned_files.is_empty()
        && !input.any_owned_written
        && input.worker_tool_calls == Some(0)
        && input.worker_thinking_chars.unwrap_or(0) >= cfg.spiral_thinking_chars
        && input.elapsed_secs >= cfg.min_age_secs
    {
        return Some(JudgeOutcome {
            verdict: Verdict::OverReading,
            confidence: 0.9,
            hint: "You have written NOTHING and taken no action — you are stuck deliberating (a long \
                   reasoning spiral). You already have the spec, the file layout, and the injected dependency \
                   APIs; there is nothing left to work out. STOP thinking and WRITE your owned file(s) NOW: \
                   the SIMPLEST version that satisfies the spec FIRST (a small working file), then refine it. \
                   A minimal working file beats a plan you never wrote down."
                .to_string(),
            proposed_split: None,
            deterministic: true,
        });
    }
    // Blind fallback: no file on disk and the clock ran out. Unlike the branch above this one has NO
    // evidence term — it fires on a stopwatch alone, so it is the ONLY branch that can ever reach a worker
    // whose tool_calls are 0. Two corrections to that bluntness, both inert at the default config:
    //   * the hint no longer ASSERTS over-reading. A worker with 0 tool calls has read nothing, and
    //     telling it to "stop exploring/re-reading" is a false diagnosis injected as a supervisor note.
    // I ALSO TRIED widening this deadline per owned file and REMOVED it. The deadline is on
    // TIME-TO-FIRST-BYTE, and how many files a task owes in total has nothing to do with when it writes
    // its FIRST one — the scaling was incoherent on its own terms. MEASURED, the run that would have used
    // it: a 4-file task wrote its first file at ~140s and was SPLIT, while the task that died at 457s had
    // made ZERO tool calls. tool_calls is the discriminator, not file count. Widening the clock would only
    // buy a spiralling worker more silence.
    // ARM ONLY ON A CODE DELIVERABLE. "You have read a lot and written nothing" is a defect for a
    // worker whose job is to produce source. It is the JOB DESCRIPTION of `integrate-verify`, which
    // runs the assembled app, reads what it finds, and fixes failures.
    //
    // MEASURED across 13 runs, perfect separation. The sink normally owns nothing and the gate stays
    // disarmed — 7 runs, 1 attempt each, ZERO over-read kills. In 3 runs the planner happened to give
    // it `README.md`, and in two of those the gate armed and killed it repeatedly with the canned
    // hint "You have produced no file yet. STOP reading/deliberating ... WRITE your file(s) NOW" —
    // 2 kills and 3 kills, attempts exhausted, and the 3-kill run is the ONLY sink failure in the
    // corpus. A verification task was told to stop verifying and write a README.
    //
    // A doc or manifest is not the deliverable that makes "wrote nothing" diagnostic, so it must not
    // arm the trip. This is the same class as the worker prompt handing implementer rules to a
    // test-author: a rule written for one kind of task applied to another.
    let owns_code = input.owned_files.iter().any(|f| is_code_deliverable(f));
    if owns_code && !input.any_owned_written && input.elapsed_secs >= cfg.min_age_secs.max(420) {
        let read_nothing = input.worker_tool_calls == Some(0);
        return Some(JudgeOutcome {
            verdict: Verdict::OverReading,
            confidence: 0.9,
            hint: if read_nothing {
                "You have produced no file yet and have taken no action at all — you are deliberating \
                 instead of building. You already have the spec, the file layout, and the injected \
                 dependency APIs; there is nothing left to work out. WRITE your owned file(s) NOW: the \
                 SIMPLEST version that satisfies the spec FIRST (a small working file), then refine it. \
                 A minimal working file beats a perfect plan you never wrote down."
                    .to_string()
            } else {
                "You have produced no file yet. STOP reading/deliberating — you already have the \
                 spec, the file layout, and the injected dependency APIs. WRITE your owned file(s) now. \
                 If the task feels large or hard, write the SIMPLEST version that satisfies the spec \
                 FIRST (a small working file), then refine it — a minimal working file beats endless \
                 planning, and you can always improve it once it exists."
                    .to_string()
            },
            proposed_split: None,
            deterministic: true,
        });
    }
    // Finalize-spin: the worker DID produce its owned file(s) but has not touched them in a long
    // time while still running — it is stuck re-reading or re-verifying instead of reporting done.
    // The over-read check above can't see this (a file exists), so catch it here. Excludes the
    // integrate-verify sink, which legitimately edits OTHER modules' files for a long stretch and so
    // can leave its own file untouched while still doing real work.
    if input.any_owned_written
        && input.task_id != "integrate-verify"
        && input.elapsed_secs >= cfg.min_age_secs.max(420)
        && input.secs_since_last_write.is_some_and(|s| s >= 420)
    {
        return Some(JudgeOutcome {
            verdict: Verdict::Looping,
            confidence: 0.9,
            hint: spin_hint(input),
            proposed_split: None,
            deterministic: true,
        });
    }
    None
}

/// The finalize-spin correction, COMPOSED FROM WHAT THIS WORKER ACTUALLY DID.
///
/// This branch fired **40 times across the archive with one identical sentence** — the single most
/// repeated string the engine produces, sent to forty workers doing forty different jobs. It is the
/// clearest instance of the standing rule that a generic instruction to a node IS the failure, not a
/// rough edge: a supervisor that has read the worker's files, knows their sizes, holds its compile
/// errors and can see how long each has sat untouched, and then says only "your owned file(s) are
/// written but unchanged for minutes".
///
/// Everything below is already in `JudgeInput` at the moment of the kill and was being discarded.
/// The order is deliberate — OBSERVATION first (so the worker can check the claim against reality
/// rather than take it on authority), then the DECISIVE EVIDENCE if any exists, then the principle.
/// A compile error is the most actionable thing a stuck worker can be handed, and the canned text
/// threw it away in favour of "make the SIMPLEST change that works".
///
/// No model call: this is composition from observed state, so it costs nothing and cannot hallucinate.
fn spin_hint(input: &JudgeInput) -> String {
    let mins = |s: u64| format!("{:.1}", s as f64 / 60.0);
    let mut h = String::new();

    // 1. THE OBSERVATION, naming the actual files and the actual sizes on disk.
    let files: Vec<String> = input
        .file_contents
        .iter()
        .map(|(f, c)| format!("`{f}` ({} bytes)", c.len()))
        .collect();
    let idle = input.secs_since_last_write.unwrap_or(0);
    if files.is_empty() {
        h.push_str(&format!(
            "You have been running {} minutes and your owned file(s) have not changed for {} of them.",
            mins(input.elapsed_secs),
            mins(idle)
        ));
    } else {
        h.push_str(&format!(
            "You wrote {} and have not touched {} for {} minutes, while continuing to run for {} \
             minutes in total.",
            files.join(" and "),
            if files.len() > 1 { "them" } else { "it" },
            mins(idle),
            mins(input.elapsed_secs)
        ));
    }

    // 2. THE DECISIVE EVIDENCE. If the engine already knows why the file is not acceptable, that is
    //    the one thing worth saying — and it is specific by construction.
    if let Some((file, err)) = input.compile_errors.first() {
        let e = err.trim();
        let e = if e.chars().count() > 400 {
            format!("{}…", e.chars().take(400).collect::<String>())
        } else {
            e.to_string()
        };
        h.push_str(&format!(
            "\n\nIt does not compile. `{file}` reports:\n{e}\n\nFIX EXACTLY THAT and nothing else, \
             then report done. Do not re-read the project looking for other problems — this is the \
             problem."
        ));
        return h;
    }

    // 3. NO ERROR KNOWN: then the file is plausibly finished and the worker is polishing. Say what
    //    would settle it, in terms of this task's own deliverable, rather than a general exhortation.
    let owned = input
        .owned_files
        .first()
        .map(|f| format!("`{f}`"))
        .unwrap_or_else(|| "your file".to_string());
    h.push_str(&format!(
        "\n\nNothing is reported failing, so {owned} is most likely already done and you are polishing \
         or re-verifying. Re-read your own task statement above: if every deliverable it names is \
         present in the file you already wrote, REPORT DONE NOW — that is the correct end of this \
         task. If one deliverable is genuinely missing, add ONLY that one, in a single edit, then \
         report done."
    ));
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The 40x canned sentence is gone: a spin hint must name THIS worker's files and, when the
    /// engine already knows why the file is unacceptable, must lead with that error instead of a
    /// general exhortation. Two workers in different states must not receive the same text.
    #[test]
    fn a_spin_hint_is_composed_from_what_this_worker_actually_did() {
        let mut a = mk("store", true, Some(500), 900);
        a.file_contents = vec![("store.py".into(), "x = 1\n".repeat(20))];
        let ha = spin_hint(&a);
        assert!(ha.contains("store.py"), "must name the file: {ha}");
        assert!(ha.contains("8.3"), "must state the real idle minutes: {ha}");
        assert!(
            ha.contains("REPORT DONE NOW"),
            "no error known => settle it: {ha}"
        );

        // Same shape, but the engine HOLDS a compile error — that must lead, and must be quoted.
        let mut b = mk("api", true, Some(600), 1000);
        b.file_contents = vec![("api.py".into(), "def f(:\n".into())];
        b.compile_errors = vec![(
            "api.py".into(),
            "SyntaxError: invalid syntax (line 1)".into(),
        )];
        let hb = spin_hint(&b);
        assert!(
            hb.contains("SyntaxError: invalid syntax"),
            "must quote the real error: {hb}"
        );
        assert!(hb.contains("FIX EXACTLY THAT"), "must point at it: {hb}");
        assert!(
            !hb.contains("REPORT DONE NOW"),
            "an uncompilable file is not done: {hb}"
        );

        assert_ne!(
            ha, hb,
            "two workers in different states must not get the same text"
        );
    }

    fn mk(task_id: &str, written: bool, last_write: Option<u64>, elapsed: u64) -> JudgeInput {
        JudgeInput {
            task_id: task_id.to_string(),
            description: "spec".to_string(),
            owned_files: vec!["m.py".to_string()],
            file_contents: if written {
                vec![("m.py".to_string(), "x = 1\n".to_string())]
            } else {
                vec![]
            },
            compile_errors: vec![],
            elapsed_secs: elapsed,
            any_owned_written: written,
            secs_since_last_write: last_write,
            worker_tool_calls: None,
            worker_thinking_chars: None,
            split_count: 0,
        }
    }

    /// The exact shape that got `api-app` killed three times: N owned files, nothing on disk yet, and a
    /// worker that has made ZERO tool calls because it is a reasoning model streaming thinking.
    fn mk_no_write(owned: usize, elapsed: u64, tool_calls: u32) -> JudgeInput {
        JudgeInput {
            owned_files: (0..owned).map(|i| format!("f{i}.py")).collect(),
            worker_tool_calls: Some(tool_calls),
            ..mk("api-app", false, None, elapsed)
        }
    }

    /// REGRESSION — the measured kill. baseline3's `api-app` owned 4 files, had made 0 tool calls (a
    /// reasoning model streams thinking, which the digest could not see), and was killed at 457s / 450s /
    /// 430s across all three attempts. At the default config the flat 420s deadline still fires, exactly
    /// as it did — this pins today's behaviour so the grace lever's effect is visible as a DIFF.
    /// THE SINK WAS KILLED FOR DOING ITS JOB. `integrate-verify` runs the assembled app and fixes
    /// failures; it writes no source. MEASURED across 13 runs: when it owned nothing the over-read
    /// gate stayed disarmed (7 runs, 1 attempt, zero kills), and in the 3 runs where the planner gave
    /// it `README.md` the gate armed — 2 and 3 kills, attempts exhausted, and the 3-kill run is the
    /// only sink failure in the corpus. A doc is not the deliverable that makes "wrote nothing"
    /// diagnostic.
    #[test]
    fn over_read_does_not_arm_on_a_doc_only_task() {
        assert!(!is_code_deliverable("README.md"));
        assert!(!is_code_deliverable("pyproject.toml"));
        assert!(!is_code_deliverable("go.mod"));
        assert!(is_code_deliverable("vendorsync/meridian.py"));
        assert!(is_code_deliverable("cmd/app/main.go"));
        assert!(is_code_deliverable("src/lib.rs"));
    }

    #[test]
    fn blind_deadline_kills_a_zero_tool_call_worker_at_420s_by_default() {
        let cfg = JudgeConfig::default();
        let out =
            deterministic_verdict(&mk_no_write(4, 457, 0), &cfg).expect("killed, as measured");
        assert_eq!(out.verdict, Verdict::OverReading);
        // ...and the hint must NOT accuse a worker that has read nothing of re-reading.
        assert!(
            out.hint.contains("taken no action at all"),
            "a 0-tool-call worker must not be told it is exploring/re-reading: {}",
            out.hint
        );
    }

    /// The behavioural branch owns the case where the worker HAS acted — its hint is the over-read one.
    #[test]
    fn behavioural_branch_still_owns_the_thrashing_worker() {
        let cfg = JudgeConfig::default();
        let out =
            deterministic_verdict(&mk_no_write(1, 300, 16), &cfg).expect("thrashing is caught");
        assert_eq!(out.verdict, Verdict::OverReading);
        assert!(out.hint.contains("taken many actions"), "{}", out.hint);
    }

    #[test]
    fn finalize_spin_fires_when_owned_file_goes_stale() {
        let v = deterministic_verdict(
            &mk("scan-module", true, Some(500), 700),
            &JudgeConfig::default(),
        );
        assert_eq!(v.map(|o| o.verdict), Some(Verdict::Looping));
    }

    #[test]
    fn finalize_spin_excludes_integrate_verify() {
        let v = deterministic_verdict(
            &mk("integrate-verify", true, Some(500), 700),
            &JudgeConfig::default(),
        );
        assert!(
            v.is_none(),
            "the verify sink edits other files; must not be finalize-spin-killed"
        );
    }

    #[test]
    fn finalize_spin_quiet_when_recently_written() {
        let v = deterministic_verdict(
            &mk("scan-module", true, Some(60), 700),
            &JudgeConfig::default(),
        );
        assert!(
            v.is_none(),
            "a worker that wrote recently is making progress"
        );
    }

    #[test]
    fn over_read_fires_with_no_output_on_old_attempt() {
        let v = deterministic_verdict(
            &mk("scan-module", false, None, 500),
            &JudgeConfig::default(),
        );
        assert_eq!(v.map(|o| o.verdict), Some(Verdict::OverReading));
    }

    #[test]
    fn healthy_young_worker_is_quiet() {
        let v = deterministic_verdict(
            &mk("scan-module", true, Some(30), 100),
            &JudgeConfig::default(),
        );
        assert!(v.is_none());
    }

    #[test]
    fn behavioral_over_read_fires_early_on_many_actions_no_output() {
        // 0 writes + 16 tool calls past min-age → over-read caught at 150s, NOT the 420s fallback.
        let mut i = mk("core-tree", false, None, 150);
        i.worker_tool_calls = Some(16);
        let v = deterministic_verdict(&i, &JudgeConfig::default());
        assert_eq!(v.map(|o| o.verdict), Some(Verdict::OverReading));
    }

    #[test]
    fn over_read_exempts_no_owned_task() {
        // integrate-verify (and any pure verifier sink) owns NO files, so `!any_owned_written` is permanently
        // true and it legitimately reads the whole program + RUNS it. It must NOT be over-read-killed even
        // with many tool calls on an old attempt (the observed false-negative: integrate-verify judge_killed
        // x3 -> run reported FAILED though the app worked). The idle-based worker_timeout bounds it instead.
        let mut i = mk("integrate-verify", false, None, 500);
        i.owned_files = vec![];
        i.worker_tool_calls = Some(40);
        assert!(
            deterministic_verdict(&i, &JudgeConfig::default()).is_none(),
            "a no-owned verifier task must not be over-read-killed"
        );
    }

    #[test]
    fn behavioral_over_read_quiet_while_writing() {
        // A worker that HAS written its file is making progress — many tool calls are fine, no kill.
        let mut i = mk("core-tree", true, Some(20), 300);
        i.worker_tool_calls = Some(40);
        assert!(deterministic_verdict(&i, &JudgeConfig::default()).is_none());
    }

    #[test]
    fn behavioral_over_read_quiet_for_slow_single_generation() {
        // Slow but healthy: few tool calls (one long generation), no file yet, under the time fallback.
        let mut i = mk("core-tree", false, None, 200);
        i.worker_tool_calls = Some(4);
        assert!(deterministic_verdict(&i, &JudgeConfig::default()).is_none());
    }

    #[test]
    fn split_candidate_only_for_big_productive_multifile_tasks() {
        // split_threshold_secs = 900; enable the master gate so the detector can fire in this unit test.
        let cfg = JudgeConfig {
            split_enabled: true,
            ..JudgeConfig::default()
        };
        // Long, producing, owns 2 files, not over-reading, never split -> SPLIT candidate.
        let mut big = mk("core", true, Some(10), 1000);
        big.owned_files = vec!["a.py".to_string(), "b.py".to_string()];
        big.worker_tool_calls = Some(5);
        assert!(is_split_candidate(&big, &cfg));
        // Same but only ONE owned file -> not splittable.
        let one = mk("core", true, Some(10), 1000); // owned_files = ["m.py"]
        assert!(!is_split_candidate(&one, &cfg));
        // Long + multi-file but OVER-READING (thrashing) -> re-dispatch path, not split.
        let mut thrash = big.clone();
        thrash.worker_tool_calls = Some(cfg.over_read_tool_calls + 1);
        assert!(!is_split_candidate(&thrash, &cfg));
        // Already split once -> never split again.
        let mut again = big.clone();
        again.split_count = 1;
        assert!(!is_split_candidate(&again, &cfg));
        // Not yet old enough -> not a candidate.
        let mut young = big.clone();
        young.elapsed_secs = 300;
        assert!(!is_split_candidate(&young, &cfg));
    }
}

#[cfg(test)]
mod provenance_tests {
    use super::*;

    /// The standing rule: only a DETERMINISTIC engine event may create or kill a verdict. This locks the
    /// PROVENANCE half of it — every verdict `deterministic_verdict` can emit must be marked deterministic,
    /// and everything the judge MODEL authors must not be. scheduler.rs's `terminal` reads this flag, so if
    /// a future branch forgets it, a model opinion silently regains the power to fail a task and a run.
    #[test]
    fn model_authored_outcomes_are_never_marked_deterministic() {
        // The convenience constructors are model/neutral paths — never deterministic.
        assert!(!JudgeOutcome::ok().deterministic);
        assert!(
            !JudgeOutcome::split(vec![]).deterministic,
            "a SPLIT proposal comes from the judge model — it must not be marked deterministic"
        );
    }

    /// A deterministic BrokenCode verdict (a real compile error) must be marked deterministic, otherwise the
    /// terminal-fail path can never fire for a genuinely broken task and a doomed task spins to its timeout.
    #[test]
    fn a_real_compile_error_is_marked_deterministic() {
        // JudgeInput has no Default — construct it fully so a new field forces this test to be revisited.
        let input = JudgeInput {
            task_id: "core".to_string(),
            description: String::new(),
            owned_files: vec!["a.py".to_string()],
            file_contents: vec![("a.py".to_string(), "def f(:".to_string())],
            compile_errors: vec![("a.py".to_string(), "SyntaxError: bad".to_string())],
            elapsed_secs: 600,
            any_owned_written: true,
            secs_since_last_write: Some(10),
            worker_tool_calls: Some(3),
            worker_thinking_chars: Some(100),
            split_count: 0,
        };
        let cfg = JudgeConfig::default();
        let out =
            deterministic_verdict(&input, &cfg).expect("a compile error must produce a verdict");
        assert_eq!(out.verdict, Verdict::BrokenCode);
        assert!(
            out.deterministic,
            "a compile error is an ENGINE FACT — it must be marked deterministic or it can never fail a task"
        );
    }
}
