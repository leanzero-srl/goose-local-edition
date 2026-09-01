//! The idle-node jobs a scheduler hands its dispatcher (the `PreReviewer` seam). Moved verbatim
//! from `judge.rs` when the idle-model judge was deleted (2c S6); the two jobs that remain — the
//! operator Q&A and testgen — never depended on it.

use async_trait::async_trait;

/// The idle-node jobs a scheduler can hand its dispatcher when a node would otherwise sit idle:
/// the operator Q&A and testgen — each a default no-op, each gated in the scheduler (the sink/tail
/// dimension reviews are deleted, 2c S6). The M5 completion-time pre-review that named this trait is deleted (VA-014 D1:
/// zero `pre_review` events in every measured run); the name stays so the attach seam is one line.
#[async_trait]
pub trait PreReviewer: Send + Sync {
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

    /// F790-3: answer ONE pending operator question on an idle node, with the run-state brief
    /// supplied by the scheduler. Read-only with respect to the build; the answer
    /// lands in the run's answers outbox + an event. Default no-op.
    async fn answer_user_question(&self, _model_id: &str, _goal: &str, _run_state: &str) {}
}
