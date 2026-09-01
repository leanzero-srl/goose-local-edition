//! Supervisor for local inference engine sidecars.
//!
//! Owns the full lifecycle of an OpenAI-compatible engine process (Rapid-MLX, oMLX, …):
//! spawn from a configured argv, readiness by polling `/v1/models`, restart with capped
//! backoff behind a circuit breaker, and explicit termination that takes the WHOLE engine
//! tree the wrapper launched — never anything else. Startup and restart failures carry
//! the engine's stderr tail so a dead sidecar is a diagnosable event, not a silent absence.
//!
//! # Termination: SIGTERM per-pid, then a PROVEN group kill
//!
//! The launcher is `uvx`, a real parent: it forwards SIGTERM to the engine it spawned but
//! a SIGKILL to `uvx` alone orphans that engine on the port (measured 2026-09-01 — the
//! python child re-parents to pid 1 and keeps serving). So the two legs differ:
//!
//! - **SIGTERM goes to the child pid alone.** The wrapper forwards it and waits.
//! - **SIGKILL goes to the child's OWN process group, after a proof.** `configure_subprocess`
//!   spawns the child with `process_group(0)`, making it the leader of a fresh group that
//!   only its descendants inherit. The leg first proves `getpgid(pid) == pid` (and that the
//!   pid is not the caller's own group) and only then `killpg`s THAT group. A leader that
//!   died — zombie, reaped, or an orphan whose leader is gone — fails the proof (ESRCH or a
//!   mismatched pgid, both measured), so the group is never signalled on a guess; the port
//!   is then released per-pid from `lsof`'s LISTEN entries whose pgid is the engine's own.
//!
//! This is the REAPING gate's sanctioned shape (`kill_app_tree`: a tree kill on a group the
//! wrapper OWNS). What the gate forbids — and what SIGKILLed unrelated work before — is a
//! `killpg` on a group the caller shares or has not proven; that is why the proof is not
//! optional and why the SIGTERM leg stays per-pid.

pub mod engine;
pub mod hf;
mod memory;
mod subprocess;

pub use memory::{dir_size_bytes, disk_space, measure, GateResult, MemoryGate, Verdict, GIB};

use std::collections::{BTreeSet, VecDeque};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

const STDERR_TAIL_LINES: usize = 200;

/// The one grace window in this crate: SIGTERM → SIGKILL, and how long a released port
/// is waited for. Every other wait here is bounded by it; no second seconds-literal exists.
const GRACE_TICKS: u32 = 50;
const GRACE_TICK: Duration = Duration::from_millis(100);

#[derive(Debug, Clone)]
pub struct SidecarConfig {
    pub name: String,
    /// Full argv; element 0 is the binary.
    pub command: Vec<String>,
    pub env: Vec<(String, String)>,
    /// e.g. "http://127.0.0.1:8090" — readiness and health poll GET {base_url}/v1/models.
    pub base_url: String,
    /// The id `/v1/models` must report at `data[0].id` before a 200 counts as ready or
    /// healthy. A 200 from some OTHER engine on the same port — an orphan of a previous
    /// goosed answering while this child is still resolving — is not readiness.
    pub expected_model_id: String,
    /// The startup terminator is PROGRESS, not a clock: while the child tree keeps making
    /// progress — a stderr line, CPU time, memory, or a pid appearing or leaving — the
    /// start waits as long as it takes (a cold uv resolve of the pinned fork, a 5.6 GB
    /// safetensors load). This is the longest ZERO-progress interval tolerated before the
    /// start is declared failed with the stderr tail. Measured 2026-09-01: a legitimate
    /// warm load of the 9B model held one 4.91 s zero-progress interval (uv alone, silent,
    /// before it spawned python), so this must sit well above single-digit seconds.
    pub startup_stall_window: Duration,
    pub restart_window: Duration,
    pub max_restarts_in_window: u32,
    pub backoff_initial: Duration,
    pub backoff_cap: Duration,
}

impl SidecarConfig {
    pub fn new(
        name: impl Into<String>,
        command: Vec<String>,
        base_url: impl Into<String>,
        expected_model_id: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            command,
            env: Vec::new(),
            base_url: base_url.into(),
            expected_model_id: expected_model_id.into(),
            startup_stall_window: Duration::from_secs(180),
            restart_window: Duration::from_secs(600),
            max_restarts_in_window: 3,
            backoff_initial: Duration::from_secs(1),
            backoff_cap: Duration::from_secs(30),
        }
    }
}

struct ChildHandle {
    child: Child,
    stderr_tail: Arc<StdMutex<VecDeque<String>>>,
    stderr_lines: Arc<AtomicU64>,
}

struct State {
    handle: Option<ChildHandle>,
    restarts: VecDeque<Instant>,
    backoff: Duration,
}

pub struct Sidecar {
    config: SidecarConfig,
    client: reqwest::Client,
    state: Mutex<State>,
}

impl Sidecar {
    /// Spawn the engine and wait until `/v1/models` serves the expected id. Bounded by
    /// progress (see `startup_stall_window`), never by a clock on the load itself.
    pub async fn start(config: SidecarConfig) -> Result<Self> {
        anyhow::ensure!(!config.command.is_empty(), "sidecar command is empty");
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()?;
        let backoff = config.backoff_initial;
        let sidecar = Self {
            config,
            client,
            state: Mutex::new(State {
                handle: None,
                restarts: VecDeque::new(),
                backoff,
            }),
        };
        {
            let mut state = sidecar.state.lock().await;
            let handle = sidecar.spawn_child()?;
            sidecar.await_ready(&mut state, handle).await?;
        }
        Ok(sidecar)
    }

    pub fn base_url(&self) -> &str {
        &self.config.base_url
    }

    pub async fn pid(&self) -> Option<u32> {
        self.state
            .lock()
            .await
            .handle
            .as_ref()
            .and_then(|h| h.child.id())
    }

    pub async fn healthy(&self) -> bool {
        self.probe().await.is_ok()
    }

    /// Restart the engine if its process died or it stops answering. Errors once the
    /// circuit breaker trips (too many restarts inside the window), carrying stderr.
    pub async fn ensure_running(&self) -> Result<()> {
        let mut state = self.state.lock().await;

        let process_dead = match state.handle.as_mut() {
            None => true,
            Some(h) => h.child.try_wait().context("try_wait on sidecar")?.is_some(),
        };
        let unhealthy = if process_dead {
            "process exited".to_string()
        } else {
            match self.probe().await {
                Ok(()) => {
                    state.backoff = self.config.backoff_initial;
                    return Ok(());
                }
                Err(reason) => reason.to_string(),
            }
        };

        let tail = state
            .handle
            .as_ref()
            .map(|h| stderr_tail_string(&h.stderr_tail))
            .unwrap_or_default();
        tracing::warn!(
            sidecar = %self.config.name,
            process_dead,
            "sidecar unhealthy ({unhealthy}); restarting. stderr tail:\n{tail}"
        );

        if let Some(mut h) = state.handle.take() {
            let owned_group = terminate(&mut h.child).await;
            self.release_port(owned_group).await;
        }

        let now = Instant::now();
        while let Some(front) = state.restarts.front() {
            if now.duration_since(*front) > self.config.restart_window {
                state.restarts.pop_front();
            } else {
                break;
            }
        }
        if state.restarts.len() as u32 >= self.config.max_restarts_in_window {
            bail!(
                "sidecar '{}' circuit breaker open: {} restarts within {:?}. Last stderr:\n{}",
                self.config.name,
                state.restarts.len(),
                self.config.restart_window,
                tail
            );
        }
        state.restarts.push_back(now);

        let backoff = state.backoff;
        state.backoff = (state.backoff * 2).min(self.config.backoff_cap);
        tokio::time::sleep(backoff).await;

        let handle = self.spawn_child()?;
        self.await_ready(&mut state, handle).await
    }

    /// SIGTERM to the child pid, the grace window, then SIGKILL to the child's PROVEN own
    /// process group (see the crate doc); afterwards the listen port is waited for and any
    /// residue of that same group is terminated per-pid, so a re-mount finds the port free.
    pub async fn shutdown(&self) {
        let mut state = self.state.lock().await;
        if let Some(mut h) = state.handle.take() {
            let owned_group = terminate(&mut h.child).await;
            self.release_port(owned_group).await;
        }
    }

    fn spawn_child(&self) -> Result<ChildHandle> {
        let mut cmd = Command::new(&self.config.command[0]);
        cmd.args(&self.config.command[1..])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        for (k, v) in &self.config.env {
            cmd.env(k, v);
        }
        subprocess::configure_subprocess(&mut cmd);
        let mut child = cmd.spawn().with_context(|| {
            format!(
                "failed to spawn sidecar '{}' ({})",
                self.config.name, self.config.command[0]
            )
        })?;

        let stderr_tail = Arc::new(StdMutex::new(VecDeque::with_capacity(STDERR_TAIL_LINES)));
        let stderr_lines = Arc::new(AtomicU64::new(0));
        if let Some(stderr) = child.stderr.take() {
            let tail = Arc::clone(&stderr_tail);
            let count = Arc::clone(&stderr_lines);
            let name = self.config.name.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    tracing::debug!(sidecar = %name, "{line}");
                    count.fetch_add(1, Ordering::Relaxed);
                    let mut tail = tail.lock().unwrap();
                    if tail.len() == STDERR_TAIL_LINES {
                        tail.pop_front();
                    }
                    tail.push_back(line);
                }
            });
        }
        Ok(ChildHandle {
            child,
            stderr_tail,
            stderr_lines,
        })
    }

    /// Wait for readiness with a PROGRESS terminator: the child exiting is an immediate loud
    /// failure; otherwise the start continues for as long as the child tree keeps changing,
    /// and fails only after `startup_stall_window` of NO change — no stderr line, no CPU
    /// time, no memory movement, no pid joining or leaving — with the last probe reason and
    /// the stderr tail. A slow load that is working is never declared failed by a clock.
    async fn await_ready(&self, state: &mut State, mut handle: ChildHandle) -> Result<()> {
        let mut sys = System::new();
        let mut last_mark = progress_mark(&mut sys, &handle);
        let mut last_progress = Instant::now();
        loop {
            if let Some(status) = handle.child.try_wait().context("try_wait during startup")? {
                let tail = stderr_tail_string(&handle.stderr_tail);
                bail!(
                    "sidecar '{}' exited during startup ({status}). stderr:\n{tail}",
                    self.config.name
                );
            }
            let mark = progress_mark(&mut sys, &handle);
            let not_ready = match self.probe().await {
                Ok(()) => {
                    state.handle = Some(handle);
                    tracing::info!(sidecar = %self.config.name, base_url = %self.config.base_url, "sidecar ready");
                    return Ok(());
                }
                Err(NotReady::OtherId(served)) => {
                    // A catalog with another id is decisive when OUR OWN tree is the listener:
                    // the engine ignored the alias, and every probe it answers would count as
                    // progress forever. A listener outside the tree is someone else's engine;
                    // our child then dies on its bind and the exit above reports it.
                    if self.port_served_by_tree(&mark).await {
                        let tail = stderr_tail_string(&handle.stderr_tail);
                        let owned_group = terminate(&mut handle.child).await;
                        self.release_port(owned_group).await;
                        bail!(
                            "sidecar '{}' serves '{served}', expected '{}' — its own listener \
                             answered, so the engine ignored the served model name. stderr:\n{tail}",
                            self.config.name,
                            self.config.expected_model_id
                        );
                    }
                    format!(
                        "{}/v1/models serves '{served}', expected '{}' — a listener outside this \
                         sidecar's process tree holds the port",
                        self.config.base_url, self.config.expected_model_id
                    )
                }
                Err(NotReady::Unanswered(reason)) => reason,
            };
            if mark != last_mark {
                last_mark = mark;
                last_progress = Instant::now();
            } else if last_progress.elapsed() >= self.config.startup_stall_window {
                let stalled_for = last_progress.elapsed();
                let tail = stderr_tail_string(&handle.stderr_tail);
                let owned_group = terminate(&mut handle.child).await;
                self.release_port(owned_group).await;
                bail!(
                    "sidecar '{}' stalled during startup: no progress for {stalled_for:?} \
                     (no stderr, no CPU time, no memory change across {} process(es)); \
                     last probe: {not_ready}. stderr:\n{tail}",
                    self.config.name,
                    mark.tree.len()
                );
            }
            tokio::time::sleep(Duration::from_millis(400)).await;
        }
    }

    /// Ready/healthy means `/v1/models` answers 200 AND `data[0].id` is the expected id.
    /// The `Err` carries why not, so a startup that never gets there says which it was:
    /// nothing listening, a non-2xx, a catalog without an id, or another id on our port.
    async fn probe(&self) -> std::result::Result<(), NotReady> {
        let url = format!("{}/v1/models", self.config.base_url);
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| NotReady::Unanswered(format!("GET {url}: {e}")))?;
        let status = resp.status();
        let body = resp
            .text()
            .await
            .map_err(|e| NotReady::Unanswered(format!("reading {url} body: {e}")))?;
        if !status.is_success() {
            return Err(NotReady::Unanswered(format!(
                "GET {url} returned HTTP {status}"
            )));
        }
        let served = serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|v| {
                v.get("data")?
                    .get(0)?
                    .get("id")?
                    .as_str()
                    .map(str::to_string)
            });
        match served {
            Some(id) if id == self.config.expected_model_id => Ok(()),
            Some(id) => Err(NotReady::OtherId(id)),
            None => Err(NotReady::Unanswered(format!(
                "{url} answered {status} without a data[0].id: {}",
                body.chars().take(200).collect::<String>()
            ))),
        }
    }

    fn listen_port(&self) -> Option<u16> {
        reqwest::Url::parse(&self.config.base_url)
            .ok()
            .and_then(|u| u.port_or_known_default())
    }

    /// Whether a LISTEN socket on our port belongs to a pid in the child's sampled tree.
    async fn port_served_by_tree(&self, mark: &ProgressMark) -> bool {
        let Some(port) = self.listen_port() else {
            return false;
        };
        match listening_pids(port).await {
            Ok(listeners) => listeners
                .iter()
                .any(|pid| mark.tree.iter().any(|(member, _, _)| member == pid)),
            Err(e) => {
                tracing::warn!(port, error = %e, "cannot attribute the listener; lsof unavailable");
                false
            }
        }
    }

    /// After the child is gone, wait (the grace window) for the listen port to clear. What
    /// still listens afterwards is either RESIDUE of the engine's own process group — the
    /// wrapper died without forwarding, its engine kept the socket — which is terminated
    /// per-pid, or a listener outside that group, which is NOT ours and is left alone and
    /// logged (the manager refuses to mount over it; an explicit unmount reclaims it).
    async fn release_port(&self, owned_group: Option<u32>) {
        let Some(port) = self.listen_port() else {
            tracing::warn!(
                sidecar = %self.config.name,
                base_url = %self.config.base_url,
                "base_url carries no port; cannot verify the port was released"
            );
            return;
        };
        if wait_port_clear(port).await {
            return;
        }
        let listeners = match listening_pids(port).await {
            Ok(pids) => pids,
            Err(e) => {
                tracing::warn!(port, error = %e, "port still occupied and lsof unavailable");
                return;
            }
        };
        let Some(group) = owned_group else {
            tracing::warn!(
                port,
                ?listeners,
                "port still occupied by a listener this sidecar never owned; left alone"
            );
            return;
        };
        let (residue, foreign): (Vec<u32>, Vec<u32>) = listeners
            .into_iter()
            .partition(|pid| process_group_of(*pid) == Some(group));
        if !foreign.is_empty() {
            tracing::warn!(
                port,
                ?foreign,
                group,
                "listeners outside the engine's process group hold the port; left alone"
            );
        }
        if residue.is_empty() {
            return;
        }
        tracing::warn!(
            port,
            ?residue,
            group,
            "engine residue still listens after the wrapper exited; SIGTERM per-pid"
        );
        signal_each(&residue, libc::SIGTERM);
        if wait_port_clear(port).await {
            return;
        }
        tracing::warn!(port, ?residue, "grace expired; SIGKILL per-pid");
        signal_each(&residue, libc::SIGKILL);
        if !wait_port_clear(port).await {
            tracing::warn!(
                port,
                "port still occupied after reclaiming the engine's residue"
            );
        }
    }
}

impl Drop for Sidecar {
    fn drop(&mut self) {
        if let Ok(mut state) = self.state.try_lock() {
            if let Some(h) = state.handle.as_mut() {
                sigkill_tree_or_pid(&mut h.child);
            }
        }
        // kill_on_drop(true) covers the path where the lock is held elsewhere; that leg
        // reaches the pid alone.
    }
}

/// SIGTERM the child pid, wait the grace window, then SIGKILL its proven own group (or the
/// pid alone when the proof fails). Returns the pid the termination operated on — the id of
/// the group its descendants live in — captured BEFORE reaping, since `Child::id` is `None`
/// once the child is waited.
async fn terminate(child: &mut Child) -> Option<u32> {
    let pid = child.id();
    #[cfg(unix)]
    if let Some(pid) = pid {
        unsafe {
            libc::kill(pid as libc::pid_t, libc::SIGTERM);
        }
        for _ in 0..GRACE_TICKS {
            if let Ok(Some(_)) = child.try_wait() {
                return Some(pid);
            }
            tokio::time::sleep(GRACE_TICK).await;
        }
    }
    sigkill_tree_or_pid(child);
    let _ = child.wait().await;
    pid
}

/// The SIGKILL leg: the child's own process group when the proof holds, else the pid alone.
fn sigkill_tree_or_pid(child: &mut Child) {
    #[cfg(unix)]
    if let Some(pid) = child.id() {
        if sigkill_owned_group(pid) {
            return;
        }
        tracing::warn!(
            pid,
            "SIGKILL leg: child is not a live leader of its own process group; signalling the pid alone"
        );
    }
    let _ = child.start_kill();
}

/// The proof behind every group kill in this crate: `pid` is the LIVE leader of its own
/// process group (`getpgid(pid) == pid`) and that group is not the caller's. A dead leader
/// (zombie or reaped) answers ESRCH; an orphan whose leader died carries the dead leader's
/// pgid, not its own — both fail here.
#[cfg(unix)]
pub fn owns_process_group(pid: u32) -> bool {
    let pid = pid as libc::pid_t;
    let own_group = unsafe { libc::getpgrp() };
    pid != own_group && unsafe { libc::getpgid(pid) } == pid
}

/// `killpg(pid, SIGKILL)` only when `owns_process_group(pid)` proves the group is the
/// child's own. Returns whether the group was signalled; `false` means nothing was.
#[cfg(unix)]
pub fn sigkill_owned_group(pid: u32) -> bool {
    if !owns_process_group(pid) {
        return false;
    }
    unsafe { libc::killpg(pid as libc::pid_t, libc::SIGKILL) == 0 }
}

#[cfg(unix)]
fn process_group_of(pid: u32) -> Option<u32> {
    let pgid = unsafe { libc::getpgid(pid as libc::pid_t) };
    (pgid > 0).then_some(pgid as u32)
}

#[cfg(unix)]
fn signal_each(pids: &[u32], signal: libc::c_int) {
    for pid in pids {
        unsafe { libc::kill(*pid as libc::pid_t, signal) };
    }
}

pub(crate) fn port_has_listener(port: u16) -> bool {
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(200)).is_ok()
}

/// Poll the port for the grace window; `true` once nothing accepts on it.
pub(crate) async fn wait_port_clear(port: u16) -> bool {
    for _ in 0..GRACE_TICKS {
        if !port_has_listener(port) {
            return true;
        }
        tokio::time::sleep(GRACE_TICK).await;
    }
    !port_has_listener(port)
}

/// Pids with a LISTEN socket on `port`. `-sTCP:LISTEN` is load-bearing: a bare `lsof -i :port`
/// also lists every process holding a CLIENT connection to the port — goosed's own keep-alive
/// pool included (measured 2026-09-01: the connected client pid appeared alongside the
/// listener) — and signalling that list would have signalled the caller.
pub(crate) async fn listening_pids(port: u16) -> Result<Vec<u32>> {
    let output = tokio::process::Command::new("lsof")
        .args(["-ti", &format!("TCP:{port}"), "-sTCP:LISTEN"])
        .output()
        .await
        .context("lsof")?;
    Ok(String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .filter_map(|s| s.parse().ok())
        .collect())
}

/// Why `/v1/models` did not count as ready. `OtherId` is kept apart because it is the one
/// answer whose meaning depends on WHO answered (see `await_ready`).
enum NotReady {
    Unanswered(String),
    OtherId(String),
}

impl std::fmt::Display for NotReady {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NotReady::Unanswered(reason) => f.write_str(reason),
            NotReady::OtherId(served) => write!(f, "/v1/models serves '{served}'"),
        }
    }
}

/// One sample of "is the start still doing something": stderr lines received so far, and
/// for every process in the child's tree (the child and its descendants by parent link —
/// uv AND the engine it launches) the accumulated CPU time and resident memory. Any change
/// between two samples is progress.
#[derive(PartialEq, Eq)]
struct ProgressMark {
    stderr_lines: u64,
    tree: Vec<(u32, u64, u64)>,
}

fn progress_mark(sys: &mut System, handle: &ChildHandle) -> ProgressMark {
    ProgressMark {
        stderr_lines: handle.stderr_lines.load(Ordering::Relaxed),
        tree: handle
            .child
            .id()
            .map(|pid| process_tree_sample(sys, pid))
            .unwrap_or_default(),
    }
}

fn process_tree_sample(sys: &mut System, root: u32) -> Vec<(u32, u64, u64)> {
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_cpu().with_memory(),
    );
    let mut members = BTreeSet::from([Pid::from_u32(root)]);
    loop {
        let before = members.len();
        for (pid, process) in sys.processes() {
            if process
                .parent()
                .is_some_and(|parent| members.contains(&parent))
            {
                members.insert(*pid);
            }
        }
        if members.len() == before {
            break;
        }
    }
    members
        .iter()
        .filter_map(|pid| {
            sys.process(*pid)
                .map(|p| (pid.as_u32(), p.accumulated_cpu_time(), p.memory()))
        })
        .collect()
}

fn stderr_tail_string(tail: &Arc<StdMutex<VecDeque<String>>>) -> String {
    tail.lock()
        .map(|t| t.iter().cloned().collect::<Vec<_>>().join("\n"))
        .unwrap_or_default()
}

/// Convenience: gate a model mount against live memory before asking the engine to load.
pub fn gate_mount(model_bytes: u64, gate: &MemoryGate) -> GateResult {
    let (available, total) = measure();
    gate.evaluate(model_bytes, available, total)
}

pub fn mount_block_error(result: &GateResult) -> Option<anyhow::Error> {
    match result.verdict {
        Verdict::Block => Some(anyhow!("memory gate BLOCK: {}", result.message)),
        _ => None,
    }
}
