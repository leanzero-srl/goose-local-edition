//! [`LinkManager`] — the top-level LeanZero Link engine goosed drives.
//!
//! It composes the four landed pieces into one state machine:
//! - [`crate::identity`] — the persisted account credential.
//! - [`crate::worker_client`] — the auth worker (OTP → identity token → mesh join key).
//! - [`crate::mesh`] — the goose-owned userspace Tailscale daemon.
//! - [`crate::control`] — the `/v1/swarm` node-to-node service and peer fabric.
//!
//! goosed constructs one `LinkManager`, exposes its methods as `leanzeroLink/*` ACP
//! methods, and surfaces [`LinkState`] to the desktop UI. The manager owns the auth
//! lifecycle (request code → verify → connect → logout) and, while connected, keeps the
//! control service's peer fabric reconciled against live mesh status.
//!
//! ## Mesh seam
//! The manager depends on the [`Mesh`] trait, not on [`crate::mesh::MeshEngine`]
//! directly, so tests inject a fake and no real `tailscaled` starts. The real engine
//! implements [`Mesh`]; [`RealMeshFactory`] is the production factory. The single guarded
//! live path stays exactly where it is — the mesh crate's own `#[ignore]` test.

use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

use crate::control::{ControlConfig, ControlError, ControlHandle, ControlService};
use crate::identity::{Identity, IdentityError, IdentityStore};
use crate::mesh::{MeshConfig, MeshEngine, MeshError, MeshPeer, MeshStatus};
use crate::state::{
    ExecuteAccepted, ExecuteError, ExecuteRequest, MlxControl, MlxControlError, MlxOp,
    PeerRegistry, RemoteExecutor, SwarmStateSource,
};
use crate::worker_client::{
    RequestCodeResult, VerifyResult, WorkerClient, WorkerError, DEFAULT_WORKER_BASE_URL,
};

/// The abstract mesh the manager drives. [`MeshEngine`] is the production impl; tests
/// supply a fake so `connect()` never spawns a daemon.
#[async_trait::async_trait]
pub trait Mesh: Send + Sync {
    async fn join(&self, auth_key: &str, hostname: &str) -> Result<(), MeshError>;
    async fn status(&self) -> Result<MeshStatus, MeshError>;
    async fn logout(&self) -> Result<(), MeshError>;
    async fn shutdown(&self);
}

#[async_trait::async_trait]
impl Mesh for MeshEngine {
    async fn join(&self, auth_key: &str, hostname: &str) -> Result<(), MeshError> {
        MeshEngine::join(self, auth_key, hostname).await
    }
    async fn status(&self) -> Result<MeshStatus, MeshError> {
        MeshEngine::status(self).await
    }
    async fn logout(&self) -> Result<(), MeshError> {
        MeshEngine::logout(self).await
    }
    async fn shutdown(&self) {
        MeshEngine::shutdown(self).await
    }
}

/// Starts a [`Mesh`] from a [`MeshConfig`]. Injected so tests never boot `tailscaled`.
#[async_trait::async_trait]
pub trait MeshFactory: Send + Sync {
    async fn start(&self, config: MeshConfig) -> Result<Arc<dyn Mesh>, MeshError>;
}

/// The production factory: spawns the real supervised userspace `tailscaled`.
pub struct RealMeshFactory;

#[async_trait::async_trait]
impl MeshFactory for RealMeshFactory {
    async fn start(&self, config: MeshConfig) -> Result<Arc<dyn Mesh>, MeshError> {
        Ok(Arc::new(MeshEngine::start(config).await?))
    }
}

/// Everything the manager needs to construct itself. `worker_base_url` and
/// `identity_path` are overridable for tests; `mesh` and `control` template the mesh
/// daemon and the `/v1/swarm` service (their `hostname` / `mesh_ip` are filled in at
/// `connect()` time).
#[derive(Debug, Clone)]
pub struct LinkManagerConfig {
    pub worker_base_url: String,
    pub identity_path: PathBuf,
    pub mesh: MeshConfig,
    pub control: ControlConfig,
}

/// The full auth + mesh lifecycle state, serialized to the UI. Serde tag `state`
/// discriminates the variant; payload fields ride alongside it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state")]
pub enum AuthState {
    LoggedOut,
    CodeSent {
        email: String,
        expires_at: DateTime<Utc>,
    },
    LoggedIn {
        email: String,
    },
    Connecting {
        email: String,
    },
    Connected {
        email: String,
        mesh_ip: String,
    },
}

/// What goosed surfaces to the desktop: the auth state, live mesh status while
/// connected, the total node count (self + reachable peers), and the last error — the
/// error is never swallowed, it rides here for the UI to show honestly.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LinkState {
    pub auth: AuthState,
    pub mesh: Option<MeshStatus>,
    pub node_count: u32,
    pub last_error: Option<String>,
}

#[derive(Debug, Error)]
pub enum LinkError {
    #[error(transparent)]
    Worker(#[from] WorkerError),
    #[error(transparent)]
    Mesh(#[from] MeshError),
    #[error(transparent)]
    Control(#[from] ControlError),
    #[error(transparent)]
    Identity(#[from] IdentityError),
    #[error("not logged in — verify an email code first")]
    NotLoggedIn,
    #[error("busy: a connect is already in progress or the mesh is connected")]
    Busy,
    #[error("remote execution is not wired on this node")]
    ExecutorUnavailable,
    #[error("not connected to the mesh — cannot reach peers for remote execution")]
    NotConnected,
    #[error("no known mesh peer with node id '{0}'")]
    UnknownPeer(String),
    #[error(transparent)]
    Execute(#[from] ExecuteError),
    #[error("remote execute request to a peer failed: {0}")]
    RemoteExecute(String),
    #[error("mlx control is not wired on this node")]
    MlxControlUnavailable,
    #[error(transparent)]
    MlxControl(#[from] MlxControlError),
    #[error("mlx proxy request to a peer failed: {0}")]
    MlxProxy(String),
    #[error("mesh joined but reported no IP — cannot compose a Connected state")]
    NoMeshIp,
    #[error("mesh reported an unparseable self IP '{ip}': {source}")]
    BadMeshIp {
        ip: String,
        source: std::net::AddrParseError,
    },
    #[error("cannot {op} '{}': {source}", path.display())]
    Io {
        op: &'static str,
        path: PathBuf,
        source: std::io::Error,
    },
}

/// A live connection's owned resources. Torn down per-pid on logout / failure.
/// `control` is an `Option` so `logout` can `take()` it and call the by-value
/// `ControlHandle::shutdown` without partial-moving out of a `Drop` type.
struct Active {
    mesh: Arc<dyn Mesh>,
    control: Option<ControlHandle>,
    registry: PeerRegistry,
    poll_task: JoinHandle<()>,
    mesh_ip: String,
}

impl Drop for Active {
    /// Belt-and-suspenders: if an `Active` is dropped without an explicit `logout`
    /// (e.g. the manager itself is dropped), stop the mesh-status poll loop. The
    /// `ControlHandle` and `PeerRegistry` abort their own tasks on drop.
    fn drop(&mut self) {
        self.poll_task.abort();
    }
}

struct Inner {
    auth: AuthState,
    last_error: Option<String>,
    active: Option<Active>,
}

/// Distinguishes a connect failure that should drop to `LoggedOut` (the token is dead)
/// from one that keeps the user `LoggedIn` (the mesh/control step failed, auth is fine).
struct ConnectFailure {
    error: LinkError,
    logout: bool,
}

impl From<LinkError> for ConnectFailure {
    fn from(error: LinkError) -> Self {
        Self {
            error,
            logout: false,
        }
    }
}
impl From<MeshError> for ConnectFailure {
    fn from(error: MeshError) -> Self {
        Self {
            error: error.into(),
            logout: false,
        }
    }
}
impl From<IdentityError> for ConnectFailure {
    fn from(error: IdentityError) -> Self {
        Self {
            error: error.into(),
            logout: false,
        }
    }
}
impl From<WorkerError> for ConnectFailure {
    fn from(error: WorkerError) -> Self {
        Self {
            error: error.into(),
            logout: false,
        }
    }
}
impl From<ControlError> for ConnectFailure {
    fn from(error: ControlError) -> Self {
        Self {
            error: error.into(),
            logout: false,
        }
    }
}

pub struct LinkManager {
    config: LinkManagerConfig,
    identity: IdentityStore,
    worker: WorkerClient,
    mesh_factory: Arc<dyn MeshFactory>,
    source: Arc<dyn SwarmStateSource>,
    /// The local remote-execute seam, injected beside `source` (mirroring how the
    /// [`SwarmStateSource`] is threaded). `None` → this node cannot run remote prompts
    /// (self short-circuit and the control route both answer as unavailable). goose-server
    /// supplies the real one via `set_executor` before the manager is built.
    executor: Option<Arc<dyn RemoteExecutor>>,
    /// The local MLX-engine seam, injected beside `executor`. `None` → this node cannot
    /// run remote model-management ops (its `/v1/swarm/mlx/*` routes answer `501` and the
    /// self short-circuit in [`Self::mlx_proxy`] is unavailable). goose supplies the real
    /// one (`GoosedMlxControl`) before the manager is built.
    mlx_control: Option<Arc<dyn MlxControl>>,
    inner: Mutex<Inner>,
}

impl LinkManager {
    /// Construct with the production mesh factory. Loads any persisted identity:
    /// present → `LoggedIn` (it does NOT auto-connect; goosed calls [`Self::connect`]);
    /// absent → `LoggedOut`; malformed → a loud error (never silently logged-out).
    pub fn new(
        config: LinkManagerConfig,
        source: Arc<dyn SwarmStateSource>,
    ) -> Result<Self, LinkError> {
        Self::with_mesh_factory(config, source, Arc::new(RealMeshFactory))
    }

    /// As [`Self::new`], but with an injected mesh factory (tests supply a fake so no
    /// `tailscaled` is spawned).
    pub fn with_mesh_factory(
        config: LinkManagerConfig,
        source: Arc<dyn SwarmStateSource>,
        mesh_factory: Arc<dyn MeshFactory>,
    ) -> Result<Self, LinkError> {
        let identity = IdentityStore::new(config.identity_path.clone());
        let worker = WorkerClient::new(config.worker_base_url.clone())?;
        let auth = match identity.load()? {
            Some(id) => AuthState::LoggedIn { email: id.email },
            None => AuthState::LoggedOut,
        };
        Ok(Self {
            config,
            identity,
            worker,
            mesh_factory,
            source,
            executor: None,
            mlx_control: None,
            inner: Mutex::new(Inner {
                auth,
                last_error: None,
                active: None,
            }),
        })
    }

    /// Attach the local [`RemoteExecutor`] (goose-server's `GoosedRemoteExecutor`). Used
    /// by the `POST /v1/swarm/execute` route this node serves AND by the self short-circuit
    /// in [`Self::remote_execute`]. A builder-style setter rather than a constructor arg so
    /// the existing construction paths (and their tests) stay unchanged.
    pub fn with_executor(mut self, executor: Arc<dyn RemoteExecutor>) -> Self {
        self.executor = Some(executor);
        self
    }

    /// Attach the local [`MlxControl`] (goose's `GoosedMlxControl`). Used by the
    /// `POST /v1/swarm/mlx/*` routes this node serves AND by the self short-circuit in
    /// [`Self::mlx_proxy`]. A builder-style setter, like [`Self::with_executor`], so the
    /// existing construction paths (and their tests) stay unchanged.
    pub fn with_mlx_control(mut self, mlx_control: Arc<dyn MlxControl>) -> Self {
        self.mlx_control = Some(mlx_control);
        self
    }

    /// A default config: production worker URL, `~/.leanzero/identity.json`, and the
    /// given mesh/control templates. A convenience for goosed's construction path.
    pub fn default_config(
        identity_path: PathBuf,
        mesh: MeshConfig,
        control: ControlConfig,
    ) -> LinkManagerConfig {
        LinkManagerConfig {
            worker_base_url: DEFAULT_WORKER_BASE_URL.to_string(),
            identity_path,
            mesh,
            control,
        }
    }

    /// `GET /v1/health` passthrough so the UI can show what the deployment supports.
    pub async fn health(&self) -> Result<crate::worker_client::Health, LinkError> {
        Ok(self.worker.health().await?)
    }

    /// Request an email OTP → `CodeSent`. Refused while connecting/connected.
    pub async fn request_code(&self, email: &str) -> Result<RequestCodeResult, LinkError> {
        self.ensure_not_busy().await?;
        match self.worker.request_code(email).await {
            Ok(result) => {
                let expires_at =
                    Utc::now() + chrono::Duration::seconds(result.expires_in_seconds as i64);
                let mut inner = self.inner.lock().await;
                inner.auth = AuthState::CodeSent {
                    email: result.email.clone(),
                    expires_at,
                };
                inner.last_error = None;
                Ok(result)
            }
            Err(err) => {
                self.record_error(&err).await;
                Err(err.into())
            }
        }
    }

    /// Verify the OTP → persist identity, `LoggedIn`. Returns the worker's
    /// `audienceSync` verdict so the UI can note a contact-sync failure honestly. A
    /// failed verify keeps the current (logged-out) state and records the error.
    pub async fn verify(&self, email: &str, code: &str) -> Result<VerifyResult, LinkError> {
        self.ensure_not_busy().await?;
        let result = match self.worker.verify(email, code).await {
            Ok(result) => result,
            Err(err) => {
                self.record_error(&err).await;
                return Err(err.into());
            }
        };
        self.identity
            .save(&Identity::new(result.email.clone(), result.token.clone()))?;
        let mut inner = self.inner.lock().await;
        inner.auth = AuthState::LoggedIn {
            email: result.email.clone(),
        };
        inner.last_error = None;
        Ok(result)
    }

    /// Bring up the mesh + control service (requires `LoggedIn`). On any step failing
    /// the resources are torn down per-pid and the state returns to `LoggedIn` (still
    /// authed) with `last_error` set — except a join-key `401` (the 180-day token
    /// expired/was rejected), which clears the identity and drops to `LoggedOut`.
    pub async fn connect(&self) -> Result<(), LinkError> {
        let email = {
            let mut inner = self.inner.lock().await;
            let email = match &inner.auth {
                AuthState::LoggedIn { email } => email.clone(),
                AuthState::Connecting { .. } | AuthState::Connected { .. } => {
                    return Err(LinkError::Busy)
                }
                AuthState::LoggedOut | AuthState::CodeSent { .. } => {
                    return Err(LinkError::NotLoggedIn)
                }
            };
            inner.auth = AuthState::Connecting {
                email: email.clone(),
            };
            inner.last_error = None;
            email
        };

        match self.connect_inner().await {
            Ok(active) => {
                let mut inner = self.inner.lock().await;
                inner.auth = AuthState::Connected {
                    email,
                    mesh_ip: active.mesh_ip.clone(),
                };
                inner.active = Some(active);
                Ok(())
            }
            Err(ConnectFailure { error, logout }) => {
                let mut inner = self.inner.lock().await;
                inner.auth = if logout {
                    AuthState::LoggedOut
                } else {
                    AuthState::LoggedIn { email }
                };
                inner.last_error = Some(error.to_string());
                Err(error)
            }
        }
    }

    async fn connect_inner(&self) -> Result<Active, ConnectFailure> {
        let identity = self.identity.load()?.ok_or(ConnectFailure {
            error: LinkError::NotLoggedIn,
            logout: true,
        })?;

        let node_hostname = self.node_hostname()?;

        let key = match self.worker.join_key(&identity.token).await {
            Ok(key) => key,
            Err(err @ (WorkerError::AuthExpired { .. } | WorkerError::AuthInvalid { .. })) => {
                if let Err(clear_err) = self.identity.clear() {
                    tracing::error!(error = %clear_err, "failed to clear the dead identity");
                }
                return Err(ConnectFailure {
                    error: err.into(),
                    logout: true,
                });
            }
            Err(err) => return Err(err.into()),
        };

        let mut mesh_config = self.config.mesh.clone();
        mesh_config.hostname = node_hostname.clone();
        // A Headscale key carries the control server it belongs to; join against that,
        // not the configured default. Absent/blank → keep the template's login server.
        if let Some(login_server) = key
            .login_server
            .clone()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
        {
            mesh_config.login_server = login_server;
        }
        let mesh = self.mesh_factory.start(mesh_config).await?;

        if let Err(err) = mesh.join(&key.auth_key, &node_hostname).await {
            mesh.shutdown().await;
            return Err(err.into());
        }

        let status = match mesh.status().await {
            Ok(status) => status,
            Err(err) => {
                mesh.shutdown().await;
                return Err(err.into());
            }
        };
        let mesh_ip = match status.self_ip.clone() {
            Some(ip) => ip,
            None => {
                mesh.shutdown().await;
                return Err(ConnectFailure {
                    error: LinkError::NoMeshIp,
                    logout: false,
                });
            }
        };
        let mesh_ip_addr: IpAddr = match mesh_ip.parse() {
            Ok(addr) => addr,
            Err(source) => {
                mesh.shutdown().await;
                return Err(ConnectFailure {
                    error: LinkError::BadMeshIp {
                        ip: mesh_ip.clone(),
                        source,
                    },
                    logout: false,
                });
            }
        };

        let mut control_config = self.config.control.clone();
        control_config.mesh_ip = Some(mesh_ip_addr);
        let control = match ControlService::start(
            control_config,
            self.source.clone(),
            self.executor.clone(),
            self.mlx_control.clone(),
        )
        .await
        {
            Ok(control) => control,
            Err(err) => {
                mesh.shutdown().await;
                return Err(err.into());
            }
        };

        // Peers are reached at the SHARED, fixed control port on their mesh IP, not at
        // this node's (possibly ephemeral) local port.
        let peer_port = self.config.control.port;
        let registry = control.registry().clone();
        registry.set_mesh_peers(&status.peers, peer_port);

        let poll_task = tokio::spawn(peer_sync_loop(
            mesh.clone(),
            registry.clone(),
            self.config.control.poll_interval,
            peer_port,
        ));

        Ok(Active {
            mesh,
            control: Some(control),
            registry,
            poll_task,
            mesh_ip,
        })
    }

    /// The composed live view: auth + a live mesh status read + the control node count.
    pub async fn status(&self) -> LinkState {
        let (auth, persisted_error, mesh, registry) = {
            let inner = self.inner.lock().await;
            let (mesh, registry) = match &inner.active {
                Some(active) => (Some(active.mesh.clone()), Some(active.registry.clone())),
                None => (None, None),
            };
            (inner.auth.clone(), inner.last_error.clone(), mesh, registry)
        };

        let mut live_error = None;
        let mesh_status = match mesh {
            Some(mesh) => match mesh.status().await {
                Ok(status) => Some(status),
                Err(err) => {
                    live_error = Some(format!("mesh status read failed: {err}"));
                    None
                }
            },
            None => None,
        };
        let node_count = match &registry {
            Some(registry) => 1 + registry.peer_nodes().len() as u32,
            None => 0,
        };

        LinkState {
            auth,
            mesh: mesh_status,
            node_count,
            last_error: live_error.or(persisted_error),
        }
    }

    /// The live peer-fabric registry while connected (`None` otherwise). Exposed so the
    /// swarm dispatcher / UI can read peer node states (and their Idle/Busy status) before
    /// dispatching, and so [`Self::remote_execute`] can resolve a target's URL.
    pub async fn active_registry(&self) -> Option<PeerRegistry> {
        self.inner
            .lock()
            .await
            .active
            .as_ref()
            .map(|active| active.registry.clone())
    }

    /// Drive a remote execution: tell `target_node_id`'s goose to run `req`. This is how
    /// node A acts on node B. A `target_node_id` equal to this node's own id short-circuits
    /// to the local executor (no network hop); any other id is resolved to a peer via the
    /// fabric registry and reached with `POST <peer>/v1/swarm/execute` (bearer node_token).
    ///
    /// The RECEIVE-side idle guard + `allow_remote_execution` gate live on the peer's route
    /// (a busy/observe-only peer answers `409`/`403`, surfaced here as
    /// [`LinkError::Execute`]). Callers SHOULD still read the peer's Idle status from
    /// [`Self::active_registry`] first and pick an Idle node — the route guard is the
    /// backstop, not the scheduler.
    pub async fn remote_execute(
        &self,
        target_node_id: &str,
        req: ExecuteRequest,
    ) -> Result<ExecuteAccepted, LinkError> {
        let self_node_id = self.source.local_node().await.node_id;
        if target_node_id == self_node_id {
            let executor = self
                .executor
                .clone()
                .ok_or(LinkError::ExecutorUnavailable)?;
            return Ok(executor.execute(req).await?);
        }

        let (base_url, token) = {
            let inner = self.inner.lock().await;
            let registry = inner
                .active
                .as_ref()
                .map(|active| active.registry.clone())
                .ok_or(LinkError::NotConnected)?;
            let base_url = registry
                .peer_base_url(target_node_id)
                .ok_or_else(|| LinkError::UnknownPeer(target_node_id.to_string()))?;
            (base_url, self.config.control.node_token.clone())
        };

        post_peer_execute(&base_url, &token, self.config.control.request_timeout, &req).await
    }

    /// Forward one mlxEngine model-management op to `target_node_id`. This is how node A
    /// runs a download/delete/settings-change/status-read against node B's LOCAL MLX
    /// engine. `target_node_id` equal to this node's own id short-circuits to the local
    /// [`MlxControl`] (no network hop); any other id is resolved to a peer via the fabric
    /// registry and reached with `POST <peer>/v1/swarm/mlx/<op>` (bearer node_token). `body`
    /// is the op's request DTO as opaque JSON; the `Ok` value is the op's response DTO as
    /// opaque JSON. A peer's own failure surfaces as [`LinkError::MlxControl`] (verbatim
    /// text, class preserved); an unreachable/odd peer as [`LinkError::MlxProxy`].
    pub async fn mlx_proxy(
        &self,
        target_node_id: &str,
        op: MlxOp,
        body: serde_json::Value,
    ) -> Result<serde_json::Value, LinkError> {
        let self_node_id = self.source.local_node().await.node_id;
        if target_node_id == self_node_id {
            let control = self
                .mlx_control
                .clone()
                .ok_or(LinkError::MlxControlUnavailable)?;
            return Ok(control.dispatch(op, body).await?);
        }

        let (base_url, token) = {
            let inner = self.inner.lock().await;
            let registry = inner
                .active
                .as_ref()
                .map(|active| active.registry.clone())
                .ok_or(LinkError::NotConnected)?;
            let base_url = registry
                .peer_base_url(target_node_id)
                .ok_or_else(|| LinkError::UnknownPeer(target_node_id.to_string()))?;
            (base_url, self.config.control.node_token.clone())
        };

        post_peer_mlx(
            &base_url,
            &token,
            self.config.control.request_timeout,
            op,
            &body,
        )
        .await
    }

    /// Tear down the connection (per-pid), clear the stored identity, and drop to
    /// `LoggedOut`. The mesh state dir is left for a fast re-login unless `wipe`.
    pub async fn logout(&self, wipe: bool) -> Result<(), LinkError> {
        let active = {
            let mut inner = self.inner.lock().await;
            inner.active.take()
        };
        if let Some(mut active) = active {
            active.poll_task.abort();
            if let Some(control) = active.control.take() {
                control.shutdown();
            }
            if let Err(err) = active.mesh.logout().await {
                tracing::warn!(error = %err, "mesh logout failed; forcing per-pid shutdown");
                active.mesh.shutdown().await;
            }
        }

        self.identity.clear()?;
        if wipe {
            let dir = &self.config.mesh.state_dir;
            if dir.exists() {
                std::fs::remove_dir_all(dir).map_err(|source| LinkError::Io {
                    op: "wipe the mesh state dir",
                    path: dir.clone(),
                    source,
                })?;
            }
        }

        let mut inner = self.inner.lock().await;
        inner.auth = AuthState::LoggedOut;
        inner.last_error = None;
        Ok(())
    }

    async fn ensure_not_busy(&self) -> Result<(), LinkError> {
        let inner = self.inner.lock().await;
        match inner.auth {
            AuthState::Connecting { .. } | AuthState::Connected { .. } => Err(LinkError::Busy),
            _ => Ok(()),
        }
    }

    async fn record_error(&self, err: &WorkerError) {
        let mut inner = self.inner.lock().await;
        inner.last_error = Some(err.to_string());
    }

    /// The mesh node hostname: the machine hostname joined to a short, stable,
    /// per-machine suffix so two machines that share a hostname (e.g. two default-named
    /// MacBooks) never collide in the tailnet or the peer registry. The suffix is 6 hex
    /// chars persisted once at `<identity dir>/node-id`; stable across restarts, unique
    /// per machine by construction (random on first use). See [`node_suffix`].
    fn node_hostname(&self) -> Result<String, LinkError> {
        let raw = gethostname::gethostname().to_string_lossy().into_owned();
        let id_path = self.identity.path()?;
        let dir = id_path.parent().unwrap_or_else(|| Path::new("."));
        let suffix = node_suffix(dir)?;
        // Tailscale caps machine names at 63 chars and truncates server-side; keep the
        // base short enough that the disambiguating suffix always survives (63 - "-" - 6).
        let base: String = sanitize_hostname(&raw).chars().take(56).collect();
        let base = base.trim_end_matches('-');
        Ok(format!("{base}-{suffix}"))
    }
}

/// Reduce an OS hostname to the tailnet-safe alphabet (lowercase alphanumerics and
/// hyphens), so it is a valid Tailscale machine name.
fn sanitize_hostname(raw: &str) -> String {
    let mut out: String = raw
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    while out.contains("--") {
        out = out.replace("--", "-");
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "leanzero-node".to_string()
    } else {
        trimmed
    }
}

/// Read (or, on first use, mint and persist) the machine's 6-hex-char node suffix at
/// `<dir>/node-id`. Randomness comes from `/dev/urandom` on unix; the value is written
/// once and reused thereafter so a machine keeps one stable identity.
fn node_suffix(dir: &Path) -> Result<String, LinkError> {
    let path = dir.join("node-id");
    match std::fs::read_to_string(&path) {
        Ok(existing) => {
            let trimmed = existing.trim();
            if !trimmed.is_empty() {
                return Ok(trimmed.to_string());
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(LinkError::Io {
                op: "read the node-id file",
                path,
                source,
            })
        }
    }

    let suffix = fresh_suffix();
    std::fs::create_dir_all(dir).map_err(|source| LinkError::Io {
        op: "create the identity dir for the node-id file",
        path: dir.to_path_buf(),
        source,
    })?;
    std::fs::write(&path, &suffix).map_err(|source| LinkError::Io {
        op: "persist the node-id file",
        path,
        source,
    })?;
    Ok(suffix)
}

fn fresh_suffix() -> String {
    let mut bytes = [0u8; 3];
    #[cfg(unix)]
    {
        use std::io::Read;
        if let Ok(mut file) = std::fs::File::open("/dev/urandom") {
            let _ = file.read_exact(&mut bytes);
        }
    }
    // If entropy could not be read, fold time + pid so the value is still unique enough
    // for a one-time persisted id.
    if bytes == [0u8; 3] {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        let mix = nanos ^ std::process::id().rotate_left(11);
        bytes.copy_from_slice(&mix.to_le_bytes()[..3]);
    }
    format!("{:02x}{:02x}{:02x}", bytes[0], bytes[1], bytes[2])
}

/// `POST <base_url>/v1/swarm/execute` with the bearer node_token, mapping the peer's
/// status back to a typed result: `202` → the accepted body; `403`/`409`/`400` → the
/// corresponding [`ExecuteError`] (Disabled/Busy/BadRequest, so the receive-side gates
/// surface intact); `501` → [`LinkError::ExecutorUnavailable`]; anything else →
/// [`LinkError::RemoteExecute`] carrying the code and body. Never a silent success.
async fn post_peer_execute(
    base_url: &str,
    token: &str,
    timeout: Duration,
    req: &ExecuteRequest,
) -> Result<ExecuteAccepted, LinkError> {
    let client = reqwest::Client::builder()
        .timeout(timeout)
        .build()
        .map_err(|err| LinkError::RemoteExecute(err.to_string()))?;
    let response = client
        .post(format!("{base_url}/v1/swarm/execute"))
        .bearer_auth(token)
        .json(req)
        .send()
        .await
        .map_err(|err| LinkError::RemoteExecute(err.to_string()))?;

    let status = response.status();
    if status.is_success() {
        return response.json::<ExecuteAccepted>().await.map_err(|err| {
            LinkError::RemoteExecute(format!("peer accepted but its body did not parse: {err}"))
        });
    }

    let body = response.text().await.unwrap_or_default();
    Err(match status.as_u16() {
        403 => LinkError::Execute(ExecuteError::Disabled),
        409 => LinkError::Execute(ExecuteError::Busy),
        400 => LinkError::Execute(ExecuteError::BadRequest(body)),
        501 => LinkError::ExecutorUnavailable,
        code => LinkError::RemoteExecute(format!("peer returned {code}: {body}")),
    })
}

/// `POST <base_url>/v1/swarm/mlx/<op>` with the bearer node_token, mapping the peer's
/// status back to a typed result: `2xx` → the response DTO as opaque JSON; `400` →
/// [`MlxControlError::BadRequest`] and `500` → [`MlxControlError::Failed`] (so the peer's
/// own error class + text survive intact); anything else (a `501` "not wired", a `404`
/// unknown op, an auth `401`) → [`LinkError::MlxProxy`] carrying the code and body. A
/// transport failure reaching the peer is [`LinkError::MlxProxy`] too. Never a silent
/// success.
async fn post_peer_mlx(
    base_url: &str,
    token: &str,
    timeout: Duration,
    op: MlxOp,
    body: &serde_json::Value,
) -> Result<serde_json::Value, LinkError> {
    let client = reqwest::Client::builder()
        .timeout(timeout)
        .build()
        .map_err(|err| LinkError::MlxProxy(err.to_string()))?;
    let response = client
        .post(format!("{base_url}/v1/swarm/mlx/{}", op.path()))
        .bearer_auth(token)
        .json(body)
        .send()
        .await
        .map_err(|err| LinkError::MlxProxy(err.to_string()))?;

    let status = response.status();
    if status.is_success() {
        return response.json::<serde_json::Value>().await.map_err(|err| {
            LinkError::MlxProxy(format!("peer responded but its body did not parse: {err}"))
        });
    }

    let text = response.text().await.unwrap_or_default();
    Err(match status.as_u16() {
        400 => LinkError::MlxControl(MlxControlError::BadRequest(text)),
        500 => LinkError::MlxControl(MlxControlError::Failed(text)),
        code => LinkError::MlxProxy(format!("peer returned {code}: {text}")),
    })
}

async fn peer_sync_loop(
    mesh: Arc<dyn Mesh>,
    registry: PeerRegistry,
    interval: Duration,
    control_port: u16,
) {
    let mut last: Option<Vec<MeshPeer>> = None;
    loop {
        match mesh.status().await {
            Ok(status) => {
                if last.as_deref() != Some(status.peers.as_slice()) {
                    registry.set_mesh_peers(&status.peers, control_port);
                    last = Some(status.peers.clone());
                }
            }
            Err(err) => {
                tracing::warn!(error = %err, "mesh status poll failed; will retry");
            }
        }
        tokio::time::sleep(interval).await;
    }
}
