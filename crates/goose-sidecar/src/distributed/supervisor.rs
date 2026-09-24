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
//!    run with a loud event and never restarts it.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
use tokio::sync::watch;
use tokio::task::JoinHandle;

use super::config::{Backend, DistributedConfig, Runner};
use super::exec::{NodeExec, SystemExec};
use super::launch::{self, RankProcess, RANK_MARKER};
use super::local_network;
use super::preflight::{self, now_ms, PreflightReport};
use super::probe::{self, Pressure, SseVerdict};
use super::{HANG_MEDIAN_MULTIPLE, HANG_MIN_SAMPLES};
use crate::{SidecarConfig, GIB, GRACE_TICK, GRACE_TICKS};

/// The supervisor's sampling cadence (memory, pressure, rank processes, the step counter) and the
/// rank wrapper's memory report cadence. It samples; it decides nothing by itself — the hang bound
/// is a multiple of the MEASURED progress intervals, which this cadence only quantises. Parity with
/// the MLX view's own 2 s status cadence.
const POLL_INTERVAL: Duration = Duration::from_secs(2);
/// Readiness polling tick (parity with `Sidecar::await_ready`'s 400 ms).
const READY_TICK: Duration = Duration::from_millis(400);
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
    /// A rank on THIS Mac died naming EHOSTUNREACH: macOS local network privacy (see
    /// `local_network`), not the cable.
    LocalNetworkBlocked,
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
            EventKind::LocalNetworkBlocked => "localNetworkBlocked",
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
    /// The launch plan's WITH-OVERHEAD figure (`RankPlan::with_overhead_bytes`) — the one the
    /// preflight compares with the budget and prints as "planned with overhead", so the node
    /// card and the preflight never show two different "planned" numbers.
    pub planned_bytes: Option<u64>,
    /// The caps the rank applied in-process (its own `GOOSE_RANK_CAPS` report): absent until the
    /// rank reported them.
    pub memory_limit_bytes: Option<u64>,
    pub wired_limit_bytes: Option<u64>,
    pub cache_limit_bytes: Option<u64>,
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
    pub liveness: Option<Liveness>,
    pub nodes: Vec<NodeStatus>,
    pub last_preflight: Option<PreflightReport>,
    pub events: Vec<EngineEvent>,
    pub restarts: u32,
    pub last_error: Option<String>,
    pub config: Option<DistributedConfig>,
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
            liveness: None,
            nodes: Vec::new(),
            last_preflight: None,
            events: Vec::new(),
            restarts: 0,
            last_error: None,
            config: None,
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
}

impl RefusalCode {
    pub fn as_str(self) -> &'static str {
        match self {
            RefusalCode::SingleEngineMounted => "singleEngineMounted",
            RefusalCode::AlreadyRunning => "alreadyRunning",
            RefusalCode::PreflightFailed => "preflightFailed",
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
async fn pid_alive(exec: &dyn NodeExec, host: Option<&str>, pid: u32) -> Result<bool> {
    let out = exec
        .run(host, &format!("/bin/ps -o pid=,stat=,time= -p {pid}"))
        .await?;
    if out.ssh_failed() {
        return Err(anyhow!("ssh failed: {}", out.stderr.trim()));
    }
    Ok(probe::parse_ps_row(&out.stdout)?.is_some_and(|row| !row.zombie()))
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
async fn signal_pid(exec: &dyn NodeExec, host: Option<&str>, pid: u32, signal: &str) -> String {
    match exec.run(host, &format!("/bin/kill -{signal} {pid}")).await {
        Ok(out) if out.success() => "sent".to_string(),
        Ok(out) => format!("kill exited {:?}: {}", out.status, out.stderr.trim()),
        Err(e) => format!("{e:#}"),
    }
}

async fn wait_child_exit(child: &mut tokio::process::Child) -> Option<std::process::ExitStatus> {
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
        let Some(pid) = rank.pid() else {
            report.steps.push(format!(
                "rank {} ({}): the peer never reported its pid; its ssh session is ended below and the node is swept for goose ranks",
                rank.rank, rank.node
            ));
            let swept = reclaim_marked_ranks(exec, host, &rank.node).await;
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
            let sent = signal_pid(exec, host, pid, "TERM").await;
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
                let sent = signal_pid(exec, host, pid, "KILL").await;
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

/// Every goose rank (by its command-line marker) on a node: SIGTERM, grace, SIGKILL — per pid.
/// The reclaim for ranks a previous goosed left behind (supervision state is in memory only).
async fn reclaim_marked_ranks(
    exec: &dyn NodeExec,
    host: Option<&str>,
    node: &str,
) -> (Vec<String>, bool) {
    let listing = match exec.run(host, "/bin/ps -axo pid=,command=").await {
        Ok(out) if !out.ssh_failed() => out.stdout,
        Ok(out) => {
            return (
                vec![format!("{node}: ssh failed: {}", out.stderr.trim())],
                false,
            )
        }
        Err(e) => return (vec![format!("{node}: {e:#}")], false),
    };
    let marked: Vec<u32> = probe::foreign_engine_processes(&listing, &[])
        .into_iter()
        .filter(|(_, cmd)| cmd.contains(RANK_MARKER))
        .map(|(pid, _)| pid)
        .collect();
    let mut steps = Vec::new();
    let mut verified = true;
    for pid in marked {
        let sent = signal_pid(exec, host, pid, "TERM").await;
        let mut gone = wait_gone(exec, host, pid).await;
        let mut line = format!("{node}: goose rank pid {pid} → SIGTERM ({sent})");
        if matches!(gone, Ok(None)) {
            let sent = signal_pid(exec, host, pid, "KILL").await;
            gone = wait_gone(exec, host, pid).await;
            line.push_str(&format!(" → SIGKILL ({sent})"));
        }
        let ok = matches!(gone, Ok(Some(_)));
        verified &= ok;
        line.push_str(if ok { " → gone" } else { " → STILL ALIVE" });
        steps.push(line);
    }
    (steps, verified)
}

struct RunContext {
    shared: Arc<StdMutex<Shared>>,
    exec: Arc<dyn NodeExec>,
    http: reqwest::Client,
    stream_http: reqwest::Client,
    config: DistributedConfig,
    served_id: String,
    runner: Runner,
}

impl RunContext {
    fn update(&self, f: impl FnOnce(&mut Shared)) {
        f(&mut self.shared.lock().unwrap());
    }

    fn event(&self, kind: EventKind, node: Option<&str>, message: impl Into<String>) {
        self.update(|s| s.event(kind, node, message));
    }

    fn set_node_states(&self, state: NodeState) {
        self.update(|s| s.status.nodes.iter_mut().for_each(|n| n.state = state));
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

/// A dead rank, named. A rank on THIS Mac whose last output names EHOSTUNREACH is the app's
/// Local Network privilege (the peers run under ssh, which macOS exempts), not a dead link.
fn rank_exit(
    rank: &RankProcess,
    status: std::process::ExitStatus,
    when: &str,
    after: &str,
) -> (EventKind, String) {
    let tail = rank.tail();
    let what = if rank.host.is_some() {
        "its ssh session ended"
    } else {
        "exited"
    };
    let message = format!(
        "rank {} {what}{when} ({status}){after} Last output:\n{tail}",
        rank.rank
    );
    if rank.host.is_none() && local_network::names_host_unreachable(&tail) {
        (
            EventKind::LocalNetworkBlocked,
            format!("{}: {message}", local_network::BLOCKED),
        )
    } else {
        (EventKind::RankDied, message)
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
                let (kind, message) = rank_exit(rank, status, " during startup", ".");
                return ReadyOutcome::Failed(kind, Some(rank.node.clone()), message);
            }
        }
        let pids: Vec<Option<u32>> = ranks.iter().map(RankProcess::pid).collect();
        ctx.update(|s| {
            for (node, pid) in s.status.nodes.iter_mut().zip(&pids) {
                node.pid = *pid;
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

fn sample_script(pid: Option<u32>) -> String {
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

async fn monitor(
    ctx: &RunContext,
    ranks: &mut [RankProcess],
    stop_rx: &mut watch::Receiver<bool>,
) -> RunOutcome {
    let mut meter = ProgressMeter::new(Instant::now(), ranks.len());
    let mut admission_closed = false;
    let mut blind_reported = vec![false; ranks.len()];
    loop {
        tokio::select! {
            _ = stop_rx.changed() => return RunOutcome::Stop,
            _ = tokio::time::sleep(POLL_INTERVAL) => {}
        }
        for rank in ranks.iter_mut() {
            if let Ok(Some(status)) = rank.child.try_wait() {
                let (kind, message) = rank_exit(rank, status, "", "; the pair cannot serve.");
                return RunOutcome::Failed(kind, Some(rank.node.clone()), message);
            }
        }
        let pids: Vec<Option<u32>> = ranks.iter().map(RankProcess::pid).collect();
        let samples =
            futures::future::join_all(ctx.config.nodes.iter().zip(&pids).map(|(node, pid)| {
                let exec = Arc::clone(&ctx.exec);
                let host = node.ssh.clone();
                let script = sample_script(*pid);
                async move { exec.run(host.as_deref(), &script).await }
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

        let mut cpu: Vec<Option<u64>> = vec![None; ranks.len()];
        let mut stats: Vec<Option<String>> = vec![None; ranks.len()];
        let mut worst = (Watchdog::Normal, String::new());
        for (rank, sample) in samples.into_iter().enumerate() {
            let node_name = ctx.config.nodes[rank].name.clone();
            let parsed = sample.and_then(|out| {
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
            });
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
            let (warn_ratio, critical_ratio) = ctx.config.watchdog_ratios();
            let verdict = watchdog_verdict(
                pressure,
                reading.available_bytes,
                reading.total_bytes,
                (warn_ratio, critical_ratio),
            );
            if verdict != Watchdog::Normal && verdict as u8 >= worst.0 as u8 {
                worst = (
                    verdict,
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
                    node.peak_bytes = Some(live.peak.max(node.peak_bytes.unwrap_or(0)));
                }
            });
        }

        let reading = meter.observe(Instant::now(), progress.as_ref().map(|p| p.steps), &cpu);
        let serving = progress.as_ref().is_some_and(|p| p.inflight > 0);
        ctx.update(|s| {
            s.status.inflight = progress.as_ref().map(|p| p.inflight);
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
                ctx.event(EventKind::WatchdogCritical, None, worst.1.clone());
                return RunOutcome::Critical(worst.1);
            }
            Watchdog::Warn if !admission_closed => {
                ctx.event(EventKind::WatchdogWarn, None, worst.1.clone());
                match set_admission(ctx, false, &worst.1).await {
                    Ok(()) => {
                        admission_closed = true;
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

/// The rank specs for the plan preflight approved: the tensor runner's per-rank bytes, or the
/// pipeline runner's pinned split. A preflight that carries neither cannot be launched.
fn launch_specs(
    ctx: &RunContext,
    preflight: &PreflightReport,
    context: u64,
) -> Result<Vec<launch::RankSpec>> {
    let report_seconds = POLL_INTERVAL.as_secs_f64();
    match ctx.runner {
        Runner::MlxLmTensor => {
            let bytes = preflight
                .launch_bytes()
                .ok_or_else(|| anyhow!("the preflight produced no per-rank plan"))?;
            Ok(launch::rank_specs(
                &ctx.config,
                &ctx.served_id,
                &bytes,
                context,
                report_seconds,
            ))
        }
        Runner::PipelineQwen4 => {
            let starts = preflight
                .pipeline_starts
                .as_deref()
                .ok_or_else(|| anyhow!("the preflight approved no pipeline split"))?;
            Ok(launch::pipeline_rank_specs(
                &ctx.config,
                &ctx.served_id,
                context,
                &super::plan::split_arg(starts),
                report_seconds,
            ))
        }
    }
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
            s.status.liveness = None;
            s.status.context_limit = Some(context);
            for (node, plan) in s.status.nodes.iter_mut().zip(&preflight.nodes) {
                node.state = NodeState::Loading;
                node.pid = None;
                node.planned_bytes = plan.plan.as_ref().map(|p| p.with_overhead_bytes);
            }
        });
        let mut ranks = Vec::new();
        let mut spawn_error = None;
        for (node, spec) in ctx.config.nodes.iter().zip(&specs) {
            match launch::spawn_rank(node, spec) {
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
                        match monitor(&ctx, &mut ranks, &mut stop_rx).await {
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
                // A refused Local Network privilege is the owner's click, not a transient: a
                // restart would only fail the same way.
                if !ctx.config.restart_on_failure || kind == EventKind::LocalNetworkBlocked {
                    ctx.update(|s| s.status.state = RunState::Failed);
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
                ctx.update(|s| s.status.state = RunState::Preflight);
                match preflight::run_preflight(&ctx.config, Arc::clone(&ctx.exec), true).await {
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

pub struct DistributedManager {
    shared: Arc<StdMutex<Shared>>,
    exec: Arc<dyn NodeExec>,
    control: tokio::sync::Mutex<()>,
}

impl Default for DistributedManager {
    fn default() -> Self {
        Self::new(Arc::new(SystemExec))
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
        }
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
        let report = preflight::run_preflight(config, Arc::clone(&self.exec), repair_link).await?;
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
    /// config.model_id)` over the single engine's saved settings — the caller reads them.
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
        let report = match preflight::run_preflight(&config, Arc::clone(&self.exec), true).await {
            Ok(report) => report,
            Err(e) => {
                let mut shared = self.shared.lock().unwrap();
                shared.status.state = RunState::Stopped;
                shared.status.last_error = Some(format!("{e:#}"));
                return Err(e);
            }
        };
        let mut shared = self.shared.lock().unwrap();
        for repair in &report.repairs {
            shared.event(EventKind::LinkRepaired, None, repair.clone());
        }
        shared.status.last_preflight = Some(report.clone());
        shared.status.runner = report.runner;
        if !report.ok {
            let message = report.failures().join("; ");
            shared.status.state = RunState::Stopped;
            shared.status.last_error = Some(format!("preflight refused the start: {message}"));
            shared.event(
                EventKind::StartFailed,
                None,
                format!("preflight: {message}"),
            );
            return Ok(StartOutcome::Refused {
                code: RefusalCode::PreflightFailed,
                message,
                preflight: Some(report),
            });
        }
        let Some(runner) = report.runner else {
            shared.status.state = RunState::Stopped;
            shared.status.last_error = Some("the preflight named no runner".to_string());
            return Ok(StartOutcome::Refused {
                code: RefusalCode::PreflightFailed,
                message: "the preflight named no runner".to_string(),
                preflight: Some(report),
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
                    planned_bytes: plan.map(|p| p.with_overhead_bytes),
                    memory_limit_bytes: None,
                    wired_limit_bytes: None,
                    cache_limit_bytes: None,
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
        };
        shared.stop_tx = Some(stop_tx);
        shared.task = Some(tokio::spawn(supervise(ctx, report.clone(), stop_rx)));
        Ok(StartOutcome::Started { preflight: report })
    }

    /// Stop the run and return the verified stop report. With no run supervised, the configured
    /// nodes are swept for goose ranks a previous goosed left behind (per pid, by marker).
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
        for node in &config.nodes {
            let (steps, verified) =
                reclaim_marked_ranks(self.exec.as_ref(), node.host(), &node.name).await;
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
        }
    }

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
}
