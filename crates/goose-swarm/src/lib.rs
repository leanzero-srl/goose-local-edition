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
pub mod idle_jobs;
pub mod patch;
pub mod scheduler;
pub mod stub;
pub mod verdict;

pub use coherence::{extract_signatures, SigLang};
pub use context::SharedContext;
pub use dag::{
    derived_weight, expand_subsplits, extract_subsplit, fill_fan_enabled, specs_from_plan_json,
    Dag, DeclaredExport, Difficulty, MergerOf, ModuleInterface, Node, ShardOf, TaskId, TaskSpec,
    TaskState,
};
pub use dispatch::{DispatchError, DispatchRequest, TaskDispatcher, TaskRunOutput, ToolCallRecord};
pub use event::{EventSink, NullSink, ReadyWeight, SwarmEvent};
pub use idle_jobs::PreReviewer;
pub use patch::{apply_patch, parse_patch, pin_sink_id, PlanPatch, TaskAdd, TaskEdit, SINK_ID};
pub use scheduler::{
    qa_enabled, testgen_enabled, AttemptRecord, DeviceAdmission, DeviceCfg, DeviceSummary,
    RunReport, Scheduler, TaskOutcome,
};
pub use stub::skeleton_only;
pub use verdict::{JudgeOutcome, Verdict};
