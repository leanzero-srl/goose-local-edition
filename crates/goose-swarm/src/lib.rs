//! goose-swarm: a weighted work-queue scheduler for the local LM Studio / LM Link swarm.
//!
//! Fork-only crate (goose-local-edition). The 27B planner emits a typed DAG; this crate owns the
//! locking DAG queue, per-device weights, pull-based work-stealing, and the shared-context store.
//! It is model-agnostic: tasks run through the [`dispatch::TaskDispatcher`] trait, so the
//! concurrency core is unit-testable against a mock with no model involved.

pub mod context;
pub mod dag;
pub mod dispatch;
pub mod event;
pub mod scheduler;

pub use context::SharedContext;
pub use dag::{Dag, Difficulty, Node, TaskId, TaskSpec, TaskState};
pub use dispatch::{DispatchError, DispatchRequest, TaskDispatcher, TaskRunOutput, ToolCallRecord};
pub use event::{EventSink, NullSink, SwarmEvent};
pub use scheduler::{AttemptRecord, DeviceCfg, DeviceSummary, RunReport, Scheduler, TaskOutcome};
