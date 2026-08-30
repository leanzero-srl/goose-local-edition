//! Supervisor for local inference engine sidecars.
//!
//! Owns the full lifecycle of an OpenAI-compatible engine process (Rapid-MLX, oMLX, …):
//! spawn from a configured argv, readiness by polling `/v1/models`, restart with capped
//! backoff behind a circuit breaker, and explicit per-pid termination — never killpg
//! (bare-spawned grandchildren can share the caller's group; a group kill has SIGKILLed
//! unrelated work before). Startup and restart failures carry the engine's stderr tail
//! so a dead sidecar is a diagnosable event, not a silent absence.

pub mod engine;
pub mod hf;
mod memory;
mod subprocess;

pub use memory::{dir_size_bytes, measure, GateResult, MemoryGate, Verdict, GIB};

use std::collections::VecDeque;
use std::process::Stdio;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

const STDERR_TAIL_LINES: usize = 200;

#[derive(Debug, Clone)]
pub struct SidecarConfig {
    pub name: String,
    /// Full argv; element 0 is the binary.
    pub command: Vec<String>,
    pub env: Vec<(String, String)>,
    /// e.g. "http://127.0.0.1:8090" — readiness and health poll GET {base_url}/v1/models.
    pub base_url: String,
    pub startup_timeout: Duration,
    pub restart_window: Duration,
    pub max_restarts_in_window: u32,
    pub backoff_initial: Duration,
    pub backoff_cap: Duration,
}

impl SidecarConfig {
    pub fn new(name: impl Into<String>, command: Vec<String>, base_url: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            command,
            env: Vec::new(),
            base_url: base_url.into(),
            startup_timeout: Duration::from_secs(180),
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
    /// Spawn the engine and wait until it serves `/v1/models`.
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
        self.probe().await
    }

    /// Restart the engine if its process died or it stops answering. Errors once the
    /// circuit breaker trips (too many restarts inside the window), carrying stderr.
    pub async fn ensure_running(&self) -> Result<()> {
        let mut state = self.state.lock().await;

        let process_dead = match state.handle.as_mut() {
            None => true,
            Some(h) => h.child.try_wait().context("try_wait on sidecar")?.is_some(),
        };
        if !process_dead && self.probe().await {
            state.backoff = self.config.backoff_initial;
            return Ok(());
        }

        let tail = state
            .handle
            .as_ref()
            .map(|h| stderr_tail_string(&h.stderr_tail))
            .unwrap_or_default();
        tracing::warn!(
            sidecar = %self.config.name,
            process_dead,
            "sidecar unhealthy; restarting. stderr tail:\n{tail}"
        );

        if let Some(mut h) = state.handle.take() {
            terminate_per_pid(&mut h.child).await;
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

    /// SIGTERM, a grace window, then SIGKILL — always the child pid alone.
    pub async fn shutdown(&self) {
        let mut state = self.state.lock().await;
        if let Some(mut h) = state.handle.take() {
            terminate_per_pid(&mut h.child).await;
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
        if let Some(stderr) = child.stderr.take() {
            let tail = Arc::clone(&stderr_tail);
            let name = self.config.name.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    tracing::debug!(sidecar = %name, "{line}");
                    let mut tail = tail.lock().unwrap();
                    if tail.len() == STDERR_TAIL_LINES {
                        tail.pop_front();
                    }
                    tail.push_back(line);
                }
            });
        }
        Ok(ChildHandle { child, stderr_tail })
    }

    async fn await_ready(&self, state: &mut State, mut handle: ChildHandle) -> Result<()> {
        let deadline = Instant::now() + self.config.startup_timeout;
        loop {
            if let Some(status) = handle.child.try_wait().context("try_wait during startup")? {
                let tail = stderr_tail_string(&handle.stderr_tail);
                bail!(
                    "sidecar '{}' exited during startup ({status}). stderr:\n{tail}",
                    self.config.name
                );
            }
            if self.probe().await {
                state.handle = Some(handle);
                tracing::info!(sidecar = %self.config.name, base_url = %self.config.base_url, "sidecar ready");
                return Ok(());
            }
            if Instant::now() >= deadline {
                let tail = stderr_tail_string(&handle.stderr_tail);
                terminate_per_pid(&mut handle.child).await;
                bail!(
                    "sidecar '{}' not ready within {:?}. stderr:\n{tail}",
                    self.config.name,
                    self.config.startup_timeout
                );
            }
            tokio::time::sleep(Duration::from_millis(400)).await;
        }
    }

    async fn probe(&self) -> bool {
        let url = format!("{}/v1/models", self.config.base_url);
        matches!(self.client.get(&url).send().await, Ok(r) if r.status().is_success())
    }
}

impl Drop for Sidecar {
    fn drop(&mut self) {
        if let Ok(mut state) = self.state.try_lock() {
            if let Some(h) = state.handle.as_mut() {
                let _ = h.child.start_kill();
            }
        }
        // kill_on_drop(true) covers the path where the lock is held elsewhere.
    }
}

async fn terminate_per_pid(child: &mut Child) {
    #[cfg(unix)]
    if let Some(pid) = child.id() {
        unsafe {
            libc::kill(pid as libc::pid_t, libc::SIGTERM);
        }
        for _ in 0..50 {
            if let Ok(Some(_)) = child.try_wait() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
    let _ = child.start_kill();
    let _ = child.wait().await;
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
