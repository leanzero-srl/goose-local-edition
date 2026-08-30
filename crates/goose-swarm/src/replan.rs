//! The optional dynamic replanner boundary. Mirrors [`crate::dispatch::TaskDispatcher`]: the
//! scheduler asks for more work by calling [`Replanner::replan`] at an idle point; the real impl
//! (goose-cli) drives the 27B planner, tests use a mock. Model-agnostic so the idle-trigger logic
//! stays unit-testable.

use crate::dag::TaskSpec;
use async_trait::async_trait;

/// A read-only snapshot of the run handed to the replanner when workers go idle mid-run.
#[derive(Clone, Debug)]
pub struct ReplanContext {
    /// The original user goal/prompt, so the planner keeps the objective in view.
    pub goal: String,
    /// Ids already in the DAG (any state) — the replanner MUST NOT reuse these.
    pub existing_ids: Vec<String>,
    /// Completed task ids with their (truncated) outputs — what's been produced so far.
    pub completed: Vec<(String, String)>,
    /// Failed task ids — new work must not depend on these.
    pub failed: Vec<String>,
    /// Still-incomplete ids (Pending/Ready/Claimed).
    pub incomplete: Vec<String>,
    /// Free worker slots right now (how much parallel work could start immediately).
    pub idle_capacity: u32,
    /// Replan round (0-based).
    pub round: u32,
}

/// What the replanner answered: the specs, plus its own stated WHY. r5's live splice (12:36:09,
/// added frozen-rules-tests + viz-math-oracle) emitted `Replanned { reason: "" }` because the
/// trait could only carry specs — the model's rationale was never requested, so the event's
/// reason field held the hygiene actions alone, which that round had none of. `rationale` is the
/// model's own text, verbatim; `None` when it genuinely gave none (the scheduler then names the
/// absence — never fabricates and never ships '').
pub struct ReplanAnswer {
    pub specs: Vec<TaskSpec>,
    pub rationale: Option<String>,
}

#[async_trait]
pub trait Replanner: Send + Sync {
    /// Return ADDITIONAL independent [`TaskSpec`]s to fill idle workers (empty to stop), with the
    /// replanner's own rationale for the batch. An Err is a planner-call FAILURE, distinguished
    /// from a decline by the scheduler's arms — it never aborts the run.
    async fn replan(&self, ctx: ReplanContext) -> anyhow::Result<ReplanAnswer>;
}
