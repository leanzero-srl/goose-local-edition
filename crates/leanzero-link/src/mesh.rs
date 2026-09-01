//! Goose-owned Tailscale mesh engine: spawns and supervises an isolated userspace
//! `tailscaled`, joins a tailnet with an injected auth key, and reports typed status.
//!
//! The supervision idiom (spawn via `configure_subprocess`, stderr tail, readiness
//! polling, per-pid termination) mirrors `goose-sidecar`'s `Sidecar`; that type is not
//! reused because its readiness contract is an HTTP `GET {base_url}/v1/models` probe,
//! which does not fit a unix-socket daemon whose readiness is "the local API socket
//! answers `tailscale status --json`".
//!
//! Single-instance discipline (one daemon per state dir) is the caller's job; the
//! upcoming `control` module will own that lock.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::process::{Output, Stdio};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

use crate::subprocess::configure_subprocess;

pub const DEFAULT_LOGIN_SERVER: &str = "https://controlplane.tailscale.com";

const STDERR_TAIL_LINES: usize = 200;
const READY_POLL_INTERVAL: Duration = Duration::from_millis(250);
/// Per-attempt cap on the readiness `status --json` probe; the overall budget is
/// `MeshConfig::startup_timeout`.
const READY_PROBE_TIMEOUT: Duration = Duration::from_secs(2);
/// Extra wall clock granted past the CLI's own `--timeout` before we give up on `up`.
const JOIN_WAIT_GRACE: Duration = Duration::from_secs(15);

/// Paths that belong to a system/personal Tailscale installation. This crate must never
/// read, write, or bind anything under them — see the crate-level isolation invariant.
const FORBIDDEN_PREFIXES: &[&str] = &[
    "/var/run",
    "/var/lib/tailscale",
    "/var/db",
    "/Library/Tailscale",
];

#[derive(Debug, Error)]
pub enum MeshError {
    #[error("unsafe mesh config, refusing to start: {reason}")]
    UnsafeConfig { reason: String },
    #[error("cannot resolve a home directory for the default LeanZero state dir")]
    NoHomeDir,
    #[error("cannot prepare state dir '{}': {source}", path.display())]
    StateDir {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("failed to spawn '{program}': {source}")]
    Spawn {
        program: String,
        source: std::io::Error,
    },
    #[error("tailscaled exited during startup ({status}). stderr tail:\n{stderr_tail}")]
    DaemonExited { status: String, stderr_tail: String },
    #[error(
        "tailscaled socket did not become ready within {waited:?}. stderr tail:\n{stderr_tail}"
    )]
    StartupTimeout {
        waited: Duration,
        stderr_tail: String,
    },
    #[error("auth key is empty — LeanZero Link joins only with an injected, minted key")]
    EmptyAuthKey,
    #[error("failed to run {what} ('{program}'): {source}")]
    CliRun {
        what: &'static str,
        program: String,
        source: std::io::Error,
    },
    #[error("{what} did not finish within {waited:?}")]
    CliTimeout {
        what: &'static str,
        waited: Duration,
    },
    #[error("tailscale up failed:\n{stderr}")]
    JoinFailed { stderr: String },
    #[error("tailscale up reported success but the backend is {state} — not joined")]
    JoinIncomplete { state: BackendState },
    #[error("tailscale logout failed:\n{stderr}")]
    LogoutFailed { stderr: String },
    #[error("tailscale status failed:\n{stderr}")]
    StatusFailed { stderr: String },
    #[error("cannot parse `tailscale status --json` output: {error}; output began: {snippet}")]
    StatusParse { error: String, snippet: String },
}

#[derive(Debug, Clone)]
pub struct MeshConfig {
    pub tailscaled_path: PathBuf,
    pub tailscale_cli_path: PathBuf,
    pub state_dir: PathBuf,
    pub socket_path: PathBuf,
    pub hostname: String,
    /// Control plane URL; override for Headscale.
    pub login_server: String,
    /// Optional ACL tag to advertise (e.g. "tag:leanzero").
    pub tag: Option<String>,
    pub startup_timeout: Duration,
    pub join_timeout: Duration,
    pub cli_timeout: Duration,
}

impl MeshConfig {
    /// Defaults: state under `~/.leanzero/tailscale/`, socket `tailscaled.sock` inside
    /// it, public control plane. Callers overriding `state_dir` must keep `socket_path`
    /// inside their chosen dir; `validate` re-checks both against system paths.
    pub fn new(
        tailscaled_path: PathBuf,
        tailscale_cli_path: PathBuf,
        hostname: String,
    ) -> Result<Self, MeshError> {
        let state_dir = default_state_dir()?;
        let socket_path = state_dir.join("tailscaled.sock");
        Ok(Self {
            tailscaled_path,
            tailscale_cli_path,
            state_dir,
            socket_path,
            hostname,
            login_server: DEFAULT_LOGIN_SERVER.to_string(),
            tag: None,
            startup_timeout: Duration::from_secs(30),
            join_timeout: Duration::from_secs(90),
            cli_timeout: Duration::from_secs(15),
        })
    }

    /// The isolation gate: refuses any socket/state path that belongs to a system or
    /// personal Tailscale installation. Called by `MeshEngine::start`.
    pub fn validate(&self) -> Result<(), MeshError> {
        for (label, path) in [
            ("socket_path", &self.socket_path),
            ("state_dir", &self.state_dir),
        ] {
            if !path.is_absolute() {
                return Err(MeshError::UnsafeConfig {
                    reason: format!("{label} '{}' is not absolute", path.display()),
                });
            }
            for prefix in FORBIDDEN_PREFIXES {
                if path.starts_with(prefix) {
                    return Err(MeshError::UnsafeConfig {
                        reason: format!(
                            "{label} '{}' is under '{prefix}', which belongs to the \
                             system/personal Tailscale installation LeanZero Link must \
                             never touch",
                            path.display()
                        ),
                    });
                }
            }
            if let Some(home) = dirs::home_dir() {
                let user_default = home.join(".local/share/tailscale");
                if path.starts_with(&user_default) {
                    return Err(MeshError::UnsafeConfig {
                        reason: format!(
                            "{label} '{}' is under '{}', tailscaled's per-user default \
                             state location — LeanZero Link must use its own dir",
                            path.display(),
                            user_default.display()
                        ),
                    });
                }
            }
        }
        Ok(())
    }

    /// Flags verified against tailscaled 1.98.5 on this machine: `--tun` accepts
    /// "userspace-networking" (no TUN device, no root), `--statedir` holds state with
    /// `--state` defaulting to `<statedir>/tailscaled.state`, `--socket` sets the local
    /// API unix socket (system default `/var/run/tailscaled.socket`, which `validate`
    /// forbids here). `--no-logs-no-support` keeps the goose-owned daemon from
    /// uploading logs anywhere. The WireGuard UDP port is left at its default of 0
    /// (auto-select) so a personal daemon's port is never contended.
    pub fn tailscaled_argv(&self) -> Vec<String> {
        vec![
            self.tailscaled_path.display().to_string(),
            "--tun=userspace-networking".to_string(),
            format!("--statedir={}", self.state_dir.display()),
            format!("--socket={}", self.socket_path.display()),
            "--no-logs-no-support".to_string(),
        ]
    }

    /// `--socket` is a global `tailscale` flag and must precede the subcommand
    /// (verified against tailscale 1.98.5).
    fn cli_prefix(&self) -> Vec<String> {
        vec![
            self.tailscale_cli_path.display().to_string(),
            format!("--socket={}", self.socket_path.display()),
        ]
    }

    /// `--reset` pins unspecified `up` settings to their defaults so this argv is the
    /// complete, deterministic node config; `--timeout` bounds the CLI itself, derived
    /// from `join_timeout`.
    pub fn up_argv(&self, auth_key: &str, hostname: &str) -> Vec<String> {
        let mut argv = self.cli_prefix();
        argv.push("up".to_string());
        argv.push(format!("--auth-key={auth_key}"));
        argv.push(format!("--hostname={hostname}"));
        argv.push("--accept-routes=false".to_string());
        argv.push(format!("--login-server={}", self.login_server));
        argv.push("--reset".to_string());
        argv.push(format!("--timeout={}s", self.join_timeout.as_secs()));
        if let Some(tag) = &self.tag {
            argv.push(format!("--advertise-tags={tag}"));
        }
        argv
    }

    pub fn status_argv(&self) -> Vec<String> {
        let mut argv = self.cli_prefix();
        argv.push("status".to_string());
        argv.push("--json".to_string());
        argv
    }

    pub fn logout_argv(&self) -> Vec<String> {
        let mut argv = self.cli_prefix();
        argv.push("logout".to_string());
        argv
    }
}

fn default_state_dir() -> Result<PathBuf, MeshError> {
    dirs::home_dir()
        .map(|home| home.join(".leanzero").join("tailscale"))
        .ok_or(MeshError::NoHomeDir)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub enum BackendState {
    NoState,
    InUseOtherUser,
    NeedsLogin,
    NeedsMachineAuth,
    Stopped,
    Starting,
    Running,
    /// A state name this crate does not know yet — carried verbatim, never dropped.
    Other(String),
}

impl BackendState {
    pub fn as_str(&self) -> &str {
        match self {
            Self::NoState => "NoState",
            Self::InUseOtherUser => "InUseOtherUser",
            Self::NeedsLogin => "NeedsLogin",
            Self::NeedsMachineAuth => "NeedsMachineAuth",
            Self::Stopped => "Stopped",
            Self::Starting => "Starting",
            Self::Running => "Running",
            Self::Other(name) => name,
        }
    }
}

impl From<String> for BackendState {
    fn from(name: String) -> Self {
        match name.as_str() {
            "NoState" => Self::NoState,
            "InUseOtherUser" => Self::InUseOtherUser,
            "NeedsLogin" => Self::NeedsLogin,
            "NeedsMachineAuth" => Self::NeedsMachineAuth,
            "Stopped" => Self::Stopped,
            "Starting" => Self::Starting,
            "Running" => Self::Running,
            _ => Self::Other(name),
        }
    }
}

impl From<BackendState> for String {
    fn from(state: BackendState) -> Self {
        state.as_str().to_string()
    }
}

impl std::fmt::Display for BackendState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Wire shape for the upcoming control service and UI.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MeshStatus {
    pub self_ip: Option<String>,
    pub self_hostname: Option<String>,
    pub backend_state: BackendState,
    pub online: bool,
    pub peers: Vec<MeshPeer>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MeshPeer {
    pub hostname: String,
    pub ip: Option<String>,
    pub online: bool,
    /// RFC 3339; `None` while the peer is connected (tailscaled reports the zero time).
    pub last_seen: Option<String>,
}

impl MeshStatus {
    pub fn stopped() -> Self {
        Self {
            self_ip: None,
            self_hostname: None,
            backend_state: BackendState::Stopped,
            online: false,
            peers: Vec::new(),
        }
    }
}

#[derive(Deserialize)]
struct RawStatus {
    #[serde(rename = "BackendState")]
    backend_state: String,
    #[serde(rename = "Self")]
    self_node: Option<RawNode>,
    #[serde(rename = "Peer", default)]
    peers: Option<HashMap<String, RawNode>>,
}

#[derive(Deserialize)]
struct RawNode {
    #[serde(rename = "HostName", default)]
    host_name: String,
    #[serde(rename = "TailscaleIPs", default)]
    tailscale_ips: Option<Vec<String>>,
    #[serde(rename = "Online", default)]
    online: bool,
    #[serde(rename = "LastSeen", default)]
    last_seen: Option<String>,
}

/// Parse the documented `tailscale status --json` shape (`ipn/ipnstate.Status`:
/// `BackendState`, `Self`, `Peer`, per-node `TailscaleIPs`/`Online`/`LastSeen`).
pub fn parse_status_json(raw: &str) -> Result<MeshStatus, MeshError> {
    let status: RawStatus = serde_json::from_str(raw).map_err(|e| MeshError::StatusParse {
        error: e.to_string(),
        snippet: snippet(raw),
    })?;

    let (self_ip, self_hostname, online) = match &status.self_node {
        Some(node) => (
            pick_ip(node.tailscale_ips.as_deref()),
            (!node.host_name.is_empty()).then(|| node.host_name.clone()),
            node.online,
        ),
        None => (None, None, false),
    };

    let mut peers: Vec<MeshPeer> = status
        .peers
        .unwrap_or_default()
        .into_values()
        .map(|node| MeshPeer {
            hostname: node.host_name,
            ip: pick_ip(node.tailscale_ips.as_deref()),
            online: node.online,
            last_seen: normalize_last_seen(node.last_seen),
        })
        .collect();
    peers.sort_by(|a, b| (&a.hostname, &a.ip).cmp(&(&b.hostname, &b.ip)));

    Ok(MeshStatus {
        self_ip,
        self_hostname,
        backend_state: BackendState::from(status.backend_state),
        online,
        peers,
    })
}

fn pick_ip(ips: Option<&[String]>) -> Option<String> {
    let ips = ips?;
    ips.iter()
        .find(|ip| ip.contains('.'))
        .or_else(|| ips.first())
        .cloned()
}

fn normalize_last_seen(last_seen: Option<String>) -> Option<String> {
    last_seen.filter(|t| !t.is_empty() && !t.starts_with("0001-"))
}

fn snippet(raw: &str) -> String {
    let head: String = raw.chars().take(200).collect();
    if head.is_empty() {
        "<empty>".to_string()
    } else {
        head
    }
}

struct ChildHandle {
    child: Child,
    stderr_tail: Arc<StdMutex<VecDeque<String>>>,
}

pub struct MeshEngine {
    config: MeshConfig,
    state: Mutex<Option<ChildHandle>>,
}

impl MeshEngine {
    /// Spawn the goose-owned `tailscaled` and wait until its unix socket answers
    /// `status --json` (any backend state — a fresh daemon reports `NeedsLogin`).
    pub async fn start(config: MeshConfig) -> Result<Self, MeshError> {
        config.validate()?;
        prepare_state_dir(&config.state_dir)?;

        let engine = Self {
            config,
            state: Mutex::new(None),
        };
        let mut handle = engine.spawn_daemon()?;

        let started = Instant::now();
        loop {
            if let Some(status) = handle
                .child
                .try_wait()
                .map_err(|source| MeshError::CliRun {
                    what: "try_wait on tailscaled",
                    program: engine.config.tailscaled_path.display().to_string(),
                    source,
                })?
            {
                return Err(MeshError::DaemonExited {
                    status: status.to_string(),
                    stderr_tail: stderr_tail_string(&handle.stderr_tail),
                });
            }
            if engine.config.socket_path.exists() {
                let probe = run_cli(
                    "tailscale status (readiness probe)",
                    engine.config.status_argv(),
                    READY_PROBE_TIMEOUT,
                )
                .await;
                if let Ok(output) = probe {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    if parse_status_json(&stdout).is_ok() {
                        tracing::info!(
                            socket = %engine.config.socket_path.display(),
                            "leanzero-link tailscaled ready"
                        );
                        *engine.state.lock().await = Some(handle);
                        return Ok(engine);
                    }
                }
            }
            if started.elapsed() >= engine.config.startup_timeout {
                let stderr_tail = stderr_tail_string(&handle.stderr_tail);
                terminate_per_pid(&mut handle.child).await;
                return Err(MeshError::StartupTimeout {
                    waited: engine.config.startup_timeout,
                    stderr_tail,
                });
            }
            tokio::time::sleep(READY_POLL_INTERVAL).await;
        }
    }

    pub fn config(&self) -> &MeshConfig {
        &self.config
    }

    pub async fn pid(&self) -> Option<u32> {
        self.state.lock().await.as_ref().and_then(|h| h.child.id())
    }

    /// Join the tailnet with an injected auth key. The key is a string minted by the
    /// LeanZero Link worker; this crate never talks to any auth backend. Verifies the
    /// backend actually reached `Running` — a green exit code alone is not a join.
    pub async fn join(&self, auth_key: &str, hostname: &str) -> Result<(), MeshError> {
        if auth_key.trim().is_empty() {
            return Err(MeshError::EmptyAuthKey);
        }
        let output = run_cli(
            "tailscale up",
            self.config.up_argv(auth_key, hostname),
            self.config.join_timeout + JOIN_WAIT_GRACE,
        )
        .await?;
        if !output.status.success() {
            return Err(MeshError::JoinFailed {
                stderr: cli_failure_text(&output),
            });
        }
        let status = self.status().await?;
        if status.backend_state != BackendState::Running {
            return Err(MeshError::JoinIncomplete {
                state: status.backend_state,
            });
        }
        Ok(())
    }

    /// Typed mesh status. A daemon that is not up yet (no socket, or nothing answering
    /// behind a stale socket file) is `BackendState::Stopped` — a real state, never an
    /// error dressed as an empty result. Every other failure is a loud typed error.
    pub async fn status(&self) -> Result<MeshStatus, MeshError> {
        if !self.config.socket_path.exists() {
            return Ok(MeshStatus::stopped());
        }
        let output = run_cli(
            "tailscale status",
            self.config.status_argv(),
            self.config.cli_timeout,
        )
        .await?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        match parse_status_json(&stdout) {
            Ok(status) => Ok(status),
            Err(parse_err) => {
                if output.status.success() {
                    Err(parse_err)
                } else {
                    let stderr = cli_failure_text(&output);
                    if is_connect_failure(&stderr) {
                        Ok(MeshStatus::stopped())
                    } else {
                        Err(MeshError::StatusFailed { stderr })
                    }
                }
            }
        }
    }

    /// `tailscale logout` (expires the node key on the control plane), then stops the
    /// daemon. State dir stays intact — a later re-login is fast.
    pub async fn logout(&self) -> Result<(), MeshError> {
        let output = run_cli(
            "tailscale logout",
            self.config.logout_argv(),
            self.config.join_timeout,
        )
        .await?;
        if !output.status.success() {
            return Err(MeshError::LogoutFailed {
                stderr: cli_failure_text(&output),
            });
        }
        self.shutdown().await;
        Ok(())
    }

    /// SIGTERM, a grace window, then SIGKILL — always the daemon pid alone, never a
    /// process group. State dir is left intact.
    pub async fn shutdown(&self) {
        let mut state = self.state.lock().await;
        if let Some(mut handle) = state.take() {
            terminate_per_pid(&mut handle.child).await;
        }
    }

    fn spawn_daemon(&self) -> Result<ChildHandle, MeshError> {
        let argv = self.config.tailscaled_argv();
        let mut cmd = Command::new(&argv[0]);
        cmd.args(&argv[1..])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        configure_subprocess(&mut cmd);
        let mut child = cmd.spawn().map_err(|source| MeshError::Spawn {
            program: argv[0].clone(),
            source,
        })?;

        let stderr_tail = Arc::new(StdMutex::new(VecDeque::with_capacity(STDERR_TAIL_LINES)));
        if let Some(stderr) = child.stderr.take() {
            let tail = Arc::clone(&stderr_tail);
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    tracing::debug!(daemon = "leanzero-tailscaled", "{line}");
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
}

impl Drop for MeshEngine {
    fn drop(&mut self) {
        if let Ok(mut state) = self.state.try_lock() {
            if let Some(handle) = state.as_mut() {
                let _ = handle.child.start_kill();
            }
        }
        // kill_on_drop(true) covers the path where the lock is held elsewhere.
    }
}

fn prepare_state_dir(dir: &Path) -> Result<(), MeshError> {
    std::fs::create_dir_all(dir).map_err(|source| MeshError::StateDir {
        path: dir.to_path_buf(),
        source,
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).map_err(
            |source| MeshError::StateDir {
                path: dir.to_path_buf(),
                source,
            },
        )?;
    }
    Ok(())
}

async fn run_cli(
    what: &'static str,
    argv: Vec<String>,
    wait: Duration,
) -> Result<Output, MeshError> {
    let mut cmd = Command::new(&argv[0]);
    cmd.args(&argv[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let program = argv[0].clone();
    match tokio::time::timeout(wait, cmd.output()).await {
        Err(_) => Err(MeshError::CliTimeout { what, waited: wait }),
        Ok(Err(source)) => Err(MeshError::CliRun {
            what,
            program,
            source,
        }),
        Ok(Ok(output)) => Ok(output),
    }
}

fn cli_failure_text(output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut text = stderr.trim().to_string();
    if text.is_empty() {
        text = stdout.trim().to_string();
    }
    if text.is_empty() {
        text = format!("(no output; exit {})", output.status);
    }
    text
}

fn is_connect_failure(stderr: &str) -> bool {
    let lower = stderr.to_lowercase();
    lower.contains("failed to connect")
        || lower.contains("connection refused")
        || lower.contains("no such file or directory")
        || lower.contains("is tailscale running")
        || lower.contains("is it running")
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
