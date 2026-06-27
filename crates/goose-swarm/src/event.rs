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
    /// A dynamic replan round: `added` are the spliced task ids; `stopped` means the replanner
    /// declined (empty/invalid) and no further replans will run.
    Replanned {
        round: u32,
        added: Vec<String>,
        stopped: bool,
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
