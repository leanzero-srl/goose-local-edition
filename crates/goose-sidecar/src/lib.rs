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

// The distributed engine drives ranks over `/bin/sh`, `ssh` and POSIX signals; it has no
// Windows shape, and its ACP surface answers that platform with a named refusal instead.
#[cfg(unix)]
pub mod distributed;
pub mod engine;
pub mod fit;
pub mod hf;
#[cfg(unix)]
pub mod holders;
pub mod kv_cache;
pub mod machine;
mod memory;
pub mod model_identity;
mod model_parsers;
#[cfg(unix)]
pub mod placement;
pub mod port_holder;
mod subprocess;
pub mod thinking;

pub use fit::{FitVerdict, Verdict, GIB};
pub use memory::{dir_size_bytes, disk_space, measure, MemoryReading};
pub use port_holder::{sidecar_marker, PortHeld, PortHolder, SIDECAR_MARKER_ENV};

use std::collections::{BTreeSet, VecDeque};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

const STDERR_TAIL_LINES: usize = 200;

/// The one grace window in this crate: SIGTERM → SIGKILL, and how long a released port
/// is waited for. Every other wait here is bounded by it; no second seconds-literal exists. goose's
/// stdio-extension teardown (`goose::agents::stdio_children`, Q-138) reuses it rather than minting one.
pub const GRACE_TICKS: u32 = 50;
pub const GRACE_TICK: Duration = Duration::from_millis(100);

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
    /// Where a start publishes what it has seen so far, for whoever reports the start.
    pub startup_watch: Option<Arc<StartupWatch>>,
    /// How the owner ends this start before it serves (an Unmount while the model loads).
    pub start_cancel: Option<Arc<StartCancel>>,
    /// Recognizes the engine's OWN line saying it began shutting down — the moment its listener
    /// closes while in-flight requests drain (Q-258: uvicorn closed the port at a SIGTERM and the
    /// process lived 6 more minutes finishing a prefill, with no line in goosed's log). `None`: the
    /// engine's shutdown is seen only at its exit.
    pub shutdown_line: Option<fn(&str) -> bool>,
    /// Where every stderr line of each spawned child is kept, stamped, in a file of its own
    /// (`<name>-<millis>.log`, bounded like the rank logs — `distributed::rank_log`). `None`: the
    /// stderr lives only in the in-memory tail. Q-423: the Studio's single engine logged the
    /// exception that ended a 191k-token answer mid-stream, and the only copy was that tail.
    pub log_dir: Option<std::path::PathBuf>,
}

/// Ends a start in flight — [`Sidecar::start`], or a supervised restart through
/// [`Sidecar::ensure_running_unless`]. The start stops its child the way [`Sidecar::shutdown`]
/// does (SIGTERM to the pid, the grace window, then SIGKILL to its PROVEN own group), releases
/// the port, and fails with [`StartCancelled`]. A load is ended by its owner, never by a clock:
/// Q-112's switch asked the Studio to unmount a 27B mid-load, and the load ran on — holding the
/// Mac's load lock — until it served, then was shut down.
#[derive(Debug)]
pub struct StartCancel {
    cancelled: tokio::sync::watch::Sender<bool>,
}

impl Default for StartCancel {
    fn default() -> Self {
        Self {
            cancelled: tokio::sync::watch::Sender::new(false),
        }
    }
}

impl StartCancel {
    pub fn cancel(&self) {
        self.cancelled.send_replace(true);
    }

    pub fn is_cancelled(&self) -> bool {
        *self.cancelled.borrow()
    }

    async fn cancelled(&self) {
        let mut seen = self.cancelled.subscribe();
        // The sender lives in `self`, so the channel cannot close under this wait.
        let _ = seen.wait_for(|cancelled| *cancelled).await;
    }
}

/// A start its owner ended ([`StartCancel`]) before the engine served.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartCancelled {
    pub name: String,
    /// The engine process the cancel stopped; `None` when it ended before one was spawned.
    pub pid: Option<u32>,
}

impl std::fmt::Display for StartCancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.pid {
            Some(pid) => write!(
                f,
                "sidecar '{}' was stopped while it loaded (pid {pid}), before it served",
                self.name
            ),
            None => write!(
                f,
                "sidecar '{}' was stopped before its engine was spawned",
                self.name
            ),
        }
    }
}

impl std::error::Error for StartCancelled {}

/// What a starting engine has shown so far: the resident bytes of the largest process in the
/// child's tree (the engine itself — its `uv` launcher stays a few MiB) and the engine's last
/// stderr lines. Measured 2026-09-24 on a warm 27B Q8 load (Rapid-MLX v0.14.3-lz.4): that
/// process's RSS rose 0.18 → 31.5 GB over the ~5 s between "Loading MLLM" and "MLLM loaded", ending
/// at 0.985 × the model's bytes on disk (0.994 once ready) — so resident ÷ on-disk IS the load's
/// progress, and nothing else is.
#[derive(Debug, Default)]
pub struct StartupWatch {
    seen: StdMutex<StartupSeen>,
    /// Names the phase a stderr tail shows; with it, each phase's first sighting is marked.
    phase_of: Option<fn(&[String]) -> &'static str>,
    /// Each phase the start showed, with the instant it was first seen — a load's phase times
    /// (design §6.4 step 10): the empty tail's phase at the spawn, every other one by the child's
    /// stderr reader at the line that showed it. Q-275: marked only at the startup loop's 400 ms
    /// looks, a phase superseded between two looks was never marked — CI read a restart's Loading
    /// as absent. A measurement for display, never a decision.
    phase_marks: StdMutex<Vec<(&'static str, Instant)>>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StartupSeen {
    pub resident_bytes: Option<u64>,
    pub stderr_tail: Vec<String>,
}

impl StartupWatch {
    /// A watch that marks when each phase `phase_of` names first shows in the stderr tail.
    pub fn with_phases(phase_of: fn(&[String]) -> &'static str) -> Self {
        Self {
            phase_of: Some(phase_of),
            ..Self::default()
        }
    }

    pub fn seen(&self) -> StartupSeen {
        self.seen.lock().unwrap().clone()
    }

    /// The phases seen so far, in order, each with its first sighting.
    pub fn phase_marks(&self) -> Vec<(&'static str, Instant)> {
        self.phase_marks.lock().unwrap().clone()
    }

    /// Marks the phase `tail` shows, now, unless it was already seen.
    fn mark_phase(&self, tail: &[String]) {
        let Some(phase_of) = self.phase_of else {
            return;
        };
        let phase = phase_of(tail);
        let mut marks = self.phase_marks.lock().unwrap();
        if !marks.iter().any(|(seen_phase, _)| *seen_phase == phase) {
            marks.push((phase, Instant::now()));
        }
    }

    fn publish(&self, mark: &ProgressMark, handle: &ChildHandle) {
        let seen = StartupSeen {
            resident_bytes: mark.tree.iter().map(|(_, _, memory)| *memory).max(),
            stderr_tail: handle.stderr_tail.lock().unwrap().iter().cloned().collect(),
        };
        *self.seen.lock().unwrap() = seen;
    }
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
            startup_watch: None,
            start_cancel: None,
            shutdown_line: None,
            log_dir: None,
        }
    }
}

struct ChildHandle {
    child: Child,
    /// Captured at spawn: once the exit is reaped `Child::id` answers `None`, and the exit
    /// report must still name which process died.
    pid: Option<u32>,
    stderr_tail: Arc<StdMutex<VecDeque<String>>>,
    stderr_lines: Arc<AtomicU64>,
    /// What the engine logged as an error, and how far its stderr has been read (Q-423).
    progress: Arc<StderrProgress>,
    /// The task filling `stderr_tail`; it ends at the pipe's EOF, which the kernel delivers once
    /// every process holding the write end has exited. `None` once it has been awaited.
    stderr_reader: Option<tokio::task::JoinHandle<()>>,
    /// Which goose path signalled this child, written BEFORE the signal; `None` while no goose
    /// path has. The shutdown and exit reports read it, so an engine that ends with `None` here
    /// was ended by something outside this goosed (Q-258).
    stopped_by: Arc<StdMutex<Option<String>>>,
    /// The engine's own shutdown announcement (`SidecarConfig::shutdown_line`), once seen.
    shutting_down: Arc<StdMutex<Option<String>>>,
    /// The exit watch's account of how the engine ended, once it has (`exit_report`).
    exit_report: Arc<StdMutex<Option<String>>>,
}

impl ChildHandle {
    /// Names the goose path about to signal this child; call it before the signal.
    fn stopping(&self, by: impl Into<String>) {
        *self.stopped_by.lock().unwrap() = Some(by.into());
    }

    /// The stderr tail of a child that has EXITED (the caller reaped it), with every line it
    /// wrote. A child that prints its error and exits at once can be reaped before the reader
    /// task has taken those lines off the pipe (CI, 2026-09-28: "exited during startup (exit
    /// status: 3). stderr:" with the words missing), so the tail waits for the reader's EOF —
    /// the pipe closing is the event, there is no clock. EOF comes only when no writer is left:
    /// when members of the child's process group (its descendants inherit the pipe) still live,
    /// waiting could park forever behind an orphaned engine, so the tail is returned as read so
    /// far and SAYS so.
    async fn last_words(&mut self) -> String {
        if let Some(reader) = self.stderr_reader.as_mut() {
            if let Some(holders) = stderr_holders_left(self.pid) {
                return format!(
                    "{}\n({holders}; these are the lines read so far, its last ones may be missing)",
                    stderr_tail_string(&self.stderr_tail)
                );
            }
            let ended = reader.await;
            self.stderr_reader = None;
            if let Err(e) = ended {
                return format!(
                    "{}\n(the stderr reader ended abnormally, so lines may be missing: {e})",
                    stderr_tail_string(&self.stderr_tail)
                );
            }
        }
        stderr_tail_string(&self.stderr_tail)
    }
}

/// Who, besides the exited child, may still hold its stderr pipe open — `None` when provably
/// nobody. The child spawns as the leader of its own process group (`configure_subprocess`), and
/// its descendants inherit both the group and the pipe; `killpg(group, 0)` answering ESRCH proves
/// the group empty, so the reader's EOF is due.
#[cfg(unix)]
fn stderr_holders_left(pid: Option<u32>) -> Option<String> {
    let Some(pid) = pid else {
        return Some("the exited child's pid was never known, so its process group cannot be checked for other stderr holders".to_string());
    };
    if unsafe { libc::killpg(pid as libc::pid_t, 0) } == 0 {
        return Some(format!(
            "process group {pid} still has live members after its leader exited, and they hold the stderr pipe open"
        ));
    }
    let refused = std::io::Error::last_os_error();
    match refused.raw_os_error() {
        Some(libc::ESRCH) => None,
        _ => Some(format!(
            "process group {pid} could not be checked for other stderr holders: {refused}"
        )),
    }
}

/// Off Unix there is no process group to prove the pipe's other holders gone, and a descendant
/// that inherited the pipe would hold the reader's EOF back indefinitely.
#[cfg(not(unix))]
fn stderr_holders_left(_pid: Option<u32>) -> Option<String> {
    Some("off Unix the exited child's descendants cannot be proven gone, so its stderr is not waited to EOF".to_string())
}

struct State {
    handle: Option<ChildHandle>,
    restarts: VecDeque<Instant>,
    backoff: Duration,
}

/// The engine process ended while supervised — observed (and reaped) by [`Sidecar::exited`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SidecarExit {
    pub pid: Option<u32>,
    /// `exit status: N` or `signal: 9 (SIGKILL)` — the OS's own words.
    pub status: String,
    pub stderr_tail: String,
    /// The goose path that stopped it; `None`: no goose path did — it ended on its own or by a
    /// signal from outside this goosed.
    pub stopped_by: Option<String>,
    /// The exit watch's account, written the moment the process ended (with this Mac's memory
    /// then); `None` when the watch did not see the exit (see `watch_exit`).
    pub exit_report: Option<String>,
}

/// What [`Sidecar::ensure_running_unless`] did: the engine answered and was kept, or it was
/// restarted — a load, which the caller measures like a fresh start (Q-256).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ensured {
    Healthy,
    Restarted,
}

/// A supervised restart's LOAD failed — its port claim, spawn or readiness — as opposed to a
/// refusal before any load began (the circuit breaker, a cancel during the backoff). Carried as
/// context on the error, so the engine's own words stay underneath.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RestartLoadFailed;

impl std::fmt::Display for RestartLoadFailed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the engine's restart failed to load")
    }
}

pub struct Sidecar {
    config: SidecarConfig,
    client: reqwest::Client,
    state: Mutex<State>,
    /// The cancel the start or restart in flight answers to; `None` while none is.
    cancel: StdMutex<Option<Arc<StartCancel>>>,
    /// Where the restart in flight publishes its start, in place of `config.startup_watch` — the
    /// mount that asked for it shows and measures ITS load, not the first start's (Q-256).
    watch: StdMutex<Option<Arc<StartupWatch>>>,
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
        let cancel = config.start_cancel.clone();
        let sidecar = Self {
            config,
            client,
            state: Mutex::new(State {
                handle: None,
                restarts: VecDeque::new(),
                backoff,
            }),
            cancel: StdMutex::new(cancel),
            watch: StdMutex::new(None),
        };
        {
            let mut state = sidecar.state.lock().await;
            sidecar.refuse_if_cancelled()?;
            sidecar.claim_port().await?;
            let handle = sidecar.spawn_child()?;
            sidecar.await_ready(&mut state, handle).await?;
        }
        *sidecar.cancel.lock().unwrap() = None;
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

    /// Whether the supervised process has ENDED, asked of the OS now (`try_wait`, which also
    /// reaps it — a dead engine is never left a zombie of goosed). `None` while it runs or when
    /// nothing is supervised. Nothing restarts here: the caller names the state.
    pub async fn exited(&self) -> Result<Option<SidecarExit>> {
        let mut state = self.state.lock().await;
        let Some(handle) = state.handle.as_mut() else {
            return Ok(None);
        };
        let Some(status) = handle.child.try_wait().context("try_wait on sidecar")? else {
            return Ok(None);
        };
        let stopped_by = handle.stopped_by.lock().unwrap().clone();
        let exit_report = handle.exit_report.lock().unwrap().clone();
        Ok(Some(SidecarExit {
            pid: handle.pid,
            status: status.to_string(),
            stderr_tail: handle.last_words().await,
            stopped_by,
            exit_report,
        }))
    }

    /// The engine's own announcement that it began shutting down, while it still runs: its port
    /// no longer accepts and in-flight requests drain. `None` when it has not announced one.
    pub async fn shutting_down(&self) -> Option<String> {
        let state = self.state.lock().await;
        let handle = state.handle.as_ref()?;
        let line = handle.shutting_down.lock().unwrap().clone()?;
        let stopped_by = handle.stopped_by.lock().unwrap().clone();
        Some(match stopped_by {
            Some(by) => format!("the engine is shutting down ({by}): {line}"),
            None => format!(
                "the engine is shutting down and no goose path signalled it — the signal came from \
                 outside this goosed; its port no longer accepts while in-flight requests drain. \
                 Its own words: {line}"
            ),
        })
    }

    pub async fn healthy(&self) -> bool {
        self.probe().await.is_ok()
    }

    /// Where the running child's logged errors stand now — taken as a request is sent (Q-423).
    /// `None` while no child is settled: nothing runs, or a start or restart holds the supervisor.
    pub fn error_mark(&self) -> Option<ErrorMark> {
        let state = self.state.try_lock().ok()?;
        let handle = state.handle.as_ref()?;
        Some(ErrorMark {
            pid: handle.pid?,
            errors: handle.progress.errors(),
        })
    }

    /// The error the engine logged after `mark`, answered once its stderr has been read up to
    /// now — the engine logs a stream's exception BEFORE it sends the stream's error frame, so a
    /// caller holding that frame gets the line, never a race lost to the reader. `Err` says why
    /// there is no answer: another process serves now, or the supervisor is mid-restart.
    pub async fn error_logged_since(&self, mark: ErrorMark) -> Result<LoggedError, String> {
        self.error_lookup(mark)?.logged().await
    }

    /// [`Self::error_logged_since`] in two steps: the lookup is taken under the supervisor's
    /// state, the wait for the reader runs after it is released.
    pub fn error_lookup(&self, mark: ErrorMark) -> Result<ErrorLookup, String> {
        let progress = {
            let state = self.state.try_lock().map_err(|_| {
                "the engine's supervisor is starting or restarting it, so its log cannot be matched \
                 to this answer"
                    .to_string()
            })?;
            let handle = state
                .handle
                .as_ref()
                .ok_or("no engine process is supervised any more")?;
            if handle.pid != Some(mark.pid) {
                return Err(format!(
                    "the engine process that served this answer (pid {}) is gone; {} serves now",
                    mark.pid,
                    handle
                        .pid
                        .map_or("a process of unknown pid".to_string(), |p| format!(
                            "pid {p}"
                        ))
                ));
            }
            Arc::clone(&handle.progress)
        };
        Ok(ErrorLookup {
            progress,
            errors: mark.errors,
        })
    }

    /// [`Self::ensure_running`], ended by `cancel` if it has to restart the engine and the owner
    /// stops it before the restart serves; a restart publishes its start to `watch`. Says whether
    /// the engine was kept or restarted; a restart whose load failed carries [`RestartLoadFailed`].
    pub async fn ensure_running_unless(
        &self,
        cancel: &Arc<StartCancel>,
        watch: Option<Arc<StartupWatch>>,
    ) -> Result<Ensured> {
        *self.cancel.lock().unwrap() = Some(Arc::clone(cancel));
        *self.watch.lock().unwrap() = watch;
        let outcome = self.restart_unless_healthy().await;
        *self.cancel.lock().unwrap() = None;
        *self.watch.lock().unwrap() = None;
        outcome
    }

    fn current_cancel(&self) -> Option<Arc<StartCancel>> {
        self.cancel.lock().unwrap().clone()
    }

    fn current_watch(&self) -> Option<Arc<StartupWatch>> {
        self.watch
            .lock()
            .unwrap()
            .clone()
            .or_else(|| self.config.startup_watch.clone())
    }

    fn refuse_if_cancelled(&self) -> Result<()> {
        if self.current_cancel().is_some_and(|c| c.is_cancelled()) {
            return Err(StartCancelled {
                name: self.config.name.clone(),
                pid: None,
            }
            .into());
        }
        Ok(())
    }

    /// Restart the engine if its process died or it stops answering. Errors once the
    /// circuit breaker trips (too many restarts inside the window), carrying stderr.
    pub async fn ensure_running(&self) -> Result<()> {
        self.restart_unless_healthy().await.map(|_| ())
    }

    async fn restart_unless_healthy(&self) -> Result<Ensured> {
        let mut state = self.state.lock().await;

        let exit = match state.handle.as_mut() {
            None => {
                Some("no engine process is supervised (the last start did not serve)".to_string())
            }
            Some(h) => h
                .child
                .try_wait()
                .context("try_wait on sidecar")?
                .map(|status| {
                    let by = h
                        .stopped_by
                        .lock()
                        .unwrap()
                        .clone()
                        .unwrap_or_else(|| "no goose path stopped it".to_string());
                    format!("process exited: {status} — {by}")
                }),
        };
        let process_dead = exit.is_some();
        let unhealthy = match exit {
            Some(exit) => exit,
            None => match self.probe().await {
                Ok(()) => {
                    state.backoff = self.config.backoff_initial;
                    return Ok(Ensured::Healthy);
                }
                Err(reason) => reason.to_string(),
            },
        };

        let tail = match state.handle.as_mut() {
            Some(h) if process_dead => h.last_words().await,
            Some(h) => stderr_tail_string(&h.stderr_tail),
            None => String::new(),
        };
        tracing::warn!(
            sidecar = %self.config.name,
            process_dead,
            "sidecar unhealthy ({unhealthy}); restarting. stderr tail:\n{tail}"
        );

        if let Some(mut h) = state.handle.take() {
            if !process_dead {
                h.stopping(format!(
                    "stopped by goose: its supervisor restarts an engine that stopped answering ({unhealthy})"
                ));
            }
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
        match self.current_cancel() {
            Some(cancel) => {
                tokio::select! {
                    _ = tokio::time::sleep(backoff) => {}
                    _ = cancel.cancelled() => {}
                }
            }
            None => tokio::time::sleep(backoff).await,
        }
        self.refuse_if_cancelled()?;

        let load = async {
            self.claim_port().await?;
            let handle = self.spawn_child()?;
            self.await_ready(&mut state, handle).await
        };
        load.await
            .map(|()| Ensured::Restarted)
            .map_err(|e| e.context(RestartLoadFailed))
    }

    /// What this sidecar stamps on every process it spawns (see [`port_holder`]).
    fn marker(&self) -> String {
        sidecar_marker(&self.config.name, &self.config.base_url)
    }

    /// Before a spawn: the port is free, or its holders were proven ours and stopped per pid, or
    /// the start fails naming every holder ([`PortHeld`]) — never spawning a child whose readiness
    /// probe would read someone else's listener (Q-240: a stand-in's catalog was taken for the
    /// child's own readiness), and never trying another port.
    async fn claim_port(&self) -> Result<()> {
        let Some(port) = self.listen_port() else {
            return Ok(());
        };
        #[cfg(unix)]
        {
            let reaped = port_holder::claim_port(port, &self.marker()).await?;
            if !reaped.is_empty() {
                tracing::warn!(
                    sidecar = %self.config.name,
                    port,
                    reaped = ?reaped.iter().map(|r| (r.pid, r.signal)).collect::<Vec<_>>(),
                    "the port was held by this sidecar's own leftover engine; stopped per pid"
                );
            }
        }
        #[cfg(not(unix))]
        if port_has_listener(port) {
            bail!(
                "port {port} already answers and this platform cannot read who holds it; nothing \
                 was signalled"
            );
        }
        Ok(())
    }

    /// SIGTERM to the child pid, the grace window, then SIGKILL to the child's PROVEN own
    /// process group (see the crate doc); afterwards the listen port is waited for and any
    /// residue of that same group is terminated per-pid, so a re-mount finds the port free.
    pub async fn shutdown(&self) {
        let mut state = self.state.lock().await;
        if let Some(mut h) = state.handle.take() {
            h.stopping(
                "stopped by goose: its owner shut the sidecar down (an Unmount, a remount with \
                 another model or argv, or goosed exiting)",
            );
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
        cmd.env(SIDECAR_MARKER_ENV, self.marker());
        subprocess::configure_subprocess(&mut cmd);
        let mut child = cmd.spawn().with_context(|| {
            format!(
                "failed to spawn sidecar '{}' ({})",
                self.config.name, self.config.command[0]
            )
        })?;
        // Before the reader exists, so the spawn's phase is always the first mark; from here on
        // the reader marks each phase at its line — a phase is never lost between two looks.
        let watch = self.current_watch();
        if let Some(watch) = &watch {
            watch.mark_phase(&[]);
        }

        let pid = child.id();
        let stderr_tail = Arc::new(StdMutex::new(VecDeque::with_capacity(STDERR_TAIL_LINES)));
        let stderr_lines = Arc::new(AtomicU64::new(0));
        let stopped_by = Arc::new(StdMutex::new(None));
        let shutting_down = Arc::new(StdMutex::new(None));
        let exit_report = Arc::new(StdMutex::new(None));
        let (stderr_closed, stderr_closed_rx) = tokio::sync::watch::channel(false);
        let mut log = self
            .config
            .log_dir
            .as_deref()
            .map(|dir| child_log::open(dir, &self.config.name, pid));
        let progress = Arc::new(StderrProgress::new(
            child.stderr.as_ref(),
            child_log::describe(&log),
        ));
        let stderr_reader = child.stderr.take().map(|stderr| {
            let progress = Arc::clone(&progress);
            let tail = Arc::clone(&stderr_tail);
            let count = Arc::clone(&stderr_lines);
            let name = self.config.name.clone();
            let shutdown_line = self.config.shutdown_line;
            let stopped_by = Arc::clone(&stopped_by);
            let shutting_down = Arc::clone(&shutting_down);
            tokio::spawn(async move {
                let push = |line: String| {
                    let mut tail = tail.lock().unwrap();
                    if tail.len() == STDERR_TAIL_LINES {
                        tail.pop_front();
                    }
                    tail.push_back(line);
                    if let Some(watch) = &watch {
                        watch.mark_phase(tail.make_contiguous());
                    }
                };
                if let Some(Err(why)) = &log {
                    push(why.clone());
                }
                // Bytes, decoded lossily: `lines()` ends at the first non-UTF-8 byte, and every
                // line after it — the traceback that explains a failed load — would be dropped.
                let mut stderr = BufReader::new(WatchedPipe {
                    inner: stderr,
                    progress: Arc::clone(&progress),
                });
                let mut buf = Vec::new();
                loop {
                    buf.clear();
                    match stderr.read_until(b'\n', &mut buf).await {
                        Ok(0) => break,
                        Ok(_) => {
                            let line = String::from_utf8_lossy(&buf);
                            let line = line.trim_end_matches(['\n', '\r']).to_string();
                            tracing::debug!(sidecar = %name, "{line}");
                            if let Some(stopped) = child_log::append(&mut log, &line) {
                                tracing::warn!(sidecar = %name, "{stopped}");
                                push(stopped);
                            }
                            if child_log::is_error_line(&line) {
                                tracing::warn!(
                                    sidecar = %name,
                                    log = %child_log::describe(&log),
                                    "engine error: {line}"
                                );
                                progress.logged_error(&line);
                            }
                            count.fetch_add(1, Ordering::Relaxed);
                            if shutdown_line.is_some_and(|is_shutdown| is_shutdown(&line)) {
                                announce_shutdown(&name, pid, &line, &stopped_by, &shutting_down);
                            }
                            push(line);
                            if stderr.buffer().is_empty() {
                                progress.read_up();
                            }
                        }
                        Err(e) => {
                            push(format!("(reading this engine's stderr failed: {e})"));
                            break;
                        }
                    }
                }
                progress.close();
                stderr_closed.send_replace(true);
            })
        });
        if let Some(pid) = pid {
            watch_exit(
                pid,
                self.config.name.clone(),
                Arc::clone(&stderr_tail),
                Arc::clone(&stopped_by),
                Arc::clone(&exit_report),
                stderr_closed_rx,
            );
        }
        Ok(ChildHandle {
            pid,
            child,
            stderr_tail,
            stderr_lines,
            progress,
            stderr_reader,
            stopped_by,
            shutting_down,
            exit_report,
        })
    }

    /// Wait for readiness with a PROGRESS terminator: the child exiting is an immediate loud
    /// failure; otherwise the start continues for as long as the child tree keeps changing,
    /// and fails only after `startup_stall_window` of NO change — no stderr line, no CPU
    /// time, no memory movement, no pid joining or leaving — with the last probe reason and
    /// the stderr tail. A slow load that is working is never declared failed by a clock.
    async fn await_ready(&self, state: &mut State, mut handle: ChildHandle) -> Result<()> {
        let cancel = self.current_cancel();
        let watch = self.current_watch();
        let mut sys = System::new();
        let mut last_mark = progress_mark(&mut sys, &handle);
        let mut last_progress = Instant::now();
        loop {
            if cancel.as_ref().is_some_and(|c| c.is_cancelled()) {
                let pid = handle.pid;
                handle.stopping("stopped by goose: its start was cancelled before it served");
                let owned_group = terminate(&mut handle.child).await;
                self.release_port(owned_group).await;
                return Err(StartCancelled {
                    name: self.config.name.clone(),
                    pid,
                }
                .into());
            }
            if let Some(status) = handle.child.try_wait().context("try_wait during startup")? {
                let tail = handle.last_words().await;
                bail!(
                    "sidecar '{}' exited during startup ({status}). stderr:\n{tail}",
                    self.config.name
                );
            }
            let mark = progress_mark(&mut sys, &handle);
            if let Some(watch) = &watch {
                watch.publish(&mark, &handle);
            }
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
                        handle.stopping(format!(
                            "stopped by goose: it served '{served}', not the expected model id"
                        ));
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
                handle.stopping(format!(
                    "stopped by goose: its start made no progress for {stalled_for:?}"
                ));
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
            match &cancel {
                Some(cancel) => {
                    tokio::select! {
                        _ = tokio::time::sleep(Duration::from_millis(400)) => {}
                        _ = cancel.cancelled() => {}
                    }
                }
                None => tokio::time::sleep(Duration::from_millis(400)).await,
            }
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
    /// logged (the manager refuses to mount over it; an explicit unmount reclaims it only when
    /// `port_holder`'s proof calls it this goose's own leftover — Q-252).
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
        reclaim_group_residue(port, group, listeners).await;
    }
}

impl Drop for Sidecar {
    fn drop(&mut self) {
        if let Ok(mut state) = self.state.try_lock() {
            if let Some(h) = state.handle.as_mut() {
                h.stopping("stopped by goose: its supervisor was dropped");
                sigkill_tree_or_pid(&mut h.child);
            }
        }
        // kill_on_drop(true) covers the path where the lock is held elsewhere; that leg
        // reaches the pid alone.
    }
}

/// The engine said it began shutting down: its port stops accepting now and in-flight requests
/// drain before it exits. Said once per child, loud when no goose path signalled it — Q-258's four
/// engines closed their port mid-prefill on a SIGTERM from outside goosed and drained for up to 6
/// minutes with no line in goosed's log.
fn announce_shutdown(
    name: &str,
    pid: Option<u32>,
    line: &str,
    stopped_by: &StdMutex<Option<String>>,
    shutting_down: &StdMutex<Option<String>>,
) {
    {
        let mut seen = shutting_down.lock().unwrap();
        if seen.is_some() {
            return;
        }
        *seen = Some(line.to_string());
    }
    match stopped_by.lock().unwrap().clone() {
        Some(by) => tracing::info!(
            event = "sidecar_engine_shutting_down",
            sidecar = %name,
            pid,
            stopped_by = %by,
            "the engine began shutting down ({by}): {line}"
        ),
        None => tracing::error!(
            event = "sidecar_engine_shutting_down",
            sidecar = %name,
            pid,
            "the engine began shutting down and no goose path signalled it — the signal came \
             from outside this goosed; its port stops accepting now while in-flight requests \
             drain, then it exits. Its own words: {line}"
        ),
    }
}

/// Reports the engine's exit the moment it happens — not when goose next looks (Q-258: a death
/// at 04:17 was first logged at 04:19, by the swap that found it gone). A thread waits on the
/// child WITHOUT reaping it (`waitid` with `WNOWAIT`), so the supervisor's own `try_wait` still
/// reaps and reports as before; the report adds who stopped it, this Mac's memory at that moment
/// and the engine's last words (read to the pipe's EOF, bounded by the crate's grace window since
/// an orphan may hold the pipe). A child reaped by goose before the thread looked (`ECHILD`) was
/// seen by the path that reaped it, and is not reported twice.
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn watch_exit(
    pid: u32,
    name: String,
    tail: Arc<StdMutex<VecDeque<String>>>,
    stopped_by: Arc<StdMutex<Option<String>>>,
    report: Arc<StdMutex<Option<String>>>,
    mut stderr_closed: tokio::sync::watch::Receiver<bool>,
) {
    let (ended, ended_rx) = tokio::sync::oneshot::channel();
    let watcher = std::thread::Builder::new()
        .name(format!("{name}-exit-watch"))
        .spawn(move || {
            let _ = ended.send(exit_status_unreaped(pid));
        });
    if let Err(e) = watcher {
        tracing::warn!(sidecar = %name, pid, error = %e, "the engine's exit watch could not start; its exit is reported only when goose next looks at it");
        return;
    }
    tokio::spawn(async move {
        let status = match ended_rx.await {
            Ok(Ok(status)) => status,
            Ok(Err(e)) if e.raw_os_error() == Some(libc::ECHILD) => return,
            Ok(Err(e)) => {
                tracing::warn!(sidecar = %name, pid, error = %e, "the engine's exit could not be observed; it is reported when goose next looks at it");
                return;
            }
            Err(_) => return,
        };
        for _ in 0..GRACE_TICKS {
            if *stderr_closed.borrow_and_update() {
                break;
            }
            let _ = tokio::time::timeout(GRACE_TICK, stderr_closed.changed()).await;
        }
        let words_complete = *stderr_closed.borrow();
        let memory = match crate::memory::measure() {
            Ok(reading) => format!(
                "this Mac then had {:.1} GiB available of {:.1} GiB",
                reading.available_bytes as f64 / GIB as f64,
                reading.total_bytes as f64 / GIB as f64
            ),
            Err(e) => format!("this Mac's memory could not be read then: {e:#}"),
        };
        let by = stopped_by.lock().unwrap().clone();
        let account = exit_account(pid, &status, by.as_deref(), &memory);
        *report.lock().unwrap() = Some(account.clone());
        let mut words = stderr_tail_string(&tail);
        if !words_complete {
            words.push_str("\n(the stderr pipe was still open after the grace window — something the engine started still holds it — so its last lines may be missing)");
        }
        match by {
            Some(_) => {
                tracing::info!(event = "sidecar_engine_exited", sidecar = %name, pid, status = %status, "{account}. Last words:\n{words}")
            }
            None => {
                tracing::error!(event = "sidecar_engine_exited", sidecar = %name, pid, status = %status, "{account}. Last words:\n{words}")
            }
        }
    });
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn watch_exit(
    pid: u32,
    name: String,
    _tail: Arc<StdMutex<VecDeque<String>>>,
    _stopped_by: Arc<StdMutex<Option<String>>>,
    _report: Arc<StdMutex<Option<String>>>,
    _stderr_closed: tokio::sync::watch::Receiver<bool>,
) {
    tracing::debug!(sidecar = %name, pid, "no unreaped exit wait on this platform; the engine's exit is reported when goose next looks at it");
}

/// How the engine ended, in one sentence: the OS's status, who stopped it, and memory then. An
/// exit no goose path caused says where such a signal comes from — a SIGKILL goose did not send
/// is how macOS's memory killer (jetsam) ends a process.
fn exit_account(
    pid: u32,
    status: &std::process::ExitStatus,
    stopped_by: Option<&str>,
    memory: &str,
) -> String {
    #[cfg(unix)]
    let signal = std::os::unix::process::ExitStatusExt::signal(status);
    #[cfg(not(unix))]
    let signal: Option<i32> = None;
    #[cfg(unix)]
    let sigkill = signal == Some(libc::SIGKILL);
    #[cfg(not(unix))]
    let sigkill = false;
    let who = match stopped_by {
        Some(by) => by.to_string(),
        None if sigkill => {
            "no goose path stopped it: a SIGKILL goose did not send — on macOS that \
                            is how the memory killer (jetsam) ends a process"
                .to_string()
        }
        None if signal.is_some() => "no goose path stopped it: the signal came from outside this \
                                     goosed (its last words name it)"
            .to_string(),
        None => "no goose path stopped it".to_string(),
    };
    format!("the engine process (pid {pid}) exited: {status} — {who}; {memory}")
}

/// Waits for `pid` (our child) to end and returns its status, leaving it waitable (`WNOWAIT`).
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn exit_status_unreaped(pid: u32) -> std::io::Result<std::process::ExitStatus> {
    use std::os::unix::process::ExitStatusExt;
    loop {
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        let rc = unsafe {
            libc::waitid(
                libc::P_PID,
                pid as libc::id_t,
                &mut info,
                libc::WEXITED | libc::WNOWAIT,
            )
        };
        if rc != 0 {
            let e = std::io::Error::last_os_error();
            if e.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(e);
        }
        #[cfg(target_os = "macos")]
        let status = info.si_status;
        #[cfg(target_os = "linux")]
        let status = unsafe { info.si_status() };
        // The raw wait status `ExitStatus` decodes: the code in the second byte, or the signal in
        // the low 7 bits with 0x80 for a core dump.
        let raw = match info.si_code {
            libc::CLD_EXITED => (status & 0xff) << 8,
            libc::CLD_KILLED => status & 0x7f,
            libc::CLD_DUMPED => (status & 0x7f) | 0x80,
            code => {
                return Err(std::io::Error::other(format!(
                    "waitid reported si_code {code}, not an exit"
                )))
            }
        };
        return Ok(std::process::ExitStatus::from_raw(raw));
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

/// Terminate, per-pid, the listeners on `port` that belong to the engine's own process
/// `group`; listeners outside it are logged and left alone.
#[cfg(unix)]
async fn reclaim_group_residue(port: u16, group: u32, listeners: Vec<u32>) {
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

#[cfg(not(unix))]
async fn reclaim_group_residue(port: u16, group: u32, listeners: Vec<u32>) {
    tracing::warn!(
        port,
        ?listeners,
        group,
        "port still occupied; reclaiming by process group needs Unix signals, left alone"
    );
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

/// Where `lsof` lives on the platforms goosed ships to, tried BEFORE the PATH walk. The
/// packaged app's goosed runs with a PATH that has no `/usr/sbin` (measured 2026-09-02:
/// `…/Resources/bin:…:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin`) while macOS keeps
/// lsof there — so a bare-name spawn ENOENTed under the app, and every reclaim ended in
/// "lsof unavailable; port left occupied" with the reclaim itself never reachable.
const LSOF_KNOWN_LOCATIONS: [&str; 3] =
    ["/usr/sbin/lsof", "/usr/bin/lsof", "/opt/homebrew/bin/lsof"];

/// `lsof` exists at none of the known locations and in no PATH directory. Carries every
/// path that was looked at, in the order it was looked at, so the operator sees exactly
/// what the search covered instead of a bare-name ENOENT.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LsofUnavailable {
    pub searched: Vec<std::path::PathBuf>,
}

impl std::fmt::Display for LsofUnavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "lsof not found; searched {} location(s): ",
            self.searched.len()
        )?;
        for (i, path) in self.searched.iter().enumerate() {
            if i > 0 {
                f.write_str(", ")?;
            }
            write!(f, "{}", path.display())?;
        }
        Ok(())
    }
}

impl std::error::Error for LsofUnavailable {}

/// The `lsof` binary this crate runs: the first of [`LSOF_KNOWN_LOCATIONS`] that exists,
/// else the first PATH directory holding one.
pub fn resolve_lsof() -> Result<std::path::PathBuf, LsofUnavailable> {
    resolve_lsof_in(
        &LSOF_KNOWN_LOCATIONS.map(std::path::PathBuf::from),
        std::env::var_os("PATH").as_deref(),
    )
}

fn resolve_lsof_in(
    known: &[std::path::PathBuf],
    path_env: Option<&std::ffi::OsStr>,
) -> Result<std::path::PathBuf, LsofUnavailable> {
    let from_path = path_env
        .into_iter()
        .flat_map(std::env::split_paths)
        .filter(|dir| !dir.as_os_str().is_empty())
        .map(|dir| dir.join("lsof"));
    let mut searched = Vec::new();
    for candidate in known.iter().cloned().chain(from_path) {
        if candidate.is_file() {
            return Ok(candidate);
        }
        searched.push(candidate);
    }
    Err(LsofUnavailable { searched })
}

/// Pids with a LISTEN socket on `port`. `-sTCP:LISTEN` is load-bearing: a bare `lsof -i :port`
/// also lists every process holding a CLIENT connection to the port — goosed's own keep-alive
/// pool included (measured 2026-09-01: the connected client pid appeared alongside the
/// listener) — and signalling that list would have signalled the caller.
pub(crate) async fn listening_pids(port: u16) -> Result<Vec<u32>> {
    let lsof = resolve_lsof()?;
    let output = tokio::process::Command::new(&lsof)
        .args(["-ti", &format!("TCP:{port}"), "-sTCP:LISTEN"])
        .output()
        .await
        .with_context(|| format!("running {}", lsof.display()))?;
    listening_pids_of(&lsof, &output)
}

/// An lsof that failed is an `Err`, never "no listener" — its caller signals what this names.
/// Measured on macOS 26.6 (lsof 4.91): no listener → exit 1 with empty stdout AND stderr; a
/// failed lsof (an illegal option) → exit 1 WITH stderr. `-t` implies `-w`, so a warning never
/// lands on stderr of an answered run.
pub(crate) fn listening_pids_of(
    lsof: &std::path::Path,
    output: &std::process::Output,
) -> Result<Vec<u32>> {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    match output.status.code() {
        Some(0) => Ok(stdout
            .split_whitespace()
            .filter_map(|s| s.parse().ok())
            .collect()),
        Some(1) if stdout.trim().is_empty() && stderr.trim().is_empty() => Ok(Vec::new()),
        status => anyhow::bail!(
            "{} could not answer (exit {status:?}): {}",
            lsof.display(),
            stderr.trim()
        ),
    }
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

/// Where a child's logged errors stood when a request began (Q-423): taken before the request is
/// sent, handed back to [`Sidecar::error_logged_since`] when its stream fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ErrorMark {
    pub pid: u32,
    pub errors: u64,
}

/// What the engine logged as an error after a mark, read up to the moment of the question.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoggedError {
    /// The last ERROR/CRITICAL line the engine printed after the mark; `None` when it printed none.
    pub line: Option<String>,
    /// Where the engine's whole stderr is kept, or why it is kept nowhere.
    pub log: String,
}

/// One engine's logged errors after a mark, answered by [`ErrorLookup::logged`].
pub struct ErrorLookup {
    progress: Arc<StderrProgress>,
    errors: u64,
}

impl ErrorLookup {
    pub async fn logged(self) -> Result<LoggedError, String> {
        self.progress.logged_since(self.errors).await
    }
}

#[derive(Default)]
struct ProgressState {
    errors: u64,
    last_error: Option<String>,
    closed: bool,
}

/// How far a child's stderr has been read, and the errors it logged — the barrier that matches a
/// failed stream with the exception its engine logged for it WITHOUT a clock.
///
/// Rapid-MLX logs a stream's exception on stderr and only THEN sends the client its sanitized SSE
/// error (helpers.py, disconnect_guard): by the time goose holds that error frame, the line is in
/// the pipe or already read. So "every byte written to the pipe so far has been read into lines"
/// is an event that must come, and it decides the lookup: the pipe's unread byte count
/// (`FIONREAD`) is zero AND the reader holds no byte it has not yet turned into a line. `pending`
/// goes up BEFORE each read reaches the pipe and comes down only once every byte that read took
/// has been handled, so a lookup that reads the pipe count first and `pending` second never misses
/// bytes in the reader's hands.
struct StderrProgress {
    state: StdMutex<ProgressState>,
    pending: std::sync::atomic::AtomicBool,
    changed: tokio::sync::Notify,
    log: String,
    #[cfg(unix)]
    fd: Option<std::os::fd::RawFd>,
}

impl StderrProgress {
    fn new(stderr: Option<&tokio::process::ChildStderr>, log: String) -> Self {
        #[cfg(not(unix))]
        let _ = stderr;
        Self {
            state: StdMutex::new(ProgressState::default()),
            pending: std::sync::atomic::AtomicBool::new(false),
            changed: tokio::sync::Notify::new(),
            log,
            #[cfg(unix)]
            fd: stderr.map(std::os::fd::AsRawFd::as_raw_fd),
        }
    }

    #[cfg(all(test, unix))]
    fn over_fd(fd: std::os::fd::RawFd) -> Self {
        let mut progress = Self::new(None, "test log".to_string());
        progress.fd = Some(fd);
        progress
    }

    fn logged_error(&self, line: &str) {
        let mut state = self.state.lock().unwrap();
        state.errors += 1;
        state.last_error = Some(line.to_string());
    }

    /// The reader has handled every byte it took from the pipe.
    fn read_up(&self) {
        self.pending.store(false, Ordering::SeqCst);
        self.changed.notify_waiters();
    }

    /// The pipe reached EOF or failed: nothing more will be read. Set before the reader drops the
    /// pipe, under the lock the unread count is read under, so that read never reaches a closed fd.
    fn close(&self) {
        self.state.lock().unwrap().closed = true;
        self.read_up();
    }

    fn errors(&self) -> u64 {
        self.state.lock().unwrap().errors
    }

    /// Whether every byte written to the pipe so far has been read into lines.
    fn read_up_to_now(&self) -> Result<bool, String> {
        let state = self.state.lock().unwrap();
        if state.closed {
            return Ok(true);
        }
        let unread = self.unread_in_pipe()?;
        drop(state);
        Ok(unread == 0 && !self.pending.load(Ordering::SeqCst))
    }

    #[cfg(unix)]
    fn unread_in_pipe(&self) -> Result<usize, String> {
        let fd = self
            .fd
            .ok_or("the engine's stderr was never piped to goose")?;
        let mut unread: libc::c_int = 0;
        // SAFETY: `fd` is the pipe's read end, open while `closed` is false (checked under the lock
        // the caller holds; the reader sets `closed` under it before it drops the pipe), and
        // `unread` is a valid out-pointer for FIONREAD's int.
        if unsafe { libc::ioctl(fd, libc::FIONREAD, &mut unread) } != 0 {
            return Err(format!(
                "reading how much of the engine's stderr is unread failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(usize::try_from(unread).unwrap_or(0))
    }

    #[cfg(not(unix))]
    fn unread_in_pipe(&self) -> Result<usize, String> {
        Err("how much of the engine's stderr is unread is measured on unix only".to_string())
    }

    /// The error logged after `errors` (a mark's count), once the stderr is read up to now.
    async fn logged_since(&self, errors: u64) -> Result<LoggedError, String> {
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            {
                let state = self.state.lock().unwrap();
                if state.errors > errors {
                    return Ok(LoggedError {
                        line: state.last_error.clone(),
                        log: self.log.clone(),
                    });
                }
            }
            if self.read_up_to_now()? {
                return Ok(LoggedError {
                    line: None,
                    log: self.log.clone(),
                });
            }
            changed.await;
        }
    }
}

/// The child's stderr as the reader polls it: `pending` goes up before each poll reaches the pipe
/// and comes down when the poll took nothing (the reader lowers it once it has handled what a
/// read took — [`StderrProgress::read_up`]).
struct WatchedPipe<R> {
    inner: R,
    progress: Arc<StderrProgress>,
}

impl<R: tokio::io::AsyncRead + Unpin> tokio::io::AsyncRead for WatchedPipe<R> {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        self.progress.pending.store(true, Ordering::SeqCst);
        let before = buf.filled().len();
        let polled = std::pin::Pin::new(&mut self.inner).poll_read(cx, buf);
        let took_nothing = match &polled {
            std::task::Poll::Ready(Ok(())) => buf.filled().len() == before,
            std::task::Poll::Pending | std::task::Poll::Ready(Err(_)) => true,
        };
        if took_nothing {
            self.progress.read_up();
        }
        polled
    }
}

/// The durable copy of a child's stderr (`SidecarConfig::log_dir`): the file, or the line that
/// says why there is none — never a silent absence.
mod child_log {
    use std::path::Path;

    #[cfg(unix)]
    type Log = crate::distributed::rank_log::RankLog;
    #[cfg(not(unix))]
    type Log = std::convert::Infallible;

    pub(crate) type Slot = Option<Result<Log, String>>;

    #[cfg(unix)]
    pub(crate) fn open(dir: &Path, name: &str, pid: Option<u32>) -> Result<Log, String> {
        let stem = crate::distributed::rank_log::slug(name);
        match Log::open_family(dir, &stem, &stem) {
            Ok(log) => {
                tracing::info!(
                    sidecar = name,
                    pid,
                    log = %log.path().display(),
                    "every stderr line of this engine is kept in its log"
                );
                Ok(log)
            }
            Err(e) => {
                let why = format!("goose: this engine's durable log is unavailable: {e:#}");
                tracing::warn!(sidecar = name, pid, "{why}");
                Err(why)
            }
        }
    }

    #[cfg(not(unix))]
    pub(crate) fn open(dir: &Path, _name: &str, _pid: Option<u32>) -> Result<Log, String> {
        Err(format!(
            "goose: this engine's durable log is unavailable: {} cannot hold one on this platform \
             (the log's free-space bound is measured on unix only)",
            dir.display()
        ))
    }

    /// Appends `line`. A write that fails ends the log: the slot then carries why, and the
    /// returned line says it once, naming the file that holds the output up to here.
    #[cfg(unix)]
    pub(crate) fn append(slot: &mut Slot, line: &str) -> Option<String> {
        let Some(Ok(log)) = slot else {
            return None;
        };
        let e = log.append("err", line).err()?;
        let why = format!(
            "goose: this engine's durable log stopped: {e:#} (the output up to here is in {})",
            log.path().display()
        );
        *slot = Some(Err(why.clone()));
        Some(why)
    }

    #[cfg(not(unix))]
    pub(crate) fn append(_slot: &mut Slot, _line: &str) -> Option<String> {
        None
    }

    /// Where the log is, or why there is none.
    pub(crate) fn describe(slot: &Slot) -> String {
        match slot {
            #[cfg(unix)]
            Some(Ok(log)) => log.path().display().to_string(),
            #[cfg(not(unix))]
            Some(Ok(never)) => match *never {},
            Some(Err(why)) => why.clone(),
            None => "none (only the in-memory tail keeps its stderr)".to_string(),
        }
    }

    /// A line the engine logged at ERROR or CRITICAL (Python logging's default
    /// `LEVEL:logger:message`). Rapid-MLX logs a failed stream this way before it sends the client
    /// only "Internal error during streaming" (helpers.py, F-131: the SSE body is sanitized, the
    /// server log carries the exception and its traceback) — so goose's own log must carry it.
    pub(crate) fn is_error_line(line: &str) -> bool {
        line.starts_with("ERROR:") || line.starts_with("CRITICAL:")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Q-423's barrier, with the race forced: the engine's ERROR line sits UNREAD in the pipe when
    /// the lookup is asked (the order Rapid-MLX guarantees — log first, then the error frame). The
    /// lookup must wait for the reader, not answer "nothing logged"; once the reader has taken and
    /// handled the line, it answers with it. An empty pipe with nothing in the reader's hands
    /// answers `None` at once.
    #[cfg(unix)]
    #[tokio::test]
    async fn the_lookup_waits_for_a_line_still_unread_in_the_pipe() {
        use std::io::{Read, Write};
        use std::os::fd::{AsRawFd, FromRawFd};
        let mut fds = [0 as libc::c_int; 2];
        assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
        let (mut read_end, mut write_end) = unsafe {
            (
                std::fs::File::from_raw_fd(fds[0]),
                std::fs::File::from_raw_fd(fds[1]),
            )
        };
        let progress = Arc::new(StderrProgress::over_fd(read_end.as_raw_fd()));

        assert_eq!(progress.logged_since(0).await.unwrap().line, None);

        let line =
            "ERROR:rapid_mlx.service.helpers:[disconnect_guard] generator raised RuntimeError: x";
        write_end.write_all(format!("{line}\n").as_bytes()).unwrap();
        let lookup = tokio::spawn({
            let progress = Arc::clone(&progress);
            async move { progress.logged_since(0).await }
        });
        tokio::task::yield_now().await;
        assert!(
            !lookup.is_finished(),
            "the line is still in the pipe: the lookup must wait for the reader"
        );

        progress.pending.store(true, Ordering::SeqCst);
        let mut taken = vec![0u8; line.len() + 1];
        read_end.read_exact(&mut taken).unwrap();
        progress.logged_error(line);
        progress.read_up();
        let logged = lookup.await.unwrap().unwrap();
        assert_eq!(logged.line.as_deref(), Some(line));
        assert_eq!(progress.logged_since(1).await.unwrap().line, None);
    }

    /// The real lsof, all three answers: our own listener, a port nobody listens on, and an lsof
    /// that failed — which must be an error, never an empty (and so unsignalled-but-"clear") port.
    #[tokio::test]
    async fn a_failed_lsof_is_an_error_never_an_empty_port() {
        let lsof = resolve_lsof().unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let pids = listening_pids(port).await.unwrap();
        assert_eq!(pids, vec![std::process::id()]);
        drop(listener);
        assert_eq!(listening_pids(port).await.unwrap(), Vec::<u32>::new());

        let failed = std::process::Command::new(&lsof)
            .args(["-ti", &format!("TCP:{port}"), "-sTCP:LISTEN", "-Z"])
            .output()
            .unwrap();
        let err = listening_pids_of(&lsof, &failed).unwrap_err().to_string();
        assert!(err.contains("could not answer"), "{err}");
    }

    fn dir_with_lsof(root: &std::path::Path, name: &str) -> PathBuf {
        let dir = root.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("lsof"), "#!/bin/sh\n").unwrap();
        dir
    }

    #[test]
    fn a_known_location_wins_over_path() {
        let root = tempfile::tempdir().unwrap();
        let known = dir_with_lsof(root.path(), "known").join("lsof");
        let on_path = dir_with_lsof(root.path(), "onpath");
        let resolved =
            resolve_lsof_in(std::slice::from_ref(&known), Some(on_path.as_os_str())).unwrap();
        assert_eq!(resolved, known);
    }

    #[test]
    fn path_is_walked_in_order_when_no_known_location_exists() {
        let root = tempfile::tempdir().unwrap();
        let absent = root.path().join("absent").join("lsof");
        let empty = root.path().join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        let first = dir_with_lsof(root.path(), "first");
        let second = dir_with_lsof(root.path(), "second");
        let path_env = std::env::join_paths([&empty, &first, &second]).unwrap();
        let resolved = resolve_lsof_in(&[absent], Some(&path_env)).unwrap();
        assert_eq!(resolved, first.join("lsof"));
    }

    #[test]
    fn absence_names_every_path_searched_in_order() {
        let root = tempfile::tempdir().unwrap();
        let known_a = root.path().join("a").join("lsof");
        let known_b = root.path().join("b").join("lsof");
        let dir_c = root.path().join("c");
        let dir_d = root.path().join("d");
        std::fs::create_dir_all(&dir_c).unwrap();
        let path_env = std::env::join_paths([&dir_c, &dir_d]).unwrap();

        let err =
            resolve_lsof_in(&[known_a.clone(), known_b.clone()], Some(&path_env)).unwrap_err();

        assert_eq!(
            err.searched,
            vec![
                known_a.clone(),
                known_b.clone(),
                dir_c.join("lsof"),
                dir_d.join("lsof")
            ]
        );
        let text = err.to_string();
        assert!(
            text.starts_with("lsof not found; searched 4 location(s): "),
            "{text}"
        );
        for path in [&known_a, &known_b, &dir_c.join("lsof"), &dir_d.join("lsof")] {
            assert!(text.contains(&path.display().to_string()), "{text}");
        }
    }

    /// The measured packaged-app case: a PATH with no `/usr/sbin` (the app's is
    /// `…/Resources/bin:…:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin`). The known
    /// locations resolve lsof anyway — the bare-name spawn is what ENOENTed.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_packaged_apps_path_without_usr_sbin_still_resolves_lsof() {
        let known = LSOF_KNOWN_LOCATIONS.map(PathBuf::from);
        let packaged_path = std::ffi::OsStr::new("/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin");
        let resolved = resolve_lsof_in(&known, Some(packaged_path)).unwrap();
        assert_eq!(resolved, PathBuf::from("/usr/sbin/lsof"));
        assert_eq!(resolve_lsof().unwrap(), PathBuf::from("/usr/sbin/lsof"));
    }

    #[cfg(unix)]
    fn sidecar_running(script: &str) -> Sidecar {
        Sidecar {
            config: SidecarConfig::new(
                "fast-exit",
                vec!["sh".into(), "-c".into(), script.into()],
                "http://127.0.0.1:9",
                "unused",
            ),
            client: reqwest::Client::new(),
            state: Mutex::new(State {
                handle: None,
                restarts: VecDeque::new(),
                backoff: Duration::ZERO,
            }),
            cancel: StdMutex::new(None),
            watch: StdMutex::new(None),
        }
    }

    /// Blocks the runtime's ONE thread until the child has exited, so the stderr reader task
    /// cannot have run: the CI race (2026-09-28), made certain instead of likely.
    #[cfg(unix)]
    fn reap_without_yielding(handle: &mut ChildHandle) {
        while handle.child.try_wait().unwrap().is_none() {
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn an_exited_childs_last_words_are_read_to_eof_before_they_are_reported() {
        let sidecar = sidecar_running(
            "echo 'Loading model with BatchedEngine' >&2; echo 'ValueError: no weights' >&2; exit 3",
        );
        let mut handle = sidecar.spawn_child().unwrap();
        reap_without_yielding(&mut handle);
        assert!(
            handle.stderr_tail.lock().unwrap().is_empty(),
            "the reader has not taken a line yet — the race is set up"
        );
        assert_eq!(
            handle.last_words().await,
            "Loading model with BatchedEngine\nValueError: no weights"
        );
    }

    /// A descendant that outlives the child keeps the pipe open, so EOF may never come: the
    /// words are returned at once, and say they may be incomplete — never a wait on an orphan.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_descendant_holding_the_pipe_is_named_never_waited_for() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("orphan.pid");
        let sidecar = sidecar_running(&format!(
            "sleep 600 & echo $! > '{}'; echo 'ValueError: no weights' >&2; exit 3",
            pid_file.display()
        ));
        let mut handle = sidecar.spawn_child().unwrap();
        reap_without_yielding(&mut handle);
        let words = handle.last_words().await;
        let orphan: libc::pid_t = std::fs::read_to_string(&pid_file)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        unsafe { libc::kill(orphan, libc::SIGKILL) };
        assert!(
            words.contains("still has live members after its leader exited")
                && words.contains("its last ones may be missing"),
            "{words}"
        );
    }

    /// The exit watch's report, once written — awaited on the report itself, bounded by the
    /// crate's grace window twice over (the watch itself waits up to one for the pipe's EOF).
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    async fn exit_report_of(handle: &ChildHandle) -> String {
        for _ in 0..2 * GRACE_TICKS {
            if let Some(report) = handle.exit_report.lock().unwrap().clone() {
                return report;
            }
            tokio::time::sleep(GRACE_TICK).await;
        }
        panic!("the exit watch wrote no report")
    }

    /// Q-258's shape, reproduced with a stand-in: a SIGTERM from OUTSIDE goose (here, the test)
    /// makes the engine announce its shutdown — named loudly, with no goose stopper — and drain
    /// before it exits; the exit is reported the moment it happens, still with no goose stopper,
    /// and the child stays reapable by the supervisor's own `try_wait` (`WNOWAIT`).
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[tokio::test]
    async fn an_outside_sigterm_is_announced_at_the_shutdown_and_reported_at_the_exit() {
        let mut sidecar = sidecar_running(
            "trap 'echo \"INFO:     Shutting down\" >&2; sleep 0.3; echo \"INFO:     Finished server process\" >&2; exit 0' TERM; \
             echo 'INFO:     Uvicorn running' >&2; while :; do sleep 0.05; done",
        );
        sidecar.config.shutdown_line = Some(|line: &str| line.ends_with("Shutting down"));
        let mut handle = sidecar.spawn_child().unwrap();
        let pid = handle.pid.unwrap();
        while handle.stderr_lines.load(Ordering::Relaxed) == 0 {
            tokio::time::sleep(GRACE_TICK).await;
        }
        unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) };
        while handle.shutting_down.lock().unwrap().is_none() {
            tokio::time::sleep(GRACE_TICK).await;
        }
        assert_eq!(
            handle.shutting_down.lock().unwrap().as_deref(),
            Some("INFO:     Shutting down")
        );
        assert_eq!(*handle.stopped_by.lock().unwrap(), None);

        let report = exit_report_of(&handle).await;
        assert!(
            report.starts_with(&format!(
                "the engine process (pid {pid}) exited: exit status: 0 — no goose path stopped it;"
            )),
            "{report}"
        );
        assert!(report.contains("GiB available of"), "{report}");
        let reaped = handle.child.try_wait().unwrap();
        assert_eq!(
            reaped.and_then(|s| s.code()),
            Some(0),
            "the watch left the child for the supervisor to reap"
        );
    }

    /// A SIGKILL no goose path sent is named as what macOS's memory killer does, with the memory then.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[tokio::test]
    async fn a_sigkill_goose_did_not_send_names_the_memory_killer() {
        let sidecar = sidecar_running("echo 'loading' >&2; while :; do sleep 0.05; done");
        let handle = sidecar.spawn_child().unwrap();
        let pid = handle.pid.unwrap();
        unsafe { libc::kill(pid as libc::pid_t, libc::SIGKILL) };
        let report = exit_report_of(&handle).await;
        assert!(report.contains("exited: signal: 9 (SIGKILL)"), "{report}");
        assert!(report.contains("jetsam"), "{report}");
    }

    /// A stop goose made is attributed to the path that made it, set before the signal.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[tokio::test]
    async fn a_goose_stop_is_reported_with_the_path_that_stopped_it() {
        let sidecar = sidecar_running("while :; do sleep 0.05; done");
        let mut handle = sidecar.spawn_child().unwrap();
        handle.stopping("stopped by goose: the test's Unmount");
        terminate(&mut handle.child).await;
        let report = exit_report_of(&handle).await;
        assert!(
            report.contains("— stopped by goose: the test's Unmount;"),
            "{report}"
        );
        assert!(!report.contains("no goose path"), "{report}");
    }
}
