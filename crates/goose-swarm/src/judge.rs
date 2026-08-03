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
    /// The worker has written none of its owned files AND has taken no action at all — it is stuck
    /// before its first byte, not over-reading. Distinct from `OverReading` because the remedies differ
    /// and because a log that calls a zero-tool-call worker "over_reading" misdirects every later reader.
    NoFirstWrite,
    /// The deliverable is DONE: every owned file exists and none fails its syntax/compile check, but the
    /// worker is still running. Finish it rather than spending an attempt on a kill.
    ///
    /// MEASURED (F165): `test-meridian` was recorded a TERMINAL FAILURE with its file on disk carrying 8
    /// test functions and 12 assertions, all passing — 8 of the 72 tests the crunched app passes. The
    /// engine's own hint said so before killing it: "Nothing is reported failing, so the file is most
    /// likely already done and you are polishing." Every other verdict is a way to STOP a worker; without
    /// this one the judge's only lever is kill, and the third kill is terminal.
    Accept,
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
            Verdict::Accept => "accept",
            Verdict::NoFirstWrite => "no_first_write",
        }
    }

    /// Whether this verdict means the worker is in trouble. `Accept` is a COMPLETION, not trouble — it
    /// must never reach the intervention path, or the verdict that exists to stop a task being failed
    /// would itself count toward failing it.
    pub fn is_problem(&self) -> bool {
        !matches!(self, Verdict::Ok | Verdict::Accept)
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
    /// `worker_thinking_chars` AS OF THE PREVIOUS JUDGE OBSERVATION of this same attempt, if there
    /// was one. `None` on the first look.
    ///
    /// A single snapshot cannot express "still producing". MEASURED (F143) on `test-meridian`: its
    /// tool_calls sat at 3, unchanged, while reasoning climbed 5,818 -> 8,784 characters across three
    /// consecutive observations — and it was killed for "re-reading or re-verifying", a diagnosis the
    /// engine's own numbers refuted. The pair (previous, current) is the smallest thing that can tell
    /// a worker mid-generation from a worker that has stopped.
    pub prev_thinking_chars: Option<u64>,
    /// `worker_tool_calls` AS OF THE PREVIOUS JUDGE OBSERVATION of this same attempt, if there was one.
    /// `None` on the first look. This is the ACTION counterpart to `prev_thinking_chars` and the input
    /// `is_still_producing` keys on: reasoning that grows proves only that the model is talking.
    pub prev_tool_calls: Option<u32>,
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
    // EVIDENCE TERM ON THE DEADLINE. This branch used to be a pure stopwatch: it killed at 420s without
    // ever asking whether the worker was progressing. MEASURED (F201), time-to-first-owned-write across
    // the corpus is p90 475s for implementers and p90 831s (max 1099s) for test-authors — the constant
    // sat BELOW the p90 of BOTH populations it judged, and all 11 trips fired in 420-485s. A worker that
    // is still PRODUCING therefore gets double the budget, while one that has gone quiet dies on the
    // original schedule. Combined with `is_still_producing` keying on ACTIONS rather than reasoning, a
    // spiral (thinking climbs, tool calls flat) is NOT producing and still dies at 420s — which is the
    // case this branch exists for.
    let deadline = cfg.min_age_secs.max(420) * if is_still_producing(input) { 2 } else { 1 };
    if owns_code && !input.any_owned_written && input.elapsed_secs >= deadline {
        let read_nothing = input.worker_tool_calls == Some(0);
        return Some(JudgeOutcome {
            // HONEST LABEL. `read_nothing` is computed on the line above and the hint already branches on
            // it, but the verdict was stamped `OverReading` either way — so the run log recorded
            // "over_reading" about workers whose tool-call count was ZERO (9 of 11 measured). That label
            // is the primary key of every downstream analysis and it produced three false causal chains
            // in this campaign before anyone checked the counter beside it.
            verdict: if read_nothing {
                Verdict::NoFirstWrite
            } else {
                Verdict::OverReading
            },
            confidence: 0.9,
            hint: no_file_hint(input, read_nothing),
            proposed_split: None,
            deterministic: true,
        });
    }
    // Finalize-spin: the worker DID produce its owned file(s) but has not touched them in a long
    // time while still running — it is stuck re-reading or re-verifying instead of reporting done.
    // The over-read check above can't see this (a file exists), so catch it here. Excludes the
    // integrate-verify sink, which legitimately edits OTHER modules' files for a long stretch and so
    // can leave its own file untouched while still doing real work.
    // ACCEPT before Looping. A worker that has stopped touching files is only "spinning" if the work is
    // UNFINISHED; if every owned file exists and none fails its compile check, it is finished and the
    // honest verdict is DONE. MEASURED (F165): test-meridian was killed three times and recorded a
    // TERMINAL FAILURE while its file sat on disk with 8 passing test functions that the crunched app
    // still runs. The kill path was the judge's only lever, so "looks complete" and "looks stuck" both
    // resolved to kill. This branch is deliberately placed FIRST and gated on the same evidence the
    // spin branch uses, so the only cases it takes are the ones that would otherwise burn an attempt.
    let all_owned_present = !input.owned_files.is_empty()
        && input.owned_files.iter().all(|f| {
            input
                .file_contents
                .iter()
                .any(|(p, c)| p == f && !c.trim().is_empty())
        });
    if all_owned_present
        && input.compile_errors.is_empty()
        && input.task_id != "integrate-verify"
        && input.elapsed_secs >= cfg.min_age_secs.max(420)
        && input.secs_since_last_write.is_some_and(|s| s >= 420)
        && !is_still_producing(input)
    {
        return Some(JudgeOutcome {
            verdict: Verdict::Accept,
            confidence: 1.0,
            hint: format!(
                "All {} owned file(s) exist and pass their syntax check, and nothing has changed for \
                 {}s — the deliverable is complete.",
                input.owned_files.len(),
                input.secs_since_last_write.unwrap_or(0)
            ),
            proposed_split: None,
            deterministic: true,
        });
    }
    if input.any_owned_written
        && input.task_id != "integrate-verify"
        && input.elapsed_secs >= cfg.min_age_secs.max(420)
        && input.secs_since_last_write.is_some_and(|s| s >= 420)
        && !is_still_producing(input)
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

/// The nothing-written correction, COMPOSED — and in particular, it STATES what it observes instead
/// of DIAGNOSING it.
///
/// The two canned variants it replaces fired 18 times between them, and the first one asserted
/// *"you have taken no action at all — you are deliberating instead of building"*. The comment three
/// branches above already warns about exactly this: *"the hint no longer ASSERTS over-reading … that
/// is a false diagnosis injected as a supervisor note."* The same objection applies to asserting
/// deliberation, and F131 measured the population it is aimed at: workers killed here carry a MEDIAN
/// of 1,229 thinking chars (max 4,519). Some really have been reasoning hard; some produced almost
/// nothing (one had 285 chars over 420 s). One sentence cannot be true of both, so it states the
/// counts and lets the worker draw the conclusion.
///
/// It also names THE FILES THIS WORKER OWES. The canned versions said "your owned file(s)" to a
/// worker whose entire problem is not having started — and the engine has the paths right there.
///
/// Context is kept deliberately SHORT: the observed counts, the owed paths, and one next action. A
/// supervisory nudge that arrives as a wall of text is another way to bog a worker down.
fn no_file_hint(input: &JudgeInput, read_nothing: bool) -> String {
    let owed: Vec<String> = input
        .owned_files
        .iter()
        .filter(|f| is_code_deliverable(f))
        .map(|f| format!("`{f}`"))
        .collect();
    let first = owed.first().cloned().unwrap_or_else(|| "your file".into());
    let mins = format!("{:.1}", input.elapsed_secs as f64 / 60.0);

    // THE OBSERVATION — counts, not character judgements.
    let mut h = format!("After {mins} minutes, none of the files you own exists on disk yet");
    match (read_nothing, input.worker_thinking_chars) {
        (true, Some(t)) if t > 0 => h.push_str(&format!(
            ", and you have run no command — you have emitted {t} characters of reasoning instead"
        )),
        (true, _) => h.push_str(", and you have run no command at all"),
        (false, _) => h.push_str(&format!(
            ", though you have run {} command(s)",
            input.worker_tool_calls.unwrap_or(0)
        )),
    }
    h.push('.');

    // THE OWED DELIVERABLE, by name.
    if !owed.is_empty() {
        h.push_str(&format!("\n\nYou owe: {}.", owed.join(", ")));
    }

    // ONE next action, concrete and small enough to be done immediately.
    h.push_str(&format!(
        "\n\nWrite {first} NOW, in one `write`, using the spec and the dependency APIs already in \
         your prompt — there is nothing further to look up. If the task feels too large, write the \
         SIMPLEST version that satisfies the spec and refine it after; a small working file beats a \
         plan you never wrote down. Do not read anything before that first write."
    ));
    h
}

/// Is this worker STILL GENERATING, as opposed to stopped?
///
/// `secs_since_last_write` is a fact about the FILE. It says nothing about whether the WORKER is
/// working, and on a reasoning model those two routinely disagree: the model streams deliberation
/// that is neither a tool call nor a write, so a worker mid-generation looks identical to a dead one
/// through every file-shaped lens.
///
/// MEASURED, and this is why the guard exists (F143). `test-meridian` was killed for "stuck
/// re-reading or re-verifying" at a moment when its tool_calls had been **3, unchanged**, across
/// three consecutive observations while its reasoning climbed **5,818 -> 8,784 characters**. It ran
/// no commands; it could not have been re-reading. It was generating, and it was killed for it —
/// three attempts, then a terminal failure.
///
/// The rule is the DELTA, never the level. A worker that has emitted a great deal of reasoning and
/// then STOPPED is exactly the case the spin trip is for, and suppressing that kill would be wrong —
/// so growth between two observations is required, and a flat count (or a first look, where there is
/// no previous value) leaves the trip armed.
///
/// This does not remove a backstop. `worker_timeout_secs`, the progress watchdog and the #134 spiral
/// trip all still bound a worker that generates forever; this only declines to call a producing
/// worker "stuck".
fn is_still_producing(input: &JudgeInput) -> bool {
    // ACTIONS, not reasoning. Keying this on thinking growth made it permanently true for a spiral, since
    // a spiral's thinking climbs monotonically BY DEFINITION — MEASURED (F191): test-api wrote its file at
    // 408s then ran 595s with ZERO tool calls while thinking went 2,897 -> 22,627, and every trip that
    // could have caught it was blocked by this predicate. A tool call is the only signal that separates a
    // worker doing work from one talking to itself.
    //
    // Verified not to re-introduce F163's false kill (F195): that case had flat thinking AND flat tool
    // calls, and was protected by `any_owned_written == false` in all three flat observations, so this
    // predicate never got a vote there. The change is a strict narrowing.
    match (input.prev_tool_calls, input.worker_tool_calls) {
        (Some(prev), Some(now)) => now > prev,
        _ => false,
    }
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

    /// F143: a worker whose reasoning is CLIMBING is producing, and must not be killed for "stuck
    /// re-reading". The rule is the DELTA, never the level — a worker that emitted a lot and then
    /// STOPPED is exactly what the spin trip is for, so a flat count must still kill.
    #[test]
    fn a_worker_whose_reasoning_is_climbing_is_not_stuck() {
        let cfg = JudgeConfig::default();
        // The measured shape: file written, untouched 500s, tool_calls frozen, reasoning climbing.
        let mut climbing = mk("test-meridian", true, Some(500), 900);
        climbing.worker_tool_calls = Some(3);
        climbing.prev_thinking_chars = Some(5818);
        climbing.worker_thinking_chars = Some(8784);
        assert!(
            deterministic_verdict(&climbing, &cfg).is_none(),
            "a worker mid-generation must not be killed for spinning"
        );

        // FALSIFIER, and it must hold: reasoning FLAT across two looks => genuinely stopped => kill.
        let mut flat = climbing.clone();
        flat.prev_thinking_chars = Some(8784);
        let out = deterministic_verdict(&flat, &cfg).expect("a stopped worker is still killed");
        assert_eq!(out.verdict, Verdict::Looping);

        // No previous observation (first look) leaves the trip ARMED — absence is not proof of life.
        let mut first = climbing.clone();
        first.prev_thinking_chars = None;
        assert!(
            deterministic_verdict(&first, &cfg).is_some(),
            "a first look has no delta and must not suppress the kill"
        );
    }

    /// The two "produced no file yet" variants asserted a MOTIVE ("you are deliberating") that F131
    /// measured to be true of only part of the population — median 1,229 thinking chars, but one
    /// worker at 285 over 420s. A hint must STATE the counts and name the owed paths, and a heavy
    /// thinker must not read the same as a worker that did nothing at all.
    #[test]
    fn a_no_file_hint_states_counts_and_names_the_owed_files() {
        let mut heavy = mk_no_write(2, 900, 0);
        heavy.worker_thinking_chars = Some(4519);
        let hh = no_file_hint(&heavy, true);
        assert!(
            hh.contains("4519 characters of reasoning"),
            "state the real volume: {hh}"
        );
        assert!(hh.contains("f0.py"), "name the owed file: {hh}");
        assert!(
            !hh.contains("deliberating instead of building"),
            "do not assert a motive: {hh}"
        );

        let mut idle = mk_no_write(2, 900, 0);
        idle.worker_thinking_chars = Some(0);
        let hi = no_file_hint(&idle, true);
        assert_ne!(
            hh, hi,
            "a heavy thinker and an idle worker must not get the same text"
        );

        let busy = mk_no_write(2, 900, 12);
        let hb = no_file_hint(&busy, false);
        assert!(
            hb.contains("12 command(s)"),
            "state the real command count: {hb}"
        );
        assert_ne!(
            hb, hh,
            "acted-but-wrote-nothing differs from thought-but-acted-nothing"
        );
    }

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
            prev_thinking_chars: None,
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
        //
        // This assertion used to pin the exact canned sentence ("taken no action at all"), which made
        // it a test of the WORDING rather than of the rule. The wording is now composed per worker
        // (F141/F142), so it asserts the INTENT the comment above always stated: state the zero-command
        // fact, and never accuse this worker of exploring or re-reading — it has done neither.
        let h = &out.hint;
        assert!(
            h.contains("no command"),
            "a 0-tool-call worker must be told what it actually did (nothing): {h}"
        );
        for accusation in ["re-reading", "exploring", "stuck re-reading"] {
            assert!(
                !h.contains(accusation),
                "a 0-tool-call worker must not be accused of {accusation}: {h}"
            );
        }
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
            // Added by F143, and the tripwire did its job: this literal is deliberately exhaustive so
            // a new field cannot be introduced without a human deciding what it means HERE. It means
            // "no previous observation", which leaves every trip armed — the safe default.
            prev_thinking_chars: None,
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
