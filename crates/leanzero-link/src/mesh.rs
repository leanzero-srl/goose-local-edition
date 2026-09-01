//! Goose-owned Tailscale mesh engine: spawns and supervises an isolated userspace
//! `tailscaled`, joins a tailnet with an injected auth key, and reports typed status.
//!
//! The supervision idiom (spawn via `configure_subprocess`, stderr tail, readiness
//! polling, per-pid termination) mirrors `goose-sidecar`'s `Sidecar`; that type is not
//! reused because its readiness contract is an HTTP `GET {base_url}/v1/models` probe,
//! which does not fit a unix-socket daemon whose readiness is "the local API socket
//! answers `tailscale status --json`".
//!
//! Single-instance discipline: `start` probes the socket BEFORE spawning and refuses
//! ([`MeshError::AlreadyRunning`]) when a daemon already answers there — this crate
//! never adopts, drives, or logs out a `tailscaled` it did not spawn. (A second
//! `tailscaled` on the same socket exits within milliseconds with "address already in
//! use", so without the probe the readiness check would be answered by the OTHER
//! process's daemon and `start` would return holding a dead child.)

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
    #[error(
        "a tailscaled already answers on socket '{}' (listener pid {}) — LeanZero Link never \
         adopts a daemon it did not spawn; stop the other goose (or the stale daemon) first",
        socket.display(),
        listener_pid.map_or("unknown".to_string(), |p| p.to_string())
    )]
    AlreadyRunning {
        socket: PathBuf,
        /// The pid the kernel reports behind the socket (peer credentials), when readable.
        listener_pid: Option<u32>,
    },
    /// The supervised daemon is gone — raised by `start` (died before the socket
    /// answered) and by every later `status` read (died under a live connection).
    #[error("tailscaled exited ({status}). stderr tail:\n{stderr_tail}")]
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
    #[error("cannot write the private auth-key file '{}': {source}", path.display())]
    AuthKeyFile {
        path: PathBuf,
        source: std::io::Error,
    },
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
    /// from `join_timeout`. The auth key is never on argv (ps-visible to every local
    /// user): it is passed as `--auth-key=file:<path>` — `tailscale up --help` on 1.98.5:
    /// "node authorization key; if it begins with "file:", then it's a path to a file
    /// containing the authkey" — and [`MeshEngine::join`] owns that file's lifetime.
    pub fn up_argv(&self, auth_key_file: &Path, hostname: &str) -> Vec<String> {
        let mut argv = self.cli_prefix();
        argv.push("up".to_string());
        argv.push(format!("--auth-key=file:{}", auth_key_file.display()));
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
    ///
    /// Before spawning, the socket is probed: a daemon already answering there is
    /// [`MeshError::AlreadyRunning`] — never adopted. Readiness itself is PROVEN, not
    /// inferred: after `status --json` answers, the kernel's peer credentials on a fresh
    /// connection ([`listener_pid`]) must name OUR child as the listener. A child that
    /// died meanwhile is [`MeshError::DaemonExited`]; a listener that is not our child
    /// (the pre-spawn probe was fooled by a transient CLI failure) is
    /// [`MeshError::AlreadyRunning`] with our own spawn terminated per-pid — never an
    /// engine holding a corpse over someone else's daemon.
    pub async fn start(config: MeshConfig) -> Result<Self, MeshError> {
        config.validate()?;
        prepare_state_dir(&config.state_dir)?;
        // The pre-spawn probe waits the full CLI timeout: a daemon that answers SLOWLY
        // (a loaded machine) still owns the socket, and a probe cut short here would
        // spawn a second daemon on top of it.
        if config.socket_path.exists() && probe_socket(&config, config.cli_timeout).await.is_ok() {
            return Err(MeshError::AlreadyRunning {
                listener_pid: listener_pid(&config.socket_path).ok(),
                socket: config.socket_path.clone(),
            });
        }

        let engine = Self {
            config,
            state: Mutex::new(None),
        };
        let mut handle = engine.spawn_daemon()?;

        let started = Instant::now();
        loop {
            if let Some(status) = child_exit(&mut handle, &engine.config)? {
                return Err(MeshError::DaemonExited {
                    status: status.to_string(),
                    stderr_tail: stderr_tail_string(&handle.stderr_tail),
                });
            }
            if engine.config.socket_path.exists()
                && probe_socket(&engine.config, READY_PROBE_TIMEOUT)
                    .await
                    .is_ok()
            {
                if let Some(status) = child_exit(&mut handle, &engine.config)? {
                    return Err(MeshError::DaemonExited {
                        status: status.to_string(),
                        stderr_tail: stderr_tail_string(&handle.stderr_tail),
                    });
                }
                match listener_pid(&engine.config.socket_path) {
                    Ok(pid) if Some(pid) == handle.child.id() => {
                        tracing::info!(
                            socket = %engine.config.socket_path.display(),
                            pid,
                            "leanzero-link tailscaled ready"
                        );
                        *engine.state.lock().await = Some(handle);
                        return Ok(engine);
                    }
                    Ok(pid) => {
                        tracing::error!(
                            socket = %engine.config.socket_path.display(),
                            listener_pid = pid,
                            our_pid = ?handle.child.id(),
                            "a daemon we did not spawn answers on our socket; \
                             terminating our own spawn per-pid"
                        );
                        terminate_per_pid(&mut handle.child).await;
                        return Err(MeshError::AlreadyRunning {
                            socket: engine.config.socket_path.clone(),
                            listener_pid: Some(pid),
                        });
                    }
                    Err(err) => {
                        tracing::warn!(
                            error = %err,
                            "socket answered but its listener pid is not readable yet; retrying"
                        );
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
    ///
    /// The key is written `0600` under the state dir (a `0700` directory) and handed to
    /// the CLI as `--auth-key=file:<path>`, so it never appears in `ps`; the file is
    /// removed as soon as `up` returns, success or failure (R-L1).
    pub async fn join(&self, auth_key: &str, hostname: &str) -> Result<(), MeshError> {
        if auth_key.trim().is_empty() {
            return Err(MeshError::EmptyAuthKey);
        }
        let key_path = self
            .config
            .state_dir
            .join(format!("auth-key.{}", std::process::id()));
        write_private(&key_path, auth_key.as_bytes()).map_err(|source| MeshError::AuthKeyFile {
            path: key_path.clone(),
            source,
        })?;
        let result = run_cli(
            "tailscale up",
            self.config.up_argv(&key_path, hostname),
            self.config.join_timeout + JOIN_WAIT_GRACE,
        )
        .await;
        if let Err(err) = std::fs::remove_file(&key_path) {
            tracing::error!(
                path = %key_path.display(),
                error = %err,
                "could not remove the auth-key file after `tailscale up`"
            );
        }
        let output = result?;
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

    /// Typed mesh status. The held daemon is `try_wait`ed FIRST: a child that has exited
    /// is [`MeshError::DaemonExited`] (with its stderr tail) on every call until
    /// [`Self::shutdown`] — never a calm `Stopped`. `BackendState::Stopped` is honest only
    /// when this engine holds NO child (never started, or explicitly shut down) and
    /// nothing listens behind the socket path. With a live child every probe failure —
    /// a vanished socket, EACCES, an unparseable answer — is a loud typed error, because
    /// a daemon we supervise that cannot be talked to is a fault, not an absence.
    pub async fn status(&self) -> Result<MeshStatus, MeshError> {
        let live_pid = {
            let mut state = self.state.lock().await;
            match state.as_mut() {
                None => None,
                Some(handle) => {
                    if let Some(status) =
                        handle
                            .child
                            .try_wait()
                            .map_err(|source| MeshError::CliRun {
                                what: "try_wait on tailscaled",
                                program: self.config.tailscaled_path.display().to_string(),
                                source,
                            })?
                    {
                        return Err(MeshError::DaemonExited {
                            status: status.to_string(),
                            stderr_tail: stderr_tail_string(&handle.stderr_tail),
                        });
                    }
                    Some(handle.child.id())
                }
            }
        };
        match live_pid {
            None => self.status_with_no_child().await,
            Some(pid) => self.status_of_live_child(pid).await,
        }
    }

    /// No child held: absence is a real state. A missing socket, or a socket nobody
    /// listens behind (ENOENT / ECONNREFUSED from the CLI), is `Stopped`; anything else
    /// the CLI reports is an error.
    async fn status_with_no_child(&self) -> Result<MeshStatus, MeshError> {
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

    /// A live child is held: nothing here may read as `Stopped`.
    async fn status_of_live_child(&self, pid: Option<u32>) -> Result<MeshStatus, MeshError> {
        if !self.config.socket_path.exists() {
            return Err(MeshError::StatusFailed {
                stderr: format!(
                    "socket '{}' is missing while the supervised tailscaled (pid {}) is alive",
                    self.config.socket_path.display(),
                    pid.map_or("unknown".to_string(), |p| p.to_string())
                ),
            });
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
            Err(parse_err) if output.status.success() => Err(parse_err),
            Err(_) => Err(MeshError::StatusFailed {
                stderr: cli_failure_text(&output),
            }),
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

/// `try_wait` the spawned daemon; `Some(status)` means it is gone.
fn child_exit(
    handle: &mut ChildHandle,
    config: &MeshConfig,
) -> Result<Option<std::process::ExitStatus>, MeshError> {
    handle.child.try_wait().map_err(|source| MeshError::CliRun {
        what: "try_wait on tailscaled",
        program: config.tailscaled_path.display().to_string(),
        source,
    })
}

/// The pid of the process LISTENING behind `socket_path`, read from the kernel's peer
/// credentials on a fresh (never accepted) connection: macOS `LOCAL_PEERPID` (measured
/// on 26.6: an un-accepted connection reports the listener's pid), Linux `SO_PEERCRED`.
/// This is the proof `status --json` cannot give — WHO answers, not just that someone
/// does. Any other platform is a loud `Unsupported` (readiness then times out with that
/// text), never an assumption.
#[cfg(unix)]
fn listener_pid(socket_path: &Path) -> std::io::Result<u32> {
    use std::os::unix::io::AsRawFd;
    let stream = std::os::unix::net::UnixStream::connect(socket_path)?;
    let fd = stream.as_raw_fd();
    #[cfg(target_os = "macos")]
    {
        let mut pid: libc::pid_t = 0;
        let mut len = std::mem::size_of::<libc::pid_t>() as libc::socklen_t;
        let rc = unsafe {
            libc::getsockopt(
                fd,
                libc::SOL_LOCAL,
                libc::LOCAL_PEERPID,
                &mut pid as *mut libc::pid_t as *mut libc::c_void,
                &mut len,
            )
        };
        if rc != 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(pid as u32)
    }
    #[cfg(target_os = "linux")]
    {
        let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
        let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
        let rc = unsafe {
            libc::getsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                &mut cred as *mut libc::ucred as *mut libc::c_void,
                &mut len,
            )
        };
        if rc != 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(cred.pid as u32)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = fd;
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "listener pid proof (peer credentials) is not implemented on this platform",
        ))
    }
}

#[cfg(not(unix))]
fn listener_pid(_socket_path: &Path) -> std::io::Result<u32> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "listener pid proof (peer credentials) is not implemented on this platform",
    ))
}

/// Does a tailscaled answer `status --json` on the configured socket within `wait`?
/// `Err` carries why not (CLI failure text or the parse error) for the caller's record.
async fn probe_socket(config: &MeshConfig, wait: Duration) -> Result<(), String> {
    let output = run_cli(
        "tailscale status (readiness probe)",
        config.status_argv(),
        wait,
    )
    .await
    .map_err(|err| err.to_string())?;
    if !output.status.success() {
        return Err(cli_failure_text(&output));
    }
    parse_status_json(&String::from_utf8_lossy(&output.stdout))
        .map(|_| ())
        .map_err(|err| err.to_string())
}

/// Create-or-truncate `path` with `0600` and write `bytes`. The mode is applied at open
/// AND re-applied afterwards (`OpenOptions::mode` only affects creation).
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut file = opts.open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    file.write_all(bytes)?;
    file.sync_all()
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

/// True only for the two errno texts that mean "nothing listens behind this socket
/// path". Measured on the bundled `tailscale` 1.98.5 (`--socket=<p> status --json`,
/// no daemon involved), whose stderr ends `dial unix <p>: connect: <errno text>`:
/// - ENOENT `no such file or directory` — the socket file is gone;
/// - ECONNREFUSED `connection refused` — a socket file left behind by a dead daemon.
///
/// Every other errno the same probe produced is a real fault and must stay an error:
/// EACCES `permission denied`, ENOTSOCK `socket operation on non-socket` (a plain file
/// or directory at the path), EINVAL `invalid argument` (a path longer than `sun_path`).
/// The CLI wraps ALL of them in the same "failed to connect to local tailscaled …
/// not running?" prose, which is why the prose is never matched here.
fn is_connect_failure(stderr: &str) -> bool {
    let lower = stderr.to_lowercase();
    lower.contains("no such file or directory") || lower.contains("connection refused")
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

#[cfg(test)]
mod tests {
    use super::is_connect_failure;

    const PROSE: &str = "failed to connect to local tailscaled (which appears to be running as \
        tailscaled, pid 323). Got error: Failed to connect to local Tailscale daemon for \
        /localapi/v0/status; not running? Error: dial unix /tmp/lzp/sock: connect: ";

    #[test]
    fn only_enoent_and_econnrefused_mean_nothing_is_listening() {
        assert!(is_connect_failure(&format!(
            "{PROSE}no such file or directory"
        )));
        assert!(is_connect_failure(&format!("{PROSE}connection refused")));
    }

    #[test]
    fn faults_wrapped_in_the_same_prose_stay_errors() {
        for errno in [
            "permission denied",
            "socket operation on non-socket",
            "invalid argument",
        ] {
            assert!(
                !is_connect_failure(&format!("{PROSE}{errno}")),
                "{errno} must not read as Stopped"
            );
        }
        assert!(
            !is_connect_failure("failed to connect to local tailscaled; is it running?"),
            "the wrapper prose alone carries no errno and must not match"
        );
    }
}
