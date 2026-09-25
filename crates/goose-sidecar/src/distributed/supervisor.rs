//! The distributed engine's supervisor: one task per run owns the rank processes, and
//! everything the STEP1b soak said a supervisor must own lives here:
//!
//! 1. its own liveness measure — the rank-0 generation loop's step counter plus every rank's CPU
//!    time, judged by the soak's hang rule (no progress for `HANG_MEDIAN_MULTIPLE` × the running
//!    median progress interval), because a FROZEN rank or a dropped ring link gives no error, no
//!    exit and no EOF;
//! 2. a stream without `[DONE]` is a failure (readiness), whatever the HTTP status said;
//! 3. any rank dying stops the whole run and the restart policy decides what happens next;
//! 4. STOP is SIGTERM to the local rank, then the peer rank's own pid verified gone over ssh
//!    (SIGTERM, then SIGKILL, per pid — never a process group); a pipeline peer is first given
//!    the grace window to leave on rank 0's shutdown broadcast;
//! 5. the memory watchdog on the same poll: WARN stops admitting new requests, CRITICAL stops the
//!    run with a loud event and never restarts it; a busy stretch that has already taken what is
//!    left above CRITICAL on a node closes admission too (`MemoryGrowth`), before the floor;
//! 6. a rank that died of memory (`RankOutOfMemory`) restarts the pair once memory recovered on
//!    every node — once per outage, whatever `restart_on_failure` says.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
use tokio::sync::watch;
use tokio::task::JoinHandle;

use super::compaction::{self, CompactionRefusal, NodeCompaction};
use super::config::{Backend, DistributedConfig, Runner};
use super::exec::{NodeExec, SystemExec};
use super::launch::{self, RankMemory, RankPhase, RankProcess};
use super::link_control::{self, ControlEvent, LinkRoutedExec};
use super::local_network;
use super::node_op::{NodeOp, Signal};
use super::preflight::{self, now_ms, PreflightReport};
use super::probe::{self, Pressure, SseVerdict};
use super::{HANG_MEDIAN_MULTIPLE, HANG_MIN_SAMPLES};
use crate::{SidecarConfig, GIB, GRACE_TICK, GRACE_TICKS};

/// The supervisor's sampling cadence (memory, pressure, rank processes, the step counter) and the
/// rank wrapper's memory report cadence. It samples; it decides nothing by itself — the hang bound
/// is a multiple of the MEASURED progress intervals, which this cadence only quantises. Parity with
/// the MLX view's own 2 s status cadence.
pub(crate) const POLL_INTERVAL: Duration = Duration::from_secs(2);
/// Readiness polling tick (parity with `Sidecar::await_ready`'s 400 ms).
pub(crate) const READY_TICK: Duration = Duration::from_millis(400);
/// A transport bound on the control endpoints (`/goose/progress`, `/goose/admission`) — parity
/// with the single manager's 5 s probe client. The readiness completion has NO read timeout: it
/// is bounded by rank progress, never by a clock.
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);
/// The running-median window of progress intervals (an algorithm constant).
const MEDIAN_WINDOW: usize = 64;
const EVENT_LIMIT: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RunState {
    Stopped,
    Preflight,
    Starting,
    Ready,
    Serving,
    Failed,
    Stopping,
    /// A rank died of memory: the pair is stopped and waits for every node's memory to recover,
    /// then restarts once (`DistributedStatus::memory_recovery` says what it waits for).
    Recovering,
}

impl RunState {
    pub fn as_str(self) -> &'static str {
        match self {
            RunState::Stopped => "stopped",
            RunState::Preflight => "preflight",
            RunState::Starting => "starting",
            RunState::Ready => "ready",
            RunState::Serving => "serving",
            RunState::Failed => "failed",
            RunState::Stopping => "stopping",
            RunState::Recovering => "recovering",
        }
    }

    /// The distributed engine owns this Mac (the single engine must not mount).
    pub fn owns_the_mac(self) -> bool {
        matches!(
            self,
            RunState::Preflight
                | RunState::Starting
                | RunState::Ready
                | RunState::Serving
                | RunState::Stopping
                | RunState::Recovering
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeState {
    Preflight,
    Loading,
    Ready,
    Serving,
    Failed,
    Stopped,
}

impl NodeState {
    pub fn as_str(self) -> &'static str {
        match self {
            NodeState::Preflight => "preflight",
            NodeState::Loading => "loading",
            NodeState::Ready => "ready",
            NodeState::Serving => "serving",
            NodeState::Failed => "failed",
            NodeState::Stopped => "stopped",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EventKind {
    Preflight,
    LinkRepaired,
    Launched,
    Ready,
    StartFailed,
    RankDied,
    RankFrozen,
    Hang,
    StreamWithoutDone,
    Restart,
    BreakerOpen,
    WatchdogWarn,
    WatchdogCritical,
    WatchdogBlind,
    AdmissionClosed,
    AdmissionOpened,
    StopRequested,
    Stopped,
    OrphanReclaimed,
    /// A start or restart found this install's previous ranks still shutting down under a live
    /// parent; the message names the pids and what will end them.
    PreviousSplitWaiting,
    /// The LeanZero Link control session to a Link node stopped answering. The ranks are not
    /// stopped by it: the message says whether the data plane (rank 0's step counter) still moves.
    LinkControlLost,
    /// It answers again.
    LinkControlRestored,
    /// A rank on THIS Mac died naming EHOSTUNREACH: macOS local network privacy (see
    /// `local_network`), not the cable.
    LocalNetworkBlocked,
    /// A node's memory was compacted ("Make room"): the message carries before → settled and the
    /// kernel's WARN point.
    MemoryCompacted,
    /// A compaction did not run on a node (an engine loaded there, pressure not NORMAL) or failed.
    CompactionSkipped,
    /// A rank died of memory: its own words name a Metal out-of-memory (or the wrapper's
    /// `RANK_FATAL` says so), or it was SIGKILLed while the watchdog held admission for memory.
    RankOutOfMemory,
    /// The memory a busy stretch has consumed on a node is at least what is left before its
    /// CRITICAL reserve: one more stretch like it would reach CRITICAL, so admission closes.
    MemoryGrowth,
    /// A rank's MLX active bytes exceed its plan (with overhead): the plan's accounting no longer
    /// holds on that rank.
    RankOverPlan,
    /// After a memory death, every node is back above its WARN reserve: the pair restarts.
    MemoryRecovered,
}

impl EventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            EventKind::Preflight => "preflight",
            EventKind::LinkRepaired => "linkRepaired",
            EventKind::Launched => "launched",
            EventKind::Ready => "ready",
            EventKind::StartFailed => "startFailed",
            EventKind::RankDied => "rankDied",
            EventKind::RankFrozen => "rankFrozen",
            EventKind::Hang => "hang",
            EventKind::StreamWithoutDone => "streamWithoutDone",
            EventKind::Restart => "restart",
            EventKind::BreakerOpen => "breakerOpen",
            EventKind::WatchdogWarn => "watchdogWarn",
            EventKind::WatchdogCritical => "watchdogCritical",
            EventKind::WatchdogBlind => "watchdogBlind",
            EventKind::AdmissionClosed => "admissionClosed",
            EventKind::AdmissionOpened => "admissionOpened",
            EventKind::StopRequested => "stopRequested",
            EventKind::Stopped => "stopped",
            EventKind::OrphanReclaimed => "orphanReclaimed",
            EventKind::PreviousSplitWaiting => "previousSplitWaiting",
            EventKind::LinkControlLost => "linkControlLost",
            EventKind::LinkControlRestored => "linkControlRestored",
            EventKind::LocalNetworkBlocked => "localNetworkBlocked",
            EventKind::MemoryCompacted => "memoryCompacted",
            EventKind::CompactionSkipped => "compactionSkipped",
            EventKind::RankOutOfMemory => "rankOutOfMemory",
            EventKind::MemoryGrowth => "memoryGrowth",
            EventKind::RankOverPlan => "rankOverPlan",
            EventKind::MemoryRecovered => "memoryRecovered",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EngineEvent {
    pub at_ms: u64,
    pub kind: EventKind,
    pub node: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NodeStatus {
    pub name: String,
    pub rank: usize,
    /// "coordinator" (rank 0, serves HTTP) | "worker".
    pub role: String,
    pub host: Option<String>,
    pub state: NodeState,
    pub pid: Option<u32>,
    pub layer_start: Option<u32>,
    pub layer_end: Option<u32>,
    pub shard_index: Option<u32>,
    pub shard_count: Option<u32>,
    pub available_bytes: Option<u64>,
    pub total_bytes: Option<u64>,
    pub pressure: Option<String>,
    pub memory_error: Option<String>,
    /// MLX's own counters on the rank (`mx.get_active_memory` / `get_peak_memory`).
    pub active_bytes: Option<u64>,
    pub peak_bytes: Option<u64>,
    /// MLX's free-buffer cache on the rank (`mx.get_cache_memory`): resident, counted by the
    /// node's footprint, invisible in `active_bytes`.
    pub cache_bytes: Option<u64>,
    /// The launch plan's WITH-OVERHEAD figure (`RankPlan::with_overhead_bytes`) — the one the
    /// preflight compares with the budget and prints as "planned with overhead", so the node
    /// card and the preflight never show two different "planned" numbers.
    pub planned_bytes: Option<u64>,
    /// The caps the rank applied in-process (its own `GOOSE_RANK_CAPS` report): absent until the
    /// rank reported them.
    pub memory_limit_bytes: Option<u64>,
    pub wired_limit_bytes: Option<u64>,
    pub cache_limit_bytes: Option<u64>,
    /// Pipeline only, from rank 0's `/v1/status`: the KV/state + workspace bytes the requests in
    /// flight hold on this rank (0 when idle), and this rank's budget for them (its planned state +
    /// workspace for `slots` full-context sequences). `None` for the tensor runner, before the
    /// first poll, or when the poll failed (`DistributedStatus::server_status_error`).
    pub kv_reserved_bytes: Option<u64>,
    pub kv_budget_bytes: Option<u64>,
    /// While the node is `Loading`: where its rank's start is (`RankLive::phase` — "loading" |
    /// "warming" | "ready"); `active_bytes` ÷ `planned_weight_bytes` is the load itself (the rank's
    /// MLX active bytes against the weights preflight planned on it). `None` once the engine is
    /// up, and before the rank reported anything.
    pub load_phase: Option<RankPhase>,
    pub planned_weight_bytes: Option<u64>,
    pub backend: Backend,
    pub tb_ip: String,
    pub tb_interface: String,
    pub link_speed: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Liveness {
    pub samples: usize,
    pub median_ms: Option<u64>,
    pub bound_ms: Option<u64>,
    pub silent_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DistributedStatus {
    pub state: RunState,
    pub backend: Option<Backend>,
    pub runner: Option<Runner>,
    pub model_id: Option<String>,
    /// The id the ranks serve on `/v1/models` and accept in requests — derived by the SAME
    /// `engine::served_model_id` the single engine's `--served-model-name` comes from, so a swarm
    /// node names one id whichever engine owns this Mac.
    pub served_model_id: Option<String>,
    pub base_url: Option<String>,
    pub context_limit: Option<u64>,
    pub admission_open: bool,
    pub inflight: Option<u32>,
    /// Rank 0's `/v1/status`, read on every poll: requests queued behind the running batch (the
    /// pipeline's KV admission holds a request that would overrun some rank's budget — it waits,
    /// FIFO, never dropped).
    pub waiting: Option<u32>,
    /// Pipeline only: the full-context sequences the split was planned for, the worst rank's
    /// reservation in slot units (ceil), and the sequences in the running batch. `None` for the
    /// tensor runner (mlx_lm.server has no slots), before the first poll, or when it failed.
    pub slots: Option<u32>,
    pub slots_in_use: Option<u32>,
    pub sequences_in_flight: Option<u32>,
    /// Why the last `/v1/status` read failed or broke its contract; `None` when it answered.
    pub server_status_error: Option<String>,
    pub liveness: Option<Liveness>,
    pub nodes: Vec<NodeStatus>,
    pub last_preflight: Option<PreflightReport>,
    pub events: Vec<EngineEvent>,
    pub restarts: u32,
    pub last_error: Option<String>,
    pub config: Option<DistributedConfig>,
    /// The latest compaction per node (automatic or "Make room"), newest last.
    pub compactions: Vec<NodeCompaction>,
    /// The Macs macOS is reclaiming memory on right now, before the start judges them again.
    pub making_room: Vec<String>,
    /// While `state` is `recovering`: what the restart after a memory death waits for, per node.
    pub memory_recovery: Option<String>,
}

impl DistributedStatus {
    fn stopped() -> Self {
        Self {
            state: RunState::Stopped,
            backend: None,
            runner: None,
            model_id: None,
            served_model_id: None,
            base_url: None,
            context_limit: None,
            admission_open: true,
            inflight: None,
            waiting: None,
            slots: None,
            slots_in_use: None,
            sequences_in_flight: None,
            server_status_error: None,
            liveness: None,
            nodes: Vec::new(),
            last_preflight: None,
            events: Vec::new(),
            restarts: 0,
            last_error: None,
            config: None,
            compactions: Vec::new(),
            making_room: Vec::new(),
            memory_recovery: None,
        }
    }

    pub fn mode(&self) -> &'static str {
        if self.state.owns_the_mac() {
            "distributed"
        } else {
            "single"
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RefusalCode {
    /// The single MLX engine is mounted on this Mac; unmount it first (the UI offers it).
    SingleEngineMounted,
    AlreadyRunning,
    PreflightFailed,
    /// THIS install's previous split still runs on `node` under a live parent (its goose is
    /// stopping it, or a peer's lease will): nothing is signalled; a start once its pids are gone
    /// goes through. Orphans never reach this — they are reclaimed per pid.
    PreviousSplitShuttingDown,
    /// A distributed MLX process this install did not launch runs on `node`; `detail` names it.
    ForeignSplit,
}

impl RefusalCode {
    pub fn as_str(self) -> &'static str {
        match self {
            RefusalCode::SingleEngineMounted => "singleEngineMounted",
            RefusalCode::AlreadyRunning => "alreadyRunning",
            RefusalCode::PreflightFailed => "preflightFailed",
            RefusalCode::PreviousSplitShuttingDown => "previousSplitShuttingDown",
            RefusalCode::ForeignSplit => "foreignSplit",
        }
    }
}

#[derive(Debug, Clone)]
pub enum StartOutcome {
    Started {
        preflight: PreflightReport,
    },
    Refused {
        code: RefusalCode,
        message: String,
        preflight: Option<PreflightReport>,
        /// The Mac the refusal is about, by its configured name.
        node: Option<String>,
        /// What stands behind the message — pids and command lines — for a Details disclosure.
        detail: Option<String>,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StopReport {
    /// Each step, in order, with what was signalled and what was observed.
    pub steps: Vec<String>,
    /// Every rank's own pid was observed gone (the peer's over ssh).
    pub verified: bool,
}

struct Shared {
    status: DistributedStatus,
    stop_tx: Option<watch::Sender<bool>>,
    task: Option<JoinHandle<StopReport>>,
}

impl Shared {
    fn record_compaction(&mut self, record: NodeCompaction) {
        let (kind, message) = match (&record.report, &record.refusal, &record.error) {
            (Some(report), _, _) => (EventKind::MemoryCompacted, report.summary()),
            (_, Some(refusal), _) => (
                EventKind::CompactionSkipped,
                format!("{}: {}", refusal.code, refusal.message),
            ),
            (_, _, Some(error)) => (EventKind::CompactionSkipped, format!("failed: {error}")),
            (None, None, None) => (EventKind::CompactionSkipped, "no outcome".to_string()),
        };
        self.event(kind, Some(&record.node), message);
        self.status.compactions.retain(|c| c.node != record.node);
        self.status.compactions.push(record);
    }

    fn event(&mut self, kind: EventKind, node: Option<&str>, message: impl Into<String>) {
        let message = message.into();
        match kind {
            EventKind::RankDied
            | EventKind::RankFrozen
            | EventKind::Hang
            | EventKind::StreamWithoutDone
            | EventKind::WatchdogCritical
            | EventKind::BreakerOpen
            | EventKind::StartFailed
            | EventKind::LinkControlLost
            | EventKind::LocalNetworkBlocked => {
                tracing::warn!(kind = kind.as_str(), node, "distributed engine: {message}")
            }
            _ => tracing::info!(kind = kind.as_str(), node, "distributed engine: {message}"),
        }
        if self.status.events.len() == EVENT_LIMIT {
            self.status.events.remove(0);
        }
        self.status.events.push(EngineEvent {
            at_ms: now_ms(),
            kind,
            node: node.map(str::to_string),
            message,
        });
    }
}

/// The soak's hang rule over the supervisor's own measure. Progress on a poll = the rank-0 step
/// counter advanced OR every rank's CPU time advanced (a long prefill chunk freezes the counter but
/// not the ranks; a frozen rank or a blocked collective freezes both). A HANG is no progress for
/// longer than `HANG_MEDIAN_MULTIPLE` × the running median of the intervals between progress —
/// held until `HANG_MIN_SAMPLES` intervals exist.
pub struct ProgressMeter {
    last_progress: Instant,
    intervals: VecDeque<Duration>,
    last_steps: Option<u64>,
    last_cpu: Vec<Option<u64>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MeterReading {
    pub progressed: bool,
    pub silent_for: Duration,
    pub median: Option<Duration>,
    pub bound: Option<Duration>,
    pub hang: bool,
}

impl ProgressMeter {
    pub fn new(now: Instant, ranks: usize) -> Self {
        Self {
            last_progress: now,
            intervals: VecDeque::new(),
            last_steps: None,
            last_cpu: vec![None; ranks],
        }
    }

    pub fn median(&self) -> Option<Duration> {
        if self.intervals.len() < HANG_MIN_SAMPLES {
            return None;
        }
        let mut sorted: Vec<Duration> = self.intervals.iter().copied().collect();
        sorted.sort();
        Some(sorted[sorted.len() / 2])
    }

    pub fn observe(
        &mut self,
        now: Instant,
        steps: Option<u64>,
        cpu: &[Option<u64>],
    ) -> MeterReading {
        let steps_advanced = matches!((self.last_steps, steps), (Some(a), Some(b)) if b > a);
        let cpu_advanced = cpu.len() == self.last_cpu.len()
            && cpu
                .iter()
                .zip(&self.last_cpu)
                .all(|(now, before)| matches!((before, now), (Some(a), Some(b)) if b > a));
        let progressed = steps_advanced || cpu_advanced;
        if steps.is_some() {
            self.last_steps = steps;
        }
        for (slot, value) in self.last_cpu.iter_mut().zip(cpu) {
            if value.is_some() {
                *slot = *value;
            }
        }
        if progressed {
            let interval = now.saturating_duration_since(self.last_progress);
            if self.intervals.len() == MEDIAN_WINDOW {
                self.intervals.pop_front();
            }
            self.intervals.push_back(interval);
            self.last_progress = now;
        }
        let silent_for = now.saturating_duration_since(self.last_progress);
        let median = self.median();
        let bound = median.map(|m| m.mul_f64(HANG_MEDIAN_MULTIPLE));
        MeterReading {
            progressed,
            silent_for,
            median,
            bound,
            hang: bound.is_some_and(|b| silent_for > b),
        }
    }
}

impl ProgressMeter {
    /// Nothing is in flight: silence is the idle engine, not a stall. The silence clock restarts
    /// here, so it measures only time spent WITH work (since fork 286ed77f7 an idle pipeline burns
    /// no CPU on any rank — the spinning worker used to keep this rule quiet by accident).
    pub fn rest(&mut self, now: Instant) {
        self.last_progress = now;
    }
}

/// The hang rule's verdict for one poll: silence counts only while rank 0 reports work in flight.
/// An unreadable progress counter counts as work — a rank-0 server that stopped answering must
/// still be caught.
fn judge_silence(meter: &mut ProgressMeter, now: Instant, busy: bool, reading: &mut MeterReading) {
    if !busy {
        meter.rest(now);
        reading.hang = false;
        reading.silent_for = Duration::ZERO;
    }
}

/// The restart breaker, parity with `Sidecar::ensure_running`: the count is checked BEFORE the
/// push, so `max` restarts inside `window` are allowed and the next failure opens it.
fn breaker_allows(
    restarts: &mut VecDeque<Instant>,
    now: Instant,
    window: Duration,
    max: u32,
) -> bool {
    while restarts
        .front()
        .is_some_and(|front| now.duration_since(*front) > window)
    {
        restarts.pop_front();
    }
    if restarts.len() as u32 >= max {
        return false;
    }
    restarts.push_back(now);
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Watchdog {
    Normal,
    Warn,
    Critical,
}

/// The kernel's own level, or the configured reserves of RAM — whichever is worse.
fn watchdog_verdict(
    pressure: Pressure,
    available: u64,
    total: u64,
    (warn_ratio, critical_ratio): (f64, f64),
) -> Watchdog {
    let available = available as f64;
    let total = total as f64;
    if pressure == Pressure::Critical || available < critical_ratio * total {
        Watchdog::Critical
    } else if pressure == Pressure::Warn || available < warn_ratio * total {
        Watchdog::Warn
    } else {
        Watchdog::Normal
    }
}

fn gib(bytes: u64) -> String {
    format!("{:.1} GiB", bytes as f64 / GIB as f64)
}

/// Whether `pid` still runs on `host` (a zombie counts as gone: it holds no memory and no port).
/// A ps that could not answer is an `Err`, never "gone": every caller signals or verifies on it.
async fn pid_alive(exec: &dyn NodeExec, host: Option<&str>, pid: u32) -> Result<bool> {
    let out = exec.run_op(host, &NodeOp::PidRow { pid }).await?;
    if out.ssh_failed() {
        return Err(anyhow!("ssh failed: {}", out.stderr.trim()));
    }
    match out.ps_answer()? {
        None => Ok(false),
        Some(rows) => Ok(probe::parse_ps_row(rows)?.is_some_and(|row| !row.zombie())),
    }
}

async fn wait_gone(exec: &dyn NodeExec, host: Option<&str>, pid: u32) -> Result<Option<Duration>> {
    let started = Instant::now();
    for _ in 0..GRACE_TICKS / 5 {
        if !pid_alive(exec, host, pid).await? {
            return Ok(Some(started.elapsed()));
        }
        tokio::time::sleep(GRACE_TICK * 5).await;
    }
    Ok((!pid_alive(exec, host, pid).await?).then(|| started.elapsed()))
}

/// Signal one pid on a node, per pid: `kill -<SIG> <pid>`.
async fn signal_pid(exec: &dyn NodeExec, host: Option<&str>, pid: u32, signal: Signal) -> String {
    match exec.run_op(host, &NodeOp::Signal { pid, signal }).await {
        Ok(out) if out.success() => "sent".to_string(),
        Ok(out) => format!("kill exited {:?}: {}", out.status, out.stderr.trim()),
        Err(e) => format!("{e:#}"),
    }
}

pub(crate) async fn wait_child_exit(
    child: &mut tokio::process::Child,
) -> Option<std::process::ExitStatus> {
    for _ in 0..GRACE_TICKS {
        if let Ok(Some(status)) = child.try_wait() {
            return Some(status);
        }
        tokio::time::sleep(GRACE_TICK).await;
    }
    child.try_wait().ok().flatten()
}

fn signal_local(pid: u32, signal: libc::c_int) {
    unsafe {
        libc::kill(pid as libc::pid_t, signal);
    }
}

/// THE STOP SEQUENCE. Every signal targets one pid; no process group is ever signalled.
/// 1. SIGTERM the local rank(s) and wait the crate's grace window for the exit.
/// 2. For each peer rank: observe its own pid over ssh — once (tensor: a peer does not follow
///    rank 0), or through the grace window when `peers_follow_rank0` (pipeline: rank 0's SIGTERM
///    broadcasts a shutdown every rank obeys, measured on the tiny model: both pids gone within
///    1 s); alive → SIGTERM that pid; still alive after the grace → SIGKILL that pid; the final
///    observation decides `verified`.
/// 3. A local rank that outlived its grace → SIGKILL its pid.
/// 4. Each peer's local ssh client: it ends with its session; if not, SIGTERM then SIGKILL its pid.
/// 5. The API port is waited free.
pub(crate) async fn stop_ranks(
    ranks: &mut [RankProcess],
    exec: &dyn NodeExec,
    port: Option<u16>,
    peers_follow_rank0: bool,
) -> StopReport {
    let mut report = StopReport {
        steps: Vec::new(),
        verified: true,
    };
    let started = Instant::now();
    for rank in ranks.iter_mut().filter(|r| r.host.is_none()) {
        match rank.child.id() {
            Some(pid) => {
                signal_local(pid, libc::SIGTERM);
                let exit = wait_child_exit(&mut rank.child).await;
                report.steps.push(match exit {
                    Some(status) => format!(
                        "rank {} ({}) pid {pid}: SIGTERM → exited ({status}) after {} ms",
                        rank.rank,
                        rank.node,
                        started.elapsed().as_millis()
                    ),
                    None => format!(
                        "rank {} ({}) pid {pid}: SIGTERM → still running after the grace window",
                        rank.rank, rank.node
                    ),
                });
            }
            None => report.steps.push(format!(
                "rank {} ({}): already exited",
                rank.rank, rank.node
            )),
        }
    }
    for rank in ranks.iter_mut().filter(|r| r.host.is_some()) {
        let host = rank.host.clone();
        let host = host.as_deref();
        if link_control::link_peer(host).is_some() {
            // The peer's own goosed stops its rank per pid and verifies it gone.
            let (line, verified) = link_control::stop_link_rank(rank, peers_follow_rank0).await;
            report.verified &= verified;
            report.steps.push(line);
            continue;
        }
        let Some(pid) = rank.pid() else {
            report.steps.push(format!(
                "rank {} ({}): the peer never reported its pid; its ssh session is ended below and the node is swept for goose ranks",
                rank.rank, rank.node
            ));
            let swept = reclaim_marked_ranks(exec, host, &rank.node, rank.owner.as_deref()).await;
            report.verified &= swept.1;
            report.steps.extend(swept.0);
            continue;
        };
        // Tensor: one observation, no wait — measured 2026-09-24, a peer rank does NOT exit when
        // rank 0 does (mlx.launch's cleanup script did that; goose's launcher signals it itself).
        let mut gone = if peers_follow_rank0 {
            wait_gone(exec, host, pid).await
        } else {
            match pid_alive(exec, host, pid).await {
                Ok(true) => Ok(None),
                Ok(false) => Ok(Some(Duration::ZERO)),
                Err(e) => Err(e),
            }
        };
        let mut line = match &gone {
            Ok(Some(after)) if peers_follow_rank0 => format!(
                "rank {} ({}) pid {pid}: left on rank 0's shutdown broadcast within {} ms \
                 (verified over ssh)",
                rank.rank,
                rank.node,
                after.as_millis()
            ),
            Ok(Some(_)) => format!(
                "rank {} ({}) pid {pid}: already gone after rank 0's exit (verified over ssh)",
                rank.rank, rank.node
            ),
            Ok(None) => String::new(),
            Err(e) => format!(
                "rank {} ({}) pid {pid}: cannot observe: {e:#}",
                rank.rank, rank.node
            ),
        };
        if matches!(gone, Ok(None)) {
            let sent = signal_pid(exec, host, pid, Signal::Term).await;
            gone = wait_gone(exec, host, pid).await;
            line = format!(
                "rank {} ({}) pid {pid}: alive after rank 0's exit{} → SIGTERM ({sent})",
                rank.rank,
                rank.node,
                if peers_follow_rank0 {
                    " and the grace window"
                } else {
                    ""
                }
            );
            if matches!(gone, Ok(None)) {
                let sent = signal_pid(exec, host, pid, Signal::Kill).await;
                gone = wait_gone(exec, host, pid).await;
                line.push_str(&format!(" → alive after the grace → SIGKILL ({sent})"));
            }
            line.push_str(match &gone {
                Ok(Some(_)) => " → gone (verified over ssh)",
                Ok(None) => " → STILL ALIVE",
                Err(_) => " → could not observe",
            });
        }
        report.verified &= matches!(gone, Ok(Some(_)));
        report.steps.push(line);
    }
    for rank in ranks.iter_mut().filter(|r| r.host.is_none()) {
        if let (Ok(None), Some(pid)) = (rank.child.try_wait(), rank.child.id()) {
            signal_local(pid, libc::SIGKILL);
            let exit = wait_child_exit(&mut rank.child).await;
            report.verified &= exit.is_some();
            report.steps.push(format!(
                "rank {} ({}) pid {pid}: SIGKILL → {}",
                rank.rank,
                rank.node,
                if exit.is_some() {
                    "exited"
                } else {
                    "STILL ALIVE"
                }
            ));
        }
    }
    for rank in ranks.iter_mut().filter(|r| r.host.is_some()) {
        if wait_child_exit(&mut rank.child).await.is_some() {
            continue;
        }
        if let Some(pid) = rank.child.id() {
            signal_local(pid, libc::SIGTERM);
            if wait_child_exit(&mut rank.child).await.is_none() {
                signal_local(pid, libc::SIGKILL);
                let _ = rank.child.wait().await;
            }
            report.steps.push(format!(
                "rank {} ({}): its ssh client pid {pid} outlived the session; terminated per pid",
                rank.rank, rank.node
            ));
        }
    }
    if let Some(port) = port {
        if crate::wait_port_clear(port).await {
            report.steps.push(format!("port {port} released"));
        } else {
            report.verified = false;
            report.steps.push(format!("port {port} STILL OCCUPIED"));
        }
    }
    report
}

/// Every goose rank on a node that carries `owner` (this install's token): SIGTERM, grace,
/// SIGKILL — per pid. The reclaim for ranks a previous goosed left behind (supervision state is
/// in memory only). The marker alone never licenses a signal: a cargo test's stand-in rank and
/// another install's live split carry it too. No owner = nothing provable = nothing signalled.
async fn reclaim_marked_ranks(
    exec: &dyn NodeExec,
    host: Option<&str>,
    node: &str,
    owner: Option<&str>,
) -> (Vec<String>, bool) {
    let listing = match exec.run_op(host, &NodeOp::ProcessList).await {
        Ok(out) if out.success() => out.stdout,
        Ok(out) if out.ssh_failed() => {
            return (
                vec![format!("{node}: ssh failed: {}", out.stderr.trim())],
                false,
            )
        }
        // An empty listing from a ps that failed is not "no goose ranks": nothing is signalled
        // and the sweep is unverified.
        Ok(out) => {
            tracing::warn!(
                event = "rank_sweep_unproven",
                node,
                status = ?out.status,
                stderr = out.stderr.trim(),
                "the process listing failed; no rank was signalled"
            );
            return (
                vec![format!(
                    "{node}: the process listing failed (exit {:?}: {}); nothing signalled",
                    out.status,
                    out.stderr.trim()
                )],
                false,
            );
        }
        Err(e) => return (vec![format!("{node}: {e:#}")], false),
    };
    let Some(owner) = owner else {
        return (
            vec![format!(
                "{node}: this goose launched its ranks with no owner token, so no rank here is \
                 provably its own; nothing signalled"
            )],
            false,
        );
    };
    // Judged on the WHOLE command line: the marker and the token follow the program's base64,
    // far past the 160 characters a displayed row keeps.
    let marked: Vec<u32> = probe::goose_rank_processes(&listing, None, &[])
        .into_iter()
        .filter(|r| r.owner.as_deref() == Some(owner))
        .map(|r| r.pid)
        .collect();
    let mut steps = Vec::new();
    let mut verified = true;
    for pid in marked {
        let (line, ok) = reclaim_rank_pid(exec, host, node, pid).await;
        verified &= ok;
        steps.push(line);
    }
    (steps, verified)
}

/// One goose rank nobody supervises: SIGTERM, grace, SIGKILL — to this pid only — and whether it
/// was observed gone.
pub(crate) async fn reclaim_rank_pid(
    exec: &dyn NodeExec,
    host: Option<&str>,
    node: &str,
    pid: u32,
) -> (String, bool) {
    let sent = signal_pid(exec, host, pid, Signal::Term).await;
    let mut gone = wait_gone(exec, host, pid).await;
    let mut line = format!("{node}: goose rank pid {pid} → SIGTERM ({sent})");
    if matches!(gone, Ok(None)) {
        let sent = signal_pid(exec, host, pid, Signal::Kill).await;
        gone = wait_gone(exec, host, pid).await;
        line.push_str(&format!(" → SIGKILL ({sent})"));
    }
    let ok = matches!(gone, Ok(Some(_)));
    line.push_str(if ok { " → gone" } else { " → STILL ALIVE" });
    (line, ok)
}

struct RunContext {
    shared: Arc<StdMutex<Shared>>,
    exec: Arc<dyn NodeExec>,
    http: reqwest::Client,
    stream_http: reqwest::Client,
    config: DistributedConfig,
    served_id: String,
    runner: Runner,
    /// This install's owner token, written on every rank this run launches.
    owner: Option<String>,
}

impl RunContext {
    fn update(&self, f: impl FnOnce(&mut Shared)) {
        f(&mut self.shared.lock().unwrap());
    }

    fn event(&self, kind: EventKind, node: Option<&str>, message: impl Into<String>) {
        self.update(|s| s.event(kind, node, message));
    }

    fn set_node_states(&self, state: NodeState) {
        self.update(|s| {
            s.status.nodes.iter_mut().for_each(|n| {
                n.state = state;
                n.load_phase = None;
            })
        });
    }
}

enum ReadyOutcome {
    Ready,
    Stop,
    Failed(EventKind, Option<String>, String),
}

enum RunOutcome {
    Stop,
    Critical(String),
    Failed(EventKind, Option<String>, String),
}

/// How a launch ended when it did not fail: a requested stop, or the watchdog's CRITICAL.
enum Finish {
    Stop,
    Critical(String),
}

fn sidecar_parity() -> SidecarConfig {
    SidecarConfig::new("distributed", Vec::new(), "", "")
}

async fn readiness_ping(http: reqwest::Client, base: String, served: String) -> Result<SseVerdict> {
    let resp = http
        .post(format!("{base}/v1/chat/completions"))
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(serde_json::to_vec(&serde_json::json!({
            "model": served,
            "messages": [{"role": "user", "content": "ping"}],
            "max_tokens": 1,
            "temperature": 0,
            "stream": true,
            "chat_template_kwargs": {"enable_thinking": false},
        }))?)
        .send()
        .await?;
    let status = resp.status();
    let body = resp.text().await?;
    anyhow::ensure!(
        status.is_success(),
        "readiness completion answered HTTP {status}: {}",
        body.chars().take(300).collect::<String>()
    );
    Ok(probe::sse_verdict(&body))
}

fn local_progress_mark(sys: &mut System, ranks: &[RankProcess]) -> Vec<u64> {
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_cpu().with_memory(),
    );
    ranks
        .iter()
        .flat_map(|rank| {
            let lines = rank.live.lock().unwrap().lines;
            let local = rank
                .host
                .is_none()
                .then(|| rank.child.id())
                .flatten()
                .and_then(|pid| sys.process(Pid::from_u32(pid)))
                .map(|p| (p.accumulated_cpu_time(), p.memory()))
                .unwrap_or_default();
            [lines, local.0, local.1]
        })
        .collect()
}

/// A dead rank, named. A rank an APP runs — this Mac's, or a Link peer's (spawned by the peer's
/// goosed) — whose last output names EHOSTUNREACH is that app's Local Network privilege, not a
/// dead link; a peer under ssh is exempt (TN3179). A rank whose death names memory
/// ([`memory_death`]) is `RankOutOfMemory`. A Link rank's own end (who ended it, its code) is the
/// peer's report line in the tail — E2E #2's read "exited on its own with code 0", which the old
/// "its LeanZero Link session closed" contradicted.
fn rank_exit(
    rank: &RankProcess,
    status: std::process::ExitStatus,
    when: &str,
    after: &str,
    memory_short: Option<&str>,
) -> (EventKind, String) {
    let tail = rank.tail();
    let link = link_control::link_peer(rank.host.as_deref()).is_some();
    let what = match (&rank.host, link) {
        (None, _) => "exited",
        (Some(_), true) => "ended on its Link node",
        (Some(_), false) => "its ssh session ended",
    };
    let message = format!(
        "rank {} {what}{when} ({status}){after} Last output:\n{tail}",
        rank.rank
    );
    if (rank.host.is_none() || link) && local_network::names_host_unreachable(&tail) {
        let whose = if link {
            format!("on {}", rank.node)
        } else {
            "on this Mac".to_string()
        };
        (
            EventKind::LocalNetworkBlocked,
            format!("{} ({whose}): {message}", local_network::BLOCKED),
        )
    } else if let Some(evidence) = memory_death(&tail, status, memory_short) {
        (
            EventKind::RankOutOfMemory,
            format!("rank {} died of memory ({evidence}): {message}", rank.rank),
        )
    } else {
        (EventKind::RankDied, message)
    }
}

/// Memory's hand in a rank's death, quoted: the rank's own words — the wrapper's `RANK_FATAL`
/// with `out_of_memory`, or the Metal out-of-memory mlx raises (E2E #2's Studio rank: "RuntimeError:
/// [METAL] Command buffer execution failed: Insufficient Memory
/// (00000008:kIOGPUCommandBufferCallbackErrorOutOfMemory)") — or a SIGKILL while the watchdog held
/// admission for memory (the kernel's jetsam leaves no words; a Link session reports it as 128 + 9).
fn memory_death(
    tail: &str,
    status: std::process::ExitStatus,
    memory_short: Option<&str>,
) -> Option<String> {
    use std::os::unix::process::ExitStatusExt;
    if let Some(line) = tail.lines().rev().find(|line| {
        (line.starts_with("GOOSE_RANK_FATAL ") && line.contains("\"out_of_memory\": true"))
            || line.contains("kIOGPUCommandBufferCallbackErrorOutOfMemory")
            || line.contains("Command buffer execution failed: Insufficient Memory")
    }) {
        return Some(line.trim().to_string());
    }
    let killed =
        status.signal() == Some(libc::SIGKILL) || status.code() == Some(128 + libc::SIGKILL);
    match memory_short {
        Some(short) if killed => Some(format!("SIGKILL while {short}")),
        _ => None,
    }
}

async fn wait_ready(
    ctx: &RunContext,
    ranks: &mut [RankProcess],
    stop_rx: &mut watch::Receiver<bool>,
) -> ReadyOutcome {
    let base = ctx.config.base_url();
    let stall_window = sidecar_parity().startup_stall_window;
    let mut sys = System::new();
    let mut last_mark = local_progress_mark(&mut sys, ranks);
    let mut last_progress = Instant::now();
    let mut ping: Option<JoinHandle<Result<SseVerdict>>> = None;
    loop {
        tokio::select! {
            _ = stop_rx.changed() => return ReadyOutcome::Stop,
            _ = tokio::time::sleep(READY_TICK) => {}
        }
        for rank in ranks.iter_mut() {
            if let Ok(Some(status)) = rank.child.try_wait() {
                let (kind, message) = rank_exit(rank, status, " during startup", ".", None);
                return ReadyOutcome::Failed(kind, Some(rank.node.clone()), message);
            }
        }
        let seen: Vec<(Option<u32>, RankPhase, Option<RankMemory>)> = ranks
            .iter()
            .map(|rank| {
                let live = rank.live.lock().unwrap();
                (live.pid, live.phase(), live.memory)
            })
            .collect();
        ctx.update(|s| {
            for (node, (pid, phase, memory)) in s.status.nodes.iter_mut().zip(&seen) {
                node.pid = *pid;
                node.load_phase = Some(*phase);
                if let Some(memory) = memory {
                    node.active_bytes = Some(memory.active);
                    node.peak_bytes = Some(memory.peak.max(node.peak_bytes.unwrap_or(0)));
                }
            }
        });
        if ping.is_none() && crate::port_has_listener(ctx.config.port) {
            ping = Some(tokio::spawn(readiness_ping(
                ctx.stream_http.clone(),
                base.clone(),
                ctx.served_id.clone(),
            )));
        }
        if ping.as_ref().is_some_and(JoinHandle::is_finished) {
            return match ping.take().unwrap().await {
                Ok(Ok(SseVerdict::Complete { .. })) => ReadyOutcome::Ready,
                Ok(Ok(SseVerdict::Truncated { chunks })) => ReadyOutcome::Failed(
                    EventKind::StreamWithoutDone,
                    None,
                    format!(
                        "the readiness completion ended after {chunks} chunk(s) with no `data: [DONE]` \
                         under HTTP 200 — a rank died mid-request (STEP1b class a)"
                    ),
                ),
                Ok(Err(e)) => ReadyOutcome::Failed(
                    EventKind::StartFailed,
                    None,
                    format!("readiness completion failed: {e:#}"),
                ),
                Err(e) => ReadyOutcome::Failed(EventKind::StartFailed, None, format!("{e}")),
            };
        }
        let mark = local_progress_mark(&mut sys, ranks);
        if mark != last_mark {
            last_mark = mark;
            last_progress = Instant::now();
        } else if last_progress.elapsed() >= stall_window {
            return ReadyOutcome::Failed(
                EventKind::StartFailed,
                None,
                format!(
                    "startup stalled: no rank output and no local CPU/memory change for {:?} \
                     (the single engine's startup stall window). Rank 0 output:\n{}",
                    last_progress.elapsed(),
                    ranks[0].tail()
                ),
            );
        }
    }
}

#[derive(Debug, Deserialize)]
struct Progress {
    steps: u64,
    inflight: u32,
}

/// Rank 0's `/v1/status`. The tensor wrapper answers `{num_running, num_waiting, status}`; the
/// pipeline server (fork ea6f8dee1) adds the slot figures and per-rank KV lists.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ServerStatus {
    pub num_running: u32,
    pub num_waiting: u32,
    pub slots: Option<u32>,
    pub slots_in_use: Option<u32>,
    pub sequences_in_flight: Option<u32>,
    pub kv_reserved_bytes: Option<Vec<u64>>,
    pub kv_budget_bytes: Option<Vec<u64>>,
}

/// What the status poll publishes: the engine-wide figures and, per rank, (reserved, budget).
#[derive(Debug, Clone, PartialEq)]
pub struct ServerLoad {
    pub waiting: u32,
    pub slots: Option<u32>,
    pub slots_in_use: Option<u32>,
    pub sequences_in_flight: Option<u32>,
    pub kv: Option<Vec<(u64, u64)>>,
}

/// Read `/v1/status` for `runner` over `ranks` ranks. The pipeline server must carry its slot
/// figures and one budget per rank (an empty reservation list means idle: 0 reserved on every
/// rank); a missing figure is an error naming the broken contract, never a default. The tensor
/// wrapper has no slots and its absence is the answer.
pub fn server_load(body: &str, runner: Runner, ranks: usize) -> Result<ServerLoad> {
    let status: ServerStatus = serde_json::from_str(body)
        .map_err(|e| anyhow!("/v1/status is not the expected shape ({e}): {body}"))?;
    if runner == Runner::MlxLmTensor {
        return Ok(ServerLoad {
            waiting: status.num_waiting,
            slots: None,
            slots_in_use: None,
            sequences_in_flight: None,
            kv: None,
        });
    }
    let missing = |what: &str| {
        anyhow!(
            "the pipeline server's /v1/status carries no {what} (a fork before ea6f8dee1?): {body}"
        )
    };
    let slots = status.slots.ok_or_else(|| missing("slots"))?;
    let slots_in_use = status.slots_in_use.ok_or_else(|| missing("slots_in_use"))?;
    let sequences = status
        .sequences_in_flight
        .ok_or_else(|| missing("sequences_in_flight"))?;
    let budgets = status
        .kv_budget_bytes
        .ok_or_else(|| missing("kv_budget_bytes"))?;
    let reserved = status
        .kv_reserved_bytes
        .ok_or_else(|| missing("kv_reserved_bytes"))?;
    anyhow::ensure!(
        budgets.len() == ranks && (reserved.is_empty() || reserved.len() == ranks),
        "/v1/status lists {} budgets and {} reservations for {ranks} ranks: {body}",
        budgets.len(),
        reserved.len()
    );
    let kv = budgets
        .iter()
        .enumerate()
        .map(|(rank, budget)| (reserved.get(rank).copied().unwrap_or(0), *budget))
        .collect();
    Ok(ServerLoad {
        waiting: status.num_waiting,
        slots: Some(slots),
        slots_in_use: Some(slots_in_use),
        sequences_in_flight: Some(sequences),
        kv: Some(kv),
    })
}

/// Publish a status poll's outcome; a failed poll clears every figure and names why.
fn publish_server_load(status: &mut DistributedStatus, load: Result<ServerLoad>) {
    let load = match load {
        Ok(load) => {
            status.server_status_error = None;
            Some(load)
        }
        Err(e) => {
            status.server_status_error = Some(format!("{e:#}"));
            None
        }
    };
    status.waiting = load.as_ref().map(|l| l.waiting);
    status.slots = load.as_ref().and_then(|l| l.slots);
    status.slots_in_use = load.as_ref().and_then(|l| l.slots_in_use);
    status.sequences_in_flight = load.as_ref().and_then(|l| l.sequences_in_flight);
    let kv = load.and_then(|l| l.kv);
    for (rank, node) in status.nodes.iter_mut().enumerate() {
        let pair = kv.as_ref().and_then(|kv| kv.get(rank)).copied();
        node.kv_reserved_bytes = pair.map(|(reserved, _)| reserved);
        node.kv_budget_bytes = pair.map(|(_, budget)| budget);
    }
}

fn clear_server_load(status: &mut DistributedStatus) {
    status.waiting = None;
    status.slots = None;
    status.slots_in_use = None;
    status.sequences_in_flight = None;
    status.server_status_error = None;
    for node in &mut status.nodes {
        node.kv_reserved_bytes = None;
        node.kv_budget_bytes = None;
    }
}

async fn set_admission(ctx: &RunContext, open: bool, reason: &str) -> Result<()> {
    let resp = ctx
        .http
        .post(format!("{}/goose/admission", ctx.config.base_url()))
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(serde_json::to_vec(
            &serde_json::json!({"open": open, "reason": reason}),
        )?)
        .send()
        .await?;
    anyhow::ensure!(
        resp.status().is_success(),
        "admission answered {}",
        resp.status()
    );
    Ok(())
}

/// A Link control event, named with what the DATA plane shows at the same poll: rank 0's step
/// counter and the ranks' CPU are the supervisor's own measure, read over loopback, not the mesh.
fn link_control_event(
    ctx: &RunContext,
    rank: &RankProcess,
    event: ControlEvent,
    reading: &MeterReading,
    progress: &Option<Progress>,
) -> (EventKind, String) {
    let data_plane = format!(
        "{} over {}",
        ctx.config.backend.as_str(),
        ctx.config.nodes[rank.rank].tb_interface
    );
    match event {
        ControlEvent::Lost(error) => {
            let moving = match progress {
                Some(p) if reading.progressed => format!(
                    "the data plane ({data_plane}) is still carrying the run: rank 0's step counter \
                     is at {} and advanced this poll",
                    p.steps
                ),
                Some(p) => format!(
                    "the data plane ({data_plane}) shows no progress this poll (rank 0's step \
                     counter at {}, {} in flight) — idle, or stalled; the hang rule decides",
                    p.steps, p.inflight
                ),
                None => format!(
                    "the data plane ({data_plane}) is unverified: rank 0's progress counter did \
                     not answer either"
                ),
            };
            (
                EventKind::LinkControlLost,
                format!(
                    "the LeanZero Link control session to rank {} ({}) stopped answering: {error}. \
                     {moving}. The run is not stopped for it; the peer stops its rank if no poll \
                     reaches it for {} s (its lease)",
                    rank.rank,
                    rank.node,
                    link_control::LINK_LEASE.as_secs()
                ),
            )
        }
        ControlEvent::Restored(after) => (
            EventKind::LinkControlRestored,
            format!(
                "the LeanZero Link control session to rank {} ({}) answers again after {:.1} s",
                rank.rank,
                rank.node,
                after.as_secs_f64()
            ),
        ),
    }
}

pub(crate) fn sample_script(pid: Option<u32>) -> String {
    let mut script = String::from(
        "echo; echo @@vm; /usr/bin/vm_stat\necho; echo @@sysctl; /usr/sbin/sysctl -n hw.memsize kern.memorystatus_vm_pressure_level\n",
    );
    if let Some(pid) = pid {
        script.push_str(&format!(
            "echo; echo @@ps; /bin/ps -o pid=,stat=,time= -p {pid}\n"
        ));
    }
    script.push_str("echo; echo @@end\n");
    script
}

/// One node's `NodeOp::Sample` answer: its memory, the kernel's pressure level, and the rank's
/// ps row when a pid was asked for.
fn parse_sample(
    out: super::ExecOutput,
) -> Result<(crate::MemoryReading, Pressure, Option<probe::ProcSample>)> {
    anyhow::ensure!(!out.ssh_failed(), "ssh failed: {}", out.stderr.trim());
    let sections = probe::sections(&out.stdout);
    let values = probe::parse_sysctl_values(probe::section(&sections, "sysctl")?, 2)?;
    let reading = probe::parse_vm_stat(probe::section(&sections, "vm")?, values[0])?;
    let pressure = Pressure::from_level(values[1])?;
    let row = match sections.get("ps") {
        Some(text) => probe::parse_ps_row(text)?,
        None => None,
    };
    Ok((reading, pressure, row))
}

/// The busy stretch's own growth measured against what is left on the node: `consumed` is what
/// the node's available memory has lost since the engine was last idle, `left` what remains above
/// the CRITICAL reserve. A stretch that has already consumed at least what is left would reach
/// CRITICAL if it ran as long again — the floor alone reads that too late (E2E #2: the Studio went
/// from 49% to 14% free in 2.5 minutes, and WARN at 5% fired 2 s before the rank died). Bytes
/// against bytes over the engine's own busy/idle boundary; no clock decides it.
fn growth_projection(
    idle_available: u64,
    available: u64,
    total: u64,
    critical_ratio: f64,
) -> Option<(u64, u64)> {
    let consumed = idle_available.saturating_sub(available);
    let left = available.saturating_sub((critical_ratio * total as f64) as u64);
    (consumed > 0 && consumed >= left).then_some((consumed, left))
}

async fn monitor(
    ctx: &RunContext,
    ranks: &mut [RankProcess],
    stop_rx: &mut watch::Receiver<bool>,
    served: &mut bool,
) -> RunOutcome {
    let mut meter = ProgressMeter::new(Instant::now(), ranks.len());
    let mut admission_closed = false;
    let mut blind_reported = vec![false; ranks.len()];
    // Per node: its available memory the last time the engine was idle (the busy stretch's start).
    let mut idle_available: Vec<Option<u64>> = vec![None; ranks.len()];
    let mut over_plan = vec![false; ranks.len()];
    // Why admission is held for memory, while it is: a SIGKILL then is the kernel's.
    let mut memory_short: Option<String> = None;
    let mut was_busy = false;
    loop {
        tokio::select! {
            _ = stop_rx.changed() => return RunOutcome::Stop,
            _ = tokio::time::sleep(POLL_INTERVAL) => {}
        }
        for rank in ranks.iter_mut() {
            if let Ok(Some(status)) = rank.child.try_wait() {
                let (kind, message) = rank_exit(
                    rank,
                    status,
                    "",
                    "; the pair cannot serve.",
                    memory_short.as_deref(),
                );
                return RunOutcome::Failed(kind, Some(rank.node.clone()), message);
            }
        }
        let pids: Vec<Option<u32>> = ranks.iter().map(RankProcess::pid).collect();
        let samples =
            futures::future::join_all(ctx.config.nodes.iter().zip(&pids).map(|(node, pid)| {
                let exec = Arc::clone(&ctx.exec);
                let host = node.ssh.clone();
                let op = NodeOp::Sample { pid: *pid };
                async move { exec.run_op(host.as_deref(), &op).await }
            }))
            .await;
        let progress = async {
            let resp = ctx
                .http
                .get(format!("{}/goose/progress", ctx.config.base_url()))
                .send()
                .await?;
            anyhow::ensure!(resp.status().is_success(), "HTTP {}", resp.status());
            Ok::<Progress, anyhow::Error>(serde_json::from_str(&resp.text().await?)?)
        }
        .await
        .ok();
        let load = async {
            let resp = ctx
                .http
                .get(format!("{}/v1/status", ctx.config.base_url()))
                .send()
                .await?;
            anyhow::ensure!(
                resp.status().is_success(),
                "/v1/status answered HTTP {}",
                resp.status()
            );
            server_load(&resp.text().await?, ctx.runner, ranks.len())
        }
        .await;

        let mut cpu: Vec<Option<u64>> = vec![None; ranks.len()];
        let mut stats: Vec<Option<String>> = vec![None; ranks.len()];
        let mut worst = (Watchdog::Normal, EventKind::WatchdogWarn, String::new());
        let busy_now = progress.as_ref().map(|p| p.inflight > 0);
        let (warn_ratio, critical_ratio) = ctx.config.watchdog_ratios();
        for (rank, sample) in samples.into_iter().enumerate() {
            let node_name = ctx.config.nodes[rank].name.clone();
            let parsed = sample.and_then(parse_sample);
            let (reading, pressure, row) = match parsed {
                Ok(parsed) => parsed,
                Err(e) => {
                    let message = format!("{e:#}");
                    if !blind_reported[rank] {
                        blind_reported[rank] = true;
                        ctx.event(
                            EventKind::WatchdogBlind,
                            Some(&node_name),
                            format!("cannot sample memory/process on this node: {message}"),
                        );
                    }
                    ctx.update(|s| s.status.nodes[rank].memory_error = Some(message));
                    continue;
                }
            };
            blind_reported[rank] = false;
            if pids[rank].is_some() {
                match &row {
                    None => {
                        return RunOutcome::Failed(
                            EventKind::RankDied,
                            Some(node_name),
                            format!(
                                "rank {rank} pid {} is gone (ps shows no such process)",
                                pids[rank].unwrap_or_default()
                            ),
                        )
                    }
                    Some(row) if row.stopped() && !ctx.config.hang_ratio_only => {
                        return RunOutcome::Failed(
                            EventKind::RankFrozen,
                            Some(node_name),
                            format!(
                            "rank {rank} pid {} is STOPPED (ps stat {}): a frozen rank hangs the \
                                 pair silently and forever (STEP1b)",
                            row.pid, row.stat
                        ),
                        )
                    }
                    Some(row) => {
                        cpu[rank] = Some(row.cpu_centis);
                        stats[rank] = Some(row.stat.clone());
                    }
                }
            }
            let verdict = watchdog_verdict(
                pressure,
                reading.available_bytes,
                reading.total_bytes,
                (warn_ratio, critical_ratio),
            );
            if busy_now == Some(false) || idle_available[rank].is_none() {
                idle_available[rank] = Some(reading.available_bytes);
            }
            let growth = match (busy_now, idle_available[rank]) {
                (Some(true), Some(idle)) => growth_projection(
                    idle,
                    reading.available_bytes,
                    reading.total_bytes,
                    critical_ratio,
                ),
                _ => None,
            };
            if let (Some((consumed, left)), Watchdog::Normal) = (growth, worst.0) {
                worst = (
                    Watchdog::Warn,
                    EventKind::MemoryGrowth,
                    format!(
                        "{node_name}: this busy stretch has taken {} of available memory since \
                         the engine was last idle and {} is left above the CRITICAL reserve \
                         ({critical_ratio:.3} × RAM) — one more stretch like it would reach \
                         CRITICAL (available {} of {})",
                        gib(consumed),
                        gib(left),
                        gib(reading.available_bytes),
                        gib(reading.total_bytes),
                    ),
                );
            }
            if verdict != Watchdog::Normal && verdict as u8 >= worst.0 as u8 {
                worst = (
                    verdict,
                    match verdict {
                        Watchdog::Critical => EventKind::WatchdogCritical,
                        _ => EventKind::WatchdogWarn,
                    },
                    format!(
                        "{node_name}: kernel pressure {}, available {} of {} (WARN below {} = \
                         {warn_ratio:.3} × RAM, CRITICAL below {} = {critical_ratio:.3} × RAM)",
                        pressure.as_str(),
                        gib(reading.available_bytes),
                        gib(reading.total_bytes),
                        gib((warn_ratio * reading.total_bytes as f64) as u64),
                        gib((critical_ratio * reading.total_bytes as f64) as u64),
                    ),
                );
            }
            let (live, caps) = {
                let live = ranks[rank].live.lock().unwrap();
                (live.memory, live.caps.clone())
            };
            let cap = |key: &str| {
                caps.as_ref()
                    .and_then(|c| c.get(key))
                    .and_then(|v| v.as_u64())
            };
            let (memory_limit, wired_limit, cache_limit) =
                (cap("memory_limit"), cap("wired_limit"), cap("cache_limit"));
            let was_over = over_plan[rank];
            let mut over = was_over;
            ctx.update(|s| {
                let node = &mut s.status.nodes[rank];
                node.memory_limit_bytes = memory_limit;
                node.wired_limit_bytes = wired_limit;
                node.cache_limit_bytes = cache_limit;
                node.available_bytes = Some(reading.available_bytes);
                node.total_bytes = Some(reading.total_bytes);
                node.pressure = Some(pressure.as_str().to_string());
                node.memory_error = None;
                if let Some(live) = live {
                    node.active_bytes = Some(live.active);
                    node.cache_bytes = Some(live.cache);
                    node.peak_bytes = Some(live.peak.max(node.peak_bytes.unwrap_or(0)));
                    if let Some(planned) = node.planned_bytes {
                        over = live.active > planned;
                        if over && !was_over {
                            let message = format!(
                                "MLX active {} exceeds the plan's {} (with overhead); free-buffer \
                                 cache {} beside it",
                                gib(live.active),
                                gib(planned),
                                gib(live.cache)
                            );
                            s.event(EventKind::RankOverPlan, Some(&node_name), message);
                        }
                    }
                }
            });
            over_plan[rank] = over;
        }
        if let Some(busy) = busy_now {
            if was_busy && !busy {
                *served = true;
            }
            was_busy = busy;
        }

        let now = Instant::now();
        let mut reading = meter.observe(now, progress.as_ref().map(|p| p.steps), &cpu);
        let busy = progress.as_ref().is_none_or(|p| p.inflight > 0);
        judge_silence(&mut meter, now, busy, &mut reading);
        for rank in ranks.iter() {
            for event in link_control::drain_events(rank) {
                let (kind, message) = link_control_event(ctx, rank, event, &reading, &progress);
                ctx.event(kind, Some(&rank.node), message);
            }
        }
        let serving = progress.as_ref().is_some_and(|p| p.inflight > 0);
        ctx.update(|s| {
            s.status.inflight = progress.as_ref().map(|p| p.inflight);
            publish_server_load(&mut s.status, load);
            s.status.liveness = Some(Liveness {
                samples: meter.intervals.len(),
                median_ms: reading.median.map(|m| m.as_millis() as u64),
                bound_ms: reading.bound.map(|b| b.as_millis() as u64),
                silent_ms: reading.silent_for.as_millis() as u64,
            });
            s.status.state = if serving {
                RunState::Serving
            } else {
                RunState::Ready
            };
            let node_state = if serving {
                NodeState::Serving
            } else {
                NodeState::Ready
            };
            s.status.nodes.iter_mut().for_each(|n| n.state = node_state);
        });
        if reading.hang {
            return RunOutcome::Failed(
                EventKind::Hang,
                None,
                format!(
                    "progress-ratio rule: samples {}, median {} ms, bound {} ms ({HANG_MEDIAN_MULTIPLE}× \
                     median), silent {} ms — the rank-0 step counter (last {:?}) and every rank's CPU \
                     time stood still; rank ps stats {:?}",
                    meter.intervals.len(),
                    reading.median.unwrap_or_default().as_millis(),
                    reading.bound.unwrap_or_default().as_millis(),
                    reading.silent_for.as_millis(),
                    progress.as_ref().map(|p| p.steps),
                    stats,
                ),
            );
        }
        match worst.0 {
            Watchdog::Critical => {
                ctx.event(EventKind::WatchdogCritical, None, worst.2.clone());
                return RunOutcome::Critical(worst.2);
            }
            Watchdog::Warn if !admission_closed => {
                ctx.event(worst.1, None, worst.2.clone());
                match set_admission(ctx, false, &worst.2).await {
                    Ok(()) => {
                        admission_closed = true;
                        memory_short = Some(worst.2.clone());
                        ctx.update(|s| s.status.admission_open = false);
                        ctx.event(
                            EventKind::AdmissionClosed,
                            None,
                            "new requests are answered 503 until memory recovers",
                        );
                    }
                    Err(e) => ctx.event(
                        EventKind::WatchdogWarn,
                        None,
                        format!("could not close admission: {e:#}"),
                    ),
                }
            }
            Watchdog::Normal if admission_closed => match set_admission(ctx, true, "").await {
                Ok(()) => {
                    admission_closed = false;
                    memory_short = None;
                    ctx.update(|s| s.status.admission_open = true);
                    ctx.event(
                        EventKind::AdmissionOpened,
                        None,
                        "memory recovered on every node",
                    );
                }
                Err(e) => ctx.event(
                    EventKind::WatchdogWarn,
                    None,
                    format!("could not reopen admission: {e:#}"),
                ),
            },
            _ => {}
        }
    }
}

/// After a memory death, with the ranks stopped: sample every node on the poll cadence until each
/// is back above its WARN reserve with the kernel NORMAL, publishing what it waits for. Nothing
/// bounds the wait but the owner's Stop — memory that never comes back is said, not guessed
/// around. `None` = a stop was requested.
async fn wait_memory_recovered(
    ctx: &RunContext,
    stop_rx: &mut watch::Receiver<bool>,
) -> Option<String> {
    let (warn_ratio, critical_ratio) = ctx.config.watchdog_ratios();
    loop {
        let samples = futures::future::join_all(ctx.config.nodes.iter().map(|node| {
            let exec = Arc::clone(&ctx.exec);
            let host = node.ssh.clone();
            async move {
                exec.run_op(host.as_deref(), &NodeOp::Sample { pid: None })
                    .await
            }
        }))
        .await;
        let mut short = Vec::new();
        let mut recovered = Vec::new();
        for (node, sample) in ctx.config.nodes.iter().zip(samples) {
            match sample.and_then(parse_sample) {
                Ok((reading, pressure, _)) => {
                    let line = format!(
                        "{}: kernel {}, available {} of {} (WARN below {})",
                        node.name,
                        pressure.as_str(),
                        gib(reading.available_bytes),
                        gib(reading.total_bytes),
                        gib((warn_ratio * reading.total_bytes as f64) as u64),
                    );
                    let verdict = watchdog_verdict(
                        pressure,
                        reading.available_bytes,
                        reading.total_bytes,
                        (warn_ratio, critical_ratio),
                    );
                    if verdict == Watchdog::Normal {
                        recovered.push(line);
                    } else {
                        short.push(line);
                    }
                }
                Err(e) => short.push(format!("{}: cannot sample: {e:#}", node.name)),
            }
        }
        if short.is_empty() {
            ctx.update(|s| s.status.memory_recovery = None);
            return Some(recovered.join("; "));
        }
        ctx.update(|s| {
            s.status.memory_recovery = Some(format!(
                "waiting for memory to recover before restarting: {}",
                short.join("; ")
            ))
        });
        tokio::select! {
            _ = stop_rx.changed() => {
                ctx.update(|s| s.status.memory_recovery = None);
                return None;
            }
            _ = tokio::time::sleep(POLL_INTERVAL) => {}
        }
    }
}

/// The rank specs for the plan preflight approved: the tensor runner's per-rank bytes, or the
/// pipeline runner's pinned split. A preflight that carries neither cannot be launched.
fn launch_specs(
    ctx: &RunContext,
    preflight: &PreflightReport,
    context: u64,
) -> Result<Vec<launch::RankSpec>> {
    let report_seconds = POLL_INTERVAL.as_secs_f64();
    let mut specs = match ctx.runner {
        Runner::MlxLmTensor => {
            let launches = preflight.tensor_launches()?;
            launch::rank_specs(
                &ctx.config,
                &ctx.served_id,
                &launches,
                context,
                report_seconds,
            )
        }
        Runner::PipelineQwen4 => {
            let starts = preflight
                .pipeline_starts
                .as_deref()
                .ok_or_else(|| anyhow!("the preflight approved no pipeline split"))?;
            launch::pipeline_rank_specs(
                &ctx.config,
                &ctx.served_id,
                context,
                &super::plan::split_arg(starts),
                report_seconds,
            )
        }
    };
    for (spec, node) in specs.iter_mut().zip(&preflight.nodes) {
        spec.planned_weight_bytes = node.plan.as_ref().map(|p| p.weights_bytes);
        spec.owner = ctx.owner.clone();
    }
    Ok(specs)
}

async fn supervise(
    ctx: RunContext,
    mut preflight: PreflightReport,
    mut stop_rx: watch::Receiver<bool>,
) -> StopReport {
    let parity = sidecar_parity();
    let mut restarts: VecDeque<Instant> = VecDeque::new();
    let mut backoff = parity.backoff_initial;
    let peers_follow_rank0 = ctx.runner == Runner::PipelineQwen4;
    // One restart per memory outage: spent by a memory death's restart, re-armed once the
    // restarted pair has served a request to its end.
    let mut memory_restart_armed = true;
    loop {
        let context = preflight.context_limit.unwrap_or_default();
        let specs = match launch_specs(&ctx, &preflight, context) {
            Ok(specs) => specs,
            Err(e) => {
                ctx.update(|s| {
                    s.status.state = RunState::Failed;
                    s.status.last_error = Some(format!("{e:#}"));
                });
                return StopReport::default();
            }
        };
        ctx.update(|s| {
            s.status.state = RunState::Starting;
            s.status.admission_open = true;
            s.status.inflight = None;
            clear_server_load(&mut s.status);
            s.status.liveness = None;
            s.status.context_limit = Some(context);
            for (node, plan) in s.status.nodes.iter_mut().zip(&preflight.nodes) {
                node.state = NodeState::Loading;
                node.pid = None;
                node.load_phase = None;
                node.active_bytes = None;
                node.planned_bytes = plan.plan.as_ref().map(|p| p.with_overhead_bytes);
                node.planned_weight_bytes = plan.plan.as_ref().map(|p| p.weights_bytes);
            }
        });
        let mut ranks = Vec::new();
        let mut spawn_error = None;
        let link_launch = link_control::LinkLaunch {
            run_id: format!("{}-{}", std::process::id(), now_ms()),
            model_id: ctx.config.model_id.clone(),
            served_model_id: ctx.served_id.clone(),
            runner: ctx.runner,
        };
        for (node, spec) in ctx.config.nodes.iter().zip(&specs) {
            let spawned = match link_control::link_peer(node.host()) {
                Some(_) => link_control::spawn_link_rank(node, spec, &link_launch).await,
                None => launch::spawn_rank(node, spec),
            };
            match spawned {
                Ok(rank) => ranks.push(rank),
                Err(e) => {
                    spawn_error = Some((node.name.clone(), format!("{e:#}")));
                    break;
                }
            }
        }
        let outcome = match spawn_error {
            Some((node, error)) => Err((EventKind::StartFailed, Some(node), error)),
            None => {
                ctx.event(
                    EventKind::Launched,
                    None,
                    format!(
                        "{} ranks over {} ({} context)",
                        ranks.len(),
                        ctx.config.backend.as_str(),
                        context
                    ),
                );
                match wait_ready(&ctx, &mut ranks, &mut stop_rx).await {
                    ReadyOutcome::Stop => Ok(Finish::Stop),
                    ReadyOutcome::Failed(kind, node, message) => Err((kind, node, message)),
                    ReadyOutcome::Ready => {
                        backoff = parity.backoff_initial;
                        ctx.update(|s| s.status.state = RunState::Ready);
                        ctx.set_node_states(NodeState::Ready);
                        ctx.event(
                            EventKind::Ready,
                            None,
                            "the readiness completion ended with [DONE]",
                        );
                        let mut served = false;
                        let outcome = monitor(&ctx, &mut ranks, &mut stop_rx, &mut served).await;
                        if served {
                            memory_restart_armed = true;
                        }
                        match outcome {
                            RunOutcome::Failed(kind, node, message) => Err((kind, node, message)),
                            RunOutcome::Stop => Ok(Finish::Stop),
                            RunOutcome::Critical(reason) => Ok(Finish::Critical(reason)),
                        }
                    }
                }
            }
        };
        let port = Some(ctx.config.port);
        match outcome {
            Ok(Finish::Stop) => {
                ctx.update(|s| s.status.state = RunState::Stopping);
                let report =
                    stop_ranks(&mut ranks, ctx.exec.as_ref(), port, peers_follow_rank0).await;
                ctx.update(|s| {
                    s.status.state = RunState::Stopped;
                    s.status.inflight = None;
                    clear_server_load(&mut s.status);
                    s.status
                        .nodes
                        .iter_mut()
                        .for_each(|n| n.state = NodeState::Stopped);
                    s.event(
                        EventKind::Stopped,
                        None,
                        format!(
                            "{}; {}",
                            if report.verified {
                                "verified"
                            } else {
                                "NOT verified"
                            },
                            report.steps.join("; ")
                        ),
                    );
                });
                return report;
            }
            Ok(Finish::Critical(reason)) => {
                ctx.update(|s| s.status.state = RunState::Stopping);
                let report =
                    stop_ranks(&mut ranks, ctx.exec.as_ref(), port, peers_follow_rank0).await;
                ctx.update(|s| {
                    s.status.state = RunState::Stopped;
                    s.status.inflight = None;
                    clear_server_load(&mut s.status);
                    s.status.last_error = Some(format!(
                        "stopped by the memory watchdog (CRITICAL, never restarted): {reason}"
                    ));
                    s.status
                        .nodes
                        .iter_mut()
                        .for_each(|n| n.state = NodeState::Stopped);
                    s.event(EventKind::Stopped, None, report.steps.join("; "));
                });
                return report;
            }
            Err((kind, node, message)) => {
                ctx.update(|s| {
                    s.event(kind, node.as_deref(), message.clone());
                    if let Some(cut) = s.status.inflight.filter(|n| *n > 0) {
                        s.event(
                            EventKind::StreamWithoutDone,
                            node.as_deref(),
                            format!(
                                "{cut} in-flight request(s) cut by the {}: their streams end without \
                                 `data: [DONE]` (an HTTP 200 already sent is not a completion)",
                                kind.as_str()
                            ),
                        );
                    }
                    s.status.last_error = Some(message.clone());
                    s.status.state = RunState::Stopping;
                    clear_server_load(&mut s.status);
                    for n in &mut s.status.nodes {
                        n.state = if Some(&n.name) == node.as_ref() {
                            NodeState::Failed
                        } else {
                            NodeState::Stopped
                        };
                    }
                });
                let report =
                    stop_ranks(&mut ranks, ctx.exec.as_ref(), port, peers_follow_rank0).await;
                ctx.event(
                    EventKind::Stopped,
                    None,
                    format!(
                        "after {}: {}; {}",
                        kind.as_str(),
                        if report.verified {
                            "verified"
                        } else {
                            "NOT verified"
                        },
                        report.steps.join("; ")
                    ),
                );
                let memory = kind == EventKind::RankOutOfMemory;
                // A refused Local Network privilege is the owner's click, not a transient: a
                // restart would only fail the same way. A memory death restarts on its own
                // policy (once per outage, after memory recovered) whatever `restart_on_failure`
                // says: the engine the owner started stays up through a memory spike.
                if !memory
                    && (!ctx.config.restart_on_failure || kind == EventKind::LocalNetworkBlocked)
                {
                    ctx.update(|s| s.status.state = RunState::Failed);
                    return report;
                }
                if memory && !memory_restart_armed {
                    ctx.update(|s| {
                        s.status.state = RunState::Failed;
                        s.event(
                            EventKind::BreakerOpen,
                            None,
                            format!(
                                "a rank died of memory again before the pair restarted after the \
                                 last memory death had served a request; not restarting. Last \
                                 failure: {message}"
                            ),
                        );
                    });
                    return report;
                }
                if !breaker_allows(
                    &mut restarts,
                    Instant::now(),
                    parity.restart_window,
                    parity.max_restarts_in_window,
                ) {
                    ctx.update(|s| {
                        s.status.state = RunState::Failed;
                        s.event(
                            EventKind::BreakerOpen,
                            None,
                            format!(
                                "{} restarts within {:?}; not restarting. Last failure: {message}",
                                restarts.len(),
                                parity.restart_window
                            ),
                        );
                    });
                    return report;
                }
                if memory {
                    memory_restart_armed = false;
                    ctx.update(|s| s.status.state = RunState::Recovering);
                    let Some(recovered) = wait_memory_recovered(&ctx, &mut stop_rx).await else {
                        ctx.update(|s| s.status.state = RunState::Stopped);
                        return report;
                    };
                    ctx.update(|s| {
                        s.status.restarts += 1;
                        s.event(EventKind::MemoryRecovered, None, recovered);
                        s.event(
                            EventKind::Restart,
                            None,
                            "restarting once after rankOutOfMemory: memory recovered on every node",
                        );
                    });
                } else {
                    ctx.update(|s| {
                        s.status.restarts += 1;
                        s.event(
                            EventKind::Restart,
                            None,
                            format!("restarting after {} (backoff {backoff:?})", kind.as_str()),
                        );
                    });
                    tokio::select! {
                        _ = stop_rx.changed() => {
                            ctx.update(|s| s.status.state = RunState::Stopped);
                            return report;
                        }
                        _ = tokio::time::sleep(backoff) => {}
                    }
                    backoff = (backoff * 2).min(parity.backoff_cap);
                }
                ctx.update(|s| s.status.state = RunState::Preflight);
                let Some(restart_preflight) =
                    preflight_past_own_leftovers(&ctx, &mut stop_rx).await
                else {
                    ctx.update(|s| s.status.state = RunState::Stopped);
                    return report;
                };
                match restart_preflight {
                    Ok(report) if report.ok => {
                        for repair in &report.repairs {
                            ctx.event(EventKind::LinkRepaired, None, repair.clone());
                        }
                        ctx.update(|s| s.status.last_preflight = Some(report.clone()));
                        preflight = report;
                    }
                    Ok(report) => {
                        let failures = report.failures().join("; ");
                        ctx.update(|s| {
                            s.status.last_preflight = Some(report);
                            s.status.state = RunState::Failed;
                            s.status.last_error =
                                Some(format!("restart refused by preflight: {failures}"));
                        });
                        return StopReport::default();
                    }
                    Err(e) => {
                        ctx.update(|s| {
                            s.status.state = RunState::Failed;
                            s.status.last_error = Some(format!("restart preflight: {e:#}"));
                        });
                        return StopReport::default();
                    }
                }
            }
        }
    }
}

/// The ranks a failed preflight found SHORT (their plan exceeds their budget) whose owner left
/// "Free memory automatically" on. None when the preflight passed, and none while this install's
/// previous ranks still run anywhere: their exit (or reclaim) frees what they hold, and
/// compaction beside a loaded model is refused anyway.
fn nodes_to_compact(config: &DistributedConfig, report: &PreflightReport) -> Vec<usize> {
    if report.ok || report.nodes.iter().any(|n| !n.leftovers.is_empty()) {
        return Vec::new();
    }
    report
        .nodes
        .iter()
        .filter(|n| n.short_bytes.is_some())
        .map(|n| n.rank)
        .filter(|rank| {
            config
                .nodes
                .get(*rank)
                .is_some_and(|n| n.free_memory_automatically)
        })
        .collect()
}

/// Preflight; when a node is SHORT (its rank's plan exceeds its budget) and its owner left "Free
/// memory automatically" on, compact the short nodes (concurrently — they are different Macs) and
/// preflight once more. The second preflight's verdict is the answer; a compaction that did not
/// run is recorded with its reason and leaves the first verdict standing.
async fn preflight_making_room(
    config: &DistributedConfig,
    exec: &Arc<dyn NodeExec>,
    shared: &Arc<StdMutex<Shared>>,
    repair_link: bool,
    owner: Option<&str>,
) -> Result<PreflightReport> {
    let first = preflight::run_preflight(config, Arc::clone(exec), repair_link, owner).await?;
    let short: Vec<_> = nodes_to_compact(config, &first)
        .into_iter()
        .map(|rank| &config.nodes[rank])
        .collect();
    if short.is_empty() {
        return Ok(first);
    }
    shared.lock().unwrap().status.making_room = short.iter().map(|n| n.name.clone()).collect();
    let records = futures::future::join_all(short.iter().map(|node| {
        compaction::compact_recorded(exec.as_ref(), node.host(), &node.name, "automatic")
    }))
    .await;
    shared.lock().unwrap().status.making_room.clear();
    let ran = records.iter().any(|r| r.report.is_some());
    {
        let mut shared = shared.lock().unwrap();
        for record in records {
            shared.record_compaction(record);
        }
    }
    if !ran {
        return Ok(first);
    }
    let mut second = preflight::run_preflight(config, Arc::clone(exec), repair_link, owner).await?;
    let mut repairs = first.repairs;
    repairs.append(&mut second.repairs);
    second.repairs = repairs;
    Ok(second)
}

/// What a preflight's `leftovers` (this install's previous ranks, proven by the owner token)
/// came to.
#[derive(Debug, Default)]
struct SettledLeftovers {
    /// Orphans (their goose is gone): SIGTERM, grace, SIGKILL to each pid alone, observed gone.
    reclaimed: Vec<String>,
    /// (the Mac, what runs there): under a live parent — the goose stopping it, a peer's goose
    /// whose lease ends it, an ssh session the peer's rank dies with — or a parent the node could
    /// not name, or an orphan whose reclaim did not verify. Nothing more is signalled.
    waiting: Vec<(String, String)>,
}

impl SettledLeftovers {
    fn clear(&self) -> bool {
        self.reclaimed.is_empty() && self.waiting.is_empty()
    }

    fn waiting_detail(&self) -> String {
        self.waiting
            .iter()
            .map(|(node, what)| format!("{node}: {what}"))
            .collect::<Vec<_>>()
            .join("; ")
    }
}

async fn settle_leftovers(
    exec: &dyn NodeExec,
    config: &DistributedConfig,
    report: &PreflightReport,
) -> SettledLeftovers {
    let mut settled = SettledLeftovers::default();
    for node in &report.nodes {
        let host = config.nodes.get(node.rank).and_then(|n| n.host());
        for leftover in &node.leftovers {
            if !leftover.orphaned() {
                settled
                    .waiting
                    .push((node.name.clone(), leftover.describe()));
                continue;
            }
            let (line, gone) = reclaim_rank_pid(exec, host, &node.name, leftover.pid).await;
            let line = format!("{line} (orphaned by the previous goose)");
            if gone {
                settled.reclaimed.push(line);
            } else {
                settled.waiting.push((node.name.clone(), line));
            }
        }
    }
    settled
}

/// A restart's preflight past this run's own ranks that are still leaving: orphans are reclaimed
/// per pid, and ranks under a live parent are waited for one poll at a time — the wait ends when
/// their pids are gone (progress), never on a clock; `None` = a stop arrived meanwhile.
async fn preflight_past_own_leftovers(
    ctx: &RunContext,
    stop_rx: &mut watch::Receiver<bool>,
) -> Option<Result<PreflightReport>> {
    let mut announced = false;
    loop {
        let report = match preflight_making_room(
            &ctx.config,
            &ctx.exec,
            &ctx.shared,
            true,
            ctx.owner.as_deref(),
        )
        .await
        {
            Ok(report) => report,
            Err(e) => return Some(Err(e)),
        };
        let settled = settle_leftovers(ctx.exec.as_ref(), &ctx.config, &report).await;
        if settled.clear() {
            return Some(Ok(report));
        }
        if !settled.reclaimed.is_empty() {
            ctx.event(
                EventKind::OrphanReclaimed,
                None,
                settled.reclaimed.join("; "),
            );
        }
        if let Some((node, _)) = settled.waiting.first() {
            if !announced {
                announced = true;
                ctx.event(
                    EventKind::PreviousSplitWaiting,
                    Some(node),
                    format!(
                        "the restart waits for the previous ranks to exit: {}",
                        settled.waiting_detail()
                    ),
                );
            }
            tokio::select! {
                _ = stop_rx.changed() => return None,
                _ = tokio::time::sleep(POLL_INTERVAL) => {}
            }
        }
    }
}

pub struct DistributedManager {
    shared: Arc<StdMutex<Shared>>,
    exec: Arc<dyn NodeExec>,
    control: tokio::sync::Mutex<()>,
    /// This install's owner token (`set_owner`): written on every rank it launches, and the ONLY
    /// proof a rank found later is its own leftover. `None` = none given: no rank is provably
    /// this install's, so none is waited for, reclaimed or swept as its own.
    owner: StdMutex<Option<String>>,
}

impl Default for DistributedManager {
    fn default() -> Self {
        Self::new(Arc::new(LinkRoutedExec::new(Arc::new(SystemExec))))
    }
}

impl DistributedManager {
    pub fn new(exec: Arc<dyn NodeExec>) -> Self {
        Self {
            shared: Arc::new(StdMutex::new(Shared {
                status: DistributedStatus::stopped(),
                stop_tx: None,
                task: None,
            })),
            exec,
            control: tokio::sync::Mutex::new(()),
            owner: StdMutex::new(None),
        }
    }

    /// The install's owner token — the caller persists it across launches (goose's state dir),
    /// so a rank a previous goosed of this install left behind carries the same token.
    pub fn set_owner(&self, owner: String) {
        *self.owner.lock().unwrap() = Some(owner);
    }

    fn owner(&self) -> Option<String> {
        self.owner.lock().unwrap().clone()
    }

    pub fn status(&self) -> DistributedStatus {
        self.shared.lock().unwrap().status.clone()
    }

    /// The OpenAI base URL while the distributed engine owns this Mac and serves (or is about to).
    pub fn active_base_url(&self) -> Option<String> {
        let shared = self.shared.lock().unwrap();
        matches!(
            shared.status.state,
            RunState::Starting | RunState::Ready | RunState::Serving
        )
        .then(|| shared.status.base_url.clone())
        .flatten()
    }

    pub fn owns_the_mac(&self) -> bool {
        self.shared.lock().unwrap().status.state.owns_the_mac()
    }

    /// Dry run: every check, no launch. `repair_link` performs the documented TB repair when a
    /// JACCL GID/IPv4 check fails (TB services only).
    pub async fn preflight(
        &self,
        config: &DistributedConfig,
        repair_link: bool,
    ) -> Result<PreflightReport> {
        let owner = self.owner();
        let report = preflight_making_room(
            config,
            &self.exec,
            &self.shared,
            repair_link,
            owner.as_deref(),
        )
        .await?;
        let mut shared = self.shared.lock().unwrap();
        for repair in &report.repairs {
            shared.event(EventKind::LinkRepaired, None, repair.clone());
        }
        shared.status.last_preflight = Some(report.clone());
        Ok(report)
    }

    /// Preflight (repairing the TB link when needed), then launch under supervision. Refused —
    /// with a code the UI acts on — while the single engine is mounted, while a run is live, or
    /// when a preflight check fails. `served_id` is `engine::served_model_id(settings,
    /// config.model_id)` — the SPLIT's model, never the single engine's — the caller reads them.
    pub async fn start(
        &self,
        config: DistributedConfig,
        served_id: String,
    ) -> Result<StartOutcome> {
        let single = crate::engine::global_manager().status().await;
        self.start_with_single_state(config, served_id, &single.state, single.model_id.as_deref())
            .await
    }

    pub(crate) async fn start_with_single_state(
        &self,
        config: DistributedConfig,
        served_id: String,
        single_state: &str,
        single_model: Option<&str>,
    ) -> Result<StartOutcome> {
        let _control = self.control.lock().await;
        config.validate()?;
        {
            let shared = self.shared.lock().unwrap();
            if shared.status.state.owns_the_mac() {
                return Ok(StartOutcome::Refused {
                    code: RefusalCode::AlreadyRunning,
                    message: format!(
                        "the distributed engine is {} — stop it first",
                        shared.status.state.as_str()
                    ),
                    preflight: None,
                    node: None,
                    detail: None,
                });
            }
        }
        if matches!(single_state, "running" | "mounting") {
            return Ok(StartOutcome::Refused {
                code: RefusalCode::SingleEngineMounted,
                message: format!(
                    "the single MLX engine is {single_state} with '{}' on this Mac; one engine owns a \
                     Mac at a time — unmount it, then start the distributed engine",
                    single_model.unwrap_or("<model not reported>")
                ),
                preflight: None,
                node: None,
                detail: None,
            });
        }
        {
            let mut shared = self.shared.lock().unwrap();
            let events = std::mem::take(&mut shared.status.events);
            shared.status = DistributedStatus::stopped();
            shared.status.events = events;
            shared.status.state = RunState::Preflight;
            shared.status.config = Some(config.clone());
            shared.status.backend = Some(config.backend);
            shared.status.model_id = Some(config.model_id.clone());
            shared.status.served_model_id = Some(served_id.clone());
            shared.event(EventKind::Preflight, None, "preflight before launch");
        }
        let owner = self.owner();
        // This install's previous ranks first: an orphan is reclaimed per pid and the preflight
        // runs again over the freed node; one still under a live parent is a refusal the caller
        // retries — a start never signals a rank some goose is still stopping.
        let (report, waiting) = loop {
            let report = match preflight_making_room(
                &config,
                &self.exec,
                &self.shared,
                true,
                owner.as_deref(),
            )
            .await
            {
                Ok(report) => report,
                Err(e) => {
                    let mut shared = self.shared.lock().unwrap();
                    shared.status.state = RunState::Stopped;
                    shared.status.last_error = Some(format!("{e:#}"));
                    return Err(e);
                }
            };
            let settled = settle_leftovers(self.exec.as_ref(), &config, &report).await;
            if !settled.reclaimed.is_empty() {
                self.shared.lock().unwrap().event(
                    EventKind::OrphanReclaimed,
                    None,
                    settled.reclaimed.join("; "),
                );
            }
            if !settled.waiting.is_empty() {
                break (report, Some(settled));
            }
            if settled.clear() {
                break (report, None);
            }
        };
        let mut shared = self.shared.lock().unwrap();
        for repair in &report.repairs {
            shared.event(EventKind::LinkRepaired, None, repair.clone());
        }
        shared.status.last_preflight = Some(report.clone());
        shared.status.runner = report.runner;
        if let Some(settled) = waiting {
            let node = settled.waiting[0].0.clone();
            let detail = settled.waiting_detail();
            shared.status.state = RunState::Stopped;
            shared.status.last_error = Some(format!(
                "the previous split is still shutting down: {detail}"
            ));
            shared.event(EventKind::PreviousSplitWaiting, Some(&node), detail.clone());
            return Ok(StartOutcome::Refused {
                code: RefusalCode::PreviousSplitShuttingDown,
                message: format!(
                    "The previous split is still shutting down on {node} — start it again when \
                     that finishes"
                ),
                preflight: Some(report),
                node: Some(node),
                detail: Some(detail),
            });
        }
        if !report.ok {
            let failures = report.failures().join("; ");
            shared.status.state = RunState::Stopped;
            shared.status.last_error = Some(format!("preflight refused the start: {failures}"));
            shared.event(
                EventKind::StartFailed,
                None,
                format!("preflight: {failures}"),
            );
            let foreign = report.nodes.iter().find(|n| !n.foreign_splits.is_empty());
            return Ok(match foreign {
                Some(node) => StartOutcome::Refused {
                    code: RefusalCode::ForeignSplit,
                    message: format!(
                        "Another MLX split (not goose's) is running on {} — stop it to start this \
                         one",
                        node.name
                    ),
                    node: Some(node.name.clone()),
                    detail: Some(node.foreign_splits.join("; ")),
                    preflight: Some(report),
                },
                None => StartOutcome::Refused {
                    code: RefusalCode::PreflightFailed,
                    message: failures,
                    preflight: Some(report),
                    node: None,
                    detail: None,
                },
            });
        }
        let Some(runner) = report.runner else {
            shared.status.state = RunState::Stopped;
            shared.status.last_error = Some("the preflight named no runner".to_string());
            return Ok(StartOutcome::Refused {
                code: RefusalCode::PreflightFailed,
                message: "the preflight named no runner".to_string(),
                preflight: Some(report),
                node: None,
                detail: None,
            });
        };
        shared.status.base_url = Some(config.base_url());
        shared.status.context_limit = report.context_limit;
        shared.status.state = RunState::Starting;
        shared.status.nodes = config
            .nodes
            .iter()
            .zip(&report.nodes)
            .map(|(node, pre)| {
                let plan = pre.plan.as_ref();
                NodeStatus {
                    name: node.name.clone(),
                    rank: pre.rank,
                    role: if pre.rank == 0 {
                        "coordinator"
                    } else {
                        "worker"
                    }
                    .to_string(),
                    host: node.ssh.clone(),
                    state: NodeState::Loading,
                    pid: None,
                    layer_start: plan.map(|p| p.layer_start),
                    layer_end: plan.map(|p| p.layer_end),
                    shard_index: plan.and_then(|p| p.shard_index),
                    shard_count: plan.and_then(|p| p.shard_count),
                    available_bytes: pre.available_bytes,
                    total_bytes: pre.total_bytes,
                    pressure: pre.pressure.clone(),
                    memory_error: None,
                    active_bytes: None,
                    peak_bytes: None,
                    cache_bytes: None,
                    planned_bytes: plan.map(|p| p.with_overhead_bytes),
                    memory_limit_bytes: None,
                    wired_limit_bytes: None,
                    cache_limit_bytes: None,
                    kv_reserved_bytes: None,
                    kv_budget_bytes: None,
                    load_phase: None,
                    planned_weight_bytes: plan.map(|p| p.weights_bytes),
                    backend: config.backend,
                    tb_ip: node.tb_ip.clone(),
                    tb_interface: node.tb_interface.clone(),
                    link_speed: pre.link_speed.clone(),
                }
            })
            .collect();
        let (stop_tx, stop_rx) = watch::channel(false);
        let ctx = RunContext {
            shared: Arc::clone(&self.shared),
            exec: Arc::clone(&self.exec),
            http: reqwest::Client::builder()
                .timeout(PROBE_TIMEOUT)
                .build()
                .expect("reqwest client with static configuration"),
            stream_http: reqwest::Client::builder()
                .connect_timeout(PROBE_TIMEOUT)
                .build()
                .expect("reqwest client with static configuration"),
            config,
            served_id,
            runner,
            owner,
        };
        shared.stop_tx = Some(stop_tx);
        shared.task = Some(tokio::spawn(supervise(ctx, report.clone(), stop_rx)));
        Ok(StartOutcome::Started { preflight: report })
    }

    /// Stop the run and return the verified stop report. With no run supervised, the configured
    /// nodes are swept for this install's ranks a previous goosed left behind (per pid, by the
    /// owner token — never the bare marker).
    pub async fn stop(&self) -> StopReport {
        let _control = self.control.lock().await;
        let (stop_tx, task, config) = {
            let mut shared = self.shared.lock().unwrap();
            (
                shared.stop_tx.take(),
                shared.task.take(),
                shared.status.config.clone(),
            )
        };
        if let (Some(stop_tx), Some(task)) = (stop_tx, task) {
            self.shared
                .lock()
                .unwrap()
                .event(EventKind::StopRequested, None, "stop requested");
            let _ = stop_tx.send(true);
            return match task.await {
                Ok(report) => report,
                Err(e) => StopReport {
                    steps: vec![format!("the supervisor task ended abnormally: {e}")],
                    verified: false,
                },
            };
        }
        let Some(config) = config else {
            return StopReport {
                steps: vec!["nothing supervised and no distributed config known".to_string()],
                verified: true,
            };
        };
        let mut report = StopReport {
            steps: Vec::new(),
            verified: true,
        };
        let owner = self.owner();
        for node in &config.nodes {
            let (steps, verified) = reclaim_marked_ranks(
                self.exec.as_ref(),
                node.host(),
                &node.name,
                owner.as_deref(),
            )
            .await;
            report.verified &= verified;
            report.steps.extend(steps);
        }
        if !report.steps.is_empty() {
            self.shared.lock().unwrap().event(
                EventKind::OrphanReclaimed,
                None,
                report.steps.join("; "),
            );
        }
        report
    }

    /// "Make room" on one configured node: compact it now and record the outcome. Refused while
    /// the distributed engine owns this Mac — its ranks are loaded models, never pressured.
    pub async fn make_room(
        &self,
        config: &DistributedConfig,
        node_name: &str,
    ) -> Result<NodeCompaction> {
        let node = config
            .nodes
            .iter()
            .find(|n| n.name == node_name)
            .ok_or_else(|| {
                anyhow!(
                    "no node named '{node_name}' in the distributed config (nodes: {})",
                    config
                        .nodes
                        .iter()
                        .map(|n| n.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })?;
        let record = if self.owns_the_mac() {
            NodeCompaction::refused(
                &node.name,
                "manual",
                CompactionRefusal {
                    code: "engineLoaded".to_string(),
                    message: format!(
                        "the distributed engine is {} — its ranks are loaded models; stop it before \
                         making room",
                        self.status().state.as_str()
                    ),
                },
            )
        } else {
            let _control = self.control.lock().await;
            compaction::compact_recorded(self.exec.as_ref(), node.host(), &node.name, "manual")
                .await
        };
        self.shared
            .lock()
            .unwrap()
            .record_compaction(record.clone());
        Ok(record)
    }

    /// Remember a config without starting (the UI's editor persists through this).
    pub fn set_config(&self, config: Option<DistributedConfig>) {
        let mut shared = self.shared.lock().unwrap();
        if !shared.status.state.owns_the_mac() {
            shared.status.config = config;
        }
    }
}

pub fn global_manager() -> &'static DistributedManager {
    static MANAGER: OnceLock<DistributedManager> = OnceLock::new();
    MANAGER.get_or_init(DistributedManager::default)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::distributed::config::tests::two_mac_config;
    use crate::distributed::exec::{BoxFuture, ExecOutput};
    use crate::distributed::launch::RankLive;

    fn at(start: Instant, ms: u64) -> Instant {
        start + Duration::from_millis(ms)
    }

    /// Measured 2026-09-24 on Flash (installed 3.0.24): the last request ended at 17:17:50 and at
    /// 17:18:12 the rule stopped an IDLE engine — "silent 20298 ms … rank ps stats [S, S]",
    /// bound 20,160 ms (10 × the 2,016 ms median). Idle silence is not a hang; work silence is.
    #[test]
    fn an_idle_engine_is_never_a_hang_and_work_after_idle_is_timed_from_its_start() {
        let start = Instant::now();
        let mut meter = ProgressMeter::new(start, 2);
        meter.observe(at(start, 0), Some(0), &[Some(0), Some(0)]);
        for i in 1..=39u64 {
            let r = meter.observe(at(start, i * 2_016), Some(i), &[Some(i), Some(i)]);
            assert!(r.progressed);
        }
        let last = 39 * 2_016;
        // Idle for 5 minutes: counter and CPU frozen, nothing in flight.
        for poll in 1..=150u64 {
            let now = at(start, last + poll * 2_016);
            let mut r = meter.observe(now, Some(39), &[Some(39), Some(39)]);
            judge_silence(&mut meter, now, false, &mut r);
            assert!(!r.hang, "idle poll {poll} read as a hang");
        }
        // A request arrives and the pipeline freezes on it: caught one bound after it started.
        let resumed = last + 150 * 2_016;
        let mut caught = None;
        for poll in 1..=20u64 {
            let now = at(start, resumed + poll * 2_016);
            let mut r = meter.observe(now, Some(39), &[Some(39), Some(39)]);
            judge_silence(&mut meter, now, true, &mut r);
            if r.hang {
                caught = Some(poll);
                break;
            }
        }
        // 20,160 ms bound at a 2,016 ms poll: the 11th busy poll (22,176 ms silent).
        assert_eq!(caught, Some(11));
    }

    fn node_preflight(rank: usize, short_bytes: Option<u64>) -> preflight::NodePreflight {
        preflight::NodePreflight {
            name: format!("node{rank}"),
            rank,
            host: None,
            checks: Vec::new(),
            available_bytes: None,
            total_bytes: None,
            pressure: None,
            plan: None,
            link_speed: None,
            mlx_version: None,
            ceiling_bytes: None,
            wired_limit_mb: None,
            short_bytes,
            top_apps: Vec::new(),
            leftovers: Vec::new(),
            foreign_splits: Vec::new(),
        }
    }

    #[test]
    fn only_short_nodes_with_free_memory_on_are_compacted_and_only_after_a_failed_preflight() {
        let mut config = two_mac_config();
        let mut report = PreflightReport {
            ok: false,
            ran_at_ms: 0,
            backend: config.backend,
            runner: None,
            model_type: None,
            context_limit: None,
            context_source: None,
            max_context_fits: None,
            pipeline_starts: None,
            slots: None,
            checks: Vec::new(),
            nodes: vec![node_preflight(0, None), node_preflight(1, Some(3 * GIB))],
            repairs: Vec::new(),
        };
        assert_eq!(nodes_to_compact(&config, &report), vec![1]);
        config.nodes[1].free_memory_automatically = false;
        assert!(nodes_to_compact(&config, &report).is_empty());
        config.nodes[1].free_memory_automatically = true;
        report.ok = true;
        assert!(nodes_to_compact(&config, &report).is_empty());
    }

    #[test]
    fn the_status_keeps_the_latest_compaction_per_node_and_names_it_in_an_event() {
        let mut shared = Shared {
            status: DistributedStatus::stopped(),
            stop_tx: None,
            task: None,
        };
        let refused = NodeCompaction::refused(
            "workhorse",
            "automatic",
            CompactionRefusal {
                code: "engineLoaded".into(),
                message: "pid 7: mlx_lm.server".into(),
            },
        );
        shared.record_compaction(refused);
        let report = compaction::CompactionReport {
            node: "workhorse".into(),
            at_ms: 1,
            total_bytes: 96 * GIB,
            before_available_bytes: (72.4 * GIB as f64) as u64,
            peak_available_bytes: (3.73 * GIB as f64) as u64,
            end: compaction::PhaseEnd::Warn,
            ballast_pid: 16716,
            settled_available_bytes: 78 * GIB,
            gained_bytes: 78 * GIB as i64 - (72.4 * GIB as f64) as i64,
            settle_samples: 6,
        };
        shared.record_compaction(NodeCompaction {
            node: "workhorse".into(),
            at_ms: 2,
            trigger: "manual".into(),
            report: Some(report),
            refusal: None,
            error: None,
        });
        assert_eq!(shared.status.compactions.len(), 1);
        assert_eq!(shared.status.compactions[0].trigger, "manual");
        let kinds: Vec<EventKind> = shared.status.events.iter().map(|e| e.kind).collect();
        assert_eq!(
            kinds,
            [EventKind::CompactionSkipped, EventKind::MemoryCompacted]
        );
        let message = &shared.status.events[1].message;
        assert!(
            message.contains("freed 5.6 GiB")
                && message.contains("72.4 → 78.0")
                && message.contains("reached WARN at 3.7 GiB"),
            "{message}"
        );
    }

    #[test]
    fn the_hang_rule_waits_for_samples_then_bounds_silence_at_k_times_the_median() {
        let start = Instant::now();
        let mut meter = ProgressMeter::new(start, 2);
        // First poll only sets the baselines.
        assert!(
            !meter
                .observe(at(start, 2_000), Some(10), &[Some(100), Some(100)])
                .progressed
        );
        // Healthy: the counter advances every 2 s poll.
        for i in 1..=5u64 {
            let r = meter.observe(at(start, 2_000 + i * 2_000), Some(10 + i), &[None, None]);
            assert!(r.progressed && !r.hang);
        }
        assert_eq!(meter.median(), Some(Duration::from_millis(2_000)));
        // Silent (counter frozen, CPU unknown): 19.9 s is inside 10 × 2 s, 20.1 s is a hang.
        let last = 12_000;
        let r = meter.observe(at(start, last + 19_900), Some(15), &[None, None]);
        assert!(!r.hang, "{r:?}");
        let r = meter.observe(at(start, last + 20_100), Some(15), &[None, None]);
        assert!(r.hang, "{r:?}");
        assert_eq!(r.bound, Some(Duration::from_millis(20_000)));
    }

    #[test]
    fn a_long_prefill_chunk_is_not_a_hang_while_every_rank_computes() {
        let start = Instant::now();
        let mut meter = ProgressMeter::new(start, 2);
        meter.observe(at(start, 0), Some(0), &[Some(0), Some(0)]);
        for i in 1..=4u64 {
            meter.observe(at(start, i * 2_000), Some(i), &[Some(i), Some(i)]);
        }
        // A 60 s chunk: the counter stands still, both ranks' CPU keeps moving.
        for i in 1..=30u64 {
            let r = meter.observe(
                at(start, 8_000 + i * 2_000),
                Some(4),
                &[Some(4 + i * 50), Some(4 + i * 50)],
            );
            assert!(!r.hang, "poll {i}: {r:?}");
        }
    }

    #[test]
    fn a_frozen_rank_stalls_progress_even_when_its_peer_spins() {
        let start = Instant::now();
        let mut meter = ProgressMeter::new(start, 2);
        meter.observe(at(start, 0), Some(0), &[Some(0), Some(0)]);
        for i in 1..=4u64 {
            meter.observe(at(start, i * 2_000), Some(i), &[Some(i), Some(i)]);
        }
        // Rank 1 frozen (CPU flat), rank 0 spinning in the collective: not progress.
        let mut hang_at = None;
        for i in 1..=15u64 {
            let r = meter.observe(
                at(start, 8_000 + i * 2_000),
                Some(4),
                &[Some(100 + i * 100), Some(4)],
            );
            if r.hang {
                hang_at = Some(i * 2_000);
                break;
            }
        }
        assert_eq!(hang_at, Some(22_000), "first poll past 10 × 2 s");
    }

    #[test]
    fn the_breaker_allows_the_parity_count_then_opens() {
        let parity = sidecar_parity();
        let mut restarts = VecDeque::new();
        let now = Instant::now();
        for _ in 0..parity.max_restarts_in_window {
            assert!(breaker_allows(
                &mut restarts,
                now,
                parity.restart_window,
                parity.max_restarts_in_window
            ));
        }
        assert!(!breaker_allows(
            &mut restarts,
            now,
            parity.restart_window,
            parity.max_restarts_in_window
        ));
        let later = now + parity.restart_window + Duration::from_secs(1);
        assert!(breaker_allows(
            &mut restarts,
            later,
            parity.restart_window,
            parity.max_restarts_in_window
        ));
    }

    /// E2E #2's Studio (96 GiB, the CRITICAL reserve 0.02 × RAM = 1.9 GiB), read from the
    /// sampler's free percentage every 30 s while one busy stretch ran from 22:40:46 (49% free):
    /// 45% at 22:41:17, 34%, 32% at 22:42:18, 20% at 22:42:49, 14% at 22:43:20; the rank died at
    /// 22:43:46, 2 s after the 5% floor's WARN. The stretch's own growth against what is left
    /// fires at 22:42:49 — before the 44,430-token turn and the one that died were admitted.
    #[test]
    fn a_busy_stretch_that_took_what_is_left_closes_admission_before_the_floor() {
        let total = 96 * GIB;
        let at = |percent: u64| total * percent / 100;
        let project = |available| growth_projection(at(49), available, total, 0.02);
        assert_eq!(project(at(45)), None);
        assert_eq!(project(at(34)), None);
        assert_eq!(project(at(32)), None);
        let (consumed, left) = project(at(20)).expect("fires at 22:42:49");
        assert!(consumed >= left, "{consumed} {left}");
        assert!(
            watchdog_verdict(Pressure::Normal, at(20), total, (0.05, 0.02)) == Watchdog::Normal,
            "the floor alone still read NORMAL there"
        );
        // An idle engine re-baselines: nothing consumed, nothing projected.
        assert_eq!(growth_projection(at(20), at(20), total, 0.02), None);
    }

    fn exit_status(raw: i32) -> std::process::ExitStatus {
        use std::os::unix::process::ExitStatusExt;
        std::process::ExitStatus::from_raw(raw)
    }

    /// E2E #2's Studio rank, its last words verbatim: the generation thread's Metal OOM, then
    /// Link's report — and exit 0, which read as an ordinary rank death.
    #[test]
    fn a_metal_out_of_memory_names_the_death_even_at_exit_zero() {
        let tail = "2026-09-25 22:43:43,320 - INFO - Prompt Cache: 41 sequences, 16.00 GB\n\
            Exception in thread Thread-2 (_generate):\n\
            Traceback (most recent call last):\n\
            \x20   mx.eval([c.state for c in self.prompt_cache])\n\
            RuntimeError: [METAL] Command buffer execution failed: Insufficient Memory \
            (00000008:kIOGPUCommandBufferCallbackErrorOutOfMemory).\n\
            [LeanZero Link] Work’s Mac Studio reports its rank ended: exited on its own with code 0";
        let evidence = memory_death(tail, exit_status(0), None).expect("named");
        assert!(evidence.starts_with("RuntimeError: [METAL]"), "{evidence}");

        let fatal = "GOOSE_RANK_FATAL {\"rank\": 1, \"thread\": \"generation\", \"error\": \"x\", \
            \"out_of_memory\": true}";
        assert!(memory_death(fatal, exit_status(70 << 8), None).is_some());

        let other = "Traceback (most recent call last):\nValueError: bad shape";
        assert_eq!(memory_death(other, exit_status(70 << 8), None), None);
        // The kernel's SIGKILL leaves no words: memory's only while the watchdog held admission.
        assert_eq!(memory_death("", exit_status(libc::SIGKILL), None), None);
        assert!(memory_death("", exit_status(libc::SIGKILL), Some("Studio: WARN")).is_some());
        assert!(memory_death("", exit_status(137 << 8), Some("Studio: WARN")).is_some());
    }

    /// The Studio's memory after the rank died: short on the first poll (2.1 GiB of 96, below the
    /// 5% reserve), back on the second. The restart waits for the second and says, meanwhile,
    /// what it waits for.
    struct RecoveringNode {
        polls: StdMutex<u32>,
        shared: Arc<StdMutex<Shared>>,
        seen: StdMutex<Vec<Option<String>>>,
    }

    impl NodeExec for RecoveringNode {
        fn run<'a>(
            &'a self,
            _host: Option<&'a str>,
            _script: &'a str,
        ) -> BoxFuture<'a, Result<ExecOutput>> {
            let poll = {
                let mut polls = self.polls.lock().unwrap();
                *polls += 1;
                *polls
            };
            self.seen
                .lock()
                .unwrap()
                .push(self.shared.lock().unwrap().status.memory_recovery.clone());
            let (free, file, level) = if poll == 1 {
                (100_000, 30_000, 2)
            } else {
                (4_000_000, 900_000, 1)
            };
            let stdout = format!(
                "\n@@vm\nMach Virtual Memory Statistics: (page size of 16384 bytes)\n\
                 Pages free:                                  {free}.\n\
                 Pages active:                                1000000.\n\
                 Pages inactive:                              1000000.\n\
                 Pages speculative:                            100000.\n\
                 Pages throttled:                                   0.\n\
                 Pages wired down:                             500000.\n\
                 Pages purgeable:                               10000.\n\
                 File-backed pages:                            {file}.\n\
                 Anonymous pages:                             1000000.\n\
                 \n@@sysctl\n103079215104\n{level}\n\n@@end\n"
            );
            Box::pin(async move {
                Ok(ExecOutput {
                    status: Some(0),
                    stdout,
                    stderr: String::new(),
                })
            })
        }
    }

    #[tokio::test]
    async fn the_memory_restart_waits_until_every_node_is_back_above_its_reserve() {
        let shared = Arc::new(StdMutex::new(Shared {
            status: DistributedStatus::stopped(),
            stop_tx: None,
            task: None,
        }));
        let exec = Arc::new(RecoveringNode {
            polls: StdMutex::new(0),
            shared: Arc::clone(&shared),
            seen: StdMutex::new(Vec::new()),
        });
        let mut config = two_mac_config();
        config.nodes.truncate(1);
        let ctx = RunContext {
            shared: Arc::clone(&shared),
            exec: exec.clone(),
            http: reqwest::Client::new(),
            stream_http: reqwest::Client::new(),
            config,
            served_id: "node-alias".into(),
            runner: Runner::MlxLmTensor,
        };
        let (_stop_tx, mut stop_rx) = watch::channel(false);
        let recovered = wait_memory_recovered(&ctx, &mut stop_rx)
            .await
            .expect("recovers on the second poll");
        assert!(recovered.contains("kernel normal"), "{recovered}");
        let seen = exec.seen.lock().unwrap().clone();
        assert_eq!(seen[0], None);
        let waiting = seen[1].clone().expect("said what it waits for");
        assert!(
            waiting.starts_with("waiting for memory to recover before restarting")
                && waiting.contains("kernel warn, available 2.1 GiB of 96.0 GiB"),
            "{waiting}"
        );
        assert_eq!(shared.lock().unwrap().status.memory_recovery, None);
    }

    #[tokio::test]
    async fn a_link_rank_that_ended_of_memory_is_named_by_its_own_words() {
        let ended = sleeper("exit 0");
        let rank = rank(1, Some("link:studio-1a2b"), ended, Some(42));
        {
            let mut live = rank.live.lock().unwrap();
            live.tail.push_back(
                "RuntimeError: [METAL] Command buffer execution failed: Insufficient Memory \
                 (00000008:kIOGPUCommandBufferCallbackErrorOutOfMemory)."
                    .into(),
            );
            live.tail.push_back(
                "[LeanZero Link] Studio reports its rank ended: exited on its own with code 0"
                    .into(),
            );
        }
        let (kind, message) =
            rank_exit(&rank, exit_status(0), "", "; the pair cannot serve.", None);
        assert_eq!(kind, EventKind::RankOutOfMemory);
        assert!(
            message.starts_with("rank 1 died of memory (RuntimeError"),
            "{message}"
        );
        assert!(
            message.contains("ended on its Link node (exit status: 0)")
                && !message.contains("session closed"),
            "{message}"
        );
    }

    #[test]
    fn the_watchdog_follows_the_kernel_and_the_reserves() {
        let total = 96 * GIB;
        let ratios = (0.05, 0.02);
        let v = |p, a| watchdog_verdict(p, a, total, ratios);
        assert_eq!(v(Pressure::Normal, 40 * GIB), Watchdog::Normal);
        assert_eq!(v(Pressure::Warn, 40 * GIB), Watchdog::Warn);
        assert_eq!(v(Pressure::Critical, 40 * GIB), Watchdog::Critical);
        // 4.8 GiB is the 5% reserve of 96 GiB, 1.92 GiB the 2% one.
        assert_eq!(v(Pressure::Normal, 4 * GIB), Watchdog::Warn);
        assert_eq!(v(Pressure::Normal, GIB), Watchdog::Critical);
        // Raised reserves (a run's override): 43.2 / 33.6 GiB on 96 GiB.
        let raised = |a| watchdog_verdict(Pressure::Normal, a, total, (0.45, 0.35));
        assert_eq!(raised(50 * GIB), Watchdog::Normal);
        assert_eq!(raised(40 * GIB), Watchdog::Warn);
        assert_eq!(raised(30 * GIB), Watchdog::Critical);
    }

    /// Runs every "peer" script on this Mac: the stop sequence's remote legs, exercised against
    /// real local processes standing in for the peer rank.
    struct LocalAsPeer;
    impl NodeExec for LocalAsPeer {
        fn run<'a>(
            &'a self,
            _host: Option<&'a str>,
            script: &'a str,
        ) -> BoxFuture<'a, Result<ExecOutput>> {
            SystemExec.run(None, script)
        }
    }

    fn rank(
        rank: usize,
        host: Option<&str>,
        child: tokio::process::Child,
        pid: Option<u32>,
    ) -> RankProcess {
        RankProcess {
            rank,
            node: format!("node{rank}"),
            host: host.map(str::to_string),
            child,
            live: Arc::new(StdMutex::new(RankLive {
                pid,
                ..Default::default()
            })),
            owner: Some(OWNER.to_string()),
        }
    }

    /// The install token the tests' ranks and leftovers carry.
    const OWNER: &str = "0123456789abcdef";

    fn sleeper(script: &str) -> tokio::process::Child {
        tokio::process::Command::new("/bin/sh")
            .args(["-c", script])
            .kill_on_drop(true)
            .spawn()
            .unwrap()
    }

    fn alive(pid: u32) -> bool {
        let out = std::process::Command::new("/bin/ps")
            .args(["-o", "stat=", "-p", &pid.to_string()])
            .output()
            .unwrap();
        let stat = String::from_utf8_lossy(&out.stdout);
        !stat.trim().is_empty() && !stat.contains('Z')
    }

    #[tokio::test]
    async fn stop_terms_rank_zero_then_terms_the_lingering_peer_pid_and_verifies_it_gone() {
        let local = sleeper("exec sleep 60");
        let ssh_stand_in = sleeper("exec sleep 60");
        let peer = sleeper("exec sleep 60");
        let peer_pid = peer.id().unwrap();
        let local_pid = local.id().unwrap();
        let mut ranks = vec![
            rank(0, None, local, None),
            rank(1, Some("peer"), ssh_stand_in, Some(peer_pid)),
        ];
        let report = stop_ranks(&mut ranks, &LocalAsPeer, None, false).await;
        assert!(report.verified, "{report:?}");
        assert!(
            report.steps[0]
                .starts_with(&format!("rank 0 (node0) pid {local_pid}: SIGTERM → exited")),
            "{report:?}"
        );
        assert!(
            report.steps[1].contains("alive after rank 0's exit → SIGTERM (sent)"),
            "{report:?}"
        );
        assert!(
            report.steps[1].ends_with("→ gone (verified over ssh)"),
            "{report:?}"
        );
        assert!(!alive(local_pid) && !alive(peer_pid));
        drop(peer);
    }

    #[tokio::test]
    async fn a_peer_that_ignores_sigterm_gets_sigkill_on_its_pid_alone() {
        let local = sleeper("exec sleep 60");
        let ssh_stand_in = sleeper("exec sleep 60");
        // Ignored signals survive exec: this "rank" ignores SIGTERM.
        let stubborn = sleeper("trap '' TERM; exec sleep 60");
        let stubborn_pid = stubborn.id().unwrap();
        // A bystander in the SAME process group as the test must survive: no group is signalled.
        let bystander = sleeper("exec sleep 60");
        let bystander_pid = bystander.id().unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
        let mut ranks = vec![
            rank(0, None, local, None),
            rank(1, Some("peer"), ssh_stand_in, Some(stubborn_pid)),
        ];
        let report = stop_ranks(&mut ranks, &LocalAsPeer, None, false).await;
        assert!(report.verified, "{report:?}");
        assert!(
            report.steps[1]
                .contains("SIGTERM (sent) → alive after the grace → SIGKILL (sent) → gone"),
            "{report:?}"
        );
        assert!(!alive(stubborn_pid));
        assert!(
            alive(bystander_pid),
            "a per-pid stop never reaches a bystander"
        );
        drop(stubborn);
        drop(bystander);
    }

    /// A node whose `ps` cannot answer: every script is recorded and fails the way a ps that
    /// could not run does (non-zero exit, a stderr line, no rows).
    #[derive(Default)]
    struct PsFails {
        scripts: StdMutex<Vec<String>>,
    }
    impl NodeExec for PsFails {
        fn run<'a>(
            &'a self,
            _host: Option<&'a str>,
            script: &'a str,
        ) -> BoxFuture<'a, Result<ExecOutput>> {
            self.scripts.lock().unwrap().push(script.to_string());
            Box::pin(async {
                Ok(ExecOutput {
                    status: Some(1),
                    stdout: String::new(),
                    stderr: "ps: sysctl: Operation not permitted".into(),
                })
            })
        }
    }

    /// Gate 4, fail CLOSED: a peer pid whose liveness ps cannot prove is never signalled, the
    /// stop says so and is unverified — and so is a sweep whose process listing failed.
    #[tokio::test]
    async fn a_pid_ps_cannot_prove_is_never_signalled_and_the_stop_says_so() {
        let local = sleeper("exec sleep 60");
        let ssh_stand_in = sleeper("exit 0");
        let peer = sleeper("exec sleep 60");
        let peer_pid = peer.id().unwrap();
        let exec = PsFails::default();
        let mut ranks = vec![
            rank(0, None, local, None),
            rank(1, Some("peer"), ssh_stand_in, Some(peer_pid)),
        ];
        let report = stop_ranks(&mut ranks, &exec, None, false).await;
        assert!(!report.verified, "{report:?}");
        assert!(
            report.steps[1].contains(&format!("pid {peer_pid}: cannot observe"))
                && report.steps[1].contains("ps could not answer")
                && !report.steps[1].contains("SIGTERM"),
            "{report:?}"
        );

        let (steps, verified) =
            reclaim_marked_ranks(&exec, Some("peer"), "node1", Some(OWNER)).await;
        assert!(!verified);
        assert!(steps[0].contains("nothing signalled"), "{steps:?}");

        let scripts = exec.scripts.lock().unwrap().clone();
        assert!(
            scripts.iter().all(|s| !s.contains("kill")),
            "no signal may reach an unproven pid: {scripts:?}"
        );
        assert!(alive(peer_pid));
        drop(peer);
    }

    /// The pipeline shape: the peer leaves on its own shortly after rank 0 (the fork's shutdown
    /// broadcast); the stop waits for that exit and verifies it without signalling the peer.
    #[tokio::test]
    async fn a_pipeline_peer_that_follows_rank_zero_is_verified_without_a_signal() {
        let local = sleeper("exec sleep 60");
        let ssh_stand_in = sleeper("exec sleep 60");
        let follower = sleeper("sleep 1; exit 0");
        let follower_pid = follower.id().unwrap();
        let mut ranks = vec![
            rank(0, None, local, None),
            rank(1, Some("peer"), ssh_stand_in, Some(follower_pid)),
        ];
        let report = stop_ranks(&mut ranks, &LocalAsPeer, None, true).await;
        assert!(report.verified, "{report:?}");
        assert!(
            report.steps[1].contains("left on rank 0's shutdown broadcast within")
                && !report.steps[1].contains("SIGTERM"),
            "{report:?}"
        );
        drop(follower);
    }

    /// The same pipeline stop when the peer does NOT follow: after the grace window, per-pid
    /// SIGTERM, verified gone.
    #[tokio::test]
    async fn a_pipeline_peer_that_stays_is_termed_after_the_grace_window() {
        let local = sleeper("exec sleep 60");
        let ssh_stand_in = sleeper("exec sleep 60");
        let stayer = sleeper("exec sleep 60");
        let stayer_pid = stayer.id().unwrap();
        let mut ranks = vec![
            rank(0, None, local, None),
            rank(1, Some("peer"), ssh_stand_in, Some(stayer_pid)),
        ];
        let report = stop_ranks(&mut ranks, &LocalAsPeer, None, true).await;
        assert!(report.verified, "{report:?}");
        assert!(
            report.steps[1]
                .contains("alive after rank 0's exit and the grace window → SIGTERM (sent)")
                && report.steps[1].ends_with("→ gone (verified over ssh)"),
            "{report:?}"
        );
        assert!(!alive(stayer_pid));
        drop(stayer);
    }

    /// The pipeline server's real /v1/status shapes (fork ea6f8dee1 `_build_app.status`).
    #[test]
    fn the_pipeline_status_carries_slots_and_per_rank_kv() {
        let busy = r#"{"num_running": 2, "num_waiting": 1, "slots": 2, "slots_in_use": 2,
            "sequences_in_flight": 2, "kv_reserved_bytes": [3000000000, 3100000000],
            "kv_budget_bytes": [5163311136, 5757071360], "status": "ok"}"#;
        let load = server_load(busy, Runner::PipelineQwen4, 2).unwrap();
        assert_eq!(
            load,
            ServerLoad {
                waiting: 1,
                slots: Some(2),
                slots_in_use: Some(2),
                sequences_in_flight: Some(2),
                kv: Some(vec![
                    (3_000_000_000, 5_163_311_136),
                    (3_100_000_000, 5_757_071_360)
                ]),
            }
        );
        // Idle: the reservation list is empty — 0 reserved on every rank, budgets still known.
        let idle = r#"{"num_running": 0, "num_waiting": 0, "slots": 2, "slots_in_use": 0,
            "sequences_in_flight": 0, "kv_reserved_bytes": [], "kv_budget_bytes": [5, 6],
            "status": "ok"}"#;
        let load = server_load(idle, Runner::PipelineQwen4, 2).unwrap();
        assert_eq!(load.kv, Some(vec![(0, 5), (0, 6)]));
        assert_eq!(load.slots_in_use, Some(0));
    }

    /// Both runners' rank 0 now answer `/v1/status` with the live request table added
    /// (rank_live.py) — measured bodies, 2026-09-24: a 2-rank local ring 27B tensor split and a
    /// 2-rank local pipeline over a 4-layer Flash. The supervisor's own fields read as before.
    #[test]
    fn the_live_request_table_rides_the_runners_own_status() {
        let tensor = r#"{"num_running": 1, "num_waiting": 0, "status": "generating", "generation_tps": 10.57, "requests": [{"request_id": "req-1", "status": "running", "phase": "generation", "elapsed_s": 21.354, "prompt_tokens": 3249, "prefilled_tokens": 3249, "prompt_tokens_per_second": 154.32, "completion_tokens": 3, "max_tokens": 60, "tokens_per_second": 10.57, "ttft_s": 21.069, "cached_tokens": 0}]}"#;
        let load = server_load(tensor, Runner::MlxLmTensor, 2).unwrap();
        assert_eq!((load.waiting, load.slots), (0, None));
        let pipeline = r#"{"num_running": 1, "num_waiting": 2, "slots": 2, "slots_in_use": 1, "sequences_in_flight": 1, "kv_reserved_bytes": [2061807632, 2114216960], "kv_budget_bytes": [4123615264, 4254087168], "status": "generating", "generation_tps": 171.1, "requests": [{"request_id": "a52c969689464b6e88cbe1b5", "status": "running", "phase": "generation", "elapsed_s": 1.209, "prompt_tokens": 6546, "prefilled_tokens": 6546, "prompt_tokens_per_second": 5503.35, "completion_tokens": 4, "max_tokens": 80, "tokens_per_second": 171.1, "ttft_s": 1.19, "cached_tokens": null}, {"request_id": "58111b87b46a43b2982e9143", "status": "waiting", "phase": "queued", "elapsed_s": 1.186, "prompt_tokens": 6546, "prefilled_tokens": 0, "prompt_tokens_per_second": null, "completion_tokens": 0, "max_tokens": 80, "tokens_per_second": null, "ttft_s": null, "cached_tokens": null}]}"#;
        let load = server_load(pipeline, Runner::PipelineQwen4, 2).unwrap();
        assert_eq!(load.waiting, 2);
        assert_eq!(load.sequences_in_flight, Some(1));
        assert_eq!(
            load.kv,
            Some(vec![(2061807632, 4123615264), (2114216960, 4254087168)])
        );
    }

    #[test]
    fn a_tensor_status_has_no_slots_and_a_pipeline_one_without_them_is_an_error() {
        // The tensor wrapper's exact answer (rank_wrapper.py do_GET /v1/status).
        let tensor = r#"{"num_running": 1, "num_waiting": 0, "status": "ok"}"#;
        let load = server_load(tensor, Runner::MlxLmTensor, 2).unwrap();
        assert_eq!(
            (
                load.waiting,
                load.slots,
                load.slots_in_use,
                load.sequences_in_flight,
                load.kv
            ),
            (0, None, None, None, None)
        );
        let err = server_load(tensor, Runner::PipelineQwen4, 2)
            .unwrap_err()
            .to_string();
        assert!(err.contains("carries no slots"), "{err}");
        let short = r#"{"num_running": 0, "num_waiting": 0, "slots": 2, "slots_in_use": 0,
            "sequences_in_flight": 0, "kv_reserved_bytes": [], "kv_budget_bytes": [5],
            "status": "ok"}"#;
        assert!(server_load(short, Runner::PipelineQwen4, 2).is_err());
        assert!(server_load("<html>", Runner::MlxLmTensor, 2).is_err());
    }

    #[test]
    fn a_failed_status_poll_clears_the_figures_and_names_why() {
        let mut status = DistributedStatus::stopped();
        status.nodes = vec![node_status(0), node_status(1)];
        let body = r#"{"num_running": 1, "num_waiting": 3, "slots": 2, "slots_in_use": 1,
            "sequences_in_flight": 1, "kv_reserved_bytes": [7, 8], "kv_budget_bytes": [70, 80],
            "status": "ok"}"#;
        publish_server_load(&mut status, server_load(body, Runner::PipelineQwen4, 2));
        assert_eq!(
            (status.waiting, status.slots, status.slots_in_use),
            (Some(3), Some(2), Some(1))
        );
        assert_eq!(
            (
                status.nodes[1].kv_reserved_bytes,
                status.nodes[1].kv_budget_bytes
            ),
            (Some(8), Some(80))
        );
        publish_server_load(&mut status, Err(anyhow!("connection refused")));
        assert_eq!((status.waiting, status.slots), (None, None));
        assert_eq!(status.nodes[1].kv_budget_bytes, None);
        assert_eq!(
            status.server_status_error.as_deref(),
            Some("connection refused")
        );
    }

    fn node_status(rank: usize) -> NodeStatus {
        NodeStatus {
            name: format!("node{rank}"),
            rank,
            role: "worker".to_string(),
            host: None,
            state: NodeState::Ready,
            pid: None,
            layer_start: None,
            layer_end: None,
            shard_index: None,
            shard_count: None,
            available_bytes: None,
            total_bytes: None,
            pressure: None,
            memory_error: None,
            active_bytes: None,
            peak_bytes: None,
            cache_bytes: None,
            planned_bytes: None,
            memory_limit_bytes: None,
            wired_limit_bytes: None,
            cache_limit_bytes: None,
            kv_reserved_bytes: None,
            kv_budget_bytes: None,
            load_phase: None,
            planned_weight_bytes: None,
            backend: Backend::Jaccl,
            tb_ip: String::new(),
            tb_interface: String::new(),
            link_speed: None,
        }
    }

    #[tokio::test]
    async fn a_peer_already_gone_is_verified_without_a_signal() {
        let local = sleeper("exec sleep 60");
        let ssh_stand_in = sleeper("exit 0");
        let mut finished = sleeper("exit 0");
        let finished_pid = finished.id().unwrap();
        finished.wait().await.unwrap();
        let mut ranks = vec![
            rank(0, None, local, None),
            rank(1, Some("peer"), ssh_stand_in, Some(finished_pid)),
        ];
        let report = stop_ranks(&mut ranks, &LocalAsPeer, None, false).await;
        assert!(report.verified, "{report:?}");
        assert!(
            report.steps[1].contains("gone") && report.steps[1].contains("verified over ssh"),
            "{report:?}"
        );
        assert!(!report.steps[1].contains("SIGTERM"), "{report:?}");
    }

    #[tokio::test]
    async fn start_is_refused_while_the_single_engine_is_mounted() {
        let manager = DistributedManager::new(Arc::new(LocalAsPeer));
        let outcome = manager
            .start_with_single_state(
                two_mac_config(),
                "node-alias".to_string(),
                "running",
                Some("org/model"),
            )
            .await
            .unwrap();
        match outcome {
            StartOutcome::Refused {
                code,
                message,
                preflight,
                ..
            } => {
                assert_eq!(code, RefusalCode::SingleEngineMounted);
                assert!(
                    message.contains("org/model") && message.contains("unmount"),
                    "{message}"
                );
                assert!(preflight.is_none(), "nothing was probed");
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(manager.status().state, RunState::Stopped);
        assert_eq!(manager.status().mode(), "single");
        assert!(manager.active_base_url().is_none());
    }

    /// Two nodes' `ps` as data: every probe answers the rows still alive, `ps -p` answers from
    /// the same rows, and a `/bin/kill` is recorded and ends that pid — no process is touched.
    struct Nodes {
        rows: StdMutex<Vec<PsRow>>,
        scripts: StdMutex<Vec<String>>,
    }

    /// (host, pid, ppid, row) — host `None` = the MacBook Pro.
    type PsRow = (Option<String>, u32, u32, String);

    impl Nodes {
        fn new(rows: Vec<(Option<&str>, u32, u32, String)>) -> Arc<Self> {
            Arc::new(Self {
                rows: StdMutex::new(
                    rows.into_iter()
                        .map(|(h, pid, ppid, row)| (h.map(str::to_string), pid, ppid, row))
                        .collect(),
                ),
                scripts: StdMutex::new(Vec::new()),
            })
        }

        fn kills(&self) -> Vec<String> {
            self.scripts
                .lock()
                .unwrap()
                .iter()
                .filter(|s| s.contains("kill"))
                .cloned()
                .collect()
        }
    }

    impl NodeExec for Nodes {
        fn run<'a>(
            &'a self,
            host: Option<&'a str>,
            script: &'a str,
        ) -> BoxFuture<'a, Result<ExecOutput>> {
            self.scripts.lock().unwrap().push(script.to_string());
            let mut rows = self.rows.lock().unwrap();
            let here = |h: &Option<String>| h.as_deref() == host;
            let out = |status, stdout: String| ExecOutput {
                status: Some(status),
                stdout,
                stderr: String::new(),
            };
            let answer = if script.contains("@@psppid") {
                let ps: Vec<&str> = rows
                    .iter()
                    .filter(|r| here(&r.0))
                    .map(|r| r.3.as_str())
                    .collect();
                let parents: Vec<String> = rows
                    .iter()
                    .filter(|r| here(&r.0))
                    .map(|r| format!("{} {}", r.1, r.2))
                    .collect();
                preflight::tests::probe_answer(&ps.join("\n"), Some(&parents.join("\n")))
            } else if let Some(pid) = script
                .strip_prefix("/bin/kill -")
                .and_then(|rest| rest.split_whitespace().nth(1))
            {
                let pid: u32 = pid.parse().unwrap();
                rows.retain(|r| !(here(&r.0) && r.1 == pid));
                out(0, String::new())
            } else if let Some(pid) = script.strip_prefix("/bin/ps -o pid=,stat=,time= -p ") {
                let pid: u32 = pid.trim().parse().unwrap();
                match rows.iter().any(|r| here(&r.0) && r.1 == pid) {
                    true => out(0, format!("{pid} S 0:00.10\n")),
                    false => out(1, String::new()),
                }
            } else {
                out(1, String::new())
            };
            Box::pin(async move { Ok(answer) })
        }
    }

    async fn start_on(nodes: &Arc<Nodes>) -> (DistributedManager, StartOutcome) {
        let manager = DistributedManager::new(Arc::clone(nodes) as Arc<dyn NodeExec>);
        manager.set_owner(OWNER.to_string());
        let outcome = manager
            .start_with_single_state(two_mac_config(), "node-alias".to_string(), "stopped", None)
            .await
            .unwrap();
        (manager, outcome)
    }

    fn events(manager: &DistributedManager, kind: EventKind) -> Vec<String> {
        manager
            .status()
            .events
            .iter()
            .filter(|e| e.kind == kind)
            .map(|e| e.message.clone())
            .collect()
    }

    /// Q-77, the waiting branch: this install's previous rank still runs under a live parent
    /// (the goose stopping it, a peer's lease) — the start is refused by name, with the Mac and
    /// the pid behind Details, and NOTHING is signalled.
    #[tokio::test]
    async fn a_start_waits_for_its_own_previous_rank_under_a_live_parent_and_signals_nothing() {
        let nodes = Nodes::new(vec![(
            None,
            9425,
            4242,
            preflight::tests::rank_row(9425, Some(OWNER)),
        )]);
        let (manager, outcome) = start_on(&nodes).await;
        let StartOutcome::Refused {
            code,
            message,
            node,
            detail,
            ..
        } = outcome
        else {
            panic!("{outcome:?}")
        };
        assert_eq!(code, RefusalCode::PreviousSplitShuttingDown);
        assert_eq!(node.as_deref(), Some("MacBook Pro"));
        assert_eq!(
            message,
            "The previous split is still shutting down on MacBook Pro — start it again when that \
             finishes"
        );
        let detail = detail.unwrap();
        assert!(
            detail.contains("pid 9425 (its parent pid 4242 still runs: shutting down)"),
            "{detail}"
        );
        assert!(nodes.kills().is_empty(), "{:?}", nodes.kills());
        assert_eq!(manager.status().state, RunState::Stopped);
        assert_eq!(events(&manager, EventKind::PreviousSplitWaiting).len(), 1);
    }

    /// The reclaim branch: an ORPHAN of this install (PPID 1 — its goose is gone) is reclaimed
    /// per pid, said so in an event, and the preflight runs again over the freed node — the start
    /// is then judged on everything else (here: this test config has no model to plan).
    #[tokio::test]
    async fn a_start_reclaims_its_own_orphan_per_pid_and_preflights_again() {
        let nodes = Nodes::new(vec![
            (None, 301, 1, preflight::tests::rank_row(301, Some(OWNER))),
            (
                Some("workhorse"),
                302,
                1,
                preflight::tests::rank_row(302, Some(OWNER)),
            ),
        ]);
        let (manager, outcome) = start_on(&nodes).await;
        let StartOutcome::Refused { code, .. } = outcome else {
            panic!("{outcome:?}")
        };
        assert_eq!(code, RefusalCode::PreflightFailed, "no leftover is left");
        assert_eq!(
            nodes.kills(),
            vec![
                "/bin/kill -TERM 301".to_string(),
                "/bin/kill -TERM 302".to_string()
            ],
            "one SIGTERM per pid, gone after it — never a group, never SIGKILL"
        );
        let reclaimed = events(&manager, EventKind::OrphanReclaimed).join("; ");
        assert!(
            reclaimed.contains("MacBook Pro: goose rank pid 301 → SIGTERM (sent) → gone")
                && reclaimed.contains("workhorse: goose rank pid 302")
                && reclaimed.contains("orphaned by the previous goose"),
            "{reclaimed}"
        );
        assert!(events(&manager, EventKind::PreviousSplitWaiting).is_empty());
    }

    /// The foreign branch — Q-77's pid 9425 as it really was: the boot line and the marker but no
    /// owner token (a stand-in rank another process launched). Never waited for, never reclaimed:
    /// a refusal in words the owner can act on, the pid behind Details.
    #[tokio::test]
    async fn a_rank_without_this_installs_token_is_a_foreign_split_refusal() {
        let nodes = Nodes::new(vec![(
            None,
            9425,
            1,
            preflight::tests::rank_row(9425, None),
        )]);
        let (_manager, outcome) = start_on(&nodes).await;
        let StartOutcome::Refused {
            code,
            message,
            node,
            detail,
            ..
        } = outcome
        else {
            panic!("{outcome:?}")
        };
        assert_eq!(code, RefusalCode::ForeignSplit);
        assert_eq!(node.as_deref(), Some("MacBook Pro"));
        assert_eq!(
            message,
            "Another MLX split (not goose's) is running on MacBook Pro — stop it to start this one"
        );
        assert!(detail
            .unwrap()
            .starts_with("pid 9425 `/x/bin/python -c import base64"));
        assert!(nodes.kills().is_empty());
    }

    /// The no-run Stop sweep signals only ranks carrying this install's token — judged on the
    /// WHOLE command line (the marker sits past the 160 characters a row keeps, which is why the
    /// sweep never matched a real rank before) — and with no token of its own it signals nothing.
    #[tokio::test]
    async fn the_stop_sweep_signals_only_this_installs_ranks() {
        let nodes = Nodes::new(vec![
            (None, 301, 1, preflight::tests::rank_row(301, Some(OWNER))),
            (None, 9425, 1, preflight::tests::rank_row(9425, None)),
            (
                None,
                303,
                1,
                preflight::tests::rank_row(303, Some("another")),
            ),
        ]);
        // ProcessList is `ps -axo pid=,command=`: answer it from the same rows.
        struct Listing(Arc<Nodes>);
        impl NodeExec for Listing {
            fn run<'a>(
                &'a self,
                host: Option<&'a str>,
                script: &'a str,
            ) -> BoxFuture<'a, Result<ExecOutput>> {
                if script == crate::distributed::node_op::PROCESS_LIST_SCRIPT {
                    let rows: Vec<String> = self
                        .0
                        .rows
                        .lock()
                        .unwrap()
                        .iter()
                        .map(|r| r.3.clone())
                        .collect();
                    return Box::pin(async move {
                        Ok(ExecOutput {
                            status: Some(0),
                            stdout: rows.join("\n"),
                            stderr: String::new(),
                        })
                    });
                }
                self.0.run(host, script)
            }
        }
        let exec = Listing(Arc::clone(&nodes));
        let (steps, verified) = reclaim_marked_ranks(&exec, None, "node0", Some(OWNER)).await;
        assert!(verified, "{steps:?}");
        assert_eq!(nodes.kills(), vec!["/bin/kill -TERM 301".to_string()]);

        let (steps, verified) = reclaim_marked_ranks(&exec, None, "node0", None).await;
        assert!(!verified);
        assert!(steps[0].contains("nothing signalled"), "{steps:?}");
        assert_eq!(nodes.kills().len(), 1);
    }

    #[test]
    fn a_node_holding_a_leftover_is_never_compacted() {
        let mut config = two_mac_config();
        config.nodes[0].free_memory_automatically = true;
        let mut short = node_preflight(0, Some(GIB));
        short.leftovers = vec![probe::GooseRankProcess {
            pid: 301,
            ppid: Some(4242),
            owner: Some(OWNER.to_string()),
            carrier: false,
            command: "python".to_string(),
        }];
        let report = PreflightReport {
            ok: false,
            ran_at_ms: 0,
            backend: config.backend,
            runner: None,
            model_type: None,
            context_limit: None,
            context_source: None,
            max_context_fits: None,
            pipeline_starts: None,
            slots: None,
            checks: Vec::new(),
            nodes: vec![short],
            repairs: Vec::new(),
        };
        assert!(nodes_to_compact(&config, &report).is_empty());
    }
}
