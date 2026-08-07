//! The deterministic weighted work-queue scheduler.
//!
//! One central dispatch loop owns the [`Dag`]; per-device capacity is `weight` (max concurrent
//! in-flight tasks on that device). Each loop pass claims as many ready tasks as devices have free
//! capacity (work-stealing: a ready task prefers its planner-suggested device but falls back to any
//! free one), spawns their dispatch futures, then waits on a [`Notify`] that completions fire. A
//! task is locked (state `Claimed`, its files held) while in flight, so it is never double-claimed
//! and two tasks owning the same file never run concurrently. Completions relax dependents
//! (unlocking the DAG), merge output into the shared context, and free device capacity.

use crate::context::SharedContext;
use crate::dag::{Dag, Difficulty, TaskId, TaskState};
use crate::dispatch::{
    DispatchError, DispatchRequest, TaskDispatcher, TaskRunOutput, ToolCallRecord,
};
use crate::event::{EventSink, NullSink, SwarmEvent};
use crate::judge::{
    Judge, JudgeConfig, JudgeOutcome, JudgeRequest, PreReviewRequest, PreReviewer, Verdict,
};
use crate::replan::{ReplanContext, Replanner};
use anyhow::{bail, Result};
use serde::Serialize;
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{Mutex, Notify};

/// GOOSE_SWARM_SPLIT_INHERIT_SPEC (default OFF): give a split CHILD the parent's full implementation spec,
/// scoped to the child's own files — instead of the ~40-char label it gets today.
///
/// MEASURED (loop-04): PLAN spent 48.4 min (40% of the whole run) writing a 2038-char implementation-ready
/// spec for `data-model-persistence` (three SPM targets, Swift 6 mode, sqlite3 system library, `@Observable
/// class NoteStore: Sendable`, an undo stack). The judge then split it, and every child's ENTIRE task
/// statement became `"(split of data-model-persistence) note-store"` — 43 characters. The spec the run had
/// just paid 40% of its wall-clock to produce was thrown away at the moment of use, and the shipped app
/// showed it: 221 LOC against an ~800-1200 spec, a plain JSON store where the plan demanded SQLite.
///
/// The splitter is default-ON on the desktop path, so this fires on real runs.
///
/// Default OFF because it is a real behaviour change, not merely a restoration: handing a child the parent's
/// whole spec risks it writing its SIBLINGS' files. `child_description` therefore leads with a hard
/// file-scope header, and the lever gets an A/B before it is trusted.
/// The ONE resolution of GOOSE_SWARM_SINK_REVIEW. Both halves of the mechanism — this crate's
/// producer and goose-cli's drain — must read the same answer, or the run reports a lever it is not
/// running.
pub fn sink_review_enabled() -> bool {
    std::env::var("GOOSE_SWARM_SINK_REVIEW")
        .map(|v| {
            matches!(
                v.trim().to_lowercase().as_str(),
                "1" | "on" | "true" | "yes"
            )
        })
        .unwrap_or(false)
}

fn split_inherit_spec_enabled() -> bool {
    matches!(
        std::env::var("GOOSE_SWARM_SPLIT_INHERIT_SPEC")
            .unwrap_or_default()
            .trim()
            .to_lowercase()
            .as_str(),
        "1" | "on" | "true" | "yes"
    )
}

/// The task statement a split child receives.
///
/// OFF (today's behaviour, byte-identical): `"(split of <parent>) <child-id>"`.
/// ON: a file-scope header naming exactly the files this child owns and stating that the siblings belong to
/// other workers, followed by the parent's FULL spec as shared context. Pure string work — no model call, no
/// new judgement, no dep semantics touched. The header comes FIRST so the scope is read before the spec that
/// describes files the child must not touch.
fn child_description(
    parent_id: &str,
    parent_desc: &str,
    child: &crate::judge::ChildSpec,
    inherit_spec: bool,
) -> String {
    if !inherit_spec || parent_desc.trim().is_empty() {
        return format!("(split of {parent_id}) {}", child.id);
    }
    format!(
        "This task is one PART of a larger subtask (`{parent_id}`) that was split across workers.\n\n\
         YOU OWN ONLY THESE FILES — create/edit these and NOTHING else:\n{}\n\n\
         The other files named in the spec below belong to OTHER workers on this same plan and are being \
         written right now in parallel. Do NOT create them, do NOT edit them, and do NOT wait for them.\n\n\
         The FULL spec of the original subtask follows. Implement ONLY the parts that describe the files you \
         own; treat the rest as context for how your files must fit together.\n\n{parent_desc}",
        child
            .files
            .iter()
            .map(|f| format!("- {f}"))
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

/// GOOSE_SWARM_SALVAGE_SPIN (default ON): when a NON-TEST task terminal-fails via finalize-spin (Verdict::
/// Looping), salvage it as Done instead of Failed. Looping only fires once the owned file was written, so the
/// worker DID produce output — discarding it also fails its dependents (esp. the integrate-verify sink), which
/// reports a WORKING app as FAILED (observed UNIQ9: the entry spun on its final fix -> integrate-verify blocked
/// -> run FAILED though the app runs). Salvaging lets integrate-verify be the real gate. Off with 0/off/false/no.
fn salvage_spin_enabled() -> bool {
    std::env::var("GOOSE_SWARM_SALVAGE_SPIN")
        .map(|v| {
            !matches!(
                v.trim().to_lowercase().as_str(),
                "0" | "off" | "false" | "no"
            )
        })
        .unwrap_or(true)
}

fn looks_like_test_file(f: &str) -> bool {
    let lower = f.to_lowercase();
    let base = lower.rsplit('/').next().unwrap_or(lower.as_str());
    base.starts_with("test_")
        || base.ends_with("_test.py")
        || base.ends_with("_test.rs")
        || base.ends_with(".test.ts")
        || base.ends_with(".test.js")
        || base == "conftest.py"
        || lower.contains("/tests/")
        || lower.contains("/test/")
}

/// A test subtask: id mentions "test", or every owned file looks like a test file. Test tasks are never
/// salvaged (a spinning test is not "done", and tests do not block integrate-verify).
fn is_test_task(id: &str, owned_files: &[String]) -> bool {
    id.to_lowercase().contains("test")
        || (!owned_files.is_empty() && owned_files.iter().all(|f| looks_like_test_file(f)))
}

/// A build-system manifest / package descriptor — a task that wrote ONLY one of these has not delivered its
/// actual code. Used to keep the salvage gate from marking a task Done on a trivial go.mod.
fn looks_like_manifest_file(f: &str) -> bool {
    let base = f.rsplit('/').next().unwrap_or(f).to_lowercase();
    matches!(
        base.as_str(),
        "go.mod"
            | "go.sum"
            | "package.json"
            | "package-lock.json"
            | "cargo.toml"
            | "cargo.lock"
            | "requirements.txt"
            | "setup.py"
            | "setup.cfg"
            | "pyproject.toml"
            | "__init__.py"
            | "tsconfig.json"
            | "gemfile"
    )
}

/// GOOSE_SWARM_SALVAGE_REQUIRE_CRITICAL (#134, default OFF = byte-identical `.any()`): when a stalled/spinning
/// task is salvaged to Done, require its CRITICAL owned files to be present, not just ANY file.
fn salvage_require_critical() -> bool {
    std::env::var("GOOSE_SWARM_SALVAGE_REQUIRE_CRITICAL")
        .map(|v| {
            matches!(
                v.trim().to_lowercase().as_str(),
                "1" | "on" | "true" | "yes"
            )
        })
        .unwrap_or(false)
}

/// Whether a salvage is justified by what is on disk. DEFAULT: at least one owned file is non-empty (the
/// finalize-spin gate only fires once SOMETHING was written; a custom/LLM judge could emit Looping with
/// nothing on disk — never salvage then). STRICT (salvage_require_critical): EVERY *critical* owned file —
/// non-manifest, non-test source — must exist and be non-empty; a go.mod-only tree is not a done app. Measured
/// on mustsolve-test4: cli-entry owns cmd/logfold/main.go but stalled after writing only a 24-byte go.mod, and
/// the old `.any()` salvaged it to Done → the app shipped with NO entrypoint. Falls back to `.any()` when the
/// task owns only manifest/test files. Paths resolve against the run cwd (where workers write).
fn owned_file_written(owned_files: &[String]) -> bool {
    let nonempty = |f: &str| std::fs::metadata(f).map(|m| m.len() > 0).unwrap_or(false);
    if salvage_require_critical() {
        let critical: Vec<&String> = owned_files
            .iter()
            .filter(|f| !looks_like_manifest_file(f) && !looks_like_test_file(f))
            .collect();
        if !critical.is_empty() {
            return critical.iter().all(|f| nonempty(f));
        }
    }
    owned_files.iter().any(|f| nonempty(f))
}

/// STRICT variant used by degrade-on-stall (#134/#132): require EVERY *critical* owned file (non-manifest,
/// non-test source) to be present and non-empty; fall back to `.any()` only when the task owns no critical
/// files. Unconditionally strict — the degrade path must NEVER promote a task that wrote only a go.mod. Kept
/// separate from `owned_file_written` so the degrade decision does not depend on the salvage_require_critical
/// env. The evidence (a366f2b3, mustsolve-test4): a stalled worker EMITS events for hundreds of seconds and
/// WRITES its owned file before the model hangs mid-generation — so at exhaustion the file is usually on disk.
fn critical_owned_files_written(owned_files: &[String]) -> bool {
    let nonempty = |f: &str| std::fs::metadata(f).map(|m| m.len() > 0).unwrap_or(false);
    let critical: Vec<&String> = owned_files
        .iter()
        .filter(|f| !looks_like_manifest_file(f) && !looks_like_test_file(f))
        .collect();
    if !critical.is_empty() {
        return critical.iter().all(|f| nonempty(f));
    }
    owned_files.iter().any(|f| nonempty(f))
}

/// The degrade-on-stall decision (#134/#132), extracted so it is unit-testable without a live scheduler run.
/// Degrade a stall-exhausted task to Done only when ALL hold: the lever is on; it is NOT a content/syntax-gate
/// failure (that means a written-but-broken file — never promote it); it is not a test task; and its critical
/// owned files are present non-empty on disk. `enabled == false` => always false => the exhausted arm is
/// byte-identical.
fn should_degrade_on_stall(
    enabled: bool,
    is_content: bool,
    id: &str,
    owned_files: &[String],
) -> bool {
    if !enabled || is_content || is_test_task(id, owned_files) {
        return false;
    }
    // A task that OWNS NOTHING produces no artifact, so there is no half-written file to promote and
    // nothing for `critical_owned_files_written` to find — its trailing `any()` over an empty slice is
    // false, which silently excluded the ONE task that most needs this.
    //
    // `integrate-verify` owns nothing. It is the sole join, and MEASURED it holds the entire fleet
    // alone for 88-98% of the solo time in a 3-node run — half the wall. Its exhaustion re-dispatched
    // the WHOLE join to another node and restarted it from zero, discarding every command already run
    // and every fix already written: two of three sink retries in the campaign were `stream decode
    // error (mid-stream body drop)`, costing 15.3 min on one cell and 44.3 min (29.5% of its wall) on
    // another, on two DIFFERENT devices. A transient LAN fault is not a verdict on the work, and
    // killing the longest task in the run because a socket hiccuped buys nothing.
    //
    // Degrading one cannot manufacture a false green: `green_blocking_failed` already filters
    // owns-nothing tasks out of the green veto, so a verification task that could not finish is
    // recorded as unfinished and gates nothing either way.
    if owned_files.is_empty() {
        return true;
    }
    critical_owned_files_written(owned_files)
}

/// A pool device = one LM Link model id with a capacity weight.
#[derive(Clone, Debug)]
pub struct DeviceCfg {
    pub id: String,
    pub model_id: String,
    /// Max concurrent in-flight tasks routed to this device.
    pub weight: u32,
    pub enabled: bool,
    /// Relative throughput (higher = faster host → a LARGER share of the total tasks; the slowest host
    /// gets proportionally fewer). Default 1 = equal. On an identical-model fleet this is the lever for
    /// skewing load toward the quicker machines instead of splitting evenly.
    pub speed_weight: u32,
}

#[derive(Debug, Serialize)]
pub struct RunReport {
    pub done: Vec<TaskId>,
    pub failed: Vec<TaskId>,
    /// Ids of opportunistic/replanner-added (bonus) tasks — their failure must NOT fail the run.
    pub bonus: Vec<TaskId>,
    pub results: HashMap<TaskId, String>,
    pub context_json: serde_json::Value,
    /// Total tasks dispatched per device id (counts re-dispatches) — observability + weighting checks.
    pub dispatched_per_device: HashMap<String, u32>,
    /// Per-task outcome detail for verification (device, model, attempts, timing, session, tool calls).
    pub tasks: Vec<TaskOutcome>,
    /// Aggregates per device for cluster verification.
    pub per_device: HashMap<String, DeviceSummary>,
}

#[derive(Debug, Serialize, Clone)]
pub struct TaskOutcome {
    pub task_id: TaskId,
    /// `done` | `failed` | `incomplete`.
    pub status: String,
    /// Device of the final attempt.
    pub device: Option<String>,
    pub model: Option<String>,
    pub attempts: u32,
    pub attempt_history: Vec<AttemptRecord>,
    /// Wall-clock of the final attempt.
    pub elapsed_ms: Option<u64>,
    pub session_id: Option<String>,
    pub tool_calls: Vec<ToolCallRecord>,
    pub output: Option<String>,
    /// True when this task owns NO files (e.g. the injected `integrate-verify` model-judge sink). Such a
    /// task's failure is a MODEL self-report, never a deterministic engine event — the hard completion gate
    /// must exclude it from the green-blocking set so a judge's dissent can never veto a good app (C1).
    pub owns_nothing: bool,
}

#[derive(Debug, Serialize, Clone)]
pub struct AttemptRecord {
    pub device: Option<String>,
    pub model: Option<String>,
    /// `ok` | `transient` | `terminal`.
    pub outcome: String,
    pub error: Option<String>,
    pub elapsed_ms: u64,
}

#[derive(Debug, Serialize, Default, Clone)]
pub struct DeviceSummary {
    pub dispatched: u32,
    pub tool_calls: u32,
    pub mcp_calls: u32,
    pub retries: u32,
    /// Sum of attempt durations on this device — NOT wall-clock (tasks overlap under concurrency).
    pub busy_ms: u64,
}

struct DeviceRt {
    cfg: DeviceCfg,
    in_flight: u32,
}

/// Ready-set ordering: higher fan-out first (unblock the most work), tie-break by id ascending for
/// Releases ONE idle-job slot when an idle-job task ends — INCLUDING on panic, so a panicking judge or
/// pre-reviewer can never leak a slot and starve future idle work. Always decrements `idle_jobs`; for a
/// judge job it also clears `judge_running` so a panicked judge does not wedge the single-judge invariant.
/// Drop is synchronous, so it spawns a tiny task to update the count under the async State lock (only if a
/// runtime is still current — during shutdown the count no longer matters).
struct IdleSlotGuard {
    state: Arc<Mutex<State>>,
    is_judge: bool,
    /// The device index this idle-job CLAIMED (bumped in_flight on), so a worker dispatch + the next idle-job
    /// see it as busy and never stack a 2nd call on the same node (the "+1 QUEUED on one node, another idle"
    /// bug). `None` when the fleet was saturated so no idle device could be claimed (deterministic-only judge).
    claimed_device: Option<usize>,
}

impl Drop for IdleSlotGuard {
    fn drop(&mut self) {
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            let st = self.state.clone();
            let is_judge = self.is_judge;
            let claimed = self.claimed_device;
            handle.spawn(async move {
                let mut s = st.lock().await;
                s.idle_jobs = s.idle_jobs.saturating_sub(1);
                if is_judge {
                    s.judge_running = false;
                }
                if let Some(dev) = claimed {
                    if s.devices[dev].in_flight > 0 {
                        s.devices[dev].in_flight -= 1;
                    }
                }
            });
        }
    }
}

/// determinism. `BinaryHeap` is a max-heap, so `Ord` returns Greater for higher priority.
#[derive(Eq, PartialEq)]
struct Ranked {
    fan_out: usize,
    id: TaskId,
}

impl Ord for Ranked {
    fn cmp(&self, other: &Self) -> Ordering {
        self.fan_out
            .cmp(&other.fan_out)
            .then_with(|| other.id.cmp(&self.id))
    }
}
impl PartialOrd for Ranked {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

struct Assignment {
    task_id: TaskId,
    request: DispatchRequest,
}

/// Global cap on SPECULATIVE twins per run (GOOSE_SWARM_SPECULATE) — a long serial chokepoint cannot burn
/// unbounded compute racing twins. Generous: it is a last-resort idle-fill, not a hot path.
const SPECULATION_CAP: u32 = 8;

struct State {
    dag: Dag,
    ready: BinaryHeap<Ranked>,
    devices: Vec<DeviceRt>,
    held_files: HashSet<String>,
    held_by: HashMap<TaskId, Vec<String>>,
    claimed_device: HashMap<TaskId, usize>,
    dispatched_per_device: HashMap<String, u32>,
    ctx: SharedContext,
    max_attempts: u32,
    degrade_on_stall: bool,
    sink: Arc<dyn EventSink>,
    attempt_started_at: HashMap<TaskId, Instant>,
    attempt_log: HashMap<TaskId, Vec<AttemptRecord>>,
    task_session: HashMap<TaskId, Option<String>>,
    task_tool_calls: HashMap<TaskId, Vec<ToolCallRecord>>,
    /// (device_id, model_id) of each task's most recent attempt.
    task_final_device: HashMap<TaskId, (String, String)>,
    /// The user goal (passed to the replanner) + how many replan rounds have run.
    goal: String,
    /// The user's VERBATIM answers to the clarifying questions, for the worker prompt. Empty when the run
    /// never asked. Unlike `goal` — which reaches only the replanner/judge/pre-reviewer — this is handed to
    /// every DispatchRequest, because there was previously no path from an answer to a worker at all.
    user_decisions: String,
    /// GROUNDED research facts (Phase 1, Move 2), VERBATIM, handed to every DispatchRequest alongside
    /// `user_decisions`. Empty when DOC_PREFETCH is off => the worker prompt is byte-identical.
    doc_facts: String,
    replans_done: u32,
    /// How many tasks were still incomplete the last time the replanner answered with NOTHING.
    ///
    /// An empty answer used to burn the entire budget (`replans_done = max_replans`), which turned
    /// "no more work is needed right now" into "never ask again". MEASURED on a live 3-node run: the
    /// replan was asked at +50min with 9 of 18 tasks done, correctly declined because half the DAG was
    /// still queued, and was thereby disabled for good — so at +68min, with ONE task in flight, two
    /// nodes idle and idle_capacity()==5, the one mechanism built to fill them was off.
    ///
    /// The replanner's answer is a function of the DAG state when it was asked, so it is cached
    /// against that state rather than forever: it may be asked again once STRICTLY FEWER tasks remain,
    /// which is the only situation in which it could honestly give a different answer.
    replan_declined_at_incomplete: Option<usize>,
    /// Ids of replanner-added (bonus) tasks — failures here are non-fatal to the run.
    bonus_ids: HashSet<TaskId>,
    /// Observed per-device speed: device index -> (total completed ms, count). Used to route the
    /// hardest tasks (incl. integrate-verify) to the proven-fastest node on an identical-model fleet.
    device_speed: HashMap<usize, (u64, u32)>,
    /// Judge support — empty/false unless a judge is attached. `abort_handles` lets the judge kill a
    /// stuck worker's future; `prior_hints` carries the judge's corrective note onto the re-dispatch;
    /// `interventions` caps kills per task; `judge_running` keeps at most one judge in flight at a time
    /// (never two judging the same worker); `idle_jobs` counts ALL running idle jobs (the judge + any
    /// pre-reviews) so up to `idle_capacity()` run CONCURRENTLY — one per free node — instead of the old
    /// single shared slot that let the judge starve pre-review and left a second idle node asleep.
    abort_handles: HashMap<TaskId, tokio::task::AbortHandle>,
    prior_hints: HashMap<TaskId, String>,
    /// Every corrective note the judge has produced this run, in order, and NEVER consumed.
    ///
    /// `prior_hints` is keyed by task and REMOVED on the next dispatch of that task, so a judge
    /// finding survives exactly one re-dispatch and then vanishes. That is right for guiding a retry
    /// and wrong for everything else — most of all for the SINK, which is told "you are the ONLY task
    /// permitted to edit files here" and whose entire job is fixing what upstream found.
    ///
    /// MEASURED: the judge caught a real defect mid-run — "EXPECTED_SORTED_IDS has wrong order,
    /// pay_005 at +01:00 converts to 07:00Z (earliest), not pay_002" — handed it to `test-meridian`,
    /// and it was consumed. The sink then spent roughly 20 of its 30 minutes REDISCOVERING that same
    /// bug: six overlapping `sed` reads of test_meridian.py and a hand-written python one-liner
    /// recomputing the very sort the judge had already worked out. The information existed; nothing
    /// carried it to the one task that could act on it.
    judge_notes: Vec<(TaskId, String)>,
    interventions: HashMap<TaskId, u32>,
    /// Omni-judge aborts per task. Counted SEPARATELY from `interventions` on purpose: that map also caps
    /// how many times the deterministic judge may act on a task (max_interventions_per_task), and spending
    /// that budget on a model's reasoning-loop abort would leave a genuinely stuck task with no
    /// deterministic supervisor at the point it needs one most.
    omni_aborts: HashMap<TaskId, u32>,
    /// Split generation per task: 0 for original tasks, parent+1 for children injected by a split. Feeds
    /// JudgeRequest.split_count so the judge caps splitting at once (a split-child is never re-split).
    split_generation: HashMap<TaskId, u32>,
    judge_running: bool,
    /// Which node is running the CURRENT judge. A single Option is sufficient and correct because
    /// `judge_running` makes the judge single-flight — `judge_observed` and `judge_verdict` counts
    /// match exactly (103/103, 72/72, 64/64, 43/43) across every archived run, and they never
    /// interleave. If that invariant is ever relaxed this must become a per-task map.
    judge_node: Option<String>,
    task_salvaged: std::collections::HashMap<String, bool>,
    idle_jobs: u32,
    /// SINK IDLE-FILL (GOOSE_SWARM_SINK_REVIEW): rotating review-dimension index for idle nodes during the
    /// sink, so successive idle reviews cover different angles.
    sink_review_dim: usize,
    /// When each task was last judged, so an OK ("observed") task is NOT re-judged every 15s tick for its
    /// whole life — that fired ~4 wasted model calls/min on a single long worker, which LM Studio piled onto
    /// a busy node (one node "+1 QUEUED" while another sat idle). A re-judge waits `JUDGE_REJUDGE_COOLDOWN`.
    last_judged: HashMap<TaskId, Instant>,
    /// SPECULATIVE EXECUTION (GOOSE_SWARM_SPECULATE, default-OFF). When a node would otherwise sit idle at a
    /// serial dependency chokepoint, a TWIN of the in-flight task is raced on the idle device (first-to-finish
    /// wins). The twin runs in a shadow workspace (dispatcher side) so it never touches `held_files` — only
    /// the PRIMARY ever holds the real owned files. These maps track the twin's OWN device claim, keyed by
    /// the task id; `speculating` marks a task that currently has a twin. All empty unless the flag is on, so
    /// the validated path is byte-identical.
    spec_device: HashMap<TaskId, usize>,
    spec_started_at: HashMap<TaskId, Instant>,
    spec_abort: HashMap<TaskId, tokio::task::AbortHandle>,
    speculating: HashSet<TaskId>,
    spec_count: u32,
}

impl State {
    fn all_terminal(&self) -> bool {
        self.dag
            .tasks
            .values()
            .all(|n| matches!(n.state, TaskState::Done | TaskState::Failed))
    }

    /// Tasks not yet terminal. The replan re-arm keys off this: a decline is only stale once the DAG
    /// has actually shrunk.
    fn incomplete_count(&self) -> usize {
        self.dag
            .tasks
            .values()
            .filter(|n| !matches!(n.state, TaskState::Done | TaskState::Failed))
            .count()
    }

    /// The worker session this task ran in, if one was recorded.
    ///
    /// Every FAILURE emit site hard-coded `session_id: None`, so a failed task's full trace — every
    /// tool request and response in the sessions DB — was unjoinable, which is precisely the task you
    /// most want to read. The map was already there and already populated on dispatch; line ~1944
    /// performs this exact lookup for another event. Same class of defect as the missing `error`:
    /// the engine had the value and the event dropped it.
    fn task_session_id(&self, tid: &str) -> Option<String> {
        self.task_session.get(tid).cloned().flatten()
    }

    /// The reason the task's LAST attempt ended, or `None` if it succeeded.
    ///
    /// One helper rather than six inline expressions, because six copies of a rule is how the
    /// dispatch paths drifted apart before (`pick_device` learned speed-weight routing and the repair
    /// path did not). Every `TaskCompleted` reads this, so a successful task naturally reports `None`
    /// — the winning attempt carries no error — and a failure reports the string the engine already
    /// had and used to discard.
    fn last_attempt_error(&self, tid: &str) -> Option<String> {
        self.attempt_log
            .get(tid)
            .and_then(|a| a.last())
            .and_then(|r| r.error.clone())
    }

    fn total_in_flight(&self) -> u32 {
        self.devices.iter().map(|d| d.in_flight).sum()
    }

    /// Free worker slots across enabled devices (how much parallel work could start right now).
    fn idle_capacity(&self) -> u32 {
        self.devices
            .iter()
            .filter(|d| d.cfg.enabled)
            .map(|d| d.cfg.weight.saturating_sub(d.in_flight))
            .sum()
    }

    /// K1: is the integrate-verify SINK the (only) in-flight task? Dynamic-replan is suppressed in this
    /// window — the sink verifies-by-running the whole tree and owns NO files, so a bonus task completing
    /// here could land UNVERIFIED code AFTER the sink's PASS. Before the sink starts (its deps are every
    /// other task, so it runs alone at the end) other tasks are still in flight and replan is fine; this
    /// only guards the exact sink-race window.
    fn sink_in_flight(&self) -> bool {
        self.dag
            .tasks
            .iter()
            .any(|(id, n)| n.state == TaskState::Claimed && id.as_str() == "integrate-verify")
    }

    fn make_replan_context(&self) -> ReplanContext {
        let mut completed = Vec::new();
        let mut failed = Vec::new();
        let mut incomplete = Vec::new();
        for (id, n) in &self.dag.tasks {
            match n.state {
                TaskState::Done => completed.push((
                    id.clone(),
                    n.result
                        .clone()
                        .unwrap_or_default()
                        .chars()
                        .take(400)
                        .collect(),
                )),
                TaskState::Failed => failed.push(id.clone()),
                _ => incomplete.push(id.clone()),
            }
        }
        ReplanContext {
            goal: self.goal.clone(),
            existing_ids: self.dag.tasks.keys().cloned().collect(),
            completed,
            failed,
            incomplete,
            idle_capacity: self.idle_capacity(),
            round: self.replans_done.saturating_sub(1),
        }
    }

    fn files_conflict(&self, tid: &str) -> bool {
        self.dag.tasks[tid]
            .spec
            .owned_files
            .iter()
            .any(|f| self.held_files.contains(f))
    }

    /// Choose a device for a ready task: prefer its suggested model, avoiding the device that just
    /// failed it on a transient retry; otherwise the least-loaded enabled device with free capacity.
    fn pick_device(&self, tid: &str) -> Option<usize> {
        let n = &self.dag.tasks[tid];
        let free: Vec<usize> = self
            .devices
            .iter()
            .enumerate()
            .filter(|(_, d)| d.cfg.enabled && d.in_flight < d.cfg.weight)
            .map(|(i, _)| i)
            .collect();
        if free.is_empty() {
            return None;
        }
        let allowed: Vec<usize> = free
            .iter()
            .copied()
            .filter(|&i| n.avoid_device.as_deref() != Some(self.devices[i].cfg.id.as_str()))
            .collect();
        // If avoiding the failed device leaves nothing, fall back to any free device.
        let pool = if allowed.is_empty() { free } else { allowed };
        // Spread work across the fleet: the LEAST-LOADED device wins, so idle nodes get work before
        // any node doubles up; ties break toward the planner's preferred model, then by index for
        // determinism. (Honoring preferred_model first would pile every same-model task on one device
        // and leave the rest of the fleet idle — the opposite of what a swarm is for.)
        let pm = n.spec.preferred_model.as_deref();
        // A HARD task (the heaviest work, incl. integrate-verify) prefers the FASTEST free node: identical
        // models differ only in host speed, so the critical path shrinks if the big tasks land on the
        // quickest node. Load (in_flight) stays primary, so this never over-concentrates.
        let hard = matches!(n.spec.difficulty, Difficulty::Hard);
        pool.into_iter().min_by_key(|&i| {
            let d = &self.devices[i];
            let sw = d.cfg.speed_weight.max(1) as u64;
            let prefers_rank = match pm {
                Some(m) if m == d.cfg.model_id => 0,
                _ => 1,
            };
            // Hard-task speed: real observed avg ms/task if known (lower = faster). If not yet observed,
            // SEED from the configured speed_weight so the heaviest task lands on the known-fastest host
            // from the very first dispatch (higher speed_weight -> smaller key -> preferred).
            let speed = if hard {
                self.device_speed
                    .get(&i)
                    .map(|(t, c)| t / (*c).max(1) as u64)
                    .unwrap_or(u64::MAX - sw)
            } else {
                0
            };
            // SPEED-WEIGHTED share of the load: normalize the dispatch count by speed_weight so a faster
            // host accumulates proportionally MORE tasks before it is "even" (≈ ratio of speed_weights),
            // while the slowest host gets far fewer. Also rotates work so no host is starved.
            let weighted_load = self
                .dispatched_per_device
                .get(&d.cfg.id)
                .copied()
                .unwrap_or(0) as u64
                * 1000
                / sw;
            // ORDERING DEPENDS ON THE TASK, and this is the whole point of having weights.
            //
            // For a HARD task, SPEED IS PRIMARY: the biggest work must land on the highest-weighted
            // (fastest) node that has free capacity, every time. Every device in `pool` already passed
            // `in_flight < weight`, so preferring speed here can never oversubscribe a node — it only
            // decides WHICH free node wins. Previously `in_flight` was primary for every task, so speed
            // was a tie-break only: with the fastest host holding one in-flight task and a slower host
            // idle, the heaviest task went to the SLOW host. That is backwards — the critical path is
            // set by the big tasks, so those are exactly the ones that must not run on the slow node.
            //
            // For everything else load stays primary, which is what spreads ordinary work across the
            // fleet and keeps idle nodes busy.
            // WEIGHT IS DECISIVE FOR A HARD TASK — not a tie-break, and not overridable by a timing
            // sample. Inverted so a HIGHER speed_weight sorts FIRST; observed ms/task then breaks ties
            // among equally-weighted hosts, and load breaks ties after that.
            //
            // Ordering by observed speed alone was not enough: whichever host happened to be dispatched
            // first acquires a real average while the others are still `u64::MAX - sw`, so a single
            // sample on a slow host would beat the configured fastest host forever. The operator sets
            // these weights precisely because they know which machine is fastest; a first-dispatch
            // accident must not outrank that.
            let weight_rank = u32::MAX - d.cfg.speed_weight.max(1);
            if hard {
                (
                    weight_rank as u64,
                    speed,
                    d.in_flight as u64,
                    weighted_load,
                    i,
                )
            } else {
                (
                    d.in_flight as u64,
                    speed,
                    prefers_rank as u64,
                    weighted_load,
                    i,
                )
            }
        })
    }

    /// Claim as many ready tasks as can be placed right now (respecting weights + file holds).
    fn pick_assignments(&mut self) -> Vec<Assignment> {
        let mut out = Vec::new();
        let mut ranked: Vec<TaskId> = Vec::new();
        while let Some(r) = self.ready.pop() {
            ranked.push(r.id);
        }
        let mut leftover: Vec<TaskId> = Vec::new();
        for tid in ranked {
            if self.dag.tasks[&tid].state != TaskState::Ready {
                continue; // defensive: stale heap entry
            }
            if self.files_conflict(&tid) {
                leftover.push(tid);
                continue;
            }
            match self.pick_device(&tid) {
                Some(dev) => self.do_claim(tid, dev, &mut out),
                None => leftover.push(tid),
            }
        }
        for tid in leftover {
            let fan_out = self.dag.tasks[&tid].fan_out;
            self.ready.push(Ranked { fan_out, id: tid });
        }
        out
    }

    fn do_claim(&mut self, tid: TaskId, dev: usize, out: &mut Vec<Assignment>) {
        let deps = self.dag.tasks[&tid].spec.deps.clone();
        let neighborhood = self.neighborhood_of(&tid, &deps);
        let slice = self.ctx.slice_for(&deps);
        let (files, description, attempt) = {
            let n = self.dag.tasks.get_mut(&tid).unwrap();
            n.state = TaskState::Claimed;
            (
                n.spec.owned_files.clone(),
                n.spec.description.clone(),
                n.attempts,
            )
        };
        for f in &files {
            self.held_files.insert(f.clone());
        }
        self.devices[dev].in_flight += 1;
        self.claimed_device.insert(tid.clone(), dev);
        let device_id = self.devices[dev].cfg.id.clone();
        let model_id = self.devices[dev].cfg.model_id.clone();
        *self
            .dispatched_per_device
            .entry(device_id.clone())
            .or_default() += 1;
        self.attempt_started_at.insert(tid.clone(), Instant::now());
        self.task_final_device
            .insert(tid.clone(), (device_id.clone(), model_id.clone()));
        self.sink.emit(&SwarmEvent::TaskDispatched {
            task_id: tid.clone(),
            device: device_id.clone(),
            model: model_id.clone(),
            attempt,
            deps,
            owned_files: files.clone(),
            context_slice_len: slice.len(),
        });
        let owned_files = files.clone();
        let mut all_files: Vec<String> = self
            .dag
            .tasks
            .values()
            .flat_map(|n| n.spec.owned_files.iter().cloned())
            .collect();
        all_files.sort();
        all_files.dedup();
        self.held_by.insert(tid.clone(), files);
        let mut prior_hint = self.prior_hints.remove(&tid);
        // THE SINK INHERITS EVERY JUDGE FINDING, because it is the only task that can act on them and
        // the judge is a source of findings it otherwise cannot see. A task that owns no files and
        // joins the graph is the sink; ordinary workers keep the existing one-shot behaviour.
        if owned_files.is_empty() && !self.judge_notes.is_empty() {
            let notes = self
                .judge_notes
                .iter()
                .map(|(t, h)| format!("- [{t}] {h}"))
                .collect::<Vec<_>>()
                .join("\n");
            let block = format!(
                "WHAT THE SUPERVISOR ALREADY FOUND while these tasks ran — each was reported to the \
                 worker at the time, but you are the only task that can still act on it. Treat these \
                 as leads you do NOT need to rediscover:\n{notes}"
            );
            prior_hint = Some(match prior_hint {
                Some(existing) => format!("{existing}\n\n{block}"),
                None => block,
            });
        }
        out.push(Assignment {
            task_id: tid.clone(),
            request: DispatchRequest {
                task_id: tid,
                description,
                device_id,
                model_id,
                context_slice: slice,
                attempt,
                owned_files,
                all_files,
                prior_hint,
                speculative: false,
                // The user's own words reach a worker for the first time here. Every other channel the
                // engine claimed (research_findings, the amended spec) is planner-side only.
                user_decisions: self.user_decisions.clone(),
                doc_facts: self.doc_facts.clone(),
                neighborhood,
            },
        });
    }

    /// The DAG neighborhood of `tid`: its deps ∪ its consumers (reverse edges) ∪ itself, deduped. Used to
    /// scope the frozen-contract bundle to only the modules a worker touches.
    fn neighborhood_of(&self, tid: &str, deps: &[TaskId]) -> Vec<String> {
        let mut n: Vec<String> = deps.to_vec();
        if let Some(consumers) = self.dag.dependents.get(tid) {
            n.extend(consumers.iter().cloned());
        }
        n.push(tid.to_string());
        n.sort();
        n.dedup();
        n
    }

    /// Relax every dependent of a just-finished task: drop its indegree and promote it to Ready at zero.
    /// MUST run for BOTH a normal success AND a finalize-spin salvage (both leave the task Done) — otherwise
    /// a salvaged task leaves its dependents Pending forever, so the CLI/integrate-verify sink never
    /// dispatches and the run ends `scheduler_stuck`. Observed on expense/tmpl: a working library or a
    /// spun-but-written CLI shipped with the entry/verify tasks never run.
    fn relax_dependents(&mut self, tid: &str) {
        let dependents = self.dag.dependents.get(tid).cloned().unwrap_or_default();
        for d in dependents {
            let nd = self.dag.tasks.get_mut(&d).unwrap();
            if nd.indegree_remaining > 0 {
                nd.indegree_remaining -= 1;
            }
            if nd.indegree_remaining == 0 && nd.state == TaskState::Pending {
                nd.state = TaskState::Ready;
                let fan_out = nd.fan_out;
                self.ready.push(Ranked { fan_out, id: d });
            }
        }
    }

    fn complete(&mut self, tid: &str, attempt: u32, res: Result<TaskRunOutput, DispatchError>) {
        // Ignore a completion from an attempt the judge already superseded (killed + re-dispatched):
        // its device and file holds were released when the judge intervened, so this stale future must
        // not touch the newer attempt's bookkeeping. `attempts` advances on every kill/retry, so a
        // mismatch uniquely identifies a dead attempt.
        if self.dag.tasks.get(tid).map(|n| n.attempts) != Some(attempt) {
            return;
        }
        // SPECULATIVE first-wins: if the task is no longer Claimed, the other instance (primary or twin) of
        // this attempt already accepted it -> this completion is the loser; do nothing (each instance's own
        // device was released by its own path). With speculation OFF no twin exists and the task is always
        // Claimed here, so this guard never triggers (byte-identical).
        if self.dag.tasks.get(tid).map(|n| n.state) != Some(TaskState::Claimed) {
            return;
        }
        self.abort_handles.remove(tid);
        let released_dev = self.claimed_device.remove(tid);
        let released_dev_id = released_dev.map(|i| self.devices[i].cfg.id.clone());
        if let Some(dev) = released_dev {
            if self.devices[dev].in_flight > 0 {
                self.devices[dev].in_flight -= 1;
            }
        }
        if let Some(files) = self.held_by.remove(tid) {
            for f in files {
                self.held_files.remove(&f);
            }
        }

        let elapsed_ms = self
            .attempt_started_at
            .remove(tid)
            .map(|t| t.elapsed().as_millis() as u64)
            .unwrap_or(0);
        let (dev_id, model_id) = match self.task_final_device.get(tid).cloned() {
            Some((d, m)) => (Some(d), Some(m)),
            None => (None, None),
        };

        match res {
            Ok(run) => {
                // Record this device's throughput (successful completions only) for speed-aware routing.
                if let Some(dev) = released_dev {
                    let e = self.device_speed.entry(dev).or_insert((0, 0));
                    e.0 += elapsed_ms;
                    e.1 += 1;
                }
                let TaskRunOutput {
                    output,
                    session_id,
                    tool_calls,
                    salvaged,
                } = run;
                // Remembered per task, because the completion event is emitted from six sites and only
                // one of them is on the path that produced this value.
                self.task_salvaged.insert(tid.to_string(), salvaged);
                self.task_session
                    .insert(tid.to_string(), session_id.clone());
                self.task_tool_calls
                    .insert(tid.to_string(), tool_calls.clone());
                self.attempt_log
                    .entry(tid.to_string())
                    .or_default()
                    .push(AttemptRecord {
                        device: dev_id.clone(),
                        model: model_id.clone(),
                        outcome: "ok".to_string(),
                        error: None,
                        elapsed_ms,
                    });
                let attempts = self.attempt_log[tid].len() as u32;
                {
                    let n = self.dag.tasks.get_mut(tid).unwrap();
                    n.state = TaskState::Done;
                    n.result = Some(output.clone());
                    n.avoid_device = None;
                }
                self.ctx.merge(tid, output);
                let ended_because = self.last_attempt_error(tid);
                self.sink.emit(&SwarmEvent::TaskCompleted {
                    task_id: tid.to_string(),
                    salvaged: self.task_salvaged.get(tid).copied().unwrap_or(false),
                    status: "done".to_string(),
                    device: dev_id,
                    model: model_id,
                    attempts,
                    elapsed_ms,
                    session_id,
                    error: ended_because,
                    tool_calls,
                });
                self.relax_dependents(tid);
                // SPECULATIVE abort-loser: this PRIMARY won -> abort + release any twin still racing this
                // task. (When the TWIN won, resolve_speculation cleared `speculating` BEFORE calling
                // complete(), so this is a no-op there.) Off by default -> the maps are empty -> no-op.
                if self.speculating.remove(tid) {
                    if let Some(h) = self.spec_abort.remove(tid) {
                        h.abort();
                    }
                    if let Some(dev) = self.spec_device.remove(tid) {
                        if self.devices[dev].in_flight > 0 {
                            self.devices[dev].in_flight -= 1;
                        }
                    }
                    self.spec_started_at.remove(tid);
                }
            }
            Err(e @ (DispatchError::Transient(_) | DispatchError::ContentRetry(_))) => {
                // A CONTENT failure (pre-done syntax gate) is re-dispatched exactly like a Transient, but its
                // error is threaded into the retry's prior_hint so the fix is guided. Infra transients are not.
                let (msg, is_content) = match e {
                    DispatchError::Transient(m) => (m, false),
                    DispatchError::ContentRetry(m) => (m, true),
                    DispatchError::Terminal(_) => unreachable!(),
                };
                self.attempt_log
                    .entry(tid.to_string())
                    .or_default()
                    .push(AttemptRecord {
                        device: dev_id.clone(),
                        model: model_id.clone(),
                        outcome: if is_content {
                            "content_retry"
                        } else {
                            "transient"
                        }
                        .to_string(),
                        error: Some(msg.clone()),
                        elapsed_ms,
                    });
                // An OMNI-JUDGE abort is supervision too: a model read the call's own reasoning and stopped
                // it. It arrives as a plain Transient with no intervention increment, so without this it
                // burned the task's retry budget — the exact cost the judge-kill exclusion below exists to
                // avoid. It bites hardest on a `verify::` task, which owns no files: the progress-watchdog
                // salvage path is disabled for those, so every omni abort was a pure budget burn pushing an
                // otherwise-healthy verify toward Failed.
                if msg.contains("the judge read this call's own reasoning") {
                    *self.omni_aborts.entry(tid.to_string()).or_insert(0) += 1;
                }
                let exhausted = {
                    // Judge kills advance n.attempts (for the epoch guard) but are SUPERVISORY, not task
                    // failures — and the judge can be wrong (a borderline over-read). Don't let a judge
                    // intervention burn the transient-retry budget: exclude it from the exhaustion count.
                    let judge_kills = self.interventions.get(tid).copied().unwrap_or(0)
                        + self.omni_aborts.get(tid).copied().unwrap_or(0);
                    let n = self.dag.tasks.get_mut(tid).unwrap();
                    n.attempts += 1;
                    // A task that OWNS NOTHING CANNOT RESUME. Every re-dispatch redoes an unbounded
                    // amount of work from zero — for `integrate-verify` that is the entire join, every
                    // command re-run and every fix re-derived, while the whole fleet waits on it.
                    //
                    // One retry absorbs a genuine early blip. Past that, each further restart pays the
                    // full cost again for a fault that says nothing about the work: MEASURED, two of
                    // three sink retries were `stream decode error (mid-stream body drop)` on two
                    // DIFFERENT devices, costing 916s and 1860s of pure redo. Degrading instead records
                    // the check as unfinished, which gates nothing (`green_blocking_failed` already
                    // excludes owns-nothing from the green veto) and frees the fleet.
                    let cap = if n.spec.owned_files.is_empty() {
                        self.max_attempts.min(2)
                    } else {
                        self.max_attempts
                    };
                    n.attempts.saturating_sub(judge_kills) >= cap
                };
                if exhausted {
                    // DEGRADE-ON-STALL (#134/#132): a transient exhaustion is usually a mid-generation model
                    // hang AFTER the worker already wrote its owned file (evidence a366f2b3: stalled workers
                    // emit events for hundreds of seconds and write their file, then the stream goes silent).
                    // If the critical owned file is on disk, mark Done(degraded) + relax dependents so a single
                    // hung core task does not kill the capstone; integrate-verify + R1 gate the file honestly.
                    // NEVER a CONTENT failure (a syntax-gate reject is a broken file), never a test task, and
                    // only when the critical files are actually present. OFF by default => byte-identical.
                    let degrade = self.dag.tasks.get(tid).is_some_and(|n| {
                        should_degrade_on_stall(
                            self.degrade_on_stall,
                            is_content,
                            &n.spec.id,
                            &n.spec.owned_files,
                        )
                    });
                    let attempts = self.attempt_log[tid].len() as u32;
                    if degrade {
                        self.dag.tasks.get_mut(tid).unwrap().state = TaskState::Done;
                        self.sink.emit(&SwarmEvent::JudgeVerdict {
                            task_id: tid.to_string(),
                            device: dev_id.clone().unwrap_or_default(),
                            judge_node: self.judge_node.clone().unwrap_or_default(),
                            verdict: "degraded_stall".to_string(),
                            confidence: 1.0,
                            hint: if self
                                .dag
                                .tasks
                                .get(tid)
                                .is_some_and(|n| n.spec.owned_files.is_empty())
                            {
                                "stall-exhausted and owns no files; recorded unfinished rather than \
                                 restarted — it gates no green either way"
                                    .to_string()
                            } else {
                                "stall-exhausted but owned file written; integrate-verify gates it"
                                    .to_string()
                            },
                            action: "degraded".to_string(),
                            // The scheduler's own stall accounting, not a judge opinion.
                            deterministic: true,
                        });
                        self.relax_dependents(tid);
                        let ended_because = self.last_attempt_error(tid);
                        self.sink.emit(&SwarmEvent::TaskCompleted {
                            task_id: tid.to_string(),
                            salvaged: self.task_salvaged.get(tid).copied().unwrap_or(false),
                            status: "done".to_string(),
                            device: dev_id,
                            model: model_id,
                            attempts,
                            elapsed_ms,
                            session_id: self.task_session_id(tid),
                            error: ended_because,
                            tool_calls: Vec::new(),
                        });
                    } else {
                        self.dag.tasks.get_mut(tid).unwrap().state = TaskState::Failed;
                        self.fail_descendants(tid);
                        let ended_because = self.last_attempt_error(tid);
                        self.sink.emit(&SwarmEvent::TaskCompleted {
                            task_id: tid.to_string(),
                            salvaged: self.task_salvaged.get(tid).copied().unwrap_or(false),
                            status: "failed".to_string(),
                            device: dev_id,
                            model: model_id,
                            attempts,
                            elapsed_ms,
                            session_id: self.task_session_id(tid),
                            error: ended_because,
                            tool_calls: Vec::new(),
                        });
                    }
                } else {
                    {
                        let n = self.dag.tasks.get_mut(tid).unwrap();
                        n.avoid_device = released_dev_id.clone();
                        n.state = TaskState::Ready;
                        let fan_out = n.fan_out;
                        self.ready.push(Ranked {
                            fan_out,
                            id: tid.to_string(),
                        });
                    }
                    // Guided retry: thread the content error into the next attempt's prior_hint (surfaced to
                    // the worker as a SUPERVISOR NOTE). Infra transients carry no hint — a stale content note
                    // on a "model unloaded" retry would mislead the worker.
                    if is_content {
                        self.prior_hints.insert(tid.to_string(), msg.clone());
                        self.judge_notes.push((tid.to_string(), msg.clone()));
                    }
                    self.sink.emit(&SwarmEvent::TaskRetry {
                        task_id: tid.to_string(),
                        from_device: released_dev_id,
                        error: msg,
                        transient: true,
                    });
                }
            }
            Err(DispatchError::Terminal(msg)) => {
                self.attempt_log
                    .entry(tid.to_string())
                    .or_default()
                    .push(AttemptRecord {
                        device: dev_id.clone(),
                        model: model_id.clone(),
                        outcome: "terminal".to_string(),
                        error: Some(msg),
                        elapsed_ms,
                    });
                self.dag.tasks.get_mut(tid).unwrap().state = TaskState::Failed;
                self.fail_descendants(tid);
                let attempts = self.attempt_log[tid].len() as u32;
                let ended_because = self.last_attempt_error(tid);
                self.sink.emit(&SwarmEvent::TaskCompleted {
                    task_id: tid.to_string(),
                    salvaged: self.task_salvaged.get(tid).copied().unwrap_or(false),
                    status: "failed".to_string(),
                    device: dev_id,
                    model: model_id,
                    attempts,
                    elapsed_ms,
                    session_id: self.task_session_id(tid),
                    error: ended_because,
                    tool_calls: Vec::new(),
                });
            }
        }
    }

    /// Choose an in-flight worker for the judge to inspect: the longest-running Claimed task that is at
    /// least `min_age_secs` old and under its intervention cap, to be judged on a currently-idle device.
    /// Returns the request + the attempt inspected, and marks a judge running (at most one at a time).
    fn pick_judge_target(
        &mut self,
        cfg: &JudgeConfig,
    ) -> Option<(JudgeRequest, u32, Option<usize>)> {
        // The LLM review wants an idle device; the deterministic checks (won't-compile / no-output /
        // wrote-then-stale) need no model at all. CLAIM an idle device's slot for the review (so a worker +
        // the next idle-job never stack on it), but fall through with no claim + an empty model_id so the
        // deterministic verdicts still fire when every node is busy (saturated) — a stuck worker must not go
        // unjudged. The actual claim (in_flight bump) happens at the end, only if a task is selected.
        let claimed_device = self
            .devices
            .iter()
            .position(|d| d.cfg.enabled && d.in_flight < d.cfg.weight);
        let judge_model_id = claimed_device
            .map(|i| self.devices[i].cfg.model_id.clone())
            .unwrap_or_default();
        // Two pools: `best` = under-cap tasks (normal judging — re-dispatch on a problem); `best_terminal`
        // = cap-exhausted tasks, surfaced ONLY so the judge can make a terminal decision (a task already
        // re-dispatched to its cap that is STILL broken should be failed, not left to spin a node to
        // worker_max_turns). Under-cap judging is always preferred so a cap-exhausted-but-fine task can
        // never starve a genuinely-stuck one of the single judge slot.
        let mut best: Option<(String, u64)> = None;
        let mut best_terminal: Option<(String, u64)> = None;
        for (tid, n) in &self.dag.tasks {
            if n.state != TaskState::Claimed {
                continue;
            }
            let elapsed = self
                .attempt_started_at
                .get(tid)
                .map(|t| t.elapsed().as_secs())
                .unwrap_or(0);
            if elapsed < cfg.min_age_secs {
                continue;
            }
            let at_cap =
                self.interventions.get(tid).copied().unwrap_or(0) >= cfg.max_interventions_per_task;
            // Re-judge cooldown: an already-judged task is not re-inspected until JUDGE_REJUDGE_COOLDOWN_SECS
            // has passed, so an OK long worker is not re-judged every tick (the wasted-call/queue-on-busy-node
            // problem). Applies ONLY to UNDER-CAP re-judging — a cap-exhausted stuck task is NEVER cooled down,
            // so its terminal-fail stays prompt. The first judge is gated only by min_age_secs above.
            if !at_cap
                && self
                    .last_judged
                    .get(tid)
                    .map(|t| t.elapsed().as_secs() < cfg.rejudge_cooldown_secs)
                    .unwrap_or(false)
            {
                continue;
            }
            // Skip RE-judging an owns-NOTHING task (the integrate-verify sink). Every deterministic
            // judge gate is disarmed for it (over-read/finalize-spin/broken-code all require owned
            // files, judge.rs:292/311/332), and its LLM verdict is always a non-actionable "ok", so a
            // re-judge catches nothing yet steals an idle node from sink-review. Judge it ONCE (first
            // pass, for observability) then leave it to worker_timeout as the hard-stall backstop.
            // …that rationale held while NO verdict could fire for an owns-nothing task. One can now:
            // the Accept branch for a join that has acted and then gone quiet (judge.rs). Judging it
            // once and never again would make that branch unreachable, because a first pass early in
            // the join is always too young for it. So the skip now applies only while the task IS too
            // young for that branch — the `rejudge_cooldown_secs` check above still throttles the rest,
            // so this cannot spin re-judges at a sink-review node.
            if n.spec.owned_files.is_empty()
                && self.last_judged.contains_key(tid)
                && elapsed < cfg.min_age_secs.max(420)
            {
                continue;
            }
            let slot = if at_cap {
                &mut best_terminal
            } else {
                &mut best
            };
            if slot.as_ref().map(|(_, e)| elapsed > *e).unwrap_or(true) {
                *slot = Some((tid.clone(), elapsed));
            }
        }
        let (tid, elapsed) = best.or(best_terminal)?;
        let (description, owned_files, attempt) = {
            let n = &self.dag.tasks[&tid];
            (
                n.spec.description.clone(),
                n.spec.owned_files.clone(),
                n.attempts,
            )
        };
        // High-level run state for the semantic judge: completed tasks (with a brief of what each
        // produced), the tasks still in flight / pending, and the failed ones. Lets it judge this worker
        // against the whole build — catch it re-doing finished work, depending on a failed task, or
        // diverging from the shape the rest of the run already set.
        let mut done = Vec::new();
        let mut remaining = Vec::new();
        let mut failed = Vec::new();
        for (id, node) in &self.dag.tasks {
            if id == &tid {
                continue;
            }
            match node.state {
                TaskState::Done => done.push((
                    id.clone(),
                    node.result
                        .clone()
                        .unwrap_or_default()
                        .chars()
                        .take(200)
                        .collect(),
                )),
                TaskState::Failed => failed.push(id.clone()),
                _ => remaining.push(id.clone()),
            }
        }
        let req = JudgeRequest {
            task_id: tid.clone(),
            description,
            owned_files,
            elapsed_secs: elapsed,
            judge_model_id,
            goal: self.goal.clone(),
            done,
            remaining,
            failed,
            split_count: self.split_generation.get(&tid).copied().unwrap_or(0),
        };
        self.judge_running = true;
        // Record the JUDGING node before the call, not after: the verdict emit reads
        // `task_final_device`, which is the judged worker. `None` here is meaningful — it is the
        // deterministic-only path where no device was claimed and no inference was spent.
        self.judge_node = claimed_device.map(|i| self.devices[i].cfg.model_id.clone());
        self.idle_jobs += 1;
        self.last_judged.insert(tid.clone(), Instant::now());
        // Claim the idle device's slot now that the judge is actually firing, so a worker dispatch (which
        // sorts by in_flight) + the next idle-job avoid this node. Released by the IdleSlotGuard.
        if let Some(i) = claimed_device {
            self.devices[i].in_flight += 1;
        }
        Some((req, attempt, claimed_device))
    }

    /// M5: pick a COMPLETED-but-unreviewed task (that owns files) for an idle-node correctness pre-review,
    /// claiming one idle-job slot (does NOT take the single-judge flag, so it runs concurrently with the
    /// judge on a different free node). Returns the request, or None if no idle device is free, nothing is
    /// reviewable, or all idle slots are taken. Marks the task pre_reviewed up front so it is picked at most
    /// once even while the review is in flight.
    fn pick_prereview_request(&mut self) -> Option<(PreReviewRequest, usize)> {
        let claimed_device = self
            .devices
            .iter()
            .position(|d| d.cfg.enabled && d.in_flight < d.cfg.weight)?;
        let reviewer_model_id = self.devices[claimed_device].cfg.model_id.clone();
        let tid = self
            .dag
            .tasks
            .iter()
            .find(|(_, n)| {
                n.state == TaskState::Done && !n.pre_reviewed && !n.spec.owned_files.is_empty()
            })
            .map(|(id, _)| id.clone())?;
        let (description, owned_files) = {
            let n = &self.dag.tasks[&tid];
            (n.spec.description.clone(), n.spec.owned_files.clone())
        };
        self.dag.tasks.get_mut(&tid).unwrap().pre_reviewed = true;
        self.idle_jobs += 1;
        // Claim the idle device's slot so a worker dispatch + the next idle-job avoid this node. Released by
        // the IdleSlotGuard.
        self.devices[claimed_device].in_flight += 1;
        Some((
            PreReviewRequest {
                task_id: tid,
                description,
                owned_files,
                goal: self.goal.clone(),
                reviewer_model_id,
            },
            claimed_device,
        ))
    }

    /// SINK IDLE-FILL (GOOSE_SWARM_SINK_REVIEW): while the integrate-verify SINK runs SOLO and pre-review is
    /// exhausted, claim a genuinely-free device (never the sink's — it is at weight) for a READ-ONLY
    /// whole-tree dimension review, rotating the dimension. Returns (model_id, dim_index, goal, device).
    /// None unless the flag is on AND the sink is in flight AND a device is free (mirrors pick_prereview's
    /// claim so it never oversubscribes). Released by the IdleSlotGuard.
    fn pick_sink_review(&mut self) -> Option<(String, usize, String, usize)> {
        // ONE default, shared with the consumer. These two halves disagreed: this producer defaulted
        // OFF while run_swarm's drain and `levers_resolved` both defaulted ON — so every run REPORTED
        // sink_review enabled, the queue was never filled, `prewarmed` was always empty and the event
        // never fired. Measured as a real zero across three runs before the cause was found, and an
        // operator auditing levers would have read `sink_review: true` and believed it.
        //
        // This is the mechanism that exists to fill the biggest idle window there is: the SINK owns
        // 100% of the solo time in 2 of 3 measured runs (543-1045s with two nodes idle). It has never
        // run once.
        //
        // The default stays OFF — the truthful one, matching every measurement taken so far — so
        // baseline does not shift underneath the campaign. Turning it on is an ARM, not a silent flip.
        if !sink_review_enabled() || !self.sink_in_flight() {
            return None;
        }
        let claimed_device = self
            .devices
            .iter()
            .position(|d| d.cfg.enabled && d.in_flight < d.cfg.weight)?;
        let model_id = self.devices[claimed_device].cfg.model_id.clone();
        let dim = self.sink_review_dim;
        self.sink_review_dim = self.sink_review_dim.wrapping_add(1);
        self.idle_jobs += 1;
        self.devices[claimed_device].in_flight += 1;
        Some((model_id, dim, self.goal.clone(), claimed_device))
    }

    /// SPECULATIVE EXECUTION: pick a TWIN to race on an idle device. Choose the longest-running Claimed task
    /// that is NOT already being speculated and whose PRIMARY is on a DIFFERENT device than the idle one (so
    /// the twin truly runs on a free node — 1 task per node). Builds the same DispatchRequest the primary got
    /// but `speculative: true`, and claims the twin's OWN device slot + spec_* maps WITHOUT touching
    /// held_files / claimed_device / the task's Claimed state (only the primary holds the real files).
    fn pick_speculation_target(&mut self) -> Option<(DispatchRequest, usize)> {
        let dev = self
            .devices
            .iter()
            .position(|d| d.cfg.enabled && d.in_flight < d.cfg.weight)?;
        let mut best: Option<(TaskId, u64)> = None;
        for (tid, n) in &self.dag.tasks {
            if n.state != TaskState::Claimed || self.speculating.contains(tid) {
                continue;
            }
            // FAIL-CLOSE: never speculate a task that owns NO files (e.g. the injected integrate-verify
            // sink). A twin of such a task has nothing to promote, so a "win" would abort the primary and
            // commit a text-only merge while the twin's whole-tree edits stay stranded in its shadow —
            // dropping the integrator's files from the real tree. Only file-owning tasks are safe to
            // speculate. (This bounds the blast radius of the still-default-OFF speculation path; the full
            // promote/verify/join fix is tracked separately.)
            if n.spec.owned_files.is_empty() {
                continue;
            }
            if self.claimed_device.get(tid) == Some(&dev) {
                continue; // the twin must run on a DIFFERENT device than the primary
            }
            let elapsed = self
                .attempt_started_at
                .get(tid)
                .map(|t| t.elapsed().as_secs())
                .unwrap_or(0);
            if best.as_ref().map(|(_, e)| elapsed > *e).unwrap_or(true) {
                best = Some((tid.clone(), elapsed));
            }
        }
        let (tid, _elapsed) = best?;
        let deps = self.dag.tasks[&tid].spec.deps.clone();
        let neighborhood = self.neighborhood_of(&tid, &deps);
        let slice = self.ctx.slice_for(&deps);
        let (owned_files, description, attempt) = {
            let n = &self.dag.tasks[&tid];
            (
                n.spec.owned_files.clone(),
                n.spec.description.clone(),
                n.attempts,
            )
        };
        let device_id = self.devices[dev].cfg.id.clone();
        let model_id = self.devices[dev].cfg.model_id.clone();
        let mut all_files: Vec<String> = self
            .dag
            .tasks
            .values()
            .flat_map(|n| n.spec.owned_files.iter().cloned())
            .collect();
        all_files.sort();
        all_files.dedup();
        self.devices[dev].in_flight += 1;
        self.spec_device.insert(tid.clone(), dev);
        self.spec_started_at.insert(tid.clone(), Instant::now());
        self.speculating.insert(tid.clone());
        self.spec_count += 1;
        let req = DispatchRequest {
            task_id: tid,
            description,
            device_id,
            model_id,
            context_slice: slice,
            attempt,
            owned_files,
            all_files,
            prior_hint: None,
            speculative: true,
            user_decisions: self.user_decisions.clone(),
            doc_facts: self.doc_facts.clone(),
            neighborhood,
        };
        Some((req, dev))
    }

    /// Resolve a SPECULATIVE twin's completion. Releases the twin's OWN device + clears its spec_* maps
    /// (idempotent with the primary-win abort path). Then FIRST-WINS: if the task is no longer Claimed the
    /// PRIMARY already won -> the twin lost, nothing more to do. Otherwise the twin WON: on Ok, abort the
    /// primary's future and route the twin's output through `complete()` (which releases the primary's device
    /// + file hold and does Done/merge/relax); on Err, leave the primary running.
    fn resolve_speculation(
        &mut self,
        tid: &str,
        attempt: u32,
        res: Result<TaskRunOutput, DispatchError>,
    ) {
        if let Some(dev) = self.spec_device.remove(tid) {
            if self.devices[dev].in_flight > 0 {
                self.devices[dev].in_flight -= 1;
            }
        }
        self.spec_started_at.remove(tid);
        self.spec_abort.remove(tid);
        self.speculating.remove(tid);
        // The twin only wins if the task is STILL Claimed AND on the SAME attempt. The attempt check is
        // essential when a judge is also on: the judge can re-dispatch this task (bumping n.attempts) while a
        // twin of the OLD attempt is still running — without it, the stale twin would abort the healthy new
        // primary and, because complete()'s attempt guard then rejects the stale call, leak its device.
        // (Mirrors complete()'s and apply_judge_outcome()'s attempt guards.)
        let still_live = self
            .dag
            .tasks
            .get(tid)
            .map(|n| n.state == TaskState::Claimed && n.attempts == attempt)
            .unwrap_or(false);
        if !still_live {
            // The twin lost. Recorded rather than dropped: "speculation ran and the primary won" and
            // "speculation never ran" are opposite facts about whether an idle node bought anything,
            // and until now a run could not distinguish them.
            self.sink.emit(&SwarmEvent::Speculated {
                task_id: tid.to_string(),
                attempt,
                winner: "primary".to_string(),
            });
            return; // primary already won, OR the judge re-dispatched (attempt advanced) — the twin lost
        }
        if res.is_ok() {
            if let Some(h) = self.abort_handles.get(tid) {
                h.abort();
            }
            self.sink.emit(&SwarmEvent::Speculated {
                task_id: tid.to_string(),
                attempt,
                winner: "twin".to_string(),
            });
            self.complete(tid, attempt, res);
        } else {
            // A twin that ERRORED bought nothing and cost a device; that is the case worth seeing.
            self.sink.emit(&SwarmEvent::Speculated {
                task_id: tid.to_string(),
                attempt,
                winner: "twin_failed".to_string(),
            });
        }
        // On a twin Err: the primary keeps running; the twin's own device was already released above.
    }

    /// Apply a judge verdict. Always emits a `JudgeVerdict` event. If the verdict is an actionable
    /// problem, the inspected attempt is still the live one, the judge is confident enough, and the
    /// per-task intervention cap is not yet hit, the worker is killed and its task re-queued with the
    /// hint — otherwise the verdict is logged only (`observed`). The judge being a weak model is the
    /// reason these guards are strict.
    fn apply_judge_outcome(
        &mut self,
        tid: &str,
        attempt: u32,
        outcome: JudgeOutcome,
        cfg: &JudgeConfig,
    ) -> bool {
        let (device, model) = match self.task_final_device.get(tid) {
            Some((d, m)) => (Some(d.clone()), Some(m.clone())),
            None => (None, None),
        };
        let still_live = self
            .dag
            .tasks
            .get(tid)
            .map(|n| n.attempts == attempt && n.state == TaskState::Claimed)
            .unwrap_or(false);
        let interv = self.interventions.get(tid).copied().unwrap_or(0);
        let elapsed = self
            .attempt_started_at
            .get(tid)
            .map(|t| t.elapsed().as_secs())
            .unwrap_or(0);
        // ACCEPT — the deliverable is COMPLETE (every owned file exists and none fails its compile
        // check). Finish the task instead of spending an attempt killing a worker that has already
        // produced what it owed. This is the judge's only non-stopping lever; without it "looks done"
        // and "looks stuck" both resolved to kill, and the third kill is terminal. MEASURED (F165):
        // test-meridian was recorded a TERMINAL FAILURE with 8 passing test functions on disk that the
        // crunched app still runs.
        //
        // Deliberately NOT gated the way `salvage_spin` is. That mechanism marks a spinning task Done —
        // but excludes test tasks (`!is_test_task`), and test-authors are 93% of every failure this
        // campaign has recorded (14 of 15). Excluding them excludes the entire population the salvage
        // would help. Requires `deterministic` so a weak model can never hand itself a completion.
        if still_live && outcome.verdict == Verdict::Accept && outcome.deterministic {
            if let Some(h) = self.abort_handles.remove(tid) {
                h.abort();
            }
            if let Some(dev) = self.claimed_device.remove(tid) {
                if self.devices[dev].in_flight > 0 {
                    self.devices[dev].in_flight -= 1;
                }
            }
            if let Some(files) = self.held_by.remove(tid) {
                for f in files {
                    self.held_files.remove(&f);
                }
            }
            self.attempt_started_at.remove(tid);
            self.sink.emit(&SwarmEvent::JudgeVerdict {
                task_id: tid.to_string(),
                device: device.clone().unwrap_or_default(),
                judge_node: self.judge_node.clone().unwrap_or_default(),
                verdict: "accept".to_string(),
                confidence: outcome.confidence,
                hint: outcome.hint.clone(),
                action: "accepted".to_string(),
                deterministic: outcome.deterministic,
            });
            self.attempt_log
                .entry(tid.to_string())
                .or_default()
                .push(AttemptRecord {
                    device: device.clone(),
                    model: model.clone(),
                    outcome: "judge_accepted".to_string(),
                    error: None,
                    elapsed_ms: 0,
                });
            self.dag.tasks.get_mut(tid).unwrap().state = TaskState::Done;
            self.relax_dependents(tid);
            let attempts = self.attempt_log[tid].len() as u32;
            let ended_because = self.last_attempt_error(tid);
            self.sink.emit(&SwarmEvent::TaskCompleted {
                task_id: tid.to_string(),
                salvaged: self.task_salvaged.get(tid).copied().unwrap_or(false),
                status: "done".to_string(),
                device,
                model,
                attempts,
                elapsed_ms: 0,
                session_id: self.task_session_id(tid),
                error: ended_because,
                tool_calls: Vec::new(),
            });
            return true;
        }
        // SPLIT is an action on healthy-but-too-big work, handled separately below — keep it out of the
        // kill/re-dispatch path (which is for misbehaving workers).
        let is_split = still_live
            && outcome.verdict == crate::judge::Verdict::Split
            && outcome.confidence >= cfg.intervene_confidence
            && outcome
                .proposed_split
                .as_ref()
                .is_some_and(|c| !c.is_empty());
        let actionable = outcome.verdict.is_problem()
            && outcome.verdict != crate::judge::Verdict::Split
            && still_live
            && outcome.confidence >= cfg.intervene_confidence;
        // The judge's three actions: observe (log only), re_dispatch (kill + retry with a hint, while
        // under the cap), or fail (cap exhausted and STILL broken — cut it loose so a doomed task can't
        // spin a node to worker_max_turns and so the run terminates instead of hanging on a dead worker).
        // Terminal-fail requires a positive cap that has been used up AND a final attempt that has run a
        // meaningful while, so a brief flag on a just-(re)started attempt never fails a task prematurely.
        // TERMINAL-FAIL REQUIRES A DETERMINISTIC VERDICT. `actionable` gates on
        // `outcome.confidence >= cfg.intervene_confidence`, but the judge MODEL produces that confidence —
        // so without this term a model OPINION decides an irreversible failure. MEASURED: nf-ts-cadence's
        // integrate-verify went `over_reading -> re_dispatch, re_dispatch, FAILED` at confidence 0.90 from
        // the LLM path; under fan-verify integrate-verify depends on every verify::<M>, so that single model
        // opinion turned the whole run red. The standing rule is that only a DETERMINISTIC engine event may
        // create or kill a verdict. A model verdict keeps its full STEERING power (re_dispatch with a hint,
        // below) — it simply may no longer be the thing that fails a task; that task now runs to its own
        // deterministic backstop (worker_timeout / the spiral + repeat breakers) instead.
        let terminal = actionable
            && outcome.deterministic
            && cfg.max_interventions_per_task > 0
            && interv >= cfg.max_interventions_per_task
            && elapsed >= cfg.terminal_min_secs;
        let redispatch = actionable && interv < cfg.max_interventions_per_task;
        // SPLIT is handled FIRST so the emitted event reflects the ACTUAL outcome: apply_split validates the
        // proposal and returns false (no-op, worker keeps running) if it is malformed — in that case the
        // event must report "observed", not a "split" that never happened.
        if is_split {
            let children = outcome.proposed_split.clone().unwrap_or_default();
            let applied = self.apply_split(tid, &children);
            self.sink.emit(&SwarmEvent::JudgeVerdict {
                task_id: tid.to_string(),
                device: device.clone().unwrap_or_default(),
                judge_node: self.judge_node.clone().unwrap_or_default(),
                verdict: outcome.verdict.as_str().to_string(),
                confidence: outcome.confidence,
                hint: outcome.hint.clone(),
                action: if applied { "split" } else { "observed" }.to_string(),
                deterministic: outcome.deterministic,
            });
            return applied;
        }
        let action = if terminal {
            "failed"
        } else if redispatch {
            "re_dispatch"
        } else {
            "observed"
        };
        self.sink.emit(&SwarmEvent::JudgeVerdict {
            task_id: tid.to_string(),
            device: device.clone().unwrap_or_default(),
            judge_node: self.judge_node.clone().unwrap_or_default(),
            verdict: outcome.verdict.as_str().to_string(),
            confidence: outcome.confidence,
            hint: outcome.hint.clone(),
            action: action.to_string(),
            deterministic: outcome.deterministic,
        });
        if terminal {
            if let Some(h) = self.abort_handles.remove(tid) {
                h.abort();
            }
            if let Some(dev) = self.claimed_device.remove(tid) {
                if self.devices[dev].in_flight > 0 {
                    self.devices[dev].in_flight -= 1;
                }
            }
            if let Some(files) = self.held_by.remove(tid) {
                for f in files {
                    self.held_files.remove(&f);
                }
            }
            self.attempt_started_at.remove(tid);
            // FINALIZE-SPIN SALVAGE: a Looping terminal-fail means the owned file WAS written (the judge only
            // emits Looping once any_owned_written); the worker produced output but kept spinning after. For a
            // non-test task, discard also fails its dependents (the integrate-verify sink), so a working app is
            // reported FAILED. Mark it Done and let integrate-verify gate it honestly. Only Looping; never a
            // test task.
            let salvage = salvage_spin_enabled()
                && matches!(outcome.verdict, Verdict::Looping)
                && self.dag.tasks.get(tid).is_some_and(|n| {
                    !is_test_task(&n.spec.id, &n.spec.owned_files)
                        && owned_file_written(&n.spec.owned_files)
                });
            let (outcome_label, error_text, state, status) = if salvage {
                (
                    "salvaged_spin",
                    "finalize-spin salvaged: owned file written; integrate-verify gates it"
                        .to_string(),
                    TaskState::Done,
                    "done",
                )
            } else {
                (
                    "judge_failed",
                    outcome.verdict.as_str().to_string(),
                    TaskState::Failed,
                    "failed",
                )
            };
            if salvage {
                self.sink.emit(&SwarmEvent::JudgeVerdict {
                    task_id: tid.to_string(),
                    device: device.clone().unwrap_or_default(),
                    judge_node: self.judge_node.clone().unwrap_or_default(),
                    verdict: "salvaged_spin".to_string(),
                    confidence: 1.0,
                    hint: error_text.clone(),
                    action: "salvaged".to_string(),
                    // Engine bookkeeping on a terminal Looping, not a fresh judge call.
                    deterministic: true,
                });
            }
            self.attempt_log
                .entry(tid.to_string())
                .or_default()
                .push(AttemptRecord {
                    device: device.clone(),
                    model: model.clone(),
                    outcome: outcome_label.to_string(),
                    error: Some(error_text),
                    elapsed_ms: 0,
                });
            self.dag.tasks.get_mut(tid).unwrap().state = state;
            if salvage {
                // A salvaged task is Done: relax its dependents exactly like a success, or the CLI/verify
                // sink stays Pending forever and the run ends scheduler_stuck (backlog #7: expense/tmpl).
                self.relax_dependents(tid);
            } else {
                self.fail_descendants(tid);
            }
            let attempts = self.attempt_log[tid].len() as u32;
            let ended_because = self.last_attempt_error(tid);
            self.sink.emit(&SwarmEvent::TaskCompleted {
                task_id: tid.to_string(),
                salvaged: self.task_salvaged.get(tid).copied().unwrap_or(false),
                status: status.to_string(),
                device,
                model,
                attempts,
                elapsed_ms: 0,
                session_id: self.task_session_id(tid),
                error: ended_because,
                tool_calls: Vec::new(),
            });
            return true;
        }
        if !redispatch {
            return false;
        }
        if let Some(h) = self.abort_handles.remove(tid) {
            h.abort();
        }
        let released_dev = self.claimed_device.remove(tid);
        let released_dev_id = released_dev.map(|i| self.devices[i].cfg.id.clone());
        if let Some(dev) = released_dev {
            if self.devices[dev].in_flight > 0 {
                self.devices[dev].in_flight -= 1;
            }
        }
        if let Some(files) = self.held_by.remove(tid) {
            for f in files {
                self.held_files.remove(&f);
            }
        }
        self.attempt_started_at.remove(tid);
        *self.interventions.entry(tid.to_string()).or_default() += 1;
        self.judge_notes
            .push((tid.to_string(), outcome.hint.clone()));
        self.prior_hints.insert(tid.to_string(), outcome.hint);
        self.attempt_log
            .entry(tid.to_string())
            .or_default()
            .push(AttemptRecord {
                device,
                model,
                outcome: "judge_killed".to_string(),
                error: Some(outcome.verdict.as_str().to_string()),
                elapsed_ms: 0,
            });
        // Advance the attempt epoch so the killed future's completion is ignored, then re-queue.
        let n = self.dag.tasks.get_mut(tid).unwrap();
        n.attempts += 1;
        n.avoid_device = released_dev_id;
        n.state = TaskState::Ready;
        let fan_out = n.fan_out;
        self.ready.push(Ranked {
            fan_out,
            id: tid.to_string(),
        });
        true
    }

    /// M3 task-splitting: replace a too-big task with the judge's proposed children that PARTITION its
    /// owned files. Returns true if a VALID split was applied (worker aborted, children injected, the
    /// original's dependents re-pointed onto ALL children); false if the proposal is malformed — the caller
    /// then takes no action and the worker keeps running, so a bad proposal can never corrupt the DAG.
    fn apply_split(&mut self, tid: &str, children: &[crate::judge::ChildSpec]) -> bool {
        // ---- validate the partition against the original (no mutation yet) ----
        let (orig_files, orig_deps, orig_diff, orig_model, orig_desc) =
            match self.dag.tasks.get(tid) {
                Some(n) => (
                    n.spec
                        .owned_files
                        .iter()
                        .cloned()
                        .collect::<std::collections::BTreeSet<String>>(),
                    n.spec.deps.clone(),
                    n.spec.difficulty,
                    n.spec.preferred_model.clone(),
                    n.spec.description.clone(),
                ),
                None => return false,
            };
        if children.len() < 2 {
            return false; // need >= 2 parts to be worth splitting
        }
        let mut child_ids = std::collections::HashSet::new();
        for c in children {
            if !child_ids.insert(c.id.as_str()) || self.dag.tasks.contains_key(&c.id) {
                return false; // duplicate child id, or collides with an existing task
            }
        }
        // every child file belongs to the original, children are pairwise-disjoint, and together they
        // cover ALL of the original's files (a true partition).
        let mut union = std::collections::BTreeSet::new();
        for c in children {
            if c.files.is_empty() {
                return false;
            }
            for f in &c.files {
                if !orig_files.contains(f) || !union.insert(f.clone()) {
                    return false; // foreign file or overlap between children
                }
            }
        }
        if union != orig_files {
            return false; // does not cover the original's files
        }
        // child sibling deps may only reference sibling child ids.
        if children
            .iter()
            .any(|c| c.depends_on.iter().any(|d| !child_ids.contains(d.as_str())))
        {
            return false;
        }
        // Reject a self-dep or any cycle among siblings BEFORE aborting the worker. Otherwise such a
        // proposal passes here but fails splice_specs' Kahn check AFTER the abort, hitting the destructive
        // Err arm — which would cascade-FAIL a healthy worker and break the documented no-op contract.
        if children
            .iter()
            .any(|c| c.depends_on.iter().any(|d| d == &c.id))
        {
            return false;
        }
        {
            // Kahn topological drain over the children's sibling-dep edges; a non-empty remainder = a cycle.
            let mut indeg: std::collections::HashMap<&str, usize> =
                children.iter().map(|c| (c.id.as_str(), 0usize)).collect();
            for c in children {
                for d in &c.depends_on {
                    *indeg.get_mut(c.id.as_str()).unwrap() += 1;
                    let _ = d;
                }
            }
            let mut queue: Vec<&str> = indeg
                .iter()
                .filter(|(_, &n)| n == 0)
                .map(|(&k, _)| k)
                .collect();
            let mut drained = 0usize;
            while let Some(node) = queue.pop() {
                drained += 1;
                for c in children {
                    if c.depends_on.iter().any(|d| d == node) {
                        let e = indeg.get_mut(c.id.as_str()).unwrap();
                        *e -= 1;
                        if *e == 0 {
                            queue.push(c.id.as_str());
                        }
                    }
                }
            }
            if drained != children.len() {
                return false; // cycle among siblings — leave the worker running (no-op)
            }
        }
        // ---- abort + release the original worker (mirror the kill/re-dispatch cleanup) ----
        if let Some(h) = self.abort_handles.remove(tid) {
            h.abort();
        }
        if let Some(dev) = self.claimed_device.remove(tid) {
            if self.devices[dev].in_flight > 0 {
                self.devices[dev].in_flight -= 1;
            }
        }
        if let Some(files) = self.held_by.remove(tid) {
            for f in files {
                self.held_files.remove(&f);
            }
        }
        self.attempt_started_at.remove(tid);
        // ---- build + insert the children (deps = original's external deps + sibling deps) ----
        let inherit_spec = split_inherit_spec_enabled();
        let child_id_list: Vec<TaskId> = children.iter().map(|c| c.id.clone()).collect();
        let specs: Vec<crate::dag::TaskSpec> = children
            .iter()
            .map(|c| {
                let mut deps = orig_deps.clone();
                deps.extend(c.depends_on.iter().cloned());
                crate::dag::TaskSpec {
                    id: c.id.clone(),
                    description: child_description(tid, &orig_desc, c, inherit_spec),
                    difficulty: orig_diff,
                    preferred_model: orig_model.clone(),
                    owned_files: c.files.clone(),
                    deps,
                }
            })
            .collect();
        let newly_ready = match self.dag.splice_specs(specs) {
            Ok(r) => r,
            Err(_) => {
                // cycle/collision: abort the split. The worker is already gone, so fail the task cleanly.
                if let Some(n) = self.dag.tasks.get_mut(tid) {
                    n.state = TaskState::Failed;
                }
                self.fail_descendants(tid);
                return true;
            }
        };
        // ---- re-point every dependent of the original onto ALL children ----
        let dependents = self.dag.dependents.get(tid).cloned().unwrap_or_default();
        for d in &dependents {
            if let Some(n) = self.dag.tasks.get_mut(d) {
                n.spec.deps.retain(|x| x != tid);
                n.spec.deps.extend(child_id_list.iter().cloned());
                // the original counted as ONE unmet dependency; it is now N unmet children -> net +(N-1).
                n.indegree_remaining += child_id_list.len() - 1;
            }
            for cid in &child_id_list {
                self.dag
                    .dependents
                    .entry(cid.clone())
                    .or_default()
                    .push(d.clone());
                if let Some(cn) = self.dag.tasks.get_mut(cid) {
                    cn.fan_out += 1;
                }
            }
        }
        self.dag.dependents.remove(tid);
        // ---- record each child's split generation = parent + 1, so the cap (split once) holds: a child
        // that itself runs long carries split_count >= 1 and is never split again. ----
        let parent_gen = self.split_generation.get(tid).copied().unwrap_or(0);
        for cid in &child_id_list {
            self.split_generation.insert(cid.clone(), parent_gen + 1);
        }
        // ---- mark the original Done (no cascade) + advance its epoch so a late completion is ignored ----
        if let Some(n) = self.dag.tasks.get_mut(tid) {
            n.attempts += 1;
            n.state = TaskState::Done;
            // The split shell is superseded by its children — mark it reviewed so the idle pre-reviewer
            // (M5) never picks this phantom (Done + owns the union files) and reviews a partial file set.
            n.pre_reviewed = true;
        }
        // ---- enqueue the children that are immediately ready ----
        for id in newly_ready {
            let fan_out = self.dag.tasks[&id].fan_out;
            self.ready.push(Ranked { fan_out, id });
        }
        // A split is the mechanism by which spare nodes get more work to do, and until now it changed
        // the DAG silently. Three real runs could not be asked whether a split ever happened, because
        // the only trace was child task ids appearing in later dispatches — indistinguishable from a
        // plan that named them. Emitted at the success return ONLY, so the event means "a split was
        // applied", never "one was considered".
        self.sink.emit(&SwarmEvent::TaskSplit {
            task_id: tid.to_string(),
            children: children.iter().map(|c| c.id.clone()).collect(),
        });
        true
    }

    /// A failed task can never produce output, so its (transitive) dependents can never run —
    /// mark them Failed so the run terminates instead of deadlocking on blocked tasks.
    fn fail_descendants(&mut self, tid: &str) {
        let mut q: VecDeque<TaskId> = self
            .dag
            .dependents
            .get(tid)
            .cloned()
            .unwrap_or_default()
            .into();
        while let Some(d) = q.pop_front() {
            let n = self.dag.tasks.get_mut(&d).unwrap();
            if matches!(n.state, TaskState::Done | TaskState::Failed) {
                continue;
            }
            n.state = TaskState::Failed;
            for dd in self.dag.dependents.get(&d).cloned().unwrap_or_default() {
                q.push_back(dd);
            }
        }
    }

    fn build_report(&self) -> RunReport {
        let mut done = Vec::new();
        let mut failed = Vec::new();
        let mut results = HashMap::new();
        let mut tasks = Vec::new();
        let mut per_device: HashMap<String, DeviceSummary> = HashMap::new();
        for (id, n) in &self.dag.tasks {
            let status = match n.state {
                TaskState::Done => {
                    done.push(id.clone());
                    if let Some(r) = &n.result {
                        results.insert(id.clone(), r.clone());
                    }
                    "done"
                }
                TaskState::Failed => {
                    failed.push(id.clone());
                    "failed"
                }
                _ => "incomplete",
            };
            let history = self.attempt_log.get(id).cloned().unwrap_or_default();
            let elapsed_ms = history.last().map(|a| a.elapsed_ms);
            let (device, model) = match self.task_final_device.get(id) {
                Some((d, m)) => (Some(d.clone()), Some(m.clone())),
                None => (None, None),
            };
            let session_id = self.task_session.get(id).cloned().flatten();
            let tool_calls = self.task_tool_calls.get(id).cloned().unwrap_or_default();

            for a in &history {
                if let Some(d) = &a.device {
                    let e = per_device.entry(d.clone()).or_default();
                    e.busy_ms += a.elapsed_ms;
                    if a.outcome == "transient" {
                        e.retries += 1;
                    }
                }
            }
            if let Some(d) = &device {
                let e = per_device.entry(d.clone()).or_default();
                e.tool_calls += tool_calls.len() as u32;
                e.mcp_calls += tool_calls.iter().filter(|t| t.is_mcp).count() as u32;
            }

            tasks.push(TaskOutcome {
                task_id: id.clone(),
                status: status.to_string(),
                device,
                model,
                attempts: history.len() as u32,
                attempt_history: history,
                elapsed_ms,
                session_id,
                tool_calls,
                output: n.result.clone(),
                owns_nothing: n.spec.owned_files.is_empty(),
            });
        }
        for (d, c) in &self.dispatched_per_device {
            per_device.entry(d.clone()).or_default().dispatched = *c;
        }
        done.sort();
        failed.sort();
        tasks.sort_by(|a, b| a.task_id.cmp(&b.task_id));
        let mut bonus: Vec<TaskId> = self.bonus_ids.iter().cloned().collect();
        bonus.sort();
        RunReport {
            done,
            failed,
            bonus,
            results,
            context_json: self.ctx.to_json(),
            dispatched_per_device: self.dispatched_per_device.clone(),
            tasks,
            per_device,
        }
    }
}

pub struct Scheduler {
    devices: Vec<DeviceCfg>,
    max_attempts: u32,
    sink: Arc<dyn EventSink>,
    replanner: Option<Arc<dyn Replanner>>,
    max_replans: u32,
    judge: Option<Arc<dyn Judge>>,
    judge_cfg: JudgeConfig,
    pre_reviewer: Option<Arc<dyn PreReviewer>>,
    speculation_enabled: bool,
    /// When set, the scheduler HOLDS at task boundaries while this file exists (the in-process pause).
    /// None (default) -> pause is inert and the loop is byte-identical to before.
    pause_file: Option<std::path::PathBuf>,
    /// GROUNDED research facts (Phase 1, Move 2), VERBATIM, handed to every DispatchRequest for injection into
    /// the worker prompt — the same channel as `user_decisions`. Empty (default) -> the worker prompt is
    /// byte-identical. Set via `with_doc_facts` so `run_with_decisions`' signature is unchanged.
    doc_facts: String,
    /// GOOSE_SWARM_DEGRADE_ON_STALL (#134/#132, default OFF): when a task exhausts its transient-retry budget
    /// (a mid-generation model hang) but its CRITICAL owned file is already on disk, mark it Done(degraded) +
    /// relax dependents instead of fail_descendants — so a single hung core task does not kill the capstone.
    /// integrate-verify then gates the degraded file honestly (build + R1 missing-deliverable). false =>
    /// the exhausted arm is byte-identical (fail_descendants).
    degrade_on_stall: bool,
}

impl Scheduler {
    pub fn new(devices: Vec<DeviceCfg>, max_attempts: u32) -> Self {
        Self {
            devices,
            max_attempts,
            sink: Arc::new(NullSink),
            replanner: None,
            max_replans: 0,
            judge: None,
            judge_cfg: JudgeConfig::default(),
            pre_reviewer: None,
            speculation_enabled: false,
            pause_file: None,
            doc_facts: String::new(),
            degrade_on_stall: false,
        }
    }

    /// Attach the GROUNDED research facts (Phase 1, Move 2) that each worker gets VERBATIM. Empty (default)
    /// => the worker prompt is byte-identical, so callers that don't opt into DOC_PREFETCH are unchanged.
    pub fn with_doc_facts(mut self, doc_facts: String) -> Self {
        self.doc_facts = doc_facts;
        self
    }

    /// Attach an idle-model judge: when a node would otherwise sit idle while tasks are still in
    /// flight, it inspects a busy worker and may kill + re-dispatch one that is looping, over-reading,
    /// or producing broken code. OFF by default — with no judge attached the scheduler is unchanged.
    pub fn with_judge(mut self, judge: Arc<dyn Judge>, cfg: JudgeConfig) -> Self {
        self.judge = Some(judge);
        self.judge_cfg = cfg;
        self
    }

    /// Attach an idle-node PRE-REVIEWER (M5): when a node would otherwise idle and NO in-flight worker
    /// needs judging, it correctness-checks a COMPLETED-but-unreviewed task's output and records findings
    /// for integrate-verify. OFF by default — with none attached the scheduler is unchanged.
    pub fn with_pre_reviewer(mut self, pre_reviewer: Arc<dyn PreReviewer>) -> Self {
        self.pre_reviewer = Some(pre_reviewer);
        self
    }

    /// Enable SPECULATIVE EXECUTION (GOOSE_SWARM_SPECULATE): when a node would otherwise idle at a serial
    /// chokepoint (no ready task, no pre-review work) a TWIN of the longest-running in-flight task is raced
    /// on the idle device, first-to-finish wins. OFF by default — with it off no twin is ever spawned and
    /// the scheduler is byte-identical. The twin spawns ONLY on a genuinely idle device (1 task per node).
    pub fn with_speculation(mut self) -> Self {
        self.speculation_enabled = true;
        self
    }

    /// Enable DEGRADE-ON-STALL (GOOSE_SWARM_DEGRADE_ON_STALL, #134/#132): at transient-retry exhaustion, if the
    /// stalled task already wrote its critical owned file, mark it Done(degraded) + relax dependents instead of
    /// failing the whole subtree. OFF by default — with it off the exhausted arm is byte-identical
    /// (fail_descendants). integrate-verify gates the degraded file honestly downstream.
    pub fn with_degrade_on_stall(mut self) -> Self {
        self.degrade_on_stall = true;
        self
    }

    /// Attach an event sink for structured observability (goose-cli writes JSONL through it).
    pub fn with_sink(mut self, sink: Arc<dyn EventSink>) -> Self {
        self.sink = sink;
        self
    }

    /// Enable the in-process PAUSE hold: while `path` exists the scheduler holds at task boundaries
    /// (claims no new ready task; in-flight tasks finish). Deleting the file resumes with zero re-runs.
    /// Not called -> `pause_file` stays None -> pause is inert and the loop is byte-identical.
    pub fn with_pause_file(mut self, path: std::path::PathBuf) -> Self {
        self.pause_file = Some(path);
        self
    }

    /// Attach a dynamic replanner: when workers go idle mid-run (>=2 free slots while a task is still
    /// in flight) it is asked for more parallel work, up to `max_replans` rounds. Off by default.
    pub fn with_replanner(mut self, replanner: Arc<dyn Replanner>, max_replans: u32) -> Self {
        self.replanner = Some(replanner);
        self.max_replans = max_replans;
        self
    }

    /// Run the whole DAG to completion. Returns when every task is Done or Failed. `goal` is the user
    /// prompt, used only by the dynamic replanner (ignored when none is attached).
    pub async fn run(
        &self,
        dag: Dag,
        dispatcher: Arc<dyn TaskDispatcher>,
        goal: String,
    ) -> Result<RunReport> {
        self.run_with_decisions(dag, dispatcher, goal, String::new())
            .await
    }

    /// `run`, plus the user's verbatim clarify answers to hand to every worker. `run` delegates here with
    /// an empty string, so every existing caller and test is byte-identical.
    pub async fn run_with_decisions(
        &self,
        dag: Dag,
        dispatcher: Arc<dyn TaskDispatcher>,
        goal: String,
        user_decisions: String,
    ) -> Result<RunReport> {
        if !self.devices.iter().any(|d| d.enabled) {
            bail!("no enabled devices in the pool");
        }
        // model-id uniqueness invariant across enabled devices (LM Link routes by id alone).
        let mut seen = HashSet::new();
        for d in self.devices.iter().filter(|d| d.enabled) {
            if !seen.insert(d.model_id.clone()) {
                bail!("duplicate model_id `{}` across enabled devices — LM Link cannot distinguish them", d.model_id);
            }
            if d.weight == 0 {
                bail!(
                    "device `{}` has weight 0 (enabled) — disable it instead",
                    d.id
                );
            }
        }

        let mut ready = BinaryHeap::new();
        for (id, n) in &dag.tasks {
            if n.state == TaskState::Ready {
                ready.push(Ranked {
                    fan_out: n.fan_out,
                    id: id.clone(),
                });
            }
        }
        let state = Arc::new(Mutex::new(State {
            dag,
            ready,
            devices: self
                .devices
                .iter()
                .cloned()
                .map(|cfg| DeviceRt { cfg, in_flight: 0 })
                .collect(),
            held_files: HashSet::new(),
            held_by: HashMap::new(),
            claimed_device: HashMap::new(),
            dispatched_per_device: HashMap::new(),
            ctx: SharedContext::new(),
            max_attempts: self.max_attempts,
            degrade_on_stall: self.degrade_on_stall,
            sink: self.sink.clone(),
            attempt_started_at: HashMap::new(),
            attempt_log: HashMap::new(),
            task_session: HashMap::new(),
            task_tool_calls: HashMap::new(),
            task_final_device: HashMap::new(),
            goal,
            user_decisions,
            doc_facts: self.doc_facts.clone(),
            replans_done: 0,
            replan_declined_at_incomplete: None,
            bonus_ids: HashSet::new(),
            device_speed: HashMap::new(),
            abort_handles: HashMap::new(),
            prior_hints: HashMap::new(),
            judge_notes: Vec::new(),
            interventions: HashMap::new(),
            omni_aborts: HashMap::new(),
            split_generation: HashMap::new(),
            judge_running: false,
            judge_node: None,
            task_salvaged: std::collections::HashMap::new(),
            idle_jobs: 0,
            sink_review_dim: 0,
            last_judged: HashMap::new(),
            spec_device: HashMap::new(),
            spec_started_at: HashMap::new(),
            spec_abort: HashMap::new(),
            speculating: HashSet::new(),
            spec_count: 0,
        }));
        let notify = Arc::new(Notify::new());
        // Edge-detect pause transitions so run_paused/run_unpaused is emitted once per transition, not per tick.
        let mut was_paused = false;

        loop {
            // In-process PAUSE hold: while the sentinel exists, claim NO new ready task. Already-spawned
            // in-flight futures (below) run to completion — the hold is BETWEEN tasks, so it can never
            // corrupt a half-written file. Cheap Path::exists per wake; inert when pause_file is None.
            let paused = self.pause_file.as_ref().is_some_and(|p| p.exists());
            if paused != was_paused {
                let s = state.lock().await;
                s.sink.emit(if paused {
                    &SwarmEvent::RunPaused
                } else {
                    &SwarmEvent::RunUnpaused
                });
                was_paused = paused;
            }
            let assignments = if paused {
                Vec::new()
            } else {
                state.lock().await.pick_assignments()
            };
            let dispatched_now = !assignments.is_empty();
            for a in assignments {
                let dispatcher = dispatcher.clone();
                let task_state = state.clone();
                let notify = notify.clone();
                let task_id = a.task_id.clone();
                let attempt = a.request.attempt;
                let request = a.request;
                let done_id = task_id.clone();
                let jh = tokio::spawn(async move {
                    let res = dispatcher.run(request).await;
                    {
                        let mut s = task_state.lock().await;
                        s.complete(&done_id, attempt, res);
                    }
                    notify.notify_one();
                });
                // Register the abort handle when a judge OR speculation is on, so the loser can be killed.
                // Neither -> the map stays empty and the default path is byte-identical to before.
                if self.judge.is_some() || self.speculation_enabled {
                    state
                        .lock()
                        .await
                        .abort_handles
                        .insert(task_id, jh.abort_handle());
                }
            }

            {
                let s = state.lock().await;
                if s.all_terminal() {
                    return Ok(s.build_report());
                }
                if !paused && !dispatched_now && s.total_in_flight() == 0 {
                    // Nothing assignable and nothing running, but not all terminal: the remaining
                    // tasks are permanently blocked (deps failed, or a file deadlock).
                    // The `!paused` guard is LOAD-BEARING: while held we intentionally claim nothing and can
                    // drain to zero in-flight — without this guard that state trips the stuck-bail and turns
                    // Pause into an accidental terminate. Held + drained must idle until the sentinel clears.
                    let remaining = s
                        .dag
                        .tasks
                        .values()
                        .filter(|n| !matches!(n.state, TaskState::Done | TaskState::Failed))
                        .count();
                    s.sink.emit(&SwarmEvent::SchedulerStuck { remaining });
                    bail!(
                        "scheduler stuck: {remaining} task(s) cannot proceed (blocked by failed deps or file holds)"
                    );
                }
            }
            // Dynamic replan: workers idle while a task is still in flight (e.g. a slow tail) — ask the
            // planner for more parallel work to fill them. Gated on in_flight > 0, so it is mutually
            // exclusive with the stuck-bail above (which needs in_flight == 0). The state lock is
            // released across the async planner call; completions fire meanwhile and splice_specs
            // re-validates against the now-current DAG.
            if !paused && self.replanner.is_some() {
                let ctx = {
                    let mut s = state.lock().await;
                    if !dispatched_now
                        && s.total_in_flight() > 0
                        && s.ready.is_empty()
                        && s.idle_capacity() >= 2
                        && s.replans_done < self.max_replans
                        && !s.sink_in_flight()
                        // A previous EMPTY answer is cached against the DAG size that produced it.
                        // Re-ask only when strictly fewer tasks remain — the one change that can make
                        // the replanner answer differently — so the tail gets its ask without the
                        // planner being pestered at an unchanged state.
                        && s
                            .replan_declined_at_incomplete
                            .is_none_or(|prev| s.incomplete_count() < prev)
                    {
                        s.replans_done += 1;
                        Some(s.make_replan_context())
                    } else {
                        None
                    }
                };
                if let Some(ctx) = ctx {
                    let round = ctx.round;
                    let specs = self
                        .replanner
                        .as_ref()
                        .unwrap()
                        .replan(ctx)
                        .await
                        .unwrap_or_default();
                    let mut s = state.lock().await;
                    if s.all_terminal() {
                        return Ok(s.build_report());
                    }
                    if specs.is_empty() {
                        s.sink.emit(&SwarmEvent::Replanned {
                            round,
                            added: Vec::new(),
                            stopped: true,
                        });
                        // REFUND the round and remember the state instead of burning the budget. An
                        // empty answer costs one planner call and says nothing about a DAG that has
                        // since shrunk; consuming the whole budget for it is what left two nodes idle
                        // through an 18-minute single-task tail with the replanner switched off.
                        s.replans_done = s.replans_done.saturating_sub(1);
                        s.replan_declined_at_incomplete = Some(s.incomplete_count());
                    } else {
                        // Replanner-added tasks are OPPORTUNISTIC (idle-fill) — record them as bonus so a
                        // bonus failure cannot fail an otherwise-complete run (run success = core plan).
                        let spliced_ids: Vec<TaskId> =
                            specs.iter().map(|sp| sp.id.clone()).collect();
                        match s.dag.splice_specs(specs) {
                            Ok(new_ready) => {
                                // `added` must be what was ADDED, not what happened to become READY.
                                // A spliced task whose deps are not yet satisfied is in the DAG and
                                // will run, but it is not in `new_ready` — so reporting new_ready
                                // under-counts, and can report ZERO for a successful replan.
                                //
                                // MEASURED: a live run emitted `Replanned { added: [], stopped: false }`
                                // while `test-api-edge-cases` and `test-store-integrity` were both
                                // spliced and later dispatched. `stopped: false` with an empty `added`
                                // is a contradiction — the empty case takes the other branch — and it
                                // made a plan-vs-execution review report two legitimately-added tasks
                                // as UNPLANNED DRIFT. An event that cannot be reconciled with the
                                // dispatch log turns a correct mechanism into a false alarm.
                                let added = spliced_ids.clone();
                                s.bonus_ids.extend(spliced_ids);
                                for id in new_ready {
                                    let fan_out = s.dag.tasks[&id].fan_out;
                                    s.ready.push(Ranked { fan_out, id });
                                }
                                s.sink.emit(&SwarmEvent::Replanned {
                                    round,
                                    added,
                                    stopped: false,
                                });
                                drop(s);
                                continue;
                            }
                            Err(_) => {
                                s.sink.emit(&SwarmEvent::Replanned {
                                    round,
                                    added: Vec::new(),
                                    stopped: true,
                                });
                                s.replans_done = self.max_replans;
                            }
                        }
                    }
                }
            }
            // Idle-model judge: when a node would otherwise sit idle while tasks are still in flight,
            // inspect the longest-running worker and possibly kill + re-dispatch a stuck one. At most one
            // judge runs at a time; the whole block is skipped when no judge is attached.
            if let Some(judge) = self.judge.as_ref().filter(|_| !paused) {
                let target = {
                    let mut s = state.lock().await;
                    // The judge is NOT capacity-bounded: it must fire even on a SATURATED fleet to kill a
                    // stuck worker and free a slot (that is unblocking, not idle-node work). It still counts
                    // toward idle_jobs so pre-review (below) knows one slot is taken.
                    if s.judge_running || s.total_in_flight() == 0 {
                        None
                    } else {
                        s.pick_judge_target(&self.judge_cfg)
                    }
                };
                if let Some((req, attempt, claimed_device)) = target {
                    let tid = req.task_id.clone();
                    let judge = judge.clone();
                    let st = state.clone();
                    let nt = notify.clone();
                    let cfg = self.judge_cfg;
                    tokio::spawn(async move {
                        // The IdleSlotGuard is the SOLE releaser of the idle_jobs slot AND the claimed device
                        // slot — decrement-ONCE on BOTH normal and panic exit. A counter must not be
                        // double-decremented the way the old idempotent bool harmlessly could (that
                        // undercounts and oversubscribes the fleet). We still clear judge_running on the hot
                        // path so the next tick can re-judge immediately; the guard also clears it as the
                        // panic backstop.
                        let _slot = IdleSlotGuard {
                            state: st.clone(),
                            is_judge: true,
                            claimed_device,
                        };
                        let outcome = judge.judge(req).await;
                        let intervened = {
                            let mut s = st.lock().await;
                            let r = s.apply_judge_outcome(&tid, attempt, outcome, &cfg);
                            s.judge_running = false;
                            r
                        };
                        // Only wake the loop when the judge actually intervened (the re-dispatched task
                        // needs to be picked up). An "observed" verdict changes nothing — notifying here
                        // would immediately respawn a judge and busy-loop; the 30s tick re-evaluates.
                        if intervened {
                            nt.notify_one();
                        }
                    });
                }
            }
            // M5: put any STILL-idle node (beyond the one the judge took) on a correctness PRE-REVIEW of a
            // completed-but-unreviewed task (findings feed integrate-verify). Judge + pre-review now run
            // CONCURRENTLY, bounded by idle_capacity() so each free node gets one idle job and none is
            // oversubscribed; multiple pre-reviews can run at once (each on a distinct task, marked
            // pre_reviewed up front). Off unless a pre-reviewer is attached; None when all idle slots taken.
            if let Some(pr) = &self.pre_reviewer {
                let req = {
                    let mut s = state.lock().await;
                    // Idle-jobs now CLAIM a device (bump in_flight), so idle_capacity() already reflects them
                    // — fire a pre-review iff a device is genuinely free. (The old `idle_jobs >= idle_capacity`
                    // double-counted once claiming was added, blocking the concurrent pre-review.)
                    if s.idle_capacity() == 0 {
                        None
                    } else {
                        s.pick_prereview_request()
                    }
                };
                if let Some((req, claimed_device)) = req {
                    let pr = pr.clone();
                    let st = state.clone();
                    tokio::spawn(async move {
                        // The IdleSlotGuard is the SOLE releaser of this idle_jobs slot AND the claimed device
                        // slot — decrement-ONCE on both normal and panic exit (is_judge=false leaves
                        // judge_running untouched). Do NOT also decrement explicitly here: that double-counts
                        // the slot and oversubscribes.
                        let _slot = IdleSlotGuard {
                            state: st.clone(),
                            is_judge: false,
                            claimed_device: Some(claimed_device),
                        };
                        let tid = req.task_id.clone();
                        let dev = req.reviewer_model_id.clone();
                        let started = std::time::Instant::now();
                        let out = pr.pre_review(req).await;
                        // Emit so idle-node utilization is OBSERVABLE in the jsonl (it was previously invisible
                        // — a pre-review only left a file when it found ISSUES, so "ran + OK" looked like "never
                        // ran"). One quick sync emit under the lock, same as the judge's verdict emit.
                        st.lock().await.sink.emit(&SwarmEvent::PreReview {
                            task_id: tid,
                            device: dev,
                            had_findings: out.had_findings,
                            secs: started.elapsed().as_secs_f64(),
                        });
                    });
                }
            }
            // SINK IDLE-FILL (GOOSE_SWARM_SINK_REVIEW): when the integrate-verify SINK runs solo and
            // pre-review is exhausted, put an otherwise-idle node on a READ-ONLY whole-tree dimension review.
            // Findings accumulate in the dispatcher; run_swarm drains + re-verifies them after the sink. The
            // IdleSlotGuard releases the claimed device. Off by default (pick_sink_review returns None).
            if let Some(pr) = self.pre_reviewer.as_ref().filter(|_| !paused) {
                // Fill ALL currently-free nodes this tick (not one) — pick_sink_review claims a device each
                // call and returns None once none is free, so this saturates the idle nodes during the sink
                // instead of leaving them idle between the ~15s tick and a ~90s review finishing.
                loop {
                    let pick = { state.lock().await.pick_sink_review() };
                    let Some((model_id, dim, goal, claimed_device)) = pick else {
                        break;
                    };
                    let pr = pr.clone();
                    let st = state.clone();
                    tokio::spawn(async move {
                        let _slot = IdleSlotGuard {
                            state: st.clone(),
                            is_judge: false,
                            claimed_device: Some(claimed_device),
                        };
                        pr.idle_dimension_review(&model_id, &goal, dim).await;
                    });
                }
            }
            // SPECULATIVE EXECUTION: when speculation is ON and a node is STILL idle (runs AFTER pre-review,
            // so pre-review gets first refusal of the idle slot), race a TWIN of the longest-running in-flight
            // task on a free device — first-to-finish wins. Gated on spare capacity beyond the running idle
            // jobs (so it never oversubscribes) and no ready work. OFF by default -> the block is skipped and
            // pick_speculation_target / spec_* are never touched (byte-identical).
            if !paused && self.speculation_enabled {
                let target = {
                    let mut s = state.lock().await;
                    // Bounds: no ready work, spare capacity beyond the running idle jobs, and a global cap on
                    // total speculative spawns per run (so a long chokepoint can't burn unbounded compute).
                    if !s.ready.is_empty()
                        || s.idle_capacity() == 0
                        || s.spec_count >= SPECULATION_CAP
                    {
                        None
                    } else {
                        s.pick_speculation_target()
                    }
                };
                if let Some((req, _dev)) = target {
                    let dispatcher = dispatcher.clone();
                    let task_state = state.clone();
                    let notify = notify.clone();
                    let attempt = req.attempt;
                    let tid = req.task_id.clone();
                    let tid_spawn = tid.clone();
                    let jh = tokio::spawn(async move {
                        let res = dispatcher.run(req).await;
                        {
                            let mut s = task_state.lock().await;
                            s.resolve_speculation(&tid_spawn, attempt, res);
                        }
                        notify.notify_one();
                    });
                    state.lock().await.spec_abort.insert(tid, jh.abort_handle());
                }
            }
            // Wake on a completion, or — when a judge is attached — at least every 15s, so it can
            // inspect a worker that crosses a threshold BETWEEN completions (a lone stuck worker produces
            // no completion to wake on). A short tick means the behavioral over-read signal (many actions,
            // zero output) and the terminal-fail decision act within ~15s of tripping, not minutes.
            // tokio::Notify stores one permit, so a completion that fires before this await is not lost.
            // With no judge this is an effectively-infinite wait: byte-identical to before.
            let tick = if paused {
                // While held, re-poll the pause sentinel ~every 2s so Resume is detected promptly even when
                // there are no in-flight completions left to wake the loop.
                std::time::Duration::from_secs(2)
            } else if self.judge.is_some()
                || self.pre_reviewer.is_some()
                || self.speculation_enabled
                // The REPLANNER is an idle-node mechanism too, and it was missing from this list. Its
                // trigger is "nodes idle while a task is still in flight", which by construction produces
                // NO completion to wake on — so without a tick the one window it exists for is never
                // re-examined, and the run waits out the tail with the check unevaluated. It only worked
                // at all because a judge happened to be attached and was lending it a heartbeat.
                || self.replanner.is_some()
            {
                std::time::Duration::from_secs(15)
            } else {
                std::time::Duration::from_secs(86_400)
            };
            let _ = tokio::time::timeout(tick, notify.notified()).await;
        }
    }
}

#[cfg(test)]
mod salvage_tests {
    use super::*;

    /// The split used to hand a child a ~40-char label as its ENTIRE task statement, discarding the
    /// implementation spec PLAN had just spent 40% of the run's wall-clock writing (loop-04: a 2038-char
    /// spec -> "(split of data-model-persistence) note-store", 43 chars). These pin both arms.
    #[test]
    fn split_child_description_off_is_byte_identical() {
        let child = crate::judge::ChildSpec {
            id: "note-store".into(),
            files: vec!["Sources/NotesLibrary/NoteStore.swift".into()],
            depends_on: vec![],
        };
        // OFF -> exactly today's string, unchanged.
        assert_eq!(
            child_description(
                "data-model-persistence",
                "a 2038-char spec...",
                &child,
                false
            ),
            "(split of data-model-persistence) note-store"
        );
        // ON but the parent had no spec -> nothing to inherit, fall back to the label.
        assert_eq!(
            child_description("data-model-persistence", "   ", &child, true),
            "(split of data-model-persistence) note-store"
        );
    }

    #[test]
    fn split_child_description_on_scopes_files_then_carries_the_spec() {
        let child = crate::judge::ChildSpec {
            id: "note-store".into(),
            files: vec![
                "Sources/NotesLibrary/NoteStore.swift".into(),
                "Sources/NotesLibrary/Note.swift".into(),
            ],
            depends_on: vec![],
        };
        let parent_spec =
            "**Package.swift**: three targets. **NoteStore.swift**: @Observable class.";
        let d = child_description("data-model-persistence", parent_spec, &child, true);

        // The parent's real spec survives — that is the whole point.
        assert!(
            d.contains(parent_spec),
            "the child must receive the parent's spec"
        );
        // Every owned file is named explicitly.
        assert!(d.contains("- Sources/NotesLibrary/NoteStore.swift"));
        assert!(d.contains("- Sources/NotesLibrary/Note.swift"));
        // The scope guard comes BEFORE the spec, so the child reads its limits before reading about files
        // it must not touch (the risk this lever introduces).
        let scope_at = d
            .find("YOU OWN ONLY THESE FILES")
            .expect("scope header present");
        let spec_at = d.find(parent_spec).expect("spec present");
        assert!(
            scope_at < spec_at,
            "the file-scope header must precede the parent spec"
        );
        assert!(d.contains("belong to OTHER workers"));
        // And it is vastly more than the 43-char label it replaces.
        assert!(
            d.len() > 200,
            "expected a real task statement, got {} chars",
            d.len()
        );
    }

    #[test]
    fn test_files_and_tasks_are_recognized() {
        assert!(looks_like_test_file("tests/test_core.py"));
        assert!(looks_like_test_file("test_utils.py"));
        assert!(looks_like_test_file("habits/foo_test.py"));
        assert!(looks_like_test_file("tests/conftest.py"));
        assert!(!looks_like_test_file("habits/__main__.py"));
        assert!(!looks_like_test_file("habits/commands.py"));
        // A non-test entry task is salvageable; test tasks and empty-owned tasks are not.
        assert!(!is_test_task(
            "cli-app",
            &["habits/commands.py".into(), "habits/__main__.py".into()]
        ));
        assert!(is_test_task(
            "tests-advanced",
            &["tests/test_advanced.py".into()]
        ));
        assert!(is_test_task(
            "unit",
            &["tests/test_a.py".into(), "tests/test_b.py".into()]
        ));
        // id mentions test even if a file does not look like one.
        assert!(is_test_task("integration-test", &["run_it.py".into()]));
    }

    #[test]
    fn salvage_off_values_parse() {
        // Parse mirror of salvage_spin_enabled: unset -> ON; explicit off-values -> OFF.
        let off = |v: &str| {
            matches!(
                v.trim().to_lowercase().as_str(),
                "0" | "off" | "false" | "no"
            )
        };
        assert!(off("0") && off("off") && off("FALSE") && off(" no "));
        assert!(!off("1") && !off("true") && !off("anything"));
    }

    // A fresh temp dir + a helper to write/skip owned files, so the on-disk degrade predicate is exercised for
    // real (not mocked). Returns absolute paths, since critical_owned_files_written stats the raw path.
    fn degrade_fixture(tag: &str) -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("degrade_{}_{}_{}", tag, std::process::id(), n));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
    fn write_file(dir: &std::path::Path, name: &str, bytes: &str) -> String {
        let p = dir.join(name);
        std::fs::write(&p, bytes).unwrap();
        p.to_string_lossy().into_owned()
    }

    #[test]
    fn degrade_on_stall_off_is_byte_identical() {
        // With the lever OFF, no on-disk state can flip the decision -> exhausted arm stays fail_descendants.
        let dir = degrade_fixture("off");
        let main = write_file(&dir, "main.go", "package main\nfunc main(){}\n");
        assert!(!should_degrade_on_stall(false, false, "cli-entry", &[main]));
    }

    /// THE SINK IS THE TASK THIS EXISTS FOR, AND IT WAS THE ONE TASK EXCLUDED.
    ///
    /// `integrate-verify` owns no files, so `critical_owned_files_written` fell through to `any()` over
    /// an empty slice — false — and the join could never degrade. Measured consequence: a transient
    /// `stream decode error (mid-stream body drop)` re-dispatched the entire join to another node and
    /// restarted it from zero, twice, costing 15.3 min on one cell and 44.3 min (29.5% of its wall) on
    /// another. Killing the longest, most fleet-blocking task in the run because a socket hiccuped
    /// discards every command already run and every fix already written.
    #[test]
    fn a_task_that_owns_nothing_is_recorded_unfinished_rather_than_restarted() {
        // The sink, the per-module verifies and the e2e shards all own nothing.
        for id in ["integrate-verify", "verify::store", "verify-e2e::2"] {
            assert!(
                should_degrade_on_stall(true, false, id, &[]),
                "{id} owns nothing: a transient stall must record it unfinished, not restart it"
            );
        }
        // The lever still gates it, and a CONTENT failure still refuses — an owns-nothing task whose
        // syntax gate rejected something is a real defect, not a dropped socket.
        assert!(!should_degrade_on_stall(
            false,
            false,
            "integrate-verify",
            &[]
        ));
        assert!(!should_degrade_on_stall(
            true,
            true,
            "integrate-verify",
            &[]
        ));
    }

    #[test]
    fn degrade_on_stall_promotes_only_when_critical_file_written() {
        let dir = degrade_fixture("crit");
        let main = write_file(&dir, "main.go", "package main\nfunc main(){}\n");
        // ON + non-content + non-test + critical file present -> degrade.
        assert!(should_degrade_on_stall(
            true,
            false,
            "cli-entry",
            std::slice::from_ref(&main)
        ));
        // A missing critical file must NOT degrade (the test4 failure: shipping with no entrypoint).
        let missing = dir.join("gone.go").to_string_lossy().into_owned();
        assert!(!should_degrade_on_stall(
            true,
            false,
            "cli-entry",
            &[missing]
        ));
        // An empty critical file is not "written".
        let empty = write_file(&dir, "empty.go", "");
        assert!(!should_degrade_on_stall(true, false, "cli-entry", &[empty]));
    }

    #[test]
    fn degrade_on_stall_refuses_content_failures_and_test_tasks() {
        let dir = degrade_fixture("refuse");
        let main = write_file(&dir, "main.go", "package main\n");
        // A CONTENT (syntax-gate) failure means the file is broken -> never degrade even if it exists.
        assert!(!should_degrade_on_stall(
            true,
            true,
            "cli-entry",
            std::slice::from_ref(&main)
        ));
        // A test task is never salvaged/degraded, even with its file on disk.
        let tf = write_file(&dir, "miner_test.go", "package miner\n");
        assert!(!should_degrade_on_stall(true, false, "miner-tests", &[tf]));
    }

    #[test]
    fn degrade_on_stall_manifest_only_falls_back_to_any() {
        // A task owning ONLY a manifest (no critical source) degrades on any-nonempty (there's nothing else to
        // gate on); it is not a source task, so this cannot ship a broken entrypoint.
        let dir = degrade_fixture("manifest");
        let gomod = write_file(&dir, "go.mod", "module x\n");
        assert!(should_degrade_on_stall(true, false, "manifest", &[gomod]));
        // But a manifest-only task with an EMPTY manifest still fails (nothing on disk).
        let dir2 = degrade_fixture("manifest2");
        let empty_mod = write_file(&dir2, "go.mod", "");
        assert!(!should_degrade_on_stall(
            true,
            false,
            "manifest",
            &[empty_mod]
        ));
    }
}
