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
use serde::Serialize;

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
}

impl Verdict {
    pub fn as_str(&self) -> &'static str {
        match self {
            Verdict::Ok => "ok",
            Verdict::OverReading => "over_reading",
            Verdict::Looping => "looping",
            Verdict::BrokenCode => "broken_code",
            Verdict::SpecDrift => "spec_drift",
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
}

/// A judge's conclusion about one worker.
#[derive(Clone, Debug)]
pub struct JudgeOutcome {
    pub verdict: Verdict,
    /// 0.0–1.0. Intervention requires a high bar because the judge is itself a weak model.
    pub confidence: f32,
    /// A one-line corrective hint, prepended to the task on re-dispatch.
    pub hint: String,
}

impl JudgeOutcome {
    pub fn ok() -> Self {
        Self {
            verdict: Verdict::Ok,
            confidence: 1.0,
            hint: String::new(),
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
}

impl Default for JudgeConfig {
    fn default() -> Self {
        Self {
            min_age_secs: 90,
            intervene_confidence: 0.8,
            max_interventions_per_task: 1,
        }
    }
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
}

/// Inspects one in-flight worker and returns a verdict. Implemented in goose-cli by gathering evidence
/// (see [`JudgeInput`] / [`deterministic_verdict`]) and running an LLM on the idle device for semantic
/// review. The model-agnostic scheduler only calls this and acts on the [`JudgeOutcome`].
#[async_trait]
pub trait Judge: Send + Sync {
    async fn judge(&self, req: JudgeRequest) -> JudgeOutcome;
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
                "{path} does not compile ({snippet}). Rewrite it so it parses and imports cleanly."
            ),
        });
    }
    if !input.any_owned_written && input.elapsed_secs >= cfg.min_age_secs.max(300) {
        return Some(JudgeOutcome {
            verdict: Verdict::OverReading,
            confidence: 0.9,
            hint: "You have produced no file yet. STOP reading/deliberating — you already have the \
                   spec, the file layout, and the injected dependency APIs. WRITE your owned file(s) now."
                .to_string(),
        });
    }
    None
}
