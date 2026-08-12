//! goose-swarm: a weighted work-queue scheduler for the local LM Studio / LM Link swarm.
//!
//! Fork-only crate (goose-local-edition). The 27B planner emits a typed DAG; this crate owns the
//! locking DAG queue, per-device weights, pull-based work-stealing, and the shared-context store.
//! It is model-agnostic: tasks run through the [`dispatch::TaskDispatcher`] trait, so the
//! concurrency core is unit-testable against a mock with no model involved.

pub mod coherence;
pub mod context;
pub mod dag;
pub mod dispatch;
pub mod event;
pub mod judge;
pub mod memory_classify;
pub mod replan;
pub mod scheduler;

pub use coherence::{extract_signatures, scope_contract_bundle, SigLang};
pub use context::SharedContext;
pub use dag::{
    expand_subsplits, extract_subsplit, fill_fan_enabled, specs_from_plan_json, Dag, Difficulty,
    Node, TaskId, TaskSpec, TaskState,
};
pub use dispatch::{DispatchError, DispatchRequest, TaskDispatcher, TaskRunOutput, ToolCallRecord};
pub use event::{EventSink, NullSink, SwarmEvent};
pub use judge::{
    deterministic_verdict, is_split_candidate, ChildSpec, Judge, JudgeConfig, JudgeInput,
    JudgeOutcome, JudgeRequest, PreReviewOutput, PreReviewRequest, PreReviewer, Verdict,
};
pub use replan::{ReplanContext, Replanner};
pub use scheduler::{
    sink_review_enabled, testgen_enabled, AttemptRecord, DeviceCfg, DeviceSummary, RunReport,
    Scheduler, TaskOutcome,
};
