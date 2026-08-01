//! Streaming observability events the scheduler emits through an injected sink, so goose-cli can
//! write a structured per-run JSONL log without this model-agnostic core knowing about IO. All
//! emits happen under the scheduler's state lock, so a sink need only be `Send + Sync`.

use crate::dispatch::ToolCallRecord;
use serde::Serialize;

#[derive(Serialize, Debug)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum SwarmEvent {
    TaskDispatched {
        task_id: String,
        device: String,
        model: String,
        attempt: u32,
        deps: Vec<String>,
        owned_files: Vec<String>,
        context_slice_len: usize,
    },
    TaskCompleted {
        task_id: String,
        status: String,
        device: Option<String>,
        model: Option<String>,
        attempts: u32,
        elapsed_ms: u64,
        session_id: Option<String>,
        tool_calls: Vec<ToolCallRecord>,
    },
    TaskRetry {
        task_id: String,
        from_device: Option<String>,
        error: String,
        transient: bool,
    },
    SchedulerStuck {
        remaining: usize,
    },
    /// The scheduler reached a task boundary and is HOLDING because the pause sentinel exists. This is the
    /// in-process pause (distinct from the crash-resume `run_resumed`): in-flight tasks were left to finish
    /// and no new ready task is claimed. Emitted on the transition into the hold, so the log means "the engine
    /// actually reached the hold", not merely "the file exists".
    RunPaused,
    /// The pause sentinel was cleared; the scheduler resumed claiming ready tasks (re-runs nothing).
    RunUnpaused,
    /// A dynamic replan round: `added` are the spliced task ids; `stopped` means the replanner
    /// declined (empty/invalid) and no further replans will run.
    Replanned {
        round: u32,
        added: Vec<String>,
        stopped: bool,
    },
    /// A judge-side SPLIT was applied: one task became `children`. Emitted because it was previously
    /// invisible — `apply_split` mutated the DAG and said nothing, so "does the swarm decompose work
    /// further when it has spare nodes" could not be answered from a run at all. Measured across three
    /// real runs before this existed: no way to tell a split that never happened from one that did.
    TaskSplit {
        task_id: String,
        children: Vec<String>,
    },
    /// A SPECULATIVE twin resolved. `winner` is "twin" or "primary"; `aborted` is the side that lost.
    /// Same reason as TaskSplit: speculation is the mechanism that spends an idle node on latency, and
    /// it emitted nothing, so its contribution to node-scaling was unmeasurable by construction.
    Speculated {
        task_id: String,
        attempt: u32,
        winner: String,
    },
    /// The idle-model judge inspected an in-flight worker. `action` is "observed" (logged only) or
    /// "re_dispatch" (the worker was killed and its task re-queued with `hint`).
    JudgeVerdict {
        task_id: String,
        device: String,
        verdict: String,
        confidence: f32,
        hint: String,
        action: String,
    },
    /// An idle node ran a correctness PRE-REVIEW of a completed task on a spare device (concurrently with
    /// the judge). Makes idle-node utilization observable in the jsonl; `had_findings` = a defect was found
    /// (persisted to `.swarm/prereview/<task>.json` for integrate-verify).
    PreReview {
        task_id: String,
        device: String,
        had_findings: bool,
    },
}

pub trait EventSink: Send + Sync {
    fn emit(&self, event: &SwarmEvent);
    /// Write a caller-side event (e.g. goose-cli's run_started / plan_loaded / run_finished) into
    /// the same stream. Default: ignored.
    fn write_value(&self, _value: serde_json::Value) {}
}

/// Default sink used when no observability is wired (tests, library callers).
pub struct NullSink;

impl EventSink for NullSink {
    fn emit(&self, _event: &SwarmEvent) {}
}
