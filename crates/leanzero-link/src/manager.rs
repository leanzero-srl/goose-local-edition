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
use std::sync::{Arc, Weak};
use std::time::Duration;

use chrono::{DateTime, Utc};
use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

use crate::control::{ControlConfig, ControlError, ControlHandle, ControlService};
use crate::identity::{Identity, IdentityError, IdentityStore};
use crate::inference::{PeerCall, PeerCallResolver};
use crate::intent::{IntentCause, IntentError, IntentRecord, IntentStore, LinkIntent};
use crate::mesh::{MeshConfig, MeshEngine, MeshError, MeshPeer, MeshStatus};
use crate::peer_dial::{peer_http_client, MeshProxy, PeerDialError, PeerTimeout};
use crate::state::{
    ChatServing, DistributedNode, DistributedNodeError, ExecuteAccepted, ExecuteError,
    ExecuteRequest, MlxControl, MlxControlError, MlxOp, PeerRegistry, RemoteExecutor,
    SwarmStateSource,
};
use crate::token::node_token_from_secret;
use crate::wire::NodeStatus;
use crate::worker_client::{
    RequestCodeResult, VerifyResult, WorkerClient, WorkerError, DEFAULT_WORKER_BASE_URL,
};

/// How many CONSECUTIVE failed mesh-status looks drop a live connection whose daemon is
/// alive but no longer answers (`tailscale status` erroring on every poll). A
/// LOOK-COUNT, never a clock: one look is one `tailscale status --json` on our own
/// socket (bounded only by the CLI transport timeout), and the poll loop sleeps
/// `poll_interval` between looks. One failed look is what a loaded machine produces
/// (the single transient probe failure mesh_lifecycle's `probe-fail-once` reproduces);
/// a daemon EXIT is proven by `try_wait` and takes one look. Unresponsiveness is
/// inferred, so it takes five unbroken looks — long enough that
/// [`LinkState::mesh_poll_failures`] climbs visibly (1..4) before the connection is
/// dropped. Any successful look in between resets the count.
///
/// The same count judges the other direction: a daemon the supervisor restarted has
/// PROVEN itself after this many unbroken healthy looks; one that faults before that has
/// failed again and is not restarted again ([`ReconnectState::Failed`]). And the chat
/// relay's in-flight watch ([`crate::inference`]) ends a request only after this many
/// consecutive looks that could not reach the peer at all.
pub const MESH_POLL_FAILURE_LOOKS: u32 = 5;

/// The abstract mesh the manager drives. [`MeshEngine`] is the production impl; tests
/// supply a fake so `connect()` never spawns a daemon.
#[async_trait::async_trait]
pub trait Mesh: Send + Sync {
    async fn join(&self, auth_key: &str, hostname: &str) -> Result<(), MeshError>;
    async fn status(&self) -> Result<MeshStatus, MeshError>;
    /// The daemon's loopback SOCKS5 listener, through which every outbound peer call is
    /// dialed (userspace networking gives the host no route to mesh IPs).
    async fn peer_proxy(&self) -> Result<MeshProxy, MeshError>;
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
    async fn peer_proxy(&self) -> Result<MeshProxy, MeshError> {
        MeshEngine::peer_proxy(self).await
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
///
/// `control.node_token` is a TEMPLATE value and may be left empty: `connect()` replaces
/// it with the token derived from the worker-issued account secret before the service
/// starts, so the template never reaches [`ControlService::start`] — whose
/// [`ControlError::EmptyToken`] refusal stays as the guard behind that promise.
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

/// What the manager did about a `connected` [`LinkIntent`] WITHOUT the user — the
/// reconnect goosed runs once at launch ([`LinkManager::auto_reconnect`]), and the
/// supervisor's restart of a mesh daemon that exited or stopped answering under a live
/// connection (R3 step 1). Every outcome is a named state: a mesh that did not come back
/// is `Failed` with the reason, never a quiet "not connected". Serde tag `state`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state")]
pub enum ReconnectState {
    /// No reconnect has run on this manager: the embedding process has not asked for one,
    /// or the user has since connected or disconnected by hand (their action supersedes it).
    #[default]
    Idle,
    /// The intent says stay off (or there is nothing to reconnect); `reason` says which.
    Skipped { reason: String },
    /// The reconnect is bringing the mesh up now (auth reads `Connecting`). For a
    /// supervisor restart, `LinkState::last_error` names the fault being recovered from.
    Reconnecting { started_at: DateTime<Utc> },
    /// The mesh came back with no user action.
    Reconnected { at: DateTime<Utc>, mesh_ip: String },
    /// The mesh did not come back; `reason` is what the user needs to act (Retry = Connect).
    /// A supervisor restart's reason starts with what happened to the daemon ("LeanZero
    /// Link's mesh daemon stopped …"), then why it is not back.
    Failed { reason: String, at: DateTime<Utc> },
}

/// What goosed surfaces to the desktop: the auth state, live mesh status while
/// connected, the node count, the mesh-poll health, and the last error — the error is
/// never swallowed, it rides here for the UI to show honestly.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LinkState {
    pub auth: AuthState,
    pub mesh: Option<MeshStatus>,
    /// `1 + peers whose status is not Offline` while connected (a peer answering
    /// 4xx keeps its last status and still counts; an unreachable one does not); `0`
    /// when not connected. Never "1 + every row the mesh ever listed".
    pub node_count: u32,
    /// Consecutive failures of the connection's mesh-status poll (`tailscale status`
    /// reads that errored), reset to 0 by the next success and by a new connection.
    /// A rising number is the UI's early warning: at [`MESH_POLL_FAILURE_LOOKS`] the
    /// connection is dropped per-pid and auth returns to `LoggedIn` (the identity
    /// kept). After that drop the count stays at the value that caused it.
    #[serde(default)]
    pub mesh_poll_failures: u32,
    pub last_error: Option<String>,
    /// The user's persisted mesh intent (see [`crate::intent`]); `None` only when the
    /// record is unreadable, and then `intent_error` says why.
    #[serde(default)]
    pub intent: Option<IntentRecord>,
    #[serde(default)]
    pub intent_error: Option<String>,
    /// The launch reconnect's outcome, or the supervisor's restart of a daemon that
    /// faulted under the connection (see [`ReconnectState`]).
    #[serde(default)]
    pub reconnect: ReconnectState,
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
    #[error(transparent)]
    Intent(#[from] IntentError),
    #[error("not logged in — verify an email code first")]
    NotLoggedIn,
    #[error("the auth worker did not issue a node secret; update the worker and sign in again")]
    NoNodeSecret,
    #[error("busy: a connect is already in progress or the mesh is connected")]
    Busy,
    #[error(
        "connect aborted: the account was logged out while the mesh was coming up; the \
         fresh connection was logged out of the tailnet and shut down per-pid"
    )]
    ConnectAborted,
    #[error(
        "connect cancelled: the mesh was disconnected while it was coming up; the fresh \
         connection was shut down per-pid"
    )]
    ConnectCancelled,
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
    /// A peer's `/v1/swarm/distributed/*` answered with one of its classes (403 its switch is
    /// off, 404, 400, 409 a named refusal, 500) — carried verbatim.
    #[error(transparent)]
    DistributedNode(#[from] DistributedNodeError),
    /// The distributed request never got a classed answer: the peer was unreachable, answered
    /// `501` (not wired), or answered outside the contract.
    #[error("distributed request to a peer failed: {0}")]
    DistributedProxy(String),
    #[error(transparent)]
    PeerDial(#[from] PeerDialError),
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
/// `ControlHandle::shutdown` without partial-moving out of a `Drop` type; `poll_task`
/// is an `Option` so the poll loop itself can hand its `Active` to
/// [`teardown_active`] without aborting the task it is running on.
struct Active {
    mesh: Arc<dyn Mesh>,
    control: Option<ControlHandle>,
    registry: PeerRegistry,
    poll_task: Option<JoinHandle<()>>,
    mesh_ip: String,
    /// The account this connection belongs to — what auth returns to when the
    /// connection is dropped underneath the user (the identity stays on disk).
    email: String,
    /// The `/v1/swarm` bearer this connection serves AND presents to peers — derived at
    /// connect time from the worker-issued account secret, never from the template.
    node_token: String,
    /// Monotonic per manager; a poll loop or a status read that observed THIS
    /// connection may only tear down THIS connection, never a newer one.
    generation: u64,
    /// The seams this connection was started with — what the supervisor restarts it with.
    seams: Seams,
}

impl Drop for Active {
    /// Belt-and-suspenders: if an `Active` is dropped without an explicit `logout`
    /// (e.g. the manager itself is dropped), stop the mesh-status poll loop. The
    /// `ControlHandle` and `PeerRegistry` abort their own tasks on drop.
    fn drop(&mut self) {
        if let Some(task) = &self.poll_task {
            task.abort();
        }
    }
}

struct Inner {
    auth: AuthState,
    last_error: Option<String>,
    active: Option<Active>,
    next_generation: u64,
    mesh_poll_failures: u32,
    /// The resolved intent, or the text of why it could not be read.
    intent: Result<IntentRecord, String>,
    reconnect: ReconnectState,
    /// The generation of the most recent connect STARTED — only that connect may install
    /// its connection or record its failure (a Disconnect + Connect while an older connect
    /// was still coming up leaves `Connecting` too, but under the newer generation).
    last_connect: u64,
    /// The supervisor's restart of a faulted daemon, from the fault until the restarted
    /// daemon has PROVEN itself ([`MESH_POLL_FAILURE_LOOKS`] healthy looks); `None`
    /// otherwise. A daemon that faults while its restart is unproven is not restarted again.
    restart: Option<SupervisedRestart>,
}

/// One supervised restart in progress (see [`Inner::restart`]).
struct SupervisedRestart {
    /// What happened to the daemon that was restarted — the text every state names.
    cause: String,
    /// The connection the restart brought up; `None` while it is still coming up.
    generation: Option<u64>,
    /// Consecutive successful status looks of that connection.
    healthy_looks: u32,
}

/// What the supervisor does about a faulted daemon ([`drop_active_after_daemon_fault`]).
#[derive(Debug, Clone, PartialEq, Eq)]
enum FaultResponse {
    /// The intent says stay connected: restart it with no user action.
    Restart,
    /// The faulted daemon WAS a supervised restart that never proved healthy — it failed
    /// again; `first` is the fault that caused that restart. Loud, and no further restart.
    FailedAgain { first: String },
    /// The intent is unreadable: nothing may reconnect on its behalf (the launch reconnect
    /// refuses the same record); the fault is recorded for the user's Connect.
    Drop,
}

impl Inner {
    fn intent_fields(&self) -> (Option<IntentRecord>, Option<String>) {
        match &self.intent {
            Ok(record) => (Some(record.clone()), None),
            Err(reason) => (None, Some(reason.clone())),
        }
    }
}

/// Who asked for a connect. Only the USER's connect records intent; the launch reconnect
/// and the supervisor act on the intent and never rewrite it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConnectOrigin {
    User,
    Reconnect,
    /// The supervisor restarting a daemon that faulted under a live connection: it runs
    /// only from the `Connecting` + `Reconnecting` state the fault left, so a user action
    /// in between (Disconnect, Log out) cancels it.
    Supervisor,
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

/// The seams goose attaches after construction (the builder setters). A connection keeps
/// the seams it was started with, so the supervisor restarts it with the same ones.
#[derive(Clone, Default)]
struct Seams {
    /// The local remote-execute seam, injected beside `source` (mirroring how the
    /// [`SwarmStateSource`] is threaded). `None` → this node cannot run remote prompts
    /// (self short-circuit and the control route both answer as unavailable). goose-server
    /// supplies the real one via `set_executor` before the manager is built.
    executor: Option<Arc<dyn RemoteExecutor>>,
    /// The local MLX-engine seam, injected beside `executor`. `None` → this node cannot
    /// run remote model-management ops (its `/v1/swarm/mlx/*` routes answer `501` and the
    /// self short-circuit in [`LinkManager::mlx_proxy`] is unavailable). goose supplies the
    /// real one (`GoosedMlxControl`) before the manager is built.
    mlx_control: Option<Arc<dyn MlxControl>>,
    /// This node as a node of a peer's distributed MLX engine. `None` → its
    /// `/v1/swarm/distributed/*` routes answer `501`.
    distributed_node: Option<Arc<dyn DistributedNode>>,
    /// This node's chat engine served to peers (the inference proxy). `None` → its
    /// `/v1/swarm/inference/*` routes answer `501`.
    chat_serving: Option<Arc<dyn ChatServing>>,
}

/// Everything a connect needs, shared: the manager holds it, and a connection's poll loop
/// holds it WEAKLY — so the loop can drop a connection whose daemon faulted and hand the
/// restart to [`Core::supervised_restart`], and a dropped manager ends the loop.
struct Core {
    config: LinkManagerConfig,
    identity: IdentityStore,
    intent: IntentStore,
    worker: WorkerClient,
    mesh_factory: Arc<dyn MeshFactory>,
    source: Arc<dyn SwarmStateSource>,
    inner: Mutex<Inner>,
}

pub struct LinkManager {
    core: Arc<Core>,
    seams: Seams,
}

impl LinkManager {
    /// Construct with the production mesh factory. Loads any persisted identity:
    /// present → `LoggedIn` (it does NOT connect by itself; the embedding process calls
    /// [`Self::auto_reconnect`] at launch, which honours the persisted intent); absent →
    /// `LoggedOut`; malformed → a loud error (never silently logged-out). The intent is
    /// resolved here too ([`IntentStore::resolve`]); an unreadable one does not refuse
    /// construction — logout and Connect must stay possible — it rides `intent_error`.
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
        let intent = IntentStore::beside(&identity.path()?);
        let resolved = intent
            .resolve(
                matches!(auth, AuthState::LoggedIn { .. }),
                &config.mesh.state_dir.join("tailscaled.state"),
            )
            .map_err(|err| {
                tracing::error!(error = %err, "leanzero-link: the Link intent is unreadable");
                err.to_string()
            });
        Ok(Self {
            core: Arc::new(Core {
                config,
                identity,
                intent,
                worker,
                mesh_factory,
                source,
                inner: Mutex::new(Inner {
                    auth,
                    last_error: None,
                    active: None,
                    next_generation: 0,
                    mesh_poll_failures: 0,
                    intent: resolved,
                    reconnect: ReconnectState::Idle,
                    last_connect: 0,
                    restart: None,
                }),
            }),
            seams: Seams::default(),
        })
    }

    /// Attach the local [`RemoteExecutor`] (goose-server's `GoosedRemoteExecutor`). Used
    /// by the `POST /v1/swarm/execute` route this node serves AND by the self short-circuit
    /// in [`Self::remote_execute`]. A builder-style setter rather than a constructor arg so
    /// the existing construction paths (and their tests) stay unchanged.
    pub fn with_executor(mut self, executor: Arc<dyn RemoteExecutor>) -> Self {
        self.seams.executor = Some(executor);
        self
    }

    /// Attach the local [`MlxControl`] (goose's `GoosedMlxControl`). Used by the
    /// `POST /v1/swarm/mlx/*` routes this node serves AND by the self short-circuit in
    /// [`Self::mlx_proxy`]. A builder-style setter, like [`Self::with_executor`], so the
    /// existing construction paths (and their tests) stay unchanged.
    pub fn with_mlx_control(mut self, mlx_control: Arc<dyn MlxControl>) -> Self {
        self.seams.mlx_control = Some(mlx_control);
        self
    }

    /// Attach this node's distributed-engine node side (goose's). A builder-style setter, like
    /// [`Self::with_mlx_control`].
    pub fn with_distributed_node(mut self, node: Arc<dyn DistributedNode>) -> Self {
        self.seams.distributed_node = Some(node);
        self
    }

    /// Attach this node's chat engine for the inference proxy (goose's). A builder-style
    /// setter, like [`Self::with_distributed_node`].
    pub fn with_chat_serving(mut self, serving: Arc<dyn ChatServing>) -> Self {
        self.seams.chat_serving = Some(serving);
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
        Ok(self.core.worker.health().await?)
    }

    /// Request an email OTP → `CodeSent`. Refused while connecting/connected.
    pub async fn request_code(&self, email: &str) -> Result<RequestCodeResult, LinkError> {
        self.ensure_not_busy().await?;
        match self.core.worker.request_code(email).await {
            Ok(result) => {
                let expires_at =
                    Utc::now() + chrono::Duration::seconds(result.expires_in_seconds as i64);
                let mut inner = self.core.inner.lock().await;
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
        let result = match self.core.worker.verify(email, code).await {
            Ok(result) => result,
            Err(err) => {
                self.record_error(&err).await;
                return Err(err.into());
            }
        };
        self.core.identity
            .save(&Identity::new(result.email.clone(), result.token.clone()))?;
        let mut inner = self.core.inner.lock().await;
        inner.auth = AuthState::LoggedIn {
            email: result.email.clone(),
        };
        inner.last_error = None;
        Ok(result)
    }

    /// Bring up the mesh + control service (requires `LoggedIn`). On any step failing
    /// the resources are torn down per-pid and the state returns to `LoggedIn` (still
    /// authed) with `last_error` set — except a join-key `401` whose body carries the
    /// WORKER's own dead-token verdict (`reason` ∈ expired / malformed / bad_signature /
    /// bad_claims, see [`crate::worker_client::IDENTITY_DEAD_REASONS`]), which clears
    /// the identity and drops to `LoggedOut`. A `401` with any other body (a proxy's
    /// HTML, a truncated body) is an ordinary failure: `LoggedIn` + `last_error`, the
    /// credential untouched.
    ///
    /// This is the USER's connect: before anything starts it records the intent
    /// `connected` (so every later launch reconnects) and supersedes any launch-reconnect
    /// outcome. A record that cannot be written refuses the connect loudly — a mesh the
    /// next launch would silently not bring back is the defect this record exists to end.
    pub async fn connect(&self) -> Result<(), LinkError> {
        self.core
            .connect_as(ConnectOrigin::User, self.seams.clone())
            .await
    }

    /// The composed live view: auth + a live mesh status read + the control node count.
    /// A status read that finds the supervised daemon EXITED drops the connection right
    /// here (see [`drop_active_after_daemon_fault`]) and reports what the supervisor made
    /// of it — `Connecting` + `Reconnecting` while it restarts the daemon — so the UI never
    /// shows `Connected` over a dead daemon.
    pub async fn status(&self) -> LinkState {
        let (auth, persisted_error, poll_failures, live, (intent, intent_error), reconnect) = {
            let inner = self.core.inner.lock().await;
            let live = inner.active.as_ref().map(|active| {
                (
                    active.mesh.clone(),
                    active.registry.clone(),
                    active.generation,
                )
            });
            (
                inner.auth.clone(),
                inner.last_error.clone(),
                inner.mesh_poll_failures,
                live,
                inner.intent_fields(),
                inner.reconnect.clone(),
            )
        };

        let Some((mesh, registry, generation)) = live else {
            return LinkState {
                auth,
                mesh: None,
                node_count: 0,
                mesh_poll_failures: poll_failures,
                last_error: persisted_error,
                intent,
                intent_error,
                reconnect,
            };
        };

        match mesh.status().await {
            Ok(status) => LinkState {
                auth,
                mesh: Some(status),
                node_count: node_count(&registry),
                mesh_poll_failures: poll_failures,
                last_error: persisted_error,
                intent,
                intent_error,
                reconnect,
            },
            Err(err @ MeshError::DaemonExited { .. }) => {
                drop_active_after_daemon_fault(
                    &self.core,
                    generation,
                    DaemonFault::Exited(&err),
                    false,
                )
                .await;
                let inner = self.core.inner.lock().await;
                let (intent, intent_error) = inner.intent_fields();
                LinkState {
                    auth: inner.auth.clone(),
                    mesh: None,
                    node_count: 0,
                    mesh_poll_failures: inner.mesh_poll_failures,
                    last_error: inner.last_error.clone(),
                    intent,
                    intent_error,
                    reconnect: inner.reconnect.clone(),
                }
            }
            Err(err) => LinkState {
                auth,
                mesh: None,
                node_count: node_count(&registry),
                mesh_poll_failures: poll_failures,
                last_error: Some(format!("mesh status read failed: {err}")),
                intent,
                intent_error,
                reconnect,
            },
        }
    }

    /// The live peer-fabric registry while connected (`None` otherwise). Exposed so the
    /// swarm dispatcher / UI can read peer node states (and their Idle/Busy status) before
    /// dispatching, and so [`Self::remote_execute`] can resolve a target's URL.
    pub async fn active_registry(&self) -> Option<PeerRegistry> {
        self.core.inner
            .lock()
            .await
            .active
            .as_ref()
            .map(|active| active.registry.clone())
    }

    /// The live connection's `/v1/swarm` bearer (`None` when not connected) — what the
    /// host's loopback proxy presents to this node's own control service. Derived from
    /// the worker-issued account secret at connect time; never the template value.
    pub async fn node_token(&self) -> Option<String> {
        self.core.inner
            .lock()
            .await
            .active
            .as_ref()
            .map(|active| active.node_token.clone())
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
        let self_node_id = self.core.source.local_node().await.node_id;
        if target_node_id == self_node_id {
            let executor = self
                .seams
                .executor
                .clone()
                .ok_or(LinkError::ExecutorUnavailable)?;
            return Ok(executor.execute(req).await?);
        }

        let (base_url, token, proxy) = {
            let inner = self.core.inner.lock().await;
            let active = inner.active.as_ref().ok_or(LinkError::NotConnected)?;
            let base_url = active
                .registry
                .peer_base_url(target_node_id)
                .ok_or_else(|| LinkError::UnknownPeer(target_node_id.to_string()))?;
            (
                base_url,
                active.node_token.clone(),
                active.registry.peer_proxy(),
            )
        };

        post_peer_execute(
            proxy,
            &base_url,
            &token,
            self.core.config.control.connect_timeout,
            &req,
        )
        .await
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
        let self_node_id = self.core.source.local_node().await.node_id;
        if target_node_id == self_node_id {
            let control = self
                .seams
                .mlx_control
                .clone()
                .ok_or(LinkError::MlxControlUnavailable)?;
            return Ok(control.dispatch(op, body).await?);
        }

        let (base_url, token, proxy) = {
            let inner = self.core.inner.lock().await;
            let active = inner.active.as_ref().ok_or(LinkError::NotConnected)?;
            let base_url = active
                .registry
                .peer_base_url(target_node_id)
                .ok_or_else(|| LinkError::UnknownPeer(target_node_id.to_string()))?;
            (
                base_url,
                active.node_token.clone(),
                active.registry.peer_proxy(),
            )
        };

        post_peer_mlx(
            proxy,
            &base_url,
            &token,
            self.core.config.control.connect_timeout,
            op,
            &body,
        )
        .await
    }

    /// Forward one distributed-engine op to peer `target_node_id`: `POST
    /// <peer>/v1/swarm/distributed/<op>` with the bearer node token, through the mesh proxy.
    /// `body`/`Ok` are the op's JSON (goose types them). The peer's classed answers come back as
    /// [`LinkError::DistributedNode`]; everything else as [`LinkError::DistributedProxy`].
    pub async fn distributed_proxy(
        &self,
        target_node_id: &str,
        op: &str,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, LinkError> {
        let (base_url, token, proxy) = {
            let inner = self.core.inner.lock().await;
            let active = inner.active.as_ref().ok_or(LinkError::NotConnected)?;
            let base_url = active
                .registry
                .peer_base_url(target_node_id)
                .ok_or_else(|| LinkError::UnknownPeer(target_node_id.to_string()))?;
            (
                base_url,
                active.node_token.clone(),
                active.registry.peer_proxy(),
            )
        };
        post_peer_distributed(
            proxy,
            &base_url,
            &token,
            self.core.config.control.connect_timeout,
            op,
            body,
        )
        .await
    }

    /// How to reach peer `target_node_id`'s control service right now — its base URL, the
    /// bearer, the mesh proxy — for callers that stream their own request (the inference relay).
    pub async fn peer_call(&self, target_node_id: &str) -> Result<PeerCall, LinkError> {
        let inner = self.core.inner.lock().await;
        let active = inner.active.as_ref().ok_or(LinkError::NotConnected)?;
        let base_url = active
            .registry
            .peer_base_url(target_node_id)
            .ok_or_else(|| LinkError::UnknownPeer(target_node_id.to_string()))?;
        Ok(PeerCall {
            base_url,
            token: active.node_token.clone(),
            proxy: active.registry.peer_proxy(),
            connect_timeout: self.core.config.control.connect_timeout,
        })
    }

    /// Tear down the connection (per-pid), clear the stored identity, and drop to
    /// `LoggedOut`. The mesh state dir is left for a fast re-login unless `wipe`.
    ///
    /// A failed `tailscale logout` (the daemon is stopped per-pid instead) still logs
    /// the account out, but its text lands in `last_error` — never erased. A failed
    /// identity clear leaves the credential on disk, so auth returns to `LoggedIn`
    /// (the truthful state: mesh down, credential present) with the error recorded.
    ///
    /// The intent becomes `disconnected` first, so no later launch reconnects; a record
    /// that cannot be written does not stop the logout, its text rides `last_error`.
    pub async fn logout(&self, wipe: bool) -> Result<(), LinkError> {
        let record = IntentRecord::new(LinkIntent::Disconnected, IntentCause::UserLogout);
        let intent_error = self.core.intent.save(&record).err();
        let (active, email) = {
            let mut inner = self.core.inner.lock().await;
            let email = match &inner.auth {
                AuthState::LoggedIn { email }
                | AuthState::Connecting { email }
                | AuthState::Connected { email, .. } => Some(email.clone()),
                AuthState::LoggedOut | AuthState::CodeSent { .. } => None,
            };
            match &intent_error {
                None => inner.intent = Ok(record),
                Some(err) => inner.intent = Err(err.to_string()),
            }
            inner.reconnect = ReconnectState::Idle;
            inner.restart = None;
            (inner.active.take(), email)
        };
        let intent_note = intent_error.map(|err| {
            format!(
                "the intent record could not be set to disconnected ({err}); the next launch \
                 reports a reconnect failure until you sign in again"
            )
        });
        let mesh_logout_error = match active {
            Some(active) => teardown_active(active, true).await,
            None => None,
        };

        if let Err(err) = self.core.identity.clear() {
            let mut inner = self.core.inner.lock().await;
            inner.auth = match email {
                Some(email) => AuthState::LoggedIn { email },
                None => AuthState::LoggedOut,
            };
            inner.last_error = Some(format!(
                "logout incomplete: the mesh is down but the credential could not be removed: {err}"
            ));
            return Err(err.into());
        }
        if wipe {
            let dir = &self.core.config.mesh.state_dir;
            if dir.exists() {
                std::fs::remove_dir_all(dir).map_err(|source| LinkError::Io {
                    op: "wipe the mesh state dir",
                    path: dir.clone(),
                    source,
                })?;
            }
        }

        let mut inner = self.core.inner.lock().await;
        inner.auth = AuthState::LoggedOut;
        let mesh_note = mesh_logout_error.map(|err| {
            format!(
                "logged out, but `tailscale logout` failed and the daemon was stopped per-pid \
                 instead (the node key may linger on the control plane until it expires): {err}"
            )
        });
        inner.last_error = match (mesh_note, intent_note) {
            (Some(mesh), Some(intent)) => Some(format!("{mesh}; {intent}")),
            (mesh, intent) => mesh.or(intent),
        };
        Ok(())
    }

    /// Take this node off the mesh and KEEP it off across launches, the account kept:
    /// records the intent `disconnected` FIRST (a record that cannot be written refuses
    /// the disconnect with the mesh untouched — otherwise the next launch would silently
    /// undo it), then stops the connection per-pid (the poll loop, the control service,
    /// the daemon — SIGTERM → grace → SIGKILL, our child only; no `tailscale logout`, the
    /// state dir is kept for a fast Connect) and returns auth to `LoggedIn`. From
    /// `Connecting`, the in-flight connect finds the state moved on and tears its fresh
    /// connection down itself ([`LinkError::ConnectCancelled`]). Already `LoggedIn`: the
    /// intent alone changes — that is how a user turns the launch reconnect off.
    pub async fn disconnect(&self) -> Result<(), LinkError> {
        let active = {
            let mut inner = self.core.inner.lock().await;
            let email = match &inner.auth {
                AuthState::LoggedIn { email }
                | AuthState::Connecting { email }
                | AuthState::Connected { email, .. } => email.clone(),
                AuthState::LoggedOut | AuthState::CodeSent { .. } => {
                    return Err(LinkError::NotLoggedIn)
                }
            };
            let record = IntentRecord::new(LinkIntent::Disconnected, IntentCause::UserDisconnect);
            self.core.intent.save(&record)?;
            inner.intent = Ok(record);
            inner.reconnect = ReconnectState::Idle;
            inner.restart = None;
            inner.auth = AuthState::LoggedIn { email };
            inner.last_error = None;
            inner.mesh_poll_failures = 0;
            inner.active.take()
        };
        if let Some(active) = active {
            teardown_active(active, false).await;
        }
        Ok(())
    }

    /// The launch reconnect: bring the mesh back with no user action when — and only
    /// when — the persisted intent is `connected` and a credential is stored. The
    /// embedding process calls it once per launch (goosed: when it boots); the user's
    /// Connect / Disconnect / Log out supersede it at any point.
    ///
    /// `preflight` is the embedding layer's own refusal (goosed: mesh binaries missing),
    /// evaluated only when a reconnect is due; `describe` turns a connect failure into
    /// the text the user acts on. Every outcome is recorded in [`LinkState::reconnect`]
    /// and returned: `Skipped` (intent `disconnected`, already connected), `Reconnected`,
    /// or `Failed` with the reason — an unreadable intent, a gone credential, the
    /// preflight's refusal, or the connect's own error. The credential is only ever
    /// cleared by the worker's named dead-token verdict, exactly as a manual connect.
    pub async fn auto_reconnect(
        &self,
        preflight: Result<(), String>,
        describe: impl Fn(&LinkError) -> String,
    ) -> ReconnectState {
        let due = {
            let mut inner = self.core.inner.lock().await;
            let now = Utc::now();
            let verdict = match (&inner.intent, &inner.auth) {
                (Err(reason), _) => Err(ReconnectState::Failed {
                    reason: format!("the Link intent could not be read: {reason}"),
                    at: now,
                }),
                (Ok(record), _) if record.intent == LinkIntent::Disconnected => {
                    Err(ReconnectState::Skipped {
                        reason: skipped_reason(record.cause).to_string(),
                    })
                }
                (Ok(_), AuthState::Connecting { .. } | AuthState::Connected { .. }) => {
                    Err(ReconnectState::Skipped {
                        reason: "the mesh is already connecting or connected".to_string(),
                    })
                }
                (Ok(_), AuthState::LoggedOut | AuthState::CodeSent { .. }) => {
                    Err(ReconnectState::Failed {
                        reason: "not signed in: the mesh was on, but no account credential is \
                                 stored on this Mac any more — sign in again to reconnect"
                            .to_string(),
                        at: now,
                    })
                }
                (Ok(_), AuthState::LoggedIn { .. }) => match preflight {
                    Err(reason) => Err(ReconnectState::Failed { reason, at: now }),
                    Ok(()) => self.mesh_socket_verdict(now),
                },
            };
            inner.reconnect = match &verdict {
                Err(state) => state.clone(),
                Ok(()) => ReconnectState::Reconnecting { started_at: now },
            };
            verdict
        };
        if let Err(state) = due {
            log_reconnect(&state);
            return state;
        }

        let result = self
            .core
            .connect_as(ConnectOrigin::Reconnect, self.seams.clone())
            .await;
        let mut inner = self.core.inner.lock().await;
        if !matches!(inner.reconnect, ReconnectState::Reconnecting { .. }) {
            // The user acted meanwhile (Connect, Disconnect, Log out); theirs stands.
            return inner.reconnect.clone();
        }
        inner.reconnect = match (result, &inner.auth) {
            (Ok(()), AuthState::Connected { mesh_ip, .. }) => ReconnectState::Reconnected {
                at: Utc::now(),
                mesh_ip: mesh_ip.clone(),
            },
            (Ok(()), other) => ReconnectState::Failed {
                reason: format!("the connect returned but auth reads {other:?}"),
                at: Utc::now(),
            },
            (Err(err), _) => ReconnectState::Failed {
                reason: describe(&err),
                at: Utc::now(),
            },
        };
        log_reconnect(&inner.reconnect);
        inner.reconnect.clone()
    }

    async fn ensure_not_busy(&self) -> Result<(), LinkError> {
        let inner = self.core.inner.lock().await;
        match inner.auth {
            AuthState::Connecting { .. } | AuthState::Connected { .. } => Err(LinkError::Busy),
            _ => Ok(()),
        }
    }

    async fn record_error(&self, err: &WorkerError) {
        let mut inner = self.core.inner.lock().await;
        inner.last_error = Some(err.to_string());
    }

}

impl Core {
    async fn connect_as(
        self: &Arc<Self>,
        origin: ConnectOrigin,
        seams: Seams,
    ) -> Result<(), LinkError> {
        let (email, generation) = {
            let mut inner = self.inner.lock().await;
            let email = match (&inner.auth, origin) {
                // The supervisor continues the `Connecting` its fault handler set; anything
                // else means the user acted in between, and their action stands.
                (AuthState::Connecting { email }, ConnectOrigin::Supervisor)
                    if matches!(inner.reconnect, ReconnectState::Reconnecting { .. }) =>
                {
                    email.clone()
                }
                (_, ConnectOrigin::Supervisor) => return Err(LinkError::ConnectCancelled),
                (AuthState::LoggedIn { email }, _) => email.clone(),
                (AuthState::Connecting { .. } | AuthState::Connected { .. }, _) => {
                    return Err(LinkError::Busy)
                }
                (AuthState::LoggedOut | AuthState::CodeSent { .. }, _) => {
                    return Err(LinkError::NotLoggedIn)
                }
            };
            if origin == ConnectOrigin::User {
                let record = IntentRecord::new(LinkIntent::Connected, IntentCause::UserConnect);
                self.intent.save(&record)?;
                inner.intent = Ok(record);
                inner.reconnect = ReconnectState::Idle;
                inner.restart = None;
            }
            if origin != ConnectOrigin::Supervisor {
                // The supervisor's `last_error` names the fault it is restarting from.
                inner.auth = AuthState::Connecting {
                    email: email.clone(),
                };
                inner.last_error = None;
            }
            inner.next_generation += 1;
            let generation = inner.next_generation;
            inner.last_connect = generation;
            (email, generation)
        };

        let result = self.connect_inner(generation, &seams).await;
        let mut inner = self.inner.lock().await;
        match result {
            Ok(active) => {
                // Auth still reads the `Connecting` THIS connect set: a Disconnect + Connect
                // in between leaves `Connecting` too, but under another connect's generation.
                let still_connecting = inner.last_connect == generation
                    && matches!(
                        &inner.auth,
                        AuthState::Connecting { email: current } if *current == email
                    );
                if !still_connecting {
                    // `logout()` or `disconnect()` raced us and auth has moved on. The
                    // fresh connection is torn down exactly as that action tears one
                    // down — after a logout, `tailscale logout` (expire the node key on
                    // the control plane) then per-pid shutdown; after a disconnect, the
                    // per-pid shutdown alone — and that action's state stands.
                    // Installing it would show `Connected` over a choice to be off.
                    let logged_out = matches!(inner.auth, AuthState::LoggedOut);
                    drop(inner);
                    tracing::warn!(
                        logged_out,
                        "leanzero-link: a logout/disconnect raced the connect; tearing the fresh connection down"
                    );
                    teardown_active(active, logged_out).await;
                    return Err(if logged_out {
                        LinkError::ConnectAborted
                    } else {
                        LinkError::ConnectCancelled
                    });
                }
                inner.auth = AuthState::Connected {
                    email,
                    mesh_ip: active.mesh_ip.clone(),
                };
                if origin == ConnectOrigin::Supervisor {
                    inner.reconnect = ReconnectState::Reconnected {
                        at: Utc::now(),
                        mesh_ip: active.mesh_ip.clone(),
                    };
                    inner.last_error = None;
                    if let Some(restart) = inner.restart.as_mut() {
                        restart.generation = Some(generation);
                        restart.healthy_looks = 0;
                        tracing::info!(
                            cause = %restart.cause,
                            mesh_ip = %active.mesh_ip,
                            "leanzero_link_supervisor: the mesh daemon was restarted with no user action"
                        );
                    }
                }
                inner.active = Some(active);
                inner.mesh_poll_failures = 0;
                Ok(())
            }
            Err(ConnectFailure { error, logout }) => {
                if inner.last_connect == generation
                    && matches!(&inner.auth, AuthState::Connecting { .. })
                {
                    inner.auth = if logout {
                        AuthState::LoggedOut
                    } else {
                        AuthState::LoggedIn { email }
                    };
                    inner.last_error = Some(error.to_string());
                } else {
                    // `logout()` raced a FAILING connect: its `LoggedOut` stands; flipping
                    // back to `LoggedIn` would claim a credential that is no longer on disk.
                    tracing::warn!(
                        error = %error,
                        "leanzero-link: connect failed after a logout raced it; logged-out state kept"
                    );
                }
                Err(error)
            }
        }
    }

    async fn connect_inner(
        self: &Arc<Self>,
        generation: u64,
        seams: &Seams,
    ) -> Result<Active, ConnectFailure> {
        let identity = self.identity.load()?.ok_or(ConnectFailure {
            error: LinkError::NotLoggedIn,
            logout: true,
        })?;

        let node_hostname = self.node_hostname()?;

        let key = match self.worker.join_key(&identity.token).await {
            Ok(key) => key,
            // Only the worker's NAMED verdict on the token clears the credential; the
            // client maps every other 401 to `Unexpected` (see `WorkerClient::join_key`).
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

        // The node token is derived from the worker-issued account secret and nothing
        // else. A worker that did not issue one is a loud refusal BEFORE any daemon is
        // spawned — never a token derived from the email or the template's placeholder.
        let secret = key
            .node_secret
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or(ConnectFailure {
                error: LinkError::NoNodeSecret,
                logout: false,
            })?;
        let node_token = node_token_from_secret(secret);

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

        let peer_proxy = match mesh.peer_proxy().await {
            Ok(proxy) => proxy,
            Err(err) => {
                mesh.shutdown().await;
                return Err(err.into());
            }
        };

        let mut control_config = self.config.control.clone();
        control_config.mesh_ip = Some(mesh_ip_addr);
        control_config.node_token = node_token.clone();
        control_config.peer_proxy = Some(peer_proxy);
        let control = match ControlService::start(
            control_config,
            self.source.clone(),
            seams.executor.clone(),
            seams.mlx_control.clone(),
            seams.distributed_node.clone(),
            seams.chat_serving.clone(),
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
            Arc::downgrade(self),
            generation,
        ));

        Ok(Active {
            mesh,
            control: Some(control),
            registry,
            poll_task: Some(poll_task),
            mesh_ip,
            email: identity.email,
            node_token,
            generation,
            seams: seams.clone(),
        })
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

impl LinkManager {
    /// Who already holds this node's mesh socket, classified BEFORE a join key is minted
    /// or a daemon spawned (this manager holds no connection when it asks). Nobody → the
    /// reconnect is due. A daemon whose spawner is alive → another goose on this Mac (a
    /// second window's backend) owns the mesh: `Skipped`, nothing to do here — the old
    /// path minted a key, spawned, and failed with advice to kill a healthy daemon. A
    /// daemon whose spawner is gone → the orphan a goosed killed before its teardown
    /// leaves: `Failed`, naming the pid to stop. Never adopted either way.
    fn mesh_socket_verdict(&self, now: DateTime<Utc>) -> Result<(), ReconnectState> {
        let socket = &self.core.config.mesh.socket_path;
        match crate::mesh::socket_holder(socket) {
            Ok(None) => Ok(()),
            Ok(Some(holder)) if holder.spawner_alive() => Err(ReconnectState::Skipped {
                reason: format!(
                    "another goose on this Mac already holds the mesh (tailscaled pid {}, \
                     started by pid {}); this backend leaves it to that one",
                    holder.listener_pid,
                    holder
                        .parent_pid
                        .map_or("unknown".to_string(), |p| p.to_string())
                ),
            }),
            Ok(Some(holder)) => Err(ReconnectState::Failed {
                reason: format!(
                    "a tailscaled left behind by a goose that exited without its teardown \
                     holds the mesh socket '{}' (pid {}, its parent is gone); LeanZero Link \
                     never adopts a daemon it did not spawn — stop it per-pid (`kill {}`), \
                     then Retry",
                    socket.display(),
                    holder.listener_pid,
                    holder.listener_pid
                ),
                at: now,
            }),
            Err(err) => Err(ReconnectState::Failed {
                reason: format!(
                    "cannot tell who holds the mesh socket '{}': {err}",
                    socket.display()
                ),
                at: now,
            }),
        }
    }
}

fn skipped_reason(cause: IntentCause) -> &'static str {
    match cause {
        IntentCause::UserDisconnect => "you disconnected this Mac; it stays off until you connect",
        IntentCause::UserLogout => "you logged out of LeanZero Link on this Mac",
        IntentCause::NoRecord => "this Mac has never been connected under this sign-in",
        IntentCause::UserConnect | IntentCause::Migrated => "the intent is disconnected",
    }
}

fn log_reconnect(state: &ReconnectState) {
    match state {
        ReconnectState::Failed { reason, .. } => {
            tracing::error!(%reason, "leanzero_link_reconnect: failed")
        }
        ReconnectState::Skipped { reason } => {
            tracing::info!(%reason, "leanzero_link_reconnect: skipped")
        }
        ReconnectState::Reconnected { mesh_ip, .. } => {
            tracing::info!(%mesh_ip, "leanzero_link_reconnect: reconnected with no user action")
        }
        ReconnectState::Idle | ReconnectState::Reconnecting { .. } => {}
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
///
/// `connect_timeout` is the ONLY timeout: reaching the peer is bounded, the peer's
/// answer is not — a total cap would report failure while the peer completes the work.
/// The request goes through the mesh proxy ([`crate::peer_dial`]); no proxy is
/// [`LinkError::PeerDial`], never a direct dial.
async fn post_peer_execute(
    proxy: Option<MeshProxy>,
    base_url: &str,
    token: &str,
    connect_timeout: Duration,
    req: &ExecuteRequest,
) -> Result<ExecuteAccepted, LinkError> {
    let client = peer_http_client(proxy, PeerTimeout::ConnectOnly(connect_timeout))?;
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
///
/// `connect_timeout` is the ONLY timeout (see [`post_peer_execute`]): a model delete
/// of tens of GB or an HF fetch takes as long as it takes on the peer.
async fn post_peer_mlx(
    proxy: Option<MeshProxy>,
    base_url: &str,
    token: &str,
    connect_timeout: Duration,
    op: MlxOp,
    body: &serde_json::Value,
) -> Result<serde_json::Value, LinkError> {
    let client = peer_http_client(proxy, PeerTimeout::ConnectOnly(connect_timeout))?;
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

/// `POST <base_url>/v1/swarm/distributed/<op>`, mapping the peer's status back to its class:
/// `2xx` → the op's JSON; `403` → [`DistributedNodeError::Disabled`]; `404` →
/// [`DistributedNodeError::UnknownOp`]; `400` → [`DistributedNodeError::BadRequest`]; `409` →
/// [`DistributedNodeError::Refused`] from its `{code, message}` body; `500` →
/// [`DistributedNodeError::Failed`]; anything else (a `501`, a `401`) and any transport failure
/// → [`LinkError::DistributedProxy`] with the code and body. `connect_timeout` is the only
/// timeout: a verified rank stop takes its grace windows on the peer.
async fn post_peer_distributed(
    proxy: Option<MeshProxy>,
    base_url: &str,
    token: &str,
    connect_timeout: Duration,
    op: &str,
    body: &serde_json::Value,
) -> Result<serde_json::Value, LinkError> {
    let client = peer_http_client(proxy, PeerTimeout::ConnectOnly(connect_timeout))?;
    let response = client
        .post(format!("{base_url}/v1/swarm/distributed/{op}"))
        .bearer_auth(token)
        .json(body)
        .send()
        .await
        .map_err(|err| LinkError::DistributedProxy(err.to_string()))?;

    let status = response.status();
    if status.is_success() {
        return response.json::<serde_json::Value>().await.map_err(|err| {
            LinkError::DistributedProxy(format!("peer responded but its body did not parse: {err}"))
        });
    }
    let text = response.text().await.unwrap_or_default();
    Err(match status.as_u16() {
        403 => DistributedNodeError::Disabled(text).into(),
        404 => DistributedNodeError::UnknownOp(text).into(),
        400 => DistributedNodeError::BadRequest(text).into(),
        409 => {
            #[derive(Deserialize)]
            struct Refusal {
                code: String,
                message: String,
            }
            match serde_json::from_str::<Refusal>(&text) {
                Ok(r) => DistributedNodeError::Refused {
                    code: r.code,
                    message: r.message,
                }
                .into(),
                Err(_) => LinkError::DistributedProxy(format!(
                    "peer refused (409) outside the {{code, message}} contract: {text}"
                )),
            }
        }
        500 => DistributedNodeError::Failed(text).into(),
        code => LinkError::DistributedProxy(format!("peer returned {code}: {text}")),
    })
}

#[async_trait::async_trait]
impl PeerCallResolver for LinkManager {
    async fn peer_call(&self, peer: &str) -> Result<PeerCall, String> {
        LinkManager::peer_call(self, peer)
            .await
            .map_err(|err| err.to_string())
    }
}

/// `1 + peers that are not Offline`: a peer that answers (even wrongly) is present; an
/// unreachable one is not.
fn node_count(registry: &PeerRegistry) -> u32 {
    1 + registry
        .peer_nodes()
        .iter()
        .filter(|node| node.status != NodeStatus::Offline)
        .count() as u32
}

/// Per-pid teardown of a live connection, in dependency order: the poll loop first (so
/// nothing re-registers peers into a fabric being torn down), the control service (its
/// serve tasks + the peer fabric), then the mesh — `tailscale logout` when asked
/// (expires the node key on the control plane; the real engine stops the daemon on
/// success), always followed by the per-pid `shutdown` when logout was skipped or
/// failed. Never a process group. Returns the mesh-logout failure, if any, so the
/// caller can surface it instead of erasing it.
async fn teardown_active(mut active: Active, mesh_logout: bool) -> Option<MeshError> {
    if let Some(task) = active.poll_task.take() {
        task.abort();
    }
    if let Some(control) = active.control.take() {
        control.shutdown();
    }
    if mesh_logout {
        match active.mesh.logout().await {
            Ok(()) => return None,
            Err(err) => {
                tracing::warn!(error = %err, "mesh logout failed; forcing per-pid shutdown");
                active.mesh.shutdown().await;
                return Some(err);
            }
        }
    }
    active.mesh.shutdown().await;
    None
}

/// Why a live connection's supervised tailscaled is being given up on.
enum DaemonFault<'a> {
    /// `try_wait` proved the daemon exited ([`MeshError::DaemonExited`]).
    Exited(&'a MeshError),
    /// The child is alive, but [`MESH_POLL_FAILURE_LOOKS`] consecutive status looks
    /// failed — a daemon that cannot be talked to is wedged, not merely slow.
    Unresponsive { looks: u32, last: &'a MeshError },
}

impl DaemonFault<'_> {
    /// What happened to the daemon, as every supervisor state names it.
    fn cause(&self) -> String {
        match self {
            Self::Exited(err) => format!("LeanZero Link's mesh daemon stopped ({err})"),
            Self::Unresponsive { looks, last } => format!(
                "LeanZero Link's mesh daemon stopped answering ({looks} consecutive status \
                 failures; last: {last}) and was stopped per-pid"
            ),
        }
    }

    fn last_error(&self) -> String {
        match self {
            Self::Exited(err) => format!(
                "mesh daemon died under the connection; dropped it per-pid and kept the \
                 identity — reconnect to re-arm ({err})"
            ),
            Self::Unresponsive { looks, last } => format!(
                "mesh daemon unresponsive: {looks} consecutive status failures; last: {last} \
                 (connection dropped per-pid, identity kept; reconnect to re-arm)"
            ),
        }
    }

    fn log(&self) {
        match self {
            Self::Exited(err) => tracing::error!(
                error = %err,
                "leanzero-link: supervised tailscaled exited; connection dropped, auth back to LoggedIn"
            ),
            Self::Unresponsive { looks, last } => tracing::error!(
                error = %last,
                consecutive = looks,
                "leanzero-link: supervised tailscaled is alive but unresponsive; connection \
                 dropped per-pid (SIGTERM→grace→SIGKILL, our child only), auth back to LoggedIn"
            ),
        }
    }
}

/// The supervised tailscaled under a live connection is at fault — EXITED, or alive but
/// UNRESPONSIVE for [`MESH_POLL_FAILURE_LOOKS`] looks. Drop that connection per-pid (no
/// `tailscale logout`: there is nothing to talk to) and decide, by the process's own
/// outcomes and never by a clock, what happens next ([`FaultResponse`]):
///
/// - the intent says stay connected → RESTART with no user action: auth reads
///   `Connecting`, `reconnect` reads `Reconnecting`, `last_error` names the fault — never
///   `Connected` while the daemon is gone — and [`Core::supervised_restart`] runs the same
///   connect a launch reconnect runs (R3 step 1: an app relaunch brought Link back in ~6 s;
///   this is that path without the relaunch);
/// - the faulted daemon was itself a supervised restart that never proved healthy → it
///   FAILED AGAIN: auth `LoggedIn`, `reconnect: Failed` naming both faults, no further
///   restart (a crash loop is a loud named state, not a silent loop);
/// - the intent is unreadable → dropped, the fault recorded, auth `LoggedIn`.
///
/// The identity on disk is NEVER touched: a daemon fault is not a credential problem, and
/// the only path that clears the credential is the user's own logout (or a worker verdict
/// on the token).
///
/// Acts only if the connection in place is the one the caller observed (`generation`);
/// a stale observer never tears down a newer connection. `from_poll_loop` skips aborting
/// the poll task — the loop is the caller and returns right after. Returns whether it acted.
async fn drop_active_after_daemon_fault(
    core: &Arc<Core>,
    generation: u64,
    fault: DaemonFault<'_>,
    from_poll_loop: bool,
) -> bool {
    let (active, response, cause) = {
        let mut guard = core.inner.lock().await;
        let observed = guard
            .active
            .as_ref()
            .is_some_and(|active| active.generation == generation);
        if !observed {
            return false;
        }
        let mut active = guard.active.take().expect("checked present under the lock");
        if from_poll_loop {
            // The loop owns this call; dropping the handle detaches rather than aborts.
            active.poll_task = None;
        }
        fault.log();
        let cause = fault.cause();
        let response = fault_response(&guard, generation);
        let email = active.email.clone();
        match &response {
            FaultResponse::Restart => {
                guard.auth = AuthState::Connecting { email };
                guard.reconnect = ReconnectState::Reconnecting {
                    started_at: Utc::now(),
                };
                guard.last_error = Some(format!(
                    "{cause} — restarting it with no user action"
                ));
                guard.restart = Some(SupervisedRestart {
                    cause: cause.clone(),
                    generation: None,
                    healthy_looks: 0,
                });
            }
            FaultResponse::FailedAgain { first } => {
                let reason = format!(
                    "{cause}, again, before the automatic restart proved healthy (it followed: \
                     {first}) — not restarting it again; Connect to retry"
                );
                tracing::error!(%reason, "leanzero_link_supervisor: failed again");
                guard.auth = AuthState::LoggedIn { email };
                guard.reconnect = ReconnectState::Failed {
                    reason: reason.clone(),
                    at: Utc::now(),
                };
                guard.last_error = Some(reason);
                guard.restart = None;
            }
            FaultResponse::Drop => {
                guard.auth = AuthState::LoggedIn { email };
                guard.last_error = Some(fault.last_error());
                guard.restart = None;
            }
        }
        (active, response, cause)
    };
    let seams = active.seams.clone();
    teardown_active(active, false).await;
    if response == FaultResponse::Restart {
        let core = core.clone();
        tokio::spawn(core.supervised_restart(seams, cause));
    }
    true
}

/// The supervisor's decision for the faulted connection `generation` (see
/// [`drop_active_after_daemon_fault`]).
fn fault_response(inner: &Inner, generation: u64) -> FaultResponse {
    if let Some(restart) = &inner.restart {
        if restart.generation == Some(generation) {
            return FaultResponse::FailedAgain {
                first: restart.cause.clone(),
            };
        }
    }
    match &inner.intent {
        Ok(record) if record.intent == LinkIntent::Connected => FaultResponse::Restart,
        _ => FaultResponse::Drop,
    }
}

impl Core {
    /// Bring the faulted connection back: the launch reconnect's connect
    /// ([`ConnectOrigin::Supervisor`]), bounded by its own outcomes — the daemon comes up
    /// (`Reconnected`; proven after [`MESH_POLL_FAILURE_LOOKS`] healthy looks) or the
    /// connect fails (`Failed`, naming the fault and the failure; no retry). A user action
    /// meanwhile (Disconnect, Log out) cancels it and stands.
    ///
    /// Boxed: the restart's connect spawns the poll loop that may call back here, and a
    /// named future type is what breaks that cycle for the compiler.
    fn supervised_restart(self: Arc<Self>, seams: Seams, cause: String) -> BoxFuture<'static, ()> {
        Box::pin(async move {
            let result = self.connect_as(ConnectOrigin::Supervisor, seams).await;
            let err = match result {
                Ok(()) | Err(LinkError::ConnectCancelled | LinkError::ConnectAborted) => return,
                Err(err) => err,
            };
            let mut inner = self.inner.lock().await;
            if !matches!(inner.reconnect, ReconnectState::Reconnecting { .. }) {
                return;
            }
            let reason = format!("{cause}, and the automatic restart failed: {err}");
            tracing::error!(%reason, "leanzero_link_supervisor: restart failed");
            inner.reconnect = ReconnectState::Failed {
                reason: reason.clone(),
                at: Utc::now(),
            };
            inner.last_error = Some(reason);
            inner.restart = None;
        })
    }
}

async fn peer_sync_loop(
    mesh: Arc<dyn Mesh>,
    registry: PeerRegistry,
    interval: Duration,
    control_port: u16,
    core: Weak<Core>,
    generation: u64,
) {
    let mut last: Option<Vec<MeshPeer>> = None;
    loop {
        match mesh.status().await {
            Ok(status) => {
                if last.as_deref() != Some(status.peers.as_slice()) {
                    registry.set_mesh_peers(&status.peers, control_port);
                    last = Some(status.peers.clone());
                }
                let Some(core) = core.upgrade() else { return };
                let mut guard = core.inner.lock().await;
                if guard.mesh_poll_failures != 0 {
                    guard.mesh_poll_failures = 0;
                }
                prove_restart(&mut guard, generation);
            }
            Err(err @ MeshError::DaemonExited { .. }) => {
                let Some(core) = core.upgrade() else { return };
                core.inner.lock().await.mesh_poll_failures += 1;
                if drop_active_after_daemon_fault(
                    &core,
                    generation,
                    DaemonFault::Exited(&err),
                    true,
                )
                .await
                {
                    return;
                }
                // Our `Active` is not installed yet (connect() has not finished its Ok
                // arm) — keep polling; the next tick, or the first status read, will
                // find it and drop it.
                tracing::warn!(error = %err, "tailscaled exited before the connection was installed; retrying");
            }
            Err(err) => {
                let Some(core) = core.upgrade() else { return };
                let failures = {
                    let mut guard = core.inner.lock().await;
                    guard.mesh_poll_failures += 1;
                    guard.mesh_poll_failures
                };
                if failures < MESH_POLL_FAILURE_LOOKS {
                    tracing::warn!(
                        error = %err,
                        consecutive = failures,
                        threshold = MESH_POLL_FAILURE_LOOKS,
                        "mesh status poll failed; will retry"
                    );
                } else if drop_active_after_daemon_fault(
                    &core,
                    generation,
                    DaemonFault::Unresponsive {
                        looks: failures,
                        last: &err,
                    },
                    true,
                )
                .await
                {
                    return;
                } else {
                    // Our `Active` is not installed yet — keep looking; the count keeps
                    // climbing, so the next look past the threshold drops it.
                    tracing::warn!(
                        error = %err,
                        consecutive = failures,
                        "tailscaled unresponsive before the connection was installed; retrying"
                    );
                }
            }
        }
        tokio::time::sleep(interval).await;
    }
}

/// One healthy status look of connection `generation`: when it is the supervisor's restart,
/// count it; at [`MESH_POLL_FAILURE_LOOKS`] unbroken healthy looks the restart has proven
/// itself — the same look count that judges a daemon unresponsive judges it healthy — and a
/// later fault is a fresh one, restarted again.
fn prove_restart(inner: &mut Inner, generation: u64) {
    let Some(restart) = inner.restart.as_mut() else {
        return;
    };
    if restart.generation != Some(generation) {
        return;
    }
    restart.healthy_looks += 1;
    if restart.healthy_looks >= MESH_POLL_FAILURE_LOOKS {
        tracing::info!(
            cause = %restart.cause,
            looks = restart.healthy_looks,
            "leanzero_link_supervisor: the restarted mesh daemon proved healthy"
        );
        inner.restart = None;
    }
}
