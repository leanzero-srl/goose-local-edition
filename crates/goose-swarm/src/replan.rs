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

#[async_trait]
pub trait Replanner: Send + Sync {
    /// Return ADDITIONAL independent [`TaskSpec`]s to fill idle workers, or an empty vec to stop.
    /// Errors are treated as empty — a replanner failure never aborts the run.
    async fn replan(&self, ctx: ReplanContext) -> anyhow::Result<Vec<TaskSpec>>;
}
