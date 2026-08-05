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
        /// True when `status` is `done` only because the progress watchdog salvaged a stalled task
        /// whose owned files were already written. 14% of completed tasks in the archive took this
        /// path and were counted as ordinary successes.
        salvaged: bool,
        device: Option<String>,
        model: Option<String>,
        attempts: u32,
        elapsed_ms: u64,
        session_id: Option<String>,
        /// WHY it ended this way — `None` on success, the last attempt's error on a failure.
        ///
        /// It was absent entirely, and `TaskRetry` has carried one all along: intermediate retries
        /// recorded a reason while the TERMINAL outcome recorded nothing. MEASURED: all 14 failed
        /// test-author tasks in the whole archive say only "failed", so every later reader had to
        /// INFER the cause from judge verdicts. The engine already had the string — `AttemptRecord`
        /// is built with `error: Some(msg)` immediately before the emit — and threw it away.
        error: Option<String>,
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
        /// The device of the JUDGED WORKER — the node whose output is being judged.
        device: String,
        /// The node that RAN THE JUDGE, which is a different node and was previously unrecoverable.
        ///
        /// `device` above comes from `task_final_device`, so every judge_verdict in the archive
        /// attributes the judge's inference cost to the worker it was reviewing. A judge holds a real
        /// fleet slot (`devices[i].in_flight += 1`), so "which node did the judging" is exactly the
        /// question needed to test whether idle-job placement targets idle nodes — and the log could
        /// not answer it. Empty when the judge ran deterministic-only with no device claimed.
        judge_node: String,
        verdict: String,
        confidence: f32,
        hint: String,
        action: String,
        /// PROVENANCE: true when `deterministic_verdict` produced this — a real engine fact (a compile
        /// error, an owned file never written, a measured char/tool count) — false when the judge MODEL
        /// authored it.
        ///
        /// `confidence` cannot carry this: the model produces its own confidence, so a 0.90 from a weak
        /// model is indistinguishable from a 0.90 the engine hard-codes. The flag exists on
        /// `JudgeOutcome` and was used for the terminal-fail gate, then DROPPED before emit — so every
        /// downstream analysis had to re-derive provenance from thresholds. That is not hypothetical:
        /// it produced a wrong published finding (an `over_reading` verdict was attributed to the LLM
        /// judge on exactly that reasoning, when a deterministic 420 s branch had emitted it).
        deterministic: bool,
    },
    /// An idle node ran a correctness PRE-REVIEW of a completed task on a spare device (concurrently with
    /// the judge). Makes idle-node utilization observable in the jsonl; `had_findings` = a defect was found
    /// (persisted to `.swarm/prereview/<task>.json` for integrate-verify).
    PreReview {
        task_id: String,
        device: String,
        had_findings: bool,
        /// How long the job HELD ITS FLEET SLOT. A pre-review claims the same `in_flight` permit a
        /// task dispatch does and can hold it for up to `planner_timeout_secs` (900s), but the event
        /// fired only on completion with no start and no duration — so the one idle-node mechanism
        /// that can block a real dispatch for a quarter of an hour was the one nobody could measure.
        /// Its slot-time had to be ESTIMATED from same-device inter-arrival gaps, which is a guess.
        secs: f64,
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
