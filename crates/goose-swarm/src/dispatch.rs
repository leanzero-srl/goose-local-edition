//! The model-agnostic dispatch boundary. The scheduler only ever runs a task by calling
//! [`TaskDispatcher::run`]; the real implementation (in goose-cli) drives a Goose Agent bound to a
//! device's LM Link model id, while tests use a mock. This keeps the concurrency core testable.

use async_trait::async_trait;
use serde::Serialize;

/// One tool call a task's agent made, captured by the dispatcher for observability.
#[derive(Clone, Debug, Serialize)]
pub struct ToolCallRecord {
    pub name: String,
    /// True if the tool came from an MCP extension (vs a builtin like `developer`).
    pub is_mcp: bool,
    /// Whether the tool response was ok; `None` if no response arrived (e.g. a max-turns cutoff).
    pub ok: Option<bool>,
}

/// A successful dispatch's result: the task output plus observability (session id + tool calls).
#[derive(Clone, Debug, Default)]
pub struct TaskRunOutput {
    pub output: String,
    pub session_id: Option<String>,
    pub tool_calls: Vec<ToolCallRecord>,
}

impl From<String> for TaskRunOutput {
    fn from(output: String) -> Self {
        Self {
            output,
            session_id: None,
            tool_calls: Vec::new(),
        }
    }
}

/// One unit of work handed to a device.
#[derive(Clone, Debug)]
pub struct DispatchRequest {
    pub task_id: String,
    pub description: String,
    /// The device this task was routed to (pool id).
    pub device_id: String,
    /// The LM Link model id LM Studio routes to the device.
    pub model_id: String,
    /// The relevant slice of shared context (dependency outputs) for this task.
    pub context_slice: String,
    /// 0-based attempt number (incremented on re-dispatch).
    pub attempt: u32,
    /// The EXACT files this task must create/own (plan paths). The worker is told to write only these.
    pub owned_files: Vec<String>,
    /// The full project file manifest (every task's owned files) so the worker uses the agreed layout
    /// for imports and never invents a divergent location.
    pub all_files: Vec<String>,
    /// A corrective hint from the idle-model judge when this is a re-dispatch after the judge killed a
    /// prior attempt (e.g. "you were looping/over-reading — WRITE now"). `None` on a normal attempt.
    pub prior_hint: Option<String>,
    /// True when this is a SPECULATIVE twin of an in-flight chokepoint task, raced on an idle device
    /// (GOOSE_SWARM_SPECULATE). The dispatcher must run it in an ISOLATED shadow workspace so it never
    /// writes the real owned_files; the winner's output is promoted back. `false` for every normal task.
    pub speculative: bool,
}

/// Outcome of a failed dispatch. `Transient` is re-dispatched (and steered to a different device);
/// `Terminal` fails the task and its descendants.
#[derive(Clone, Debug)]
pub enum DispatchError {
    /// Recoverable — e.g. LM Studio "Model is unloaded". Re-dispatch within the attempt budget.
    Transient(String),
    /// A CONTENT failure (e.g. a syntax error in an owned file) caught by the pre-done gate. Re-dispatched
    /// like `Transient`, but the error is threaded into the retry's `prior_hint` so the fix is guided
    /// rather than a blind re-roll. Scoped to content — infra transients never carry a hint.
    ContentRetry(String),
    /// Unrecoverable for this task.
    Terminal(String),
}

impl std::fmt::Display for DispatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DispatchError::Transient(m) => write!(f, "transient: {m}"),
            DispatchError::ContentRetry(m) => write!(f, "content-retry: {m}"),
            DispatchError::Terminal(m) => write!(f, "terminal: {m}"),
        }
    }
}

impl std::error::Error for DispatchError {}

#[async_trait]
pub trait TaskDispatcher: Send + Sync {
    /// Run one task on its assigned device. Returns the task's output plus observability on success.
    async fn run(&self, req: DispatchRequest) -> Result<TaskRunOutput, DispatchError>;

    /// Promote a winning SPECULATIVE twin: copy its shadow-workspace output for `task_id` into the real
    /// project tree. Called by the scheduler when a speculative twin finishes first. Default no-op so the
    /// mock and the speculation-OFF path need no change; the real dispatcher overrides it in Phase 2.
    async fn promote_speculative(&self, _task_id: &str) {}

    /// Drop a shadow workspace for `task_id` WITHOUT promoting it — a lost/errored speculative shard, so its
    /// edits never reach the real tree and the shadow does not leak. Default no-op.
    async fn discard_speculative(&self, _task_id: &str) {}

    /// READ-ONLY correctness review of the produced `files` along ONE dimension, on a spare fleet model.
    /// The model is given the files as text with NO tools (it physically cannot write), so N of these run
    /// concurrently over one tree with no write-race. Returns advisory findings text, or None when clean /
    /// nothing to review. Default None so the mock and the fanout-OFF path are unchanged.
    async fn review_dimension(
        &self,
        _model_id: &str,
        _dim_id: &str,
        _dim_brief: &str,
        _goal: &str,
        _files: &[String],
    ) -> Option<String> {
        None
    }

    /// ADVERSARIAL VERIFY (the three-vote core): hand ONE review finding to a skeptic model — a DIFFERENT
    /// model than raised it — prompted to REFUTE it against the ACTUAL code (read-only, no tools). Returns
    /// true only if the skeptic independently CONFIRMS the finding is a real defect with HIGH confidence, i.e.
    /// it SURVIVES refutation. FAIL-CLOSED: default false, and any refute / low-confidence / parse-or-timeout
    /// failure returns false — an unverified finding can never drive a fix. Read-only => no write-race.
    async fn verify_finding(
        &self,
        _model_id: &str,
        _finding: &str,
        _goal: &str,
        _files: &[String],
    ) -> bool {
        false
    }
}
