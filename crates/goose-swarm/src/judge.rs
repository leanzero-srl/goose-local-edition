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
    /// The call is WORKING, but on the wrong thing — it has drifted off the goal. Redirect, never stop.
    Drifting,
    /// The call has produced nothing usable and a fresh session seeded with what it HAS established would
    /// beat continuing. The task is re-run on the SAME device with a new session — never handed to another
    /// node, because every node runs the same model, so moving work costs the session and buys nothing.
    ///
    /// Permitted only while the previous attempt produced SOMETHING (a tool call, a file byte, or new
    /// reasoning). Two consecutive attempts that produce nothing at all end the task instead, with the
    /// judge's notes attached — that is the liveness rule that stops a judge restarting forever.
    Restart,
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

/// F884: is this file still the engine's own UNIMPLEMENTED SKELETON? The dispatcher pre-creates
/// owned files as signature stubs (`def f(...) -> T: ...`) so imports resolve during the fan —
/// which means "the owned file exists and is non-empty" is true at t=0 for every task, before its
/// worker has done anything at all. MEASURED (run 10): the meridian worker ran 585s with ZERO tool
/// calls, and the deterministic Accept read the engine's own 274-byte skeleton as "the deliverable
/// is complete". A file counts as skeleton-only when it declares at least one class/def and every
/// body is `...` / `pass` / `raise NotImplementedError` / a docstring — i.e. the worker added no
/// executable statement to what the engine wrote for it.
pub fn skeleton_only(content: &str) -> bool {
    let mut saw_decl = false;
    let mut doc_quote: Option<&'static str> = None;
    for raw in content.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(q) = doc_quote {
            if line.contains(q) {
                doc_quote = None;
            }
            continue;
        }
        let triple_double = "\"\"\"";
        let triple_single = "'''";
        if let Some(rest) = line
            .strip_prefix(triple_double)
            .or_else(|| line.strip_prefix(triple_single))
        {
            let q: &'static str = if line.starts_with('"') {
                "\"\"\""
            } else {
                "'''"
            };
            if !rest.contains(q) {
                doc_quote = Some(q);
            }
            continue;
        }
        if line.starts_with("import ") || line.starts_with("from ") || line.starts_with('@') {
            continue;
        }
        let is_decl = line.starts_with("class ")
            || line.starts_with("def ")
            || line.starts_with("async def ");
        if is_decl {
            saw_decl = true;
            // A one-liner carries its body after the LAST colon: `def f(x: int) -> list[dict]: ...`.
            let body = line.rsplit(':').next().unwrap_or("").trim();
            if body.is_empty() || body == "..." || body == "pass" {
                continue;
            }
            return false;
        }
        if line == "..." || line == "pass" || line.starts_with("raise NotImplementedError") {
            continue;
        }
        return false;
    }
    saw_decl
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
            Verdict::Drifting => "drifting",
            Verdict::Restart => "restart",
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
    /// Seconds since this attempt was dispatched. DATA ONLY (r3 II-7): recorded into the judge events
    /// for the operator and the ledgers; read by no verdict branch — a wall clock may not decide
    /// model work.
    pub elapsed_secs: u64,
    /// True once at least one owned file exists and is non-empty — the worker has produced something.
    pub any_owned_written: bool,
    /// Seconds since the most-recently-modified owned file changed; `None` if nothing is written yet.
    /// DATA ONLY (r3 II-7): a fact about the FILE, recorded for the events; it says nothing about
    /// whether the WORKER is producing (a reasoning model streams deliberation that touches no file)
    /// and no verdict branch reads it.
    pub secs_since_last_write: Option<u64>,
    /// How many tool calls (actions) the live worker has taken so far this attempt, if known. A
    /// behavioral progress signal independent of wall-clock: a worker that has taken MANY actions while
    /// writing NOTHING is thrashing (exploring/re-reading). `None` when no activity heartbeat is
    /// available — and then no deterministic verdict can fire, because behaviour is the only evidence.
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
    /// `None` on the first look. This is the ACTION counterpart to `prev_thinking_chars`, and the pair
    /// (previous, current) is what `is_split_candidate` reads as "producing across looks": reasoning
    /// that grows proves only that the model is talking.
    pub prev_tool_calls: Option<u32>,
    /// `elapsed_secs` AS OF that previous observation. DATA ONLY (r3 II-7): its presence still marks
    /// "there was a prior look" (see `had_prior_look`), but the seconds value itself is provenance for
    /// the events — the staleness window that used to read it (`is_still_producing`) is deleted with
    /// the deadline it served.
    ///
    /// MEASURED on `swarm-3node-r0`, kept as the WHY for never resurrecting a rate read: the gap
    /// between consecutive judge observations of the same attempt had a median of 60s but a MAX of
    /// 1,267s, because the judge only runs when a device is idle — an observation that old cannot
    /// certify anything about NOW, which is one of the two reasons the wall-clock verdicts are gone.
    pub prev_observed_secs: Option<u64>,
    /// How many times THIS task has already been split. Splitting is capped (once) so a task can never be
    /// recursively shattered; a task that has already been split is never split again.
    pub split_count: u32,
    /// Which attempt of this task is running (0 = first). DATA for the events and the re-dispatch
    /// record; no deadline scales with it any more.
    pub attempt: u32,
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
    /// What this call has ALREADY WORKED OUT that is worth keeping, in the judge's words, drawn from what
    /// the call actually said. This is the point of the whole mechanism: a nudge that throws away the
    /// useful half of a spiralling call is just a slower kill. Empty when nothing was established.
    pub established: String,
    /// The single most concrete next action toward the goal — a file, a command, a function. Never
    /// "continue" or "proceed", which is what the old one-line hint degenerated into.
    pub next_action: String,
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
            established: String::new(),
            next_action: String::new(),
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
            established: String::new(),
            next_action: String::new(),
            proposed_split: Some(children),
            deterministic: false,
        }
    }
}

/// Tunables for when the judge runs and when its verdict is allowed to kill a worker.
///
/// TIME RULE (r3 II-7, the owner's rule): the seconds fields here are SUMMONS CADENCE — they decide
/// when the judge LOOKS (scheduler.rs reads them to pick a candidate), which §II.4 permits. No verdict
/// branch in this module reads seconds any more: every guard that used to be a wall clock is a count
/// of looks or of actions. A clock may summon a look; it may never decide one.
#[derive(Clone, Copy, Debug)]
pub struct JudgeConfig {
    /// SUMMONS CADENCE ONLY: scheduler.rs skips judging a worker younger than this (let it get
    /// started before spending an idle node on it). Read by NO verdict branch — the first-look
    /// misread this used to also guard inside `deterministic_verdict` is guarded by `had_prior_look`
    /// (a look count) instead.
    pub min_age_secs: u64,
    /// Minimum confidence for an LLM verdict to trigger a kill + re-dispatch.
    pub intervene_confidence: f32,
    /// Cap on kill+re-dispatch interventions per task, so the judge can never loop a task forever.
    pub max_interventions_per_task: u32,
    /// SUMMONS CADENCE ONLY: minimum seconds between RE-judging the SAME in-flight task. The judge
    /// tick is ~15s; without this an OK long worker would be re-judged every tick (wasted calls
    /// queued on a busy node while another idled). Bounds when the judge looks, never what it decides.
    pub rejudge_cooldown_secs: u64,
    /// Behavioral over-read trip: this many tool calls (actions) with NO owned file written means the
    /// worker is thrashing, regardless of the clock. A healthy worker — even a slow one composing a big
    /// file — writes within a handful of actions; this catches the "many actions, zero output" worker
    /// from its own behaviour, which is the only evidence that survives a slow model.
    pub over_read_tool_calls: u32,
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
            // bounded by max_attempts; the rejudge cooldown prevents rapid re-killing.
            max_interventions_per_task: 2,
            rejudge_cooldown_secs: 60,
            over_read_tool_calls: 16,
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

/// LOOK-COUNT GUARD (r3 II-7). The verdict branches used to carry `elapsed_secs >= min_age_secs` — a
/// wall clock deciding model work, and one of r2's two proven-wrong firings (three false
/// `over_reading` verdicts on a slot-starved worker that was merely queued behind a sibling). What
/// that guard actually protected against is a FIRST-look misread — a startup burst read as thrashing
/// — and a count of looks states that directly: any `prev_*` field present means the judge has
/// observed this attempt before, so this is at least the second look. Seconds stay in the record as
/// data; they arm nothing.
fn had_prior_look(input: &JudgeInput) -> bool {
    input.prev_tool_calls.is_some()
        || input.prev_thinking_chars.is_some()
        || input.prev_observed_secs.is_some()
}

/// Deterministic SPLIT detection: a task is a split candidate when it is PRODUCING across looks (an
/// owned file is written AND its action count grew between two judge looks — counts, never seconds;
/// r3 II-7 removed the `split_threshold_secs` wall clock this used to key on), owns >= 2 files (so it
/// can actually be partitioned), and has not been split before. This is deliberately distinct from the
/// over-read/looping paths — a thrashing worker is re-dispatched, not split; splitting is for work that
/// is genuinely TOO BIG for one worker, not misbehaving.
pub fn is_split_candidate(input: &JudgeInput, cfg: &JudgeConfig) -> bool {
    cfg.split_enabled
        && matches!(
            (input.prev_tool_calls, input.worker_tool_calls),
            (Some(prev), Some(now)) if now > prev
        )
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
    /// Which attempt of this task is running (0 = first). DATA for the events and the re-dispatch
    /// record; no deadline scales with it any more (r3 II-7).
    pub attempt: u32,
    /// EVERY planned deliverable in the run with what is on disk for it right now — `path
    /// [delivered]`, `[MISSING]`, `[stub]`, `[in progress]`, `[not written yet]` — not just this
    /// worker's own files.
    ///
    /// The judge was handed one task's file list, so it could only ever answer "is this worker doing
    /// its job", never "is this worker building on something that exists". A worker importing from a
    /// dependency that shipped a stub is invisible in the per-task view.
    pub tree_files: Vec<String>,
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

    /// S7 (GOOSE_SWARM_TESTGEN): generate 3-5 pytest functions from the FROZEN CONTRACTS + goal —
    /// never from the code — into a NEW auto-collected file. The dispatcher side owns extraction
    /// and the collect-only landing guard. Default no-op so mocks and thin implementors are
    /// untouched.
    async fn generate_tests(&self, _model_id: &str, _goal: &str, _seq: u32) {}

    /// F790-3 (GOOSE_SWARM_QA): is there an operator question waiting in the run's inbox? Cheap
    /// sync check the tick loop may call every pass. Default false so mocks are untouched.
    fn has_pending_question(&self) -> bool {
        false
    }

    /// F790-3: answer ONE pending operator question on an idle node, with the judge's run-state
    /// perspective supplied by the scheduler. Read-only with respect to the build; the answer
    /// lands in the run's answers outbox + an event. Default no-op.
    async fn answer_user_question(&self, _model_id: &str, _goal: &str, _run_state: &str) {}
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
            established: String::new(),
            next_action: String::new(),
            proposed_split: None,
            deterministic: true,
        });
    }
    // Behavioral over-read: the worker has TAKEN many actions (tool calls) yet produced no owned file.
    // This is thrashing — exploring/listing/re-reading instead of writing — and it is visible from the
    // worker's ACTIVITY alone. A healthy worker, even a slow one composing a large file, makes only a
    // handful of tool calls before its file appears; a worker on its Nth action with nothing written is
    // over-reading and should be redirected NOW. `had_prior_look` (a look count, r3 II-7 — never a
    // clock) keeps a fast startup burst on the judge's first look from being misread.
    // The over-read heuristic only makes sense for a worker that OWNS files it should be writing. A task
    // that owns NO files (the integrate-verify sink, a pure verifier) legitimately reads the whole program
    // and RUNS it without ever writing an owned file, so `!any_owned_written` is permanently true for it —
    // applying this gate GUARANTEES it is killed for over_reading once it makes a few tool calls (the
    // observed false-negative: integrate-verify judge_killed x3 -> run reported FAILED though the app works).
    // ARM ONLY ON A CODE DELIVERABLE, for the same measured reason the old deadline carried this
    // predicate: in the 3 corpus runs where the planner gave the sink `README.md`, "owns a file it has
    // not written" armed a trip against a verification task and killed the only sink failure in the
    // corpus into existence. A doc or manifest is not the deliverable that makes "wrote nothing"
    // diagnostic — without this term the same kill simply recurs through this branch now that the
    // deadline is gone.
    if input.owned_files.iter().any(|f| is_code_deliverable(f))
        && !input.any_owned_written
        && had_prior_look(input)
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
            established: String::new(),
            next_action: String::new(),
            proposed_split: None,
            deterministic: true,
        });
    }
    // #134 REASONING-SPIRAL trip (OFF at default cfg.spiral_thinking_chars == 0 → byte-identical): a worker
    // that has emitted a LOT of thinking with ZERO tool calls and NO file is spiralling (cli-entry: 20 799
    // thinking chars, stream then died mid-token, never wrote main.go). Catch it EARLY — at the char cap —
    // with the forceful "write the simplest version NOW" nudge. Deterministic: a char count creates the
    // verdict, never a model opinion, and `had_prior_look` (looks, not seconds — r3 II-7) is the
    // first-look guard.
    if cfg.spiral_thinking_chars > 0
        && input.owned_files.iter().any(|f| is_code_deliverable(f))
        && !input.any_owned_written
        && input.worker_tool_calls == Some(0)
        && input.worker_thinking_chars.unwrap_or(0) >= cfg.spiral_thinking_chars
        && had_prior_look(input)
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
            established: String::new(),
            next_action: String::new(),
            proposed_split: None,
            deterministic: true,
        });
    }
    // THE BLIND NO-OUTPUT DEADLINE IS DELETED, not retuned (r3 II-7, the owner's rule: nothing
    // time-related may cut, restart, retire or verdict model work — local models are slow and that is
    // expected). This was the 420s-x-attempt stopwatch: `no_output_deadline_secs(min_age_secs.max(420),
    // attempt, is_still_producing(..))`, the ONE branch that fired on a clock with no evidence term.
    // Twice measured wrong, from opposite directions:
    //   * F201: time-to-first-owned-write is p90 475s for implementers, p90 831s (max 1099s) for
    //     test-authors — the constant sat BELOW the p90 of BOTH populations it judged, and all 11
    //     archived trips fired in 420-485s.
    //   * r2: three false `over_reading` verdicts on a slot-starved worker producing zero bytes because
    //     it was QUEUED BEHIND A SIBLING's generation (PARALLEL:2) — stillness that no clock can tell
    //     from death, and each false verdict's text then threaded into the next dispatch as prior_hint.
    // The deterministic no-first-write split routing (F857a) and the composed no-file hint died with the
    // branch, because only this clock ever reached them.
    //
    // What owns the nothing-written case now — all evidence, none of it seconds:
    //   * the behavioural over-read trip above (>= over_read_tool_calls actions, nothing written, look
    //     >= 2): a worker that ACTS without producing is caught from its behaviour;
    //   * the K zero-production-looks counter (r3 II-7): K consecutive judge looks with nothing produced,
    //     each look counted ONLY while `lms ps` reads the worker's node IDLE/absent (GENERATING or
    //     PROCESSINGPROMPT ⇒ hold — the r2 starved-sibling shape, never a verdict). That counter lives
    //     with the dispatcher's look events and is SUMMONS-ONLY until K is derived from r2's healthy
    //     inter-delta gap distribution: an unmeasured K may summon a semantic look, never verdict;
    //   * the semantic judge on an idle node, the recurrence meter (content, not time), and the
    //     operator tick's lms-ps/WEDGED reading for transport-level death.
    //
    // THE FINALIZE-SPIN / ACCEPT / LOOPING TAIL IS GONE TOO, and the history matters enough to keep:
    // three verdicts here once fired off `secs_since_last_write >= 420`, and the scheduler answered an
    // Accept with `h.abort()` plus a DONE record — a wall clock truncating the longest call in the run
    // and writing it into the log as finished. The predicate guarding them counted TOOL CALLS, so a
    // worker sitting inside one long `pytest`, `cargo build` or `npm install` looked "still" while
    // being extremely busy. F165 (test-meridian recorded a TERMINAL FAILURE with 8 passing test
    // functions on disk) is why Accept exists as a VERDICT VARIANT at all — the judge model may still
    // conclude it; no clock may.
    //
    // They were first parked behind `&& false` so the reasoning stayed next to the decision. That was
    // a mistake of its own: clippy reads `if false && ..` as a logic bug, and the lint only surfaced
    // once `cargo fmt` reflowed the constant to the front of the condition — the parked code was one
    // reformat away from breaking the build. Dead code does not get to keep living as a comment
    // attached to an expression; the reasoning is preserved here, and git holds the rest.
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// F143's contract, REWRITTEN because F191 refuted it: CLIMBING REASONING IS NOT PRODUCTION.
    ///
    /// The original test asserted that growing `thinking_chars` protects a worker from the spin trip.
    /// That is exactly what a spiral looks like — a spiral's reasoning grows monotonically BY
    /// DEFINITION. MEASURED (F191): `test-api` wrote its file at 408s then ran 595s with ZERO tool
    /// calls while thinking climbed 2,897 -> 22,627, and every trip that could have caught it was
    /// suppressed by that rule. Production predicates key on ACTIONS, so the case below must never
    /// be PROTECTED by its reasoning volume. Keeping the old assertion would have pinned the defect.
    #[test]
    fn climbing_reasoning_alone_does_not_protect_a_worker() {
        let cfg = JudgeConfig::default();
        // The measured shape: file written, untouched 500s, tool_calls FROZEN, reasoning climbing.
        let mut climbing = mk("test-meridian", true, Some(500), 900);
        climbing.worker_tool_calls = Some(3);
        climbing.prev_tool_calls = Some(3);
        climbing.prev_observed_secs = Some(840);
        climbing.prev_thinking_chars = Some(5818);
        climbing.worker_thinking_chars = Some(8784);
        // Under the OLD contract this returned None — reasoning growth alone bought the worker
        // immunity. It now yields a verdict. That the verdict is `Accept` rather than a kill is the
        // separate F165 correction: the owned file exists and compiles, so the deliverable is DONE and
        // finishing it is right. What matters here is that climbing reasoning no longer BUYS SILENCE.
        // The stopwatch that used to answer this is disarmed (see `stillness_alone_never_ends_a_call`).
        // What must still hold is that frozen actions do not buy SILENCE from the terminal verdicts —
        // and that whatever comes back is not a stop.
        assert!(
            deterministic_verdict(&climbing, &cfg)
                .as_ref()
                .is_none_or(|o| o.verdict != Verdict::Accept && o.verdict != Verdict::Looping),
            "a clock must not terminate a call, however still it looks"
        );
        // The control: the same worker with its ACTION count climbing is genuinely working.
        let mut acting = climbing.clone();
        acting.worker_tool_calls = Some(7);
        assert!(
            deterministic_verdict(&acting, &cfg).is_none(),
            "a worker whose tool-call count is climbing must not be killed"
        );

        // FALSIFIER, and it must hold: a worker that has genuinely STOPPED still gets a verdict. For a
        // finished deliverable that is `Accept` (F165); the KILL path needs a worker with nothing on
        // disk, or this block would only ever be exercising the completion branch.
        let mut flat = climbing.clone();
        flat.prev_thinking_chars = Some(8784);
        assert!(
            deterministic_verdict(&flat, &cfg)
                .as_ref()
                .is_none_or(|o| o.verdict != Verdict::Accept && o.verdict != Verdict::Looping),
            "a worker that merely went quiet must not be terminated by a clock"
        );

        // REWRITTEN WITH ITS SUBJECT (r3 II-7). This block expected a stopped, nothing-written
        // worker to be KILLED — by the 420s-x-attempt deadline, the last clock verdict, now deleted
        // (r2 fired it falsely three times on a slot-starved worker queued behind a PARALLEL:2
        // sibling). Three tool calls is below the behavioural over-read bar, so there is no
        // evidence-based verdict either: the honest deterministic answer is silence, and the case
        // belongs to the K zero-production-looks summons (lms-ps-gated, verdict-less until K is
        // derived from r2) and the semantic judge.
        let mut stopped_unfinished = mk("test-meridian", false, None, 900);
        stopped_unfinished.worker_tool_calls = Some(3);
        stopped_unfinished.prev_tool_calls = Some(3);
        stopped_unfinished.prev_observed_secs = Some(840);
        stopped_unfinished.worker_thinking_chars = Some(8784);
        stopped_unfinished.prev_thinking_chars = Some(8784);
        assert!(
            deterministic_verdict(&stopped_unfinished, &cfg).is_none(),
            "below the behavioural bar there is no evidence, and a clock may not stand in for it"
        );

        // A first look with no delta used to leave the stillness trip ARMED. That trip is disarmed, so
        // what must hold now is the opposite and stronger property: an absent observation cannot
        // manufacture a termination either.
        let mut first = climbing.clone();
        first.prev_thinking_chars = None;
        assert!(
            deterministic_verdict(&first, &cfg)
                .as_ref()
                .is_none_or(|o| o.verdict != Verdict::Accept && o.verdict != Verdict::Looping),
            "absence of a prior observation must not terminate a call"
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
            prev_tool_calls: None,
            prev_observed_secs: None,
            split_count: 0,
            attempt: 0,
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

    /// THE SINK WAS KILLED FOR DOING ITS JOB. `integrate-verify` runs the assembled app and fixes
    /// failures; it writes no source. MEASURED across 13 runs: when it owned nothing the over-read
    /// gate stayed disarmed (7 runs, 1 attempt, zero kills), and in the 3 runs where the planner gave
    /// it `README.md` the gate armed — 2 and 3 kills, attempts exhausted, and the 3-kill run is the
    /// only sink failure in the corpus. A doc is not the deliverable that makes "wrote nothing"
    /// diagnostic. Since r3 II-7 this predicate guards the BEHAVIOURAL over-read trip (the deadline
    /// it used to guard is deleted), so the same kill cannot recur through the surviving branch.
    #[test]
    fn over_read_does_not_arm_on_a_doc_only_task() {
        assert!(!is_code_deliverable("README.md"));
        assert!(!is_code_deliverable("pyproject.toml"));
        assert!(!is_code_deliverable("go.mod"));
        assert!(is_code_deliverable("vendorsync/meridian.py"));
        assert!(is_code_deliverable("cmd/app/main.go"));
        assert!(is_code_deliverable("src/lib.rs"));
        // ...and the live branch respects it: a README-owning sink thrashing past the tool-call bar
        // on its second look still gets NO verdict — verification reads a lot, and that is its job.
        let cfg = JudgeConfig::default();
        let mut sink = mk("integrate-verify", false, None, 900);
        sink.owned_files = vec!["README.md".to_string()];
        sink.worker_tool_calls = Some(40);
        sink.prev_tool_calls = Some(10);
        sink.prev_observed_secs = Some(840);
        assert!(deterministic_verdict(&sink, &cfg).is_none());
    }

    /// INVERTED WITH ITS SUBJECT (r3 II-7). The doc-only half of the old regression stays below;
    /// the kill half pinned the flat 420s deadline killing `api-app`'s zero-tool-call reasoning
    /// worker at 457s — the exact shape r2 then proved wrong three more times on a slot-starved
    /// worker queued behind a PARALLEL:2 sibling. The deadline is deleted, so the property is now
    /// the opposite and holds at every age and look: a worker that has DONE nothing carries no
    /// evidence, and no seconds value may stand in for evidence. Its case belongs to the K
    /// zero-production-looks summons (each look gated on `lms ps` IDLE/absent; verdict-less until K
    /// is derived from r2's inter-delta gaps) and to the semantic judge.
    #[test]
    fn no_clock_ever_kills_a_zero_tool_call_worker() {
        let cfg = JudgeConfig::default();
        for elapsed in [457u64, 4_570, 45_700] {
            let mut i = mk_no_write(4, elapsed, 0);
            // Even well past the first look: a look count licenses READING evidence, not inventing it.
            i.prev_tool_calls = Some(0);
            i.prev_thinking_chars = Some(1_000);
            i.prev_observed_secs = Some(elapsed.saturating_sub(60));
            assert!(
                deterministic_verdict(&i, &cfg).is_none(),
                "a zero-action worker got a verdict at {elapsed}s with no evidence but the clock"
            );
        }
    }

    /// The behavioural branch owns the case where the worker HAS acted — its hint is the over-read one.
    #[test]
    fn behavioural_branch_still_owns_the_thrashing_worker() {
        let cfg = JudgeConfig::default();
        let mut i = mk_no_write(1, 300, 16);
        i.prev_tool_calls = Some(3);
        i.prev_observed_secs = Some(240);
        let out = deterministic_verdict(&i, &cfg).expect("thrashing is caught");
        assert_eq!(out.verdict, Verdict::OverReading);
        assert!(out.hint.contains("taken many actions"), "{}", out.hint);
    }

    /// F884: the VERBATIM file the run-10 accept called "the deliverable is complete" — the
    /// engine's own pre-created signature skeleton, untouched by a worker that made zero tool
    /// calls in 585 seconds.
    #[test]
    fn the_engines_own_skeleton_is_not_a_deliverable() {
        let run10_meridian = "class MeridianClient:\n    def __init__(self, base_url: str, api_key: str) -> None: ...\n    def fetch_all_payments(self) -> list[dict]: ...\n    def total_count(self) -> int: ...\n    def create_payment(self, amount_minor: int, currency: str, idempotency_key: str) -> str: ...\n";
        assert!(skeleton_only(run10_meridian));
        // pass-bodies and NotImplementedError count as stubs too.
        assert!(skeleton_only(
            "class A:\n    def f(self):\n        pass\n    def g(self):\n        raise NotImplementedError\n"
        ));
        // Docstrings do not make a stub real.
        assert!(skeleton_only(
            "def f():\n    \"\"\"Fetch everything.\"\"\"\n    ...\n"
        ));
        // ONE real statement anywhere makes it a deliverable.
        assert!(!skeleton_only(
            "class A:\n    def f(self):\n        return 1\n    def g(self): ...\n"
        ));
        // An import-only shim declares nothing — not "skeleton", just thin (other checks own it).
        assert!(!skeleton_only("from .client import MeridianClient\n"));
        // Empty file: nothing declared, not a skeleton.
        assert!(!skeleton_only(""));
    }

    /// A finished deliverable that has gone stale is ACCEPTED, not killed for looping.
    ///
    /// This test asserted `Looping` before `Verdict::Accept` existed. F165 is why it changed:
    /// `test-meridian` was recorded a TERMINAL FAILURE with its file on disk carrying 8 passing test
    /// functions, because every judge verdict was a way to STOP a worker and the third stop is fatal.
    /// The owned file here exists, is non-empty and compiles, so the deliverable IS done — killing the
    /// worker spends an attempt to reach the same artifact.
    #[test]
    /// REWRITTEN. It asserted that a worker still for 500s gets `Accept` — and the scheduler answers
    /// Accept with `h.abort()` and a DONE record, so that assertion was pinning a stopwatch that ENDS a
    /// model call. Section 7 gives the judge no power to terminate and section 8 leaves no wall-clock in
    /// the run path; the branch is disarmed, so the invariant to pin is the new one.
    ///
    /// A clock may SUMMON the judge or SUGGEST to a worker. It may never CUT one.
    fn stillness_alone_never_ends_a_call() {
        // THE GATE. Every task shape the engine dispatches, across four orders of magnitude of elapsed
        // time and both stillness states. Not one may come back Accept or Looping: the scheduler answers
        // Accept with h.abort()+DONE and a deterministic Looping aborts the attempt, so either is a
        // stopwatch ending a model call — the thing section 8 removes and that has now had to be asked
        // for three times.
        for task in ["scan-module", "integrate-verify", "fix::r0::app.py", "web"] {
            for written in [true, false] {
                for still in [None, Some(500u64), Some(5_000)] {
                    for age in [700u64, 2_000, 20_000, 200_000] {
                        let v = deterministic_verdict(
                            &mk(task, written, still, age),
                            &JudgeConfig::default(),
                        );
                        assert!(
                            v.as_ref()
                                .is_none_or(|o| o.verdict != Verdict::Accept
                                    && o.verdict != Verdict::Looping),
                            "{task} written={written} still={still:?} age={age}s gave a TERMINAL \
                             verdict from a clock: {:?}",
                            v.map(|o| o.verdict)
                        );
                    }
                }
            }
        }
    }

    /// REWRITTEN WITH ITS SUBJECT (r3 II-7). This pinned the deadline branch catching an unwritten
    /// task once the clock ran out. With every clock verdict deleted, an unwritten task whose
    /// activity is UNKNOWN (`worker_tool_calls: None` — no heartbeat) yields silence at every age:
    /// there is no evidence, and seconds may not stand in for evidence. The K zero-production-looks
    /// summons and the semantic judge own this case.
    #[test]
    fn nothing_written_with_unknown_activity_is_silence_at_any_age() {
        for age in [500u64, 700, 7_000] {
            assert!(
                deterministic_verdict(
                    &mk("scan-module", false, None, age),
                    &JudgeConfig::default()
                )
                .is_none(),
                "an unwritten task with no activity evidence was verdicted at {age}s"
            );
        }
    }

    #[test]
    fn no_verdict_for_the_sink_that_wrote_and_went_quiet() {
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
    fn quiet_when_recently_written() {
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
    fn healthy_young_worker_is_quiet() {
        let v = deterministic_verdict(
            &mk("scan-module", true, Some(30), 100),
            &JudgeConfig::default(),
        );
        assert!(v.is_none());
    }

    #[test]
    fn behavioral_over_read_fires_early_on_many_actions_no_output() {
        // 0 writes + 16 tool calls on the SECOND look → over-read caught from behaviour alone. The
        // elapsed value is irrelevant now: the first-look guard is a look count (r3 II-7), never
        // `elapsed_secs >= min_age_secs`.
        let mut i = mk("core-tree", false, None, 150);
        i.worker_tool_calls = Some(16);
        i.prev_tool_calls = Some(4);
        i.prev_observed_secs = Some(90);
        let v = deterministic_verdict(&i, &JudgeConfig::default());
        assert_eq!(v.map(|o| o.verdict), Some(Verdict::OverReading));
        // The FIRST look, same shape: not yet — one look cannot tell a startup burst from thrashing,
        // which is exactly what the seconds guard was protecting and what the look count now states.
        let mut first = mk("core-tree", false, None, 150);
        first.worker_tool_calls = Some(16);
        assert!(deterministic_verdict(&first, &JudgeConfig::default()).is_none());
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
        // THE GUARANTEE IS "NEVER KILLED", NOT "NEVER JUDGED", and this used to assert the proxy.
        // Asserting the property directly keeps the original protection (integrate-verify
        // judge_killed x3 -> run FAILED though the app worked) whatever branches come and go.
        let v = deterministic_verdict(&i, &JudgeConfig::default());
        assert!(
            v.as_ref().is_none_or(|o| !o.verdict.is_problem()),
            "a no-owned verifier task must never receive a PROBLEM verdict, got {:?}",
            v.as_ref().map(|o| o.verdict.as_str())
        );
        // The "…should be accepted, not left to a timer" half of this test is gone with its subject.
        // It pinned the owns-nothing Accept branch, which fired on a 420-second stopwatch and which the
        // scheduler answers with h.abort() and a DONE record — a clock truncating the longest call in
        // the run and logging it as finished. The guarantee above (never a PROBLEM verdict) is the one
        // that was actually protecting this task, and it still holds.
    }

    /// A join that is STILL WORKING must be left alone — and one that has gone quiet gets silence
    /// too, not a clock verdict: cutting a productive sink short and "accepting" a busy one mid-
    /// command were the same mistake from opposite directions, and both walls are deleted.
    #[test]
    fn a_still_producing_join_is_not_accepted_early() {
        let mut i = mk("integrate-verify", false, None, 500);
        i.owned_files = vec![];
        i.prev_tool_calls = Some(10);
        i.worker_tool_calls = Some(40);
        i.prev_observed_secs = Some(480);
        assert!(
            deterministic_verdict(&i, &JudgeConfig::default()).is_none(),
            "a join whose tool calls are still climbing must keep running"
        );
        // Nor a young one, however quiet.
        let mut young = mk("integrate-verify", false, None, 60);
        young.owned_files = vec![];
        young.worker_tool_calls = Some(3);
        assert!(deterministic_verdict(&young, &JudgeConfig::default()).is_none());
        // Nor one that has done NOTHING — zero actions is not work to salvage.
        let mut idle = mk("integrate-verify", false, None, 900);
        idle.owned_files = vec![];
        idle.worker_tool_calls = Some(0);
        assert!(deterministic_verdict(&idle, &JudgeConfig::default()).is_none());
    }

    #[test]
    fn over_read_still_fires_for_a_task_that_owns_files() {
        // The over-read gate keeps its teeth: a task that DOES own code and has written nothing while
        // burning tool calls is still redirected — on evidence (actions + a second look), not a clock.
        let mut i = mk("core-tree", false, None, 500);
        i.worker_tool_calls = Some(40);
        i.prev_tool_calls = Some(12);
        i.prev_observed_secs = Some(440);
        assert_eq!(
            deterministic_verdict(&i, &JudgeConfig::default()).map(|o| o.verdict),
            Some(Verdict::OverReading)
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
        // Slow but healthy: few tool calls (one long generation), no file yet — below the action bar.
        let mut i = mk("core-tree", false, None, 200);
        i.worker_tool_calls = Some(4);
        assert!(deterministic_verdict(&i, &JudgeConfig::default()).is_none());
    }

    #[test]
    fn split_candidate_only_for_big_productive_multifile_tasks() {
        // Enable the master gate so the detector can fire in this unit test. Since r3 II-7 the
        // "too big" evidence is production ACROSS LOOKS (action count grew between two judge
        // observations), never `elapsed_secs >= split_threshold_secs` — a wall clock is not evidence.
        let cfg = JudgeConfig {
            split_enabled: true,
            ..JudgeConfig::default()
        };
        // Producing across looks, owns 2 files, not over-reading, never split -> SPLIT candidate.
        let mut big = mk("core", true, Some(10), 1000);
        big.owned_files = vec!["a.py".to_string(), "b.py".to_string()];
        big.worker_tool_calls = Some(5);
        big.prev_tool_calls = Some(2);
        big.prev_observed_secs = Some(900);
        assert!(is_split_candidate(&big, &cfg));
        // Same but only ONE owned file -> not splittable.
        let mut one = mk("core", true, Some(10), 1000); // owned_files = ["m.py"]
        one.worker_tool_calls = Some(5);
        one.prev_tool_calls = Some(2);
        assert!(!is_split_candidate(&one, &cfg));
        // Multi-file but OVER-READING (thrashing) -> re-dispatch path, not split.
        let mut thrash = big.clone();
        thrash.worker_tool_calls = Some(cfg.over_read_tool_calls + 1);
        assert!(!is_split_candidate(&thrash, &cfg));
        // Already split once -> never split again.
        let mut again = big.clone();
        again.split_count = 1;
        assert!(!is_split_candidate(&again, &cfg));
        // FIRST look -> not a candidate: one observation cannot show production across looks.
        let mut first = big.clone();
        first.prev_tool_calls = None;
        assert!(!is_split_candidate(&first, &cfg));
        // Flat across looks -> not a candidate: it may be sitting inside one long command.
        let mut flat = big.clone();
        flat.prev_tool_calls = Some(5);
        assert!(!is_split_candidate(&flat, &cfg));
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
            prev_tool_calls: None,
            prev_observed_secs: None,
            split_count: 0,
            attempt: 0,
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
