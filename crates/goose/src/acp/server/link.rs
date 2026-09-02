//! ACP surface for LeanZero Link: bridges the `_goose/unstable/leanzeroLink/*`
//! custom methods to a process-wide [`LinkManager`], and implements the
//! [`SwarmStateSource`] the control service reads from goosed's live session state.
//!
//! ## What lives here
//! - [`GoosedSwarmStateSource`] — the decoupling seam. It sources the node's own
//!   [`NodeState`], the local [`SessionSummary`] index, and a live delta stream from
//!   goosed's [`AgentManager`] + [`SessionManager`]. No mesh, no network.
//! - The lazily-constructed global [`LinkManager`] (an `OnceLock` holder, mirroring
//!   `goose_sidecar::engine::global_manager`), rebuilt when its build key — the on-disk
//!   identity email, the remote-execution setting — changes while no mesh is live. The
//!   control `node_token` is NOT derived here: the manager sets it at connect from the
//!   worker-issued account secret (`leanzero_link::token`), and this layer reads it back
//!   via [`LinkManager::node_token`] for the loopback proxy.
//! - The `leanzeroLink/*` ACP handlers (`impl GooseAcpAgent`).
//!
//! ## Delta coverage
//! [`GoosedSwarmStateSource::subscribe_local_deltas`] always emits `NodeStateChanged` on
//! busy/idle (and active-count) transitions and `SessionUpserted` on session
//! create/rename/update/busy-flip, derived by polling the managers. It ALSO emits
//! per-message `SessionDelta` when — and only when — a process-wide delta tap has been
//! injected via [`set_delta_source`].
//!
//! The per-session `MessageEvent` buses live in goose-server (`session_event_bus`), a
//! crate that DEPENDS ON `goose`, so they are unreachable from here by name. The fix is
//! dependency inversion: goose-server owns a process-wide tap of every session's reply
//! events, classifies each into a [`SessionDeltaKind`] + opaque payload (the goose-server
//! side is the only place that can see `MessageEvent`), and injects it as a
//! [`DeltaSource`] at boot. If no source is injected (goose built without the server
//! boot path), the stream keeps its node/session-only behavior — loud-absent, not broken.
//! A tapped item that cannot be classified is dropped on the goose-server side (never
//! faked into a wrong kind); this layer maps whatever the source yields 1:1 to a
//! `SessionDelta`.

use super::*;

use crate::config::ConfigError;
use std::sync::{Mutex as StdMutex, OnceLock};
use std::time::Duration;

use chrono::{DateTime, Utc};
use futures::stream::BoxStream;
use futures::StreamExt;
use serde::de::DeserializeOwned;
use serde::Serialize;
use tokio_stream::wrappers::ReceiverStream;

use leanzero_link::control::{ControlConfig, DEFAULT_CONTROL_PORT};
use leanzero_link::identity;
use leanzero_link::manager::{AuthState, LinkError, LinkManager, LinkManagerConfig, LinkState};
use leanzero_link::mesh::{MeshConfig, MeshStatus};
use leanzero_link::state::SwarmStateSource;
use leanzero_link::wire::{LinkEvent, NodeState, NodeStatus, SessionSummary};
use leanzero_link::worker_client::DEFAULT_WORKER_BASE_URL;
use leanzero_link::{discovery, worker_client};

/// The delta class carried on the wire, re-exported so the goose-server boot path can
/// classify its `MessageEvent`s into it without naming `leanzero-link` directly.
pub use leanzero_link::wire::SessionDeltaKind;

/// The remote-execution seam types, re-exported so goose-server can implement
/// [`RemoteExecutor`] and drive [`ExecuteRequest`]/[`ExecuteAccepted`]/[`ExecuteError`]
/// through `goose::acp::server::*` without a direct `leanzero-link` dependency.
pub use leanzero_link::state::{ExecuteAccepted, ExecuteError, ExecuteRequest, RemoteExecutor};

/// The remote model-management seam types, re-exported so goose's `GoosedMlxControl` (in
/// `mlx_engine.rs`) and the boot path can drive the mesh MLX proxy without naming
/// `leanzero-link` directly. [`MlxOp`] is the op enum both the control routes and the
/// forwarding handlers key on; [`MlxControlError`] carries a peer's failure class verbatim.
pub use leanzero_link::state::{MlxControl, MlxControlError, MlxOp};

/// How often the delta poller re-snapshots node/session state. Matches the fabric's own
/// peer poll cadence (`ControlConfig::poll_interval` default) so a busy/idle transition
/// surfaces to peers on the same order of latency whether via our stream or their poll.
const LINK_DELTA_POLL_INTERVAL: Duration = Duration::from_secs(3);

/// The user's remote-execution switch: a goose config key (`config.yaml`), or the same
/// name as an env override — `Config::get_param` upper-cases the key, so file key and
/// env name coincide. Absent = OFF: a node is observe-only until its owner opts in, and
/// the control service then answers `403` to `/execute` and every `/mlx/*` op.
pub const ALLOW_REMOTE_EXECUTION_KEY: &str = "LEANZERO_LINK_ALLOW_REMOTE_EXECUTION";

// ---------------------------------------------------------------------------
// Delta-tap injection seam — dependency inversion for per-message mirroring.
// ---------------------------------------------------------------------------

/// One already-classified per-message delta, handed across the crate boundary by a
/// [`DeltaSource`]. The producer (goose-server) has already mapped its `MessageEvent`
/// into a [`SessionDeltaKind`] + opaque `payload` and stamped the origin-scoped `seq`;
/// this layer only wraps it into a [`LinkEvent::SessionDelta`]. Carrying `seq` here (not
/// in the brief's minimal tuple) lets the ORIGIN node's per-session delta sequence — the
/// value the wire contract requires `SessionDelta.seq` to hold — cross the seam intact
/// instead of being re-minted where the session identity is already lost.
pub struct DeltaInput {
    pub session_id: String,
    pub seq: u64,
    pub kind: SessionDeltaKind,
    pub payload: serde_json::Value,
}

/// A process-wide fan-out of every local session's per-message deltas. goose-server
/// implements this over its `session_event_bus` tap at boot; `goose` on its own never
/// has one. `subscribe` yields a fresh stream per call (one per control-service delta
/// pump); dropping the stream must end its side of the tap.
pub trait DeltaSource: Send + Sync + 'static {
    fn subscribe(&self) -> BoxStream<'static, DeltaInput>;
}

static DELTA_SOURCE: OnceLock<Arc<dyn DeltaSource>> = OnceLock::new();

/// Inject the process-wide delta tap. Called once by the goose-server boot path after
/// its `AppState` (which owns the tap) exists. A second call is ignored with a warning —
/// the tap is a singleton for the process, and silently swapping it would strand the
/// control service on a dead receiver.
pub fn set_delta_source(source: impl DeltaSource) {
    let source: Arc<dyn DeltaSource> = Arc::new(source);
    if DELTA_SOURCE.set(source).is_err() {
        warn!("leanzeroLink: delta source already injected; ignoring the duplicate");
    }
}

fn current_delta_source() -> Option<Arc<dyn DeltaSource>> {
    DELTA_SOURCE.get().cloned()
}

static EXECUTOR: OnceLock<Arc<dyn RemoteExecutor>> = OnceLock::new();

/// Inject the process-wide remote executor. Called once by the goose-server boot path
/// (its `GoosedRemoteExecutor` drives the reply machinery, which only goose-server can
/// reach). Threaded into every [`LinkManager`] built afterwards — so the node's
/// `POST /v1/swarm/execute` route and [`LinkManager::remote_execute`] self short-circuit
/// can actually run goose. If never set (goose without the server boot), the control
/// route answers `501` — loud-absent, never a fake accept. A second call is ignored with
/// a warning: the executor is a process singleton.
pub fn set_executor(executor: impl RemoteExecutor) {
    let executor: Arc<dyn RemoteExecutor> = Arc::new(executor);
    if EXECUTOR.set(executor).is_err() {
        warn!("leanzeroLink: remote executor already injected; ignoring the duplicate");
    }
}

fn current_executor() -> Option<Arc<dyn RemoteExecutor>> {
    EXECUTOR.get().cloned()
}

static MLX_CONTROL: OnceLock<Arc<dyn MlxControl>> = OnceLock::new();

/// Inject the process-wide local MLX-engine control (goose's `GoosedMlxControl`). Called
/// once at boot (mirroring [`set_executor`]). Threaded into every [`LinkManager`] built
/// afterwards — so this node's `POST /v1/swarm/mlx/*` routes run real model-management ops
/// against the local `goose_sidecar` engine. If never set, those routes answer `501`
/// (loud-absent, never a fabricated result). A second call is ignored with a warning: the
/// control is a process singleton over the one global engine manager.
pub fn set_mlx_control(control: impl MlxControl) {
    let control: Arc<dyn MlxControl> = Arc::new(control);
    if MLX_CONTROL.set(control).is_err() {
        warn!("leanzeroLink: mlx control already injected; ignoring the duplicate");
    }
}

fn current_mlx_control() -> Option<Arc<dyn MlxControl>> {
    MLX_CONTROL.get().cloned()
}

// ---------------------------------------------------------------------------
// SwarmStateSource — goosed's live session state as the swarm view.
// ---------------------------------------------------------------------------

/// Implements [`SwarmStateSource`] over goosed's [`AgentManager`] + [`SessionManager`].
pub struct GoosedSwarmStateSource {
    agent_manager: Arc<AgentManager>,
    session_manager: Arc<SessionManager>,
    node_id: String,
}

impl GoosedSwarmStateSource {
    pub fn new(agent_manager: Arc<AgentManager>, session_manager: Arc<SessionManager>) -> Self {
        Self {
            agent_manager,
            session_manager,
            node_id: stable_node_id(),
        }
    }
}

#[async_trait::async_trait]
impl SwarmStateSource for GoosedSwarmStateSource {
    async fn local_node(&self) -> NodeState {
        let snapshot =
            snapshot_local(&self.agent_manager, &self.session_manager, &self.node_id).await;
        derive_node(&self.node_id, &snapshot, Utc::now())
    }

    /// The store's error rides through verbatim (`session index unreadable: <err>`): the
    /// control service answers `503` on `/v1/swarm/sessions` and refuses `/execute`, and
    /// every peer keeps its last mirror (R-M2 / FH#2). Never `[]` for a store that could
    /// not be read — that was the window in which a Busy node's peers purged its sessions
    /// and dispatched a second job.
    async fn local_sessions(&self) -> Result<Vec<SessionSummary>, String> {
        self.local_sessions_checked().await
    }

    fn subscribe_local_deltas(&self) -> BoxStream<'static, LinkEvent> {
        self.local_deltas_with(current_delta_source())
    }
}

impl GoosedSwarmStateSource {
    /// The local session index, or the store's error verbatim (`session index unreadable:
    /// <err>`) — what the trait's `local_sessions` forwards to.
    pub async fn local_sessions_checked(&self) -> Result<Vec<SessionSummary>, String> {
        snapshot_local(&self.agent_manager, &self.session_manager, &self.node_id)
            .await
            .sessions
    }

    /// The `NodeStateChanged` / `SessionUpserted` poller stream — the always-on half of
    /// the local delta feed, derived by re-snapshotting the managers. One poller per
    /// subscription; it exits when the receiver is dropped (the control service's local
    /// delta pump ends), so nothing leaks past a disconnect.
    fn node_session_poller(&self) -> BoxStream<'static, LinkEvent> {
        let (tx, rx) = tokio::sync::mpsc::channel::<LinkEvent>(256);
        let agent_manager = self.agent_manager.clone();
        let session_manager = self.session_manager.clone();
        let node_id = self.node_id.clone();

        tokio::spawn(async move {
            let mut last_node_key: Option<(NodeStatus, u32)> = None;
            let mut last_sessions: HashMap<String, SessionSummary> = HashMap::new();
            loop {
                let snapshot = snapshot_local(&agent_manager, &session_manager, &node_id).await;

                // An unreadable index publishes NOTHING this tick: an empty index here
                // would ride out as SessionUpserted silence plus a retain-wipe, which every
                // peer folds as "this node's sessions are gone", and a NodeStateChanged
                // built on a hole. The next readable tick reconciles; peers still see the
                // live Busy/Idle through their `/nodes` poll, which never needs the store.
                let Ok(sessions) = &snapshot.sessions else {
                    tokio::time::sleep(LINK_DELTA_POLL_INTERVAL).await;
                    continue;
                };

                let node = derive_node(&node_id, &snapshot, Utc::now());
                let node_key = (node.status.clone(), node.sessions_active);
                if last_node_key.as_ref() != Some(&node_key) {
                    if tx.send(LinkEvent::NodeStateChanged(node)).await.is_err() {
                        return;
                    }
                    last_node_key = Some(node_key);
                }

                for summary in sessions {
                    if last_sessions.get(&summary.session_id) != Some(summary) {
                        if tx
                            .send(LinkEvent::SessionUpserted(summary.clone()))
                            .await
                            .is_err()
                        {
                            return;
                        }
                        last_sessions.insert(summary.session_id.clone(), summary.clone());
                    }
                }
                let present: HashSet<&str> =
                    sessions.iter().map(|s| s.session_id.as_str()).collect();
                last_sessions.retain(|id, _| present.contains(id.as_str()));

                tokio::time::sleep(LINK_DELTA_POLL_INTERVAL).await;
            }
        });

        Box::pin(ReceiverStream::new(rx))
    }

    /// Merge the always-on node/session poller with the per-message `SessionDelta` feed
    /// from an injected [`DeltaSource`]. With no source, this is exactly the poller —
    /// today's behavior. The delta half maps each classified [`DeltaInput`] 1:1 to a
    /// [`LinkEvent::SessionDelta`], preserving the origin `seq` (never re-minted here).
    fn local_deltas_with(
        &self,
        delta_source: Option<Arc<dyn DeltaSource>>,
    ) -> BoxStream<'static, LinkEvent> {
        let poller = self.node_session_poller();
        match delta_source {
            None => poller,
            Some(source) => {
                let deltas = source.subscribe().map(|input| LinkEvent::SessionDelta {
                    session_id: input.session_id,
                    seq: input.seq,
                    kind: input.kind,
                    payload: input.payload,
                });
                Box::pin(futures::stream::select(poller, deltas))
            }
        }
    }
}

/// The local view at one instant. `sessions` is the store's index (each non-archived
/// [`Session`] as a [`SessionSummary`], `live` = holds a cancel token) or the store's
/// error verbatim; `busy` is the in-flight set from the token maps alone — never from
/// the store — so a Busy node stays Busy through a store failure, transient or not.
struct LocalSnapshot {
    sessions: Result<Vec<SessionSummary>, String>,
    busy: HashSet<String>,
}

async fn snapshot_local(
    agent_manager: &Arc<AgentManager>,
    session_manager: &SessionManager,
    node_id: &str,
) -> LocalSnapshot {
    let busy = busy_session_ids(agent_manager).await;

    let sessions = match session_manager.list_sessions().await {
        Ok(sessions) => Ok(sessions
            .into_iter()
            .filter(|session| session.archived_at.is_none())
            .map(|session| SessionSummary {
                session_id: session.id.clone(),
                origin_node_id: node_id.to_string(),
                working_dir: session.working_dir.to_string_lossy().into_owned(),
                name: session.name,
                updated_at: session.updated_at,
                message_count: session.message_count as u64,
                live: busy.contains(&session.id),
            })
            .collect()),
        Err(error) => {
            error!(
                %error,
                busy = busy.len(),
                "leanzeroLink: the local session index is unreadable; Busy/Idle still comes from the live token maps"
            );
            Err(format!("session index unreadable: {error}"))
        }
    };

    LocalSnapshot { sessions, busy }
}

/// The busy set: every session id holding an in-flight cancel token, read from the token
/// maps alone — never from the session store or the agent cache. Both reply doors
/// register there (the ACP `on_prompt` run and goose-server's `spawn_reply_task`), as do
/// the orchestrator's subagent runs.
///
/// Two managers can exist in one process: the ACP server builds its own
/// (`GooseAcpAgent::new`), while goose-server's `AppState` holds the
/// [`AgentManager::instance`] singleton — so under `goosed agent` a REST reply or a
/// remote-executed prompt runs on a manager this source was not built from. The union is
/// this machine's truth; the singleton is only READ if it was already built, never
/// constructed by asking.
async fn busy_session_ids(agent_manager: &Arc<AgentManager>) -> HashSet<String> {
    let mut busy: HashSet<String> = agent_manager.busy_session_ids().await.into_iter().collect();
    if let Some(shared) = AgentManager::instance_if_built() {
        if !Arc::ptr_eq(&shared, agent_manager) {
            busy.extend(shared.busy_session_ids().await);
        }
    }
    busy
}

/// The node's own [`NodeState`]. `mesh_ip` is left `None`: the source does not know the
/// mesh IP (that is the mesh layer's knowledge); the control service's `/nodes` handler
/// fills it for the direct response, and peers learn it from the tailnet + their `/nodes`
/// polls.
///
/// The token is the fact; the index only decorates it. With a readable index the busy
/// session named is the freshest live one (`NodeStatus::from_sessions`); a token the
/// index cannot see — the store is down, or lists no such session — still makes the node
/// Busy, and `sessions_active` counts tokens, not index rows.
fn derive_node(node_id: &str, snapshot: &LocalSnapshot, now: DateTime<Utc>) -> NodeState {
    let status = match &snapshot.sessions {
        Ok(sessions) => match NodeStatus::from_sessions(sessions) {
            NodeStatus::Idle => busy_status(&snapshot.busy),
            busy => busy,
        },
        Err(_) => busy_status(&snapshot.busy),
    };
    NodeState {
        node_id: node_id.to_string(),
        hostname: hostname_string(),
        mesh_ip: None,
        status,
        sessions_active: snapshot.busy.len() as u32,
        updated_at: now,
        // The POLLER's field: a peer records why its last poll of us failed. Our own
        // report never carries one.
        last_poll_error: None,
    }
}

/// Busy on the lexically first token-holding id (deterministic without timestamps), Idle
/// when no token is held.
fn busy_status(busy: &HashSet<String>) -> NodeStatus {
    busy.iter()
        .min()
        .map(|id| NodeStatus::Busy {
            session_id: id.clone(),
        })
        .unwrap_or(NodeStatus::Idle)
}

// ---------------------------------------------------------------------------
// Node identity helpers.
// ---------------------------------------------------------------------------

fn hostname_string() -> String {
    gethostname::gethostname().to_string_lossy().into_owned()
}

/// The node id: the sanitized hostname, plus the mesh's persisted 6-hex per-machine
/// suffix once it exists (so this matches the tailnet hostname the manager mints). Two
/// machines that share a hostname stay distinct once each has connected at least once.
fn stable_node_id() -> String {
    let raw = hostname_string();
    let base: String = sanitize_hostname(&raw).chars().take(56).collect();
    let base = base.trim_end_matches('-').to_string();
    match read_persisted_node_suffix() {
        Some(suffix) => format!("{base}-{suffix}"),
        None => base,
    }
}

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

/// The per-machine suffix the mesh persists at `<identity dir>/node-id`
/// (`~/.leanzero/node-id`). Absent until the first connect — the one quiet `None`. Any
/// other failure (no home dir, an unreadable file) is logged at error level before the
/// suffix-less id is used: that id will NOT match the tailnet hostname the mesh minted,
/// and a silent mismatch is how peers end up mirroring a ghost node.
fn read_persisted_node_suffix() -> Option<String> {
    let identity_path = match identity::default_identity_path() {
        Ok(path) => path,
        Err(error) => {
            error!(
                %error,
                "leanzeroLink: no identity directory; the node id carries no per-machine suffix"
            );
            return None;
        }
    };
    let node_id_path = identity_path.parent()?.join("node-id");
    let content = match std::fs::read_to_string(&node_id_path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        Err(error) => {
            error!(
                %error,
                path = %node_id_path.display(),
                "leanzeroLink: the persisted node-id is unreadable; this start uses the suffix-less \
                 node id, which will not match the tailnet hostname"
            );
            return None;
        }
    };
    let trimmed = content.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// The on-disk account email (lowercased), or `None` when logged out or the identity
/// file is unreadable/malformed — the manager surfaces a malformed file loudly on build.
fn current_identity_email() -> Option<String> {
    let store = identity::IdentityStore::at_default().ok()?;
    match store.load() {
        Ok(Some(id)) => Some(id.email.to_lowercase()),
        _ => None,
    }
}

/// Read the switch. `NotFound` is the documented default (OFF); any other error — an
/// unparseable value, an unreadable config file — is logged and read as OFF: fail
/// closed, never silently open.
fn remote_execution_allowed() -> bool {
    allow_from_config(Config::global().get_param::<bool>(ALLOW_REMOTE_EXECUTION_KEY))
}

fn allow_from_config(read: Result<bool, ConfigError>) -> bool {
    match read {
        Ok(value) => value,
        Err(ConfigError::NotFound(_)) => false,
        Err(error) => {
            warn!(
                %error,
                key = ALLOW_REMOTE_EXECUTION_KEY,
                "leanzeroLink: the remote-execution setting is unreadable; treating it as OFF"
            );
            false
        }
    }
}

/// What binary discovery found when the manager was built, or the full searched-list
/// text of its failure — carried, never substituted by a guessed path. A failed
/// discovery keeps the manager constructible (health/requestCode/verify/status/logout
/// need no daemon) and refuses `connect` with this text; the status DTO shows it before
/// the click.
#[derive(Debug, Clone)]
struct MeshBinaries {
    tailscaled: Result<PathBuf, String>,
    tailscale: Result<PathBuf, String>,
}

impl MeshBinaries {
    fn discover() -> Self {
        let binaries = Self {
            tailscaled: discovery::find_tailscaled().map_err(|error| error.to_string()),
            tailscale: discovery::find_tailscale_cli().map_err(|error| error.to_string()),
        };
        for (name, found) in [
            ("tailscaled", &binaries.tailscaled),
            ("tailscale", &binaries.tailscale),
        ] {
            if let Err(error) = found {
                error!(%error, binary = name, "leanzeroLink: mesh binary missing; connect is refused until it is found");
            }
        }
        binaries
    }

    /// Both paths, or the refusal text naming every missing binary and everywhere
    /// discovery looked for it.
    fn paths(&self) -> Result<(PathBuf, PathBuf), String> {
        match (&self.tailscaled, &self.tailscale) {
            (Ok(daemon), Ok(cli)) => Ok((daemon.clone(), cli.clone())),
            (daemon, cli) => {
                let missing: Vec<&str> = [daemon, cli]
                    .into_iter()
                    .filter_map(|found| found.as_ref().err().map(String::as_str))
                    .collect();
                Err(format!(
                    "cannot start the mesh — {}",
                    missing.join("; and ")
                ))
            }
        }
    }

    fn to_dto(&self) -> LeanzeroLinkMeshBinariesDto {
        fn one(found: &Result<PathBuf, String>) -> LeanzeroLinkBinaryDto {
            match found {
                Ok(path) => LeanzeroLinkBinaryDto::Found {
                    path: path.display().to_string(),
                },
                Err(error) => LeanzeroLinkBinaryDto::Missing {
                    error: error.clone(),
                },
            }
        }
        LeanzeroLinkMeshBinariesDto {
            tailscaled: one(&self.tailscaled),
            tailscale: one(&self.tailscale),
        }
    }
}

/// Whether a fresh discovery verdict differs from the one a manager was built with: the
/// paths (or the refusal text) are what the manager's mesh config and the status DTO
/// carry, so equality means nothing on disk moved.
fn mesh_binaries_changed(held: &MeshBinaries, fresh: &MeshBinaries) -> bool {
    held.paths() != fresh.paths()
}

// ---------------------------------------------------------------------------
// The process-wide LinkManager (lazily built, rebuilt when its build key changes).
// ---------------------------------------------------------------------------

/// The inputs a manager is built from. A change to any of them means the cached manager
/// no longer reflects the identity on disk or the user's settings, so it is rebuilt — but
/// only while NOT Connecting/Connected: a rebuild drops the live mesh, so a live manager
/// keeps serving and the new key applies at its next connect.
#[derive(Debug, Clone, PartialEq, Eq)]
struct LinkBuildKey {
    /// The account email (lowercased) on disk, or `None` when logged out.
    email: Option<String>,
    /// [`ALLOW_REMOTE_EXECUTION_KEY`] at build time — what the control service enforces.
    allow_remote_execution: bool,
}

fn current_build_key() -> LinkBuildKey {
    LinkBuildKey {
        email: current_identity_email(),
        allow_remote_execution: remote_execution_allowed(),
    }
}

struct LinkHolder {
    key: LinkBuildKey,
    manager: Arc<LinkManager>,
    /// Discovery's verdict at build time; re-run on every rebuild.
    mesh_binaries: MeshBinaries,
    /// The text of the last connect this layer refused before the manager saw it
    /// (missing mesh binaries), cleared when a connect reaches the manager. Rides the
    /// status DTO's `lastError` because the manager's own `last_error` never learns of it.
    connect_refusal: Option<String>,
    /// Set once per deferred rebuild so a live mesh does not log the deferral every poll.
    stale_logged: bool,
}

/// What this layer knows about the manager it built, snapshotted for the status DTO.
#[derive(Debug, Clone)]
struct HolderView {
    allow_remote_execution: bool,
    mesh_binaries: MeshBinaries,
    connect_refusal: Option<String>,
}

/// Read only after `link_manager()` built the holder — every status/connect/logout
/// handler does exactly that first.
fn holder_view() -> HolderView {
    let guard = LINK
        .get()
        .expect("holder_view is read only after link_manager() built the holder")
        .lock()
        .unwrap();
    HolderView {
        allow_remote_execution: guard.key.allow_remote_execution,
        mesh_binaries: guard.mesh_binaries.clone(),
        connect_refusal: guard.connect_refusal.clone(),
    }
}

fn record_connect_refusal(refusal: Option<String>) {
    if let Some(holder) = LINK.get() {
        holder.lock().unwrap().connect_refusal = refusal;
    }
}

static LINK: OnceLock<StdMutex<LinkHolder>> = OnceLock::new();

/// Resolve the worker base URL from the env override, else the crate default; logs which.
fn resolve_worker_base_url() -> String {
    match std::env::var("LEANZERO_LINK_WORKER_URL") {
        Ok(value) if !value.trim().is_empty() => {
            let value = value.trim().to_string();
            info!(worker_base_url = %value, resolved_from = "env LEANZERO_LINK_WORKER_URL",
                  "leanzeroLink: worker URL resolved");
            value
        }
        _ => {
            info!(worker_base_url = %DEFAULT_WORKER_BASE_URL, resolved_from = "crate default",
                  "leanzeroLink: worker URL resolved");
            DEFAULT_WORKER_BASE_URL.to_string()
        }
    }
}

fn build_link_config(
    key: &LinkBuildKey,
    binaries: &MeshBinaries,
) -> Result<LinkManagerConfig, agent_client_protocol::Error> {
    let worker_base_url = resolve_worker_base_url();
    let identity_path = identity::default_identity_path()
        .internal_err_ctx("resolving the LeanZero Link identity path")?;

    let (tailscaled, tailscale_cli) = match binaries.paths() {
        Ok(paths) => paths,
        // Discovery failed: the mesh template carries EMPTY paths, which never reach a
        // spawn — `on_leanzero_link_connect` refuses with the discovery text before
        // `LinkManager::connect` can run, and an empty path fails `MeshEngine::start`
        // loudly should any other caller ever reach it. No guessed install location:
        // the manager stays constructible for the auth flows and nothing else.
        Err(_) => (PathBuf::new(), PathBuf::new()),
    };
    let mesh = MeshConfig::new(tailscaled, tailscale_cli, stable_node_id())
        .internal_err_ctx("building the LeanZero Link mesh config")?;

    // A TEMPLATE: the manager sets the real `node_token` at connect, derived from the
    // worker-issued account secret (`leanzero_link::token::node_token_from_secret`), and
    // `ControlService::start` refuses an empty one — so this empty string can never reach
    // a listening service. Nothing derivable from the email is minted here any more.
    let mut control = ControlConfig::new(String::new(), None);
    // The crate's default is observe-only; only the user's own setting opts a node in.
    control.allow_remote_execution = key.allow_remote_execution;

    Ok(LinkManagerConfig {
        worker_base_url,
        identity_path,
        mesh,
        control,
    })
}

/// Proxy the local control service's `GET /v1/swarm/nodes` (loopback, bearer `token`),
/// returning its `{ self, peers }` body verbatim so the snake_case wire shape survives.
/// Every failure — transport, a non-2xx, a body that is not the contract — is an `Err`
/// carrying the reason; this function never answers a failure with a roster.
async fn fetch_local_swarm_nodes(token: &str) -> Result<LeanzeroLinkNodesResponse, String> {
    let url = format!("http://127.0.0.1:{DEFAULT_CONTROL_PORT}/v1/swarm/nodes");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| e.to_string())?;
    let body: serde_json::Value = client
        .get(&url)
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    parse_swarm_nodes_body(body)
}

/// The control service always sends both keys, so a body missing either is a bug or a
/// foreign responder on the port — an error, never "no peers".
fn parse_swarm_nodes_body(body: serde_json::Value) -> Result<LeanzeroLinkNodesResponse, String> {
    let self_node = body
        .get("self")
        .cloned()
        .ok_or_else(|| "control service /v1/swarm/nodes body has no 'self' object".to_string())?;
    let peers = body
        .get("peers")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .ok_or_else(|| "control service /v1/swarm/nodes body has no 'peers' array".to_string())?;
    Ok(LeanzeroLinkNodesResponse { self_node, peers })
}

// ---------------------------------------------------------------------------
// DTO conversions.
// ---------------------------------------------------------------------------

fn auth_state_tag(auth: &AuthState) -> String {
    match auth {
        AuthState::LoggedOut => "loggedOut",
        AuthState::CodeSent { .. } => "codeSent",
        AuthState::LoggedIn { .. } => "loggedIn",
        AuthState::Connecting { .. } => "connecting",
        AuthState::Connected { .. } => "connected",
    }
    .to_string()
}

fn auth_state_to_dto(auth: AuthState) -> LeanzeroLinkAuthStateDto {
    match auth {
        AuthState::LoggedOut => LeanzeroLinkAuthStateDto::LoggedOut,
        AuthState::CodeSent { email, expires_at } => LeanzeroLinkAuthStateDto::CodeSent {
            email,
            expires_at: expires_at.to_rfc3339(),
        },
        AuthState::LoggedIn { email } => LeanzeroLinkAuthStateDto::LoggedIn { email },
        AuthState::Connecting { email } => LeanzeroLinkAuthStateDto::Connecting { email },
        AuthState::Connected { email, mesh_ip } => {
            LeanzeroLinkAuthStateDto::Connected { email, mesh_ip }
        }
    }
}

fn mesh_status_to_dto(mesh: MeshStatus) -> LeanzeroLinkMeshStatusDto {
    LeanzeroLinkMeshStatusDto {
        self_ip: mesh.self_ip,
        self_hostname: mesh.self_hostname,
        backend_state: mesh.backend_state.to_string(),
        online: mesh.online,
        peers: mesh
            .peers
            .into_iter()
            .map(|peer| LeanzeroLinkMeshPeerDto {
                hostname: peer.hostname,
                ip: peer.ip,
                online: peer.online,
                last_seen: peer.last_seen,
            })
            .collect(),
    }
}

/// The status DTO: the crate's [`LinkState`] plus what this layer knows about the manager
/// it built. `remote_execution_allowed` is the user's setting NOW (what a toggle shows);
/// `remote_execution_allowed_live` is the value the RUNNING control service enforces,
/// present only while Connected — so a toggle flipped mid-connection reads as "applies at
/// the next connect" instead of lying in either direction.
fn link_state_to_dto(state: LinkState, view: &HolderView) -> LeanzeroLinkStateResponse {
    let remote_execution_allowed_live = match &state.auth {
        AuthState::Connected { .. } => Some(view.allow_remote_execution),
        _ => None,
    };
    LeanzeroLinkStateResponse {
        auth: auth_state_to_dto(state.auth),
        mesh: state.mesh.map(mesh_status_to_dto),
        node_count: state.node_count,
        // A connect this layer refused (missing mesh binaries) is the latest failure the
        // user caused and the manager never saw; it leads until a connect reaches the
        // manager again.
        last_error: view.connect_refusal.clone().or(state.last_error),
        remote_execution_allowed: remote_execution_allowed(),
        remote_execution_allowed_live,
        mesh_binaries: view.mesh_binaries.to_dto(),
        // The two injection seams, as booted: `goosed agent` sets both; `goose serve` (the
        // shipped desktop) sets neither today, so its `/execute` and `/mlx/*` answer 501.
        // Shown so the panel can say "not wired" instead of discovering a 501 on use.
        remote_execution_wired: current_executor().is_some(),
        mlx_control_wired: current_mlx_control().is_some(),
    }
}

fn capabilities_to_dto(caps: worker_client::Capabilities) -> LeanzeroLinkCapabilitiesDto {
    LeanzeroLinkCapabilitiesDto {
        mail: caps.mail,
        audience: caps.audience,
        mesh: caps.mesh,
    }
}

/// Carry a manager [`LinkError`] to the UI as an actionable message (the mlx mount
/// idiom): a bad code, an expired token, a mail-not-configured worker, or a mesh spawn
/// failure must reach the panel as itself, not as a flat "Internal error".
fn link_err(error: LinkError) -> agent_client_protocol::Error {
    agent_client_protocol::Error::invalid_params().data(error.to_string())
}

/// Map a [`remote_execute`](LinkManager::remote_execute) failure the same way the mlx
/// mount handlers do: a target-STATE / target-SELECTION problem the user can act on (a
/// busy peer, remote-exec disabled on the peer, an unwired executor, an unknown target
/// id, a not-connected manager, a rejected prompt) rides through as `invalid_params` so
/// the panel/companion shows its status text verbatim ("node is busy"). Everything else —
/// a transport failure reaching the peer, or the peer's own internal error — is a flat
/// `internal_error`; its text is logged by the dispatcher, not offered as a user knob.
fn remote_execute_err(error: LinkError) -> agent_client_protocol::Error {
    match error {
        LinkError::Execute(ExecuteError::Busy)
        | LinkError::Execute(ExecuteError::Disabled)
        | LinkError::Execute(ExecuteError::BadRequest(_))
        | LinkError::ExecutorUnavailable
        | LinkError::NotConnected
        | LinkError::UnknownPeer(_) => {
            agent_client_protocol::Error::invalid_params().data(error.to_string())
        }
        _ => agent_client_protocol::Error::internal_error().data(error.to_string()),
    }
}

/// The connect-first error `remoteExecute` returns when the manager is not `Connected`.
/// A distinct, short message (not the `LinkError::NotConnected` Display) so the panel can
/// route the user straight to the Link tab's Connect action.
fn not_connected_to_mesh_err() -> agent_client_protocol::Error {
    agent_client_protocol::Error::invalid_params().data("not connected to the mesh")
}

/// Map a [`LinkManager::mlx_proxy`](LinkManager::mlx_proxy) failure to an ACP error that
/// preserves the local mlxEngine error CLASS, so a remote op fails exactly as its local
/// twin would. A peer's `invalid_params`-class failure (a mount memory-gate BLOCK, a
/// malformed repo id → HTTP `400` → [`MlxControlError::BadRequest`]) and the target-
/// selection errors (unknown peer, not connected, mlx control unwired) ride through as
/// `invalid_params` with their text verbatim — the panel shows "…" as itself. A peer's own
/// internal failure (disk read, HF fetch → `500` → [`MlxControlError::Failed`]) and any
/// transport/proxy failure ([`LinkError::MlxProxy`]) are `internal_error` (verbatim text),
/// never swallowed or faked.
/// The pure local-vs-remote decision for a mlxEngine `nodeId`: `None` (absent) and a
/// `nodeId` equal to `self_node_id` both run locally (`None`); a different id names a peer
/// (`Some`). Free and side-effect-free so the "self behaves identically to absent" contract
/// is pinned by a unit test without an agent.
fn mlx_remote_target(self_node_id: &str, requested: Option<&str>) -> Option<String> {
    match requested {
        None => None,
        Some(id) if id == self_node_id => None,
        Some(id) => Some(id.to_string()),
    }
}

fn mlx_proxy_err(error: LinkError) -> agent_client_protocol::Error {
    match error {
        LinkError::MlxControl(MlxControlError::BadRequest(_))
        | LinkError::MlxControlUnavailable
        | LinkError::NotConnected
        | LinkError::UnknownPeer(_) => {
            agent_client_protocol::Error::invalid_params().data(error.to_string())
        }
        _ => agent_client_protocol::Error::internal_error().data(error.to_string()),
    }
}

// ---------------------------------------------------------------------------
// ACP handlers.
// ---------------------------------------------------------------------------

impl GooseAcpAgent {
    fn link_source(&self) -> Arc<GoosedSwarmStateSource> {
        Arc::new(GoosedSwarmStateSource::new(
            self.agent_manager.clone(),
            self.session_manager.clone(),
        ))
    }

    fn build_link_manager(
        &self,
        key: &LinkBuildKey,
        binaries: &MeshBinaries,
    ) -> Result<Arc<LinkManager>, agent_client_protocol::Error> {
        let config = build_link_config(key, binaries)?;
        let source = self.link_source();
        let mut manager = LinkManager::new(config, source)
            .internal_err_ctx("constructing the LeanZero Link manager")?;
        // Attach the process-wide remote executor if goose-server injected one at boot; a
        // node built without it serves `/v1/swarm/execute` as `501` (execution not wired).
        if let Some(executor) = current_executor() {
            manager = manager.with_executor(executor);
        }
        // Attach the process-wide MLX control if the boot path injected one; a node built
        // without it serves `/v1/swarm/mlx/*` as `501` (mlx control not wired).
        if let Some(mlx_control) = current_mlx_control() {
            manager = manager.with_mlx_control(mlx_control);
        }
        Ok(Arc::new(manager))
    }

    /// The process-wide [`LinkManager`], built on first use in whatever auth state the
    /// identity file implies (it does NOT auto-connect). Rebuilt when its
    /// [`LinkBuildKey`] changes — the identity email on disk, the remote-execution
    /// setting — so the manager always carries what the user configured; a manager that
    /// is Connecting/Connected is never replaced (that would drop the live mesh), the new
    /// key applies at its next connect and the status DTO shows both values meanwhile.
    async fn link_manager(&self) -> Result<Arc<LinkManager>, agent_client_protocol::Error> {
        let key = current_build_key();

        let holder = match LINK.get() {
            Some(holder) => holder,
            None => {
                let binaries = MeshBinaries::discover();
                let manager = self.build_link_manager(&key, &binaries)?;
                let holder = StdMutex::new(LinkHolder {
                    key: key.clone(),
                    manager,
                    mesh_binaries: binaries,
                    connect_refusal: None,
                    stale_logged: false,
                });
                // Losing the init race is fine: the winner's holder is resolved below.
                let _ = LINK.set(holder);
                LINK.get().expect("LINK was set by this or a racing call")
            }
        };

        let current = {
            let guard = holder.lock().unwrap();
            if guard.key == key {
                return Ok(guard.manager.clone());
            }
            guard.manager.clone()
        };

        if matches!(
            current.status().await.auth,
            AuthState::Connecting { .. } | AuthState::Connected { .. }
        ) {
            let mut guard = holder.lock().unwrap();
            if !guard.stale_logged {
                info!(
                    ?key,
                    "leanzeroLink: settings changed under a live mesh; they apply at the next connect"
                );
                guard.stale_logged = true;
            }
            return Ok(guard.manager.clone());
        }

        let binaries = MeshBinaries::discover();
        let rebuilt = self.build_link_manager(&key, &binaries)?;
        let mut guard = holder.lock().unwrap();
        if guard.key != key {
            guard.key = key;
            guard.manager = rebuilt;
            guard.mesh_binaries = binaries;
            guard.connect_refusal = None;
            guard.stale_logged = false;
        }
        Ok(guard.manager.clone())
    }

    /// Re-run binary discovery right before a connect, so the refusal and the status DTO
    /// reflect the filesystem NOW. Discovery otherwise runs only when the holder is built
    /// or rebuilt (a key change), so a `tailscaled` installed or chmod-ed AFTER launch
    /// kept its stale `missing` verdict — and every retry of Connect re-read it — until a
    /// key change or an app restart. A few stat calls per click. When the paths differ
    /// from what the manager was built with, the manager is rebuilt so its mesh config
    /// carries the found paths instead of the empty template — never while
    /// Connecting/Connected, the same rule the key-change rebuild follows (a live mesh
    /// keeps its binaries; the fresh verdict applies at its next connect).
    async fn refresh_mesh_binaries(
        &self,
        current: Arc<LinkManager>,
    ) -> Result<Arc<LinkManager>, agent_client_protocol::Error> {
        let holder = LINK
            .get()
            .expect("refresh_mesh_binaries runs after link_manager() built the holder");
        let fresh = MeshBinaries::discover();
        let key = {
            let guard = holder.lock().unwrap();
            if !mesh_binaries_changed(&guard.mesh_binaries, &fresh) {
                return Ok(current);
            }
            guard.key.clone()
        };
        if matches!(
            current.status().await.auth,
            AuthState::Connecting { .. } | AuthState::Connected { .. }
        ) {
            info!("leanzeroLink: mesh binaries changed under a live mesh; they apply at the next connect");
            return Ok(current);
        }
        let rebuilt = self.build_link_manager(&key, &fresh)?;
        let mut guard = holder.lock().unwrap();
        info!(
            binaries = ?fresh,
            "leanzeroLink: mesh binaries changed since the manager was built; rebuilt for this connect"
        );
        guard.manager = rebuilt.clone();
        guard.mesh_binaries = fresh;
        Ok(rebuilt)
    }

    /// The status DTO for a manager obtained from [`Self::link_manager`].
    async fn status_response(&self, manager: &LinkManager) -> LeanzeroLinkStateResponse {
        link_state_to_dto(manager.status().await, &holder_view())
    }

    pub(super) async fn on_leanzero_link_health(
        &self,
        _req: LeanzeroLinkHealthRequest,
    ) -> Result<LeanzeroLinkHealthResponse, agent_client_protocol::Error> {
        let manager = self.link_manager().await?;
        let health = manager.health().await.internal_err()?;
        Ok(LeanzeroLinkHealthResponse {
            ok: health.ok,
            version: health.version,
            capabilities: capabilities_to_dto(health.capabilities),
        })
    }

    pub(super) async fn on_leanzero_link_request_code(
        &self,
        req: LeanzeroLinkRequestCodeRequest,
    ) -> Result<LeanzeroLinkRequestCodeResponse, agent_client_protocol::Error> {
        let manager = self.link_manager().await?;
        let result = manager.request_code(&req.email).await.map_err(link_err)?;
        Ok(LeanzeroLinkRequestCodeResponse {
            email: result.email,
            expires_in_seconds: result.expires_in_seconds,
        })
    }

    pub(super) async fn on_leanzero_link_verify(
        &self,
        req: LeanzeroLinkVerifyRequest,
    ) -> Result<LeanzeroLinkVerifyResponse, agent_client_protocol::Error> {
        let manager = self.link_manager().await?;
        let result = manager
            .verify(&req.email, &req.code)
            .await
            .map_err(link_err)?;
        let state = auth_state_tag(&manager.status().await.auth);
        Ok(LeanzeroLinkVerifyResponse {
            state,
            email: result.email,
            audience_sync: result.audience_sync,
        })
    }

    pub(super) async fn on_leanzero_link_connect(
        &self,
        _req: LeanzeroLinkConnectRequest,
    ) -> Result<LeanzeroLinkStateResponse, agent_client_protocol::Error> {
        let manager = self.link_manager().await?;
        let manager = self.refresh_mesh_binaries(manager).await?;
        // Missing mesh binaries refuse the connect HERE, with discovery's full
        // searched-list text, before the manager would try to spawn an empty path. The
        // refusal is recorded so `status.lastError` carries it until a connect gets
        // through.
        if let Err(refusal) = holder_view().mesh_binaries.paths() {
            error!(%refusal, "leanzeroLink: connect refused");
            record_connect_refusal(Some(refusal.clone()));
            return Err(agent_client_protocol::Error::invalid_params().data(refusal));
        }
        record_connect_refusal(None);
        manager.connect().await.map_err(link_err)?;
        Ok(self.status_response(&manager).await)
    }

    pub(super) async fn on_leanzero_link_status(
        &self,
        _req: LeanzeroLinkStatusRequest,
    ) -> Result<LeanzeroLinkStateResponse, agent_client_protocol::Error> {
        let manager = self.link_manager().await?;
        Ok(self.status_response(&manager).await)
    }

    pub(super) async fn on_leanzero_link_logout(
        &self,
        req: LeanzeroLinkLogoutRequest,
    ) -> Result<LeanzeroLinkStateResponse, agent_client_protocol::Error> {
        let manager = self.link_manager().await?;
        manager.logout(req.wipe).await.map_err(link_err)?;
        Ok(self.status_response(&manager).await)
    }

    pub(super) async fn on_leanzero_link_nodes(
        &self,
        _req: LeanzeroLinkNodesRequest,
    ) -> Result<LeanzeroLinkNodesResponse, agent_client_protocol::Error> {
        let manager = self.link_manager().await?;
        // The bearer is the manager's own — set at connect from the worker-issued account
        // secret. `None` means no control service is up (not connected), whatever the
        // auth state says mid-transition: the self-only answer below is then the truth.
        if let Some(token) = manager.node_token().await {
            // A failed proxy is an ERROR to the caller, never `{ self, peers: [] }`: the
            // desktop keeps its last roster on a thrown call and would replace it with an
            // empty one on a fabricated Ok.
            return fetch_local_swarm_nodes(&token).await.map_err(|error| {
                error!(%error, "leanzeroLink: proxying the local control service /v1/swarm/nodes failed");
                agent_client_protocol::Error::internal_error().data(format!(
                    "proxying the local control service /v1/swarm/nodes failed: {error}"
                ))
            });
        }
        // Not connected: there are no peers to know about — self only is the truth here.
        let self_node = self.link_source().local_node().await;
        Ok(LeanzeroLinkNodesResponse {
            self_node: serde_json::to_value(self_node).internal_err()?,
            peers: Vec::new(),
        })
    }

    pub(super) async fn on_leanzero_link_remote_execute(
        &self,
        req: LeanzeroLinkRemoteExecuteRequest,
    ) -> Result<LeanzeroLinkRemoteExecuteResponse, agent_client_protocol::Error> {
        let manager = self.link_manager().await?;
        if !matches!(manager.status().await.auth, AuthState::Connected { .. }) {
            return Err(not_connected_to_mesh_err());
        }
        // A fresh session on the target — the caller mirrors the returned id over the
        // swarm stream; `session_id: None` is the wire contract for "create a new one".
        let accepted = manager
            .remote_execute(
                &req.target_node_id,
                ExecuteRequest {
                    prompt: req.prompt,
                    working_dir: req.working_dir,
                    session_id: None,
                },
            )
            .await
            .map_err(remote_execute_err)?;
        Ok(LeanzeroLinkRemoteExecuteResponse {
            session_id: accepted.session_id,
        })
    }

    /// Decide where a mlxEngine op runs, from its optional `nodeId`. `None` and a `nodeId`
    /// equal to THIS node's id both mean "run locally" (returns `None`) — so the local path
    /// stays byte-identical whether `nodeId` is absent or names self. A different id names a
    /// peer to forward to (returns `Some(id)`). Pure and connection-agnostic; the connect
    /// check happens in [`Self::mlx_engine_forward`].
    pub(super) fn mlx_engine_remote_target(&self, node_id: Option<&str>) -> Option<String> {
        mlx_remote_target(&stable_node_id(), node_id)
    }

    /// Forward one mlxEngine op to peer `node_id` over the mesh proxy, returning the peer's
    /// response DTO as opaque JSON. Requires the mesh be `Connected` (else the short
    /// `not connected to the mesh` message so the panel routes to Connect); a peer's own
    /// failure surfaces verbatim via [`mlx_proxy_err`].
    pub(super) async fn mlx_engine_forward(
        &self,
        node_id: &str,
        op: MlxOp,
        body: serde_json::Value,
    ) -> Result<serde_json::Value, agent_client_protocol::Error> {
        let manager = self.link_manager().await?;
        if !matches!(manager.status().await.auth, AuthState::Connected { .. }) {
            return Err(not_connected_to_mesh_err());
        }
        manager
            .mlx_proxy(node_id, op, body)
            .await
            .map_err(mlx_proxy_err)
    }

    /// Serialize a mlxEngine request, forward it to `node_id`, and decode the peer's
    /// response DTO — the one-liner every mlxEngine handler's remote branch calls. `Req`
    /// and `Resp` are the op's ACP request/response types; the peer runs the identical
    /// operation against its local engine and returns the identical shape.
    pub(super) async fn mlx_engine_relay<Req, Resp>(
        &self,
        node_id: &str,
        op: MlxOp,
        req: &Req,
    ) -> Result<Resp, agent_client_protocol::Error>
    where
        Req: Serialize,
        Resp: DeserializeOwned,
    {
        let body = serde_json::to_value(req)
            .internal_err_ctx("serializing a mlxEngine request for the mesh proxy")?;
        let value = self.mlx_engine_forward(node_id, op, body).await?;
        serde_json::from_value(value)
            .internal_err_ctx("decoding a peer's mlxEngine response from the mesh proxy")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// R-H2: this layer mints NO token from the email any more — the control template is
    /// empty and the manager fills it from the worker-issued secret at connect.
    #[test]
    fn the_control_template_carries_no_email_derived_token() {
        let key = LinkBuildKey {
            email: Some("mihai@wolfaenpak.com".to_string()),
            allow_remote_execution: true,
        };
        let binaries = MeshBinaries {
            tailscaled: Ok(PathBuf::from("/app/bin/tailscaled")),
            tailscale: Ok(PathBuf::from("/app/bin/tailscale")),
        };
        let config = build_link_config(&key, &binaries).unwrap();
        assert!(
            config.control.node_token.is_empty(),
            "the template token must be empty, got {:?}",
            config.control.node_token
        );
        assert!(config.control.allow_remote_execution);
    }

    #[test]
    fn sanitize_hostname_reduces_to_the_tailnet_alphabet() {
        assert_eq!(
            sanitize_hostname("Mihai's MacBook.local"),
            "mihai-s-macbook-local"
        );
        assert_eq!(sanitize_hostname("  --Weird__Name-- "), "weird-name");
        assert_eq!(sanitize_hostname("***"), "leanzero-node");
    }

    fn summary(id: &str, live: bool, updated: i64) -> SessionSummary {
        SessionSummary {
            session_id: id.to_string(),
            origin_node_id: "node-a".to_string(),
            working_dir: "/tmp/w".to_string(),
            name: format!("session {id}"),
            updated_at: chrono::TimeZone::timestamp_opt(&Utc, updated, 0).unwrap(),
            message_count: 3,
            live,
        }
    }

    /// A readable-store snapshot whose busy set is exactly the `live` summaries.
    fn snapshot_of(sessions: Vec<SessionSummary>) -> LocalSnapshot {
        let busy = sessions
            .iter()
            .filter(|s| s.live)
            .map(|s| s.session_id.clone())
            .collect();
        LocalSnapshot {
            sessions: Ok(sessions),
            busy,
        }
    }

    #[test]
    fn derive_node_reports_busy_idle_and_active_count() {
        let now = Utc::now();

        let idle = derive_node("node-a", &snapshot_of(vec![summary("s1", false, 100)]), now);
        assert_eq!(idle.status, NodeStatus::Idle);
        assert_eq!(idle.sessions_active, 0);
        assert!(idle.mesh_ip.is_none());
        assert_eq!(idle.node_id, "node-a");

        let busy = derive_node(
            "node-a",
            &snapshot_of(vec![summary("old", true, 100), summary("new", true, 200)]),
            now,
        );
        assert_eq!(
            busy.status,
            NodeStatus::Busy {
                session_id: "new".to_string()
            },
            "busy carries the most recently updated live session"
        );
        assert_eq!(busy.sessions_active, 2);
    }

    /// R-M2: the busy set comes from the token maps, so an unreadable store never turns
    /// a running node Idle — the transient-failure window in which a Busy node used to
    /// accept a second job.
    #[test]
    fn derive_node_stays_busy_when_the_session_index_is_unreadable() {
        let now = Utc::now();
        let unreadable = LocalSnapshot {
            sessions: Err("session index unreadable: database is locked".to_string()),
            busy: HashSet::from(["s-running".to_string()]),
        };
        let node = derive_node("node-a", &unreadable, now);
        assert_eq!(
            node.status,
            NodeStatus::Busy {
                session_id: "s-running".to_string()
            }
        );
        assert_eq!(node.sessions_active, 1);

        let quiet = LocalSnapshot {
            sessions: Err("session index unreadable: database is locked".to_string()),
            busy: HashSet::new(),
        };
        assert_eq!(derive_node("node-a", &quiet, now).status, NodeStatus::Idle);
    }

    /// A token the index does not list (a session the store has not caught up on) still
    /// makes the node Busy — the token is the fact, the index only decorates it.
    #[test]
    fn derive_node_reports_busy_for_a_token_the_index_does_not_list() {
        let snapshot = LocalSnapshot {
            sessions: Ok(vec![summary("s1", false, 100)]),
            busy: HashSet::from(["s-unlisted".to_string()]),
        };
        let node = derive_node("node-a", &snapshot, Utc::now());
        assert_eq!(
            node.status,
            NodeStatus::Busy {
                session_id: "s-unlisted".to_string()
            }
        );
        assert_eq!(node.sessions_active, 1);
    }

    #[test]
    fn auth_state_tag_and_dto_use_camelcase() {
        let connected = AuthState::Connected {
            email: "a@b.com".to_string(),
            mesh_ip: "100.64.0.1".to_string(),
        };
        assert_eq!(auth_state_tag(&connected), "connected");

        let dto = auth_state_to_dto(connected);
        let value = serde_json::to_value(&dto).unwrap();
        assert_eq!(value["state"], "connected");
        assert_eq!(value["meshIp"], "100.64.0.1");
        assert_eq!(value["email"], "a@b.com");
    }

    #[test]
    fn state_response_serializes_camelcase() {
        let state = LeanzeroLinkStateResponse {
            auth: LeanzeroLinkAuthStateDto::LoggedIn {
                email: "a@b.com".to_string(),
            },
            mesh: None,
            node_count: 2,
            last_error: Some("boom".to_string()),
            remote_execution_allowed: true,
            remote_execution_allowed_live: Some(false),
            mesh_binaries: MeshBinaries {
                tailscaled: Ok(PathBuf::from("/app/bin/tailscaled")),
                tailscale: Err("could not find 'tailscale': …".to_string()),
            }
            .to_dto(),
            remote_execution_wired: false,
            mlx_control_wired: true,
        };
        let value = serde_json::to_value(&state).unwrap();
        assert_eq!(value["auth"]["state"], "loggedIn");
        assert_eq!(value["nodeCount"], 2);
        assert_eq!(value["lastError"], "boom");
        assert_eq!(value["remoteExecutionAllowed"], true);
        assert_eq!(value["remoteExecutionAllowedLive"], false);
        assert_eq!(value["remoteExecutionWired"], false);
        assert_eq!(value["mlxControlWired"], true);
        assert_eq!(value["meshBinaries"]["tailscaled"]["status"], "found");
        assert_eq!(
            value["meshBinaries"]["tailscaled"]["path"],
            "/app/bin/tailscaled"
        );
        assert_eq!(value["meshBinaries"]["tailscale"]["status"], "missing");
        assert_eq!(
            value["meshBinaries"]["tailscale"]["error"],
            "could not find 'tailscale': …"
        );
        let back: LeanzeroLinkStateResponse = serde_json::from_value(value).unwrap();
        assert_eq!(back.node_count, 2);
        assert_eq!(back.remote_execution_allowed_live, Some(false));
    }

    /// The `goose serve` boot path (`crates/goose-cli/src/cli.rs` `handle_serve_command`)
    /// injects goose's own `GoosedMlxControl` through `set_mlx_control`; the status DTO
    /// reads the same process-wide slot, so the shipped desktop's panel shows
    /// `mlxControlWired: true` and its `/v1/swarm/mlx/*` routes no longer answer `501`.
    #[test]
    fn mlx_control_injected_at_serve_boot_reads_wired_on_the_status_dto() {
        set_mlx_control(GoosedMlxControl::new());
        let view = HolderView {
            allow_remote_execution: false,
            mesh_binaries: MeshBinaries {
                tailscaled: Ok(PathBuf::from("/app/bin/tailscaled")),
                tailscale: Ok(PathBuf::from("/app/bin/tailscale")),
            },
            connect_refusal: None,
        };
        let dto = link_state_to_dto(
            LinkState {
                auth: AuthState::LoggedOut,
                mesh: None,
                node_count: 0,
                mesh_poll_failures: 0,
                last_error: None,
            },
            &view,
        );
        assert!(
            dto.mlx_control_wired,
            "the DTO reads the injected mlx control"
        );
    }

    /// A binary that appears (installed, chmod-ed) after the manager was built is a
    /// changed verdict: Connect re-discovers and rebuilds instead of re-reading the stale
    /// `missing`. An unchanged verdict — same paths, or the same refusal — is not.
    #[test]
    fn mesh_binaries_changed_tracks_the_paths_and_the_refusal_text() {
        let missing = MeshBinaries {
            tailscaled: Err("could not find 'tailscaled': searched …".to_string()),
            tailscale: Ok(PathBuf::from("/app/bin/tailscale")),
        };
        let found = MeshBinaries {
            tailscaled: Ok(PathBuf::from("/app/bin/tailscaled")),
            tailscale: Ok(PathBuf::from("/app/bin/tailscale")),
        };
        assert!(mesh_binaries_changed(&missing, &found));
        assert!(mesh_binaries_changed(&found, &missing));
        assert!(!mesh_binaries_changed(&found, &found.clone()));
        assert!(!mesh_binaries_changed(&missing, &missing.clone()));
        let elsewhere = MeshBinaries {
            tailscaled: Ok(PathBuf::from("/opt/other/tailscaled")),
            tailscale: Ok(PathBuf::from("/app/bin/tailscale")),
        };
        assert!(
            mesh_binaries_changed(&found, &elsewhere),
            "a binary that moved is a changed verdict too"
        );
    }

    /// R-H4: a failed discovery is carried as its own text — every missing binary named
    /// with everywhere discovery looked — never replaced by a guessed install path.
    #[test]
    fn mesh_binaries_refuse_with_every_missing_binary_named() {
        let both = MeshBinaries {
            tailscaled: Ok(PathBuf::from("/app/bin/tailscaled")),
            tailscale: Ok(PathBuf::from("/app/bin/tailscale")),
        };
        assert_eq!(
            both.paths().unwrap(),
            (
                PathBuf::from("/app/bin/tailscaled"),
                PathBuf::from("/app/bin/tailscale")
            )
        );

        let daemon_missing = discovery::discover(
            "tailscaled",
            discovery::TAILSCALED_ENV,
            None,
            &[PathBuf::from("/nonexistent/path-dir")],
            &["/nonexistent/known/tailscaled"],
        )
        .expect_err("nothing exists there")
        .to_string();
        let one = MeshBinaries {
            tailscaled: Err(daemon_missing.clone()),
            tailscale: Ok(PathBuf::from("/app/bin/tailscale")),
        };
        let refusal = one.paths().expect_err("a missing daemon refuses");
        assert!(refusal.starts_with("cannot start the mesh — "), "{refusal}");
        assert!(refusal.contains(&daemon_missing), "{refusal}");
        assert!(
            refusal.contains("/nonexistent/path-dir") && refusal.contains("/nonexistent/known"),
            "the refusal must carry the full searched list: {refusal}"
        );
        assert!(
            !refusal.contains("/opt/homebrew"),
            "no guessed install path may appear: {refusal}"
        );

        let none = MeshBinaries {
            tailscaled: Err("no daemon".to_string()),
            tailscale: Err("no cli".to_string()),
        };
        let refusal = none.paths().unwrap_err();
        assert!(
            refusal.contains("no daemon") && refusal.contains("no cli"),
            "{refusal}"
        );
    }

    /// With discovery failed the manager config still builds (the auth flows need no
    /// daemon) and its mesh template carries EMPTY paths — not a guessed binary.
    #[test]
    fn build_link_config_without_mesh_binaries_carries_empty_paths_not_a_guess() {
        let key = LinkBuildKey {
            email: Some("a@b.com".to_string()),
            allow_remote_execution: false,
        };
        let binaries = MeshBinaries {
            tailscaled: Err("no daemon".to_string()),
            tailscale: Err("no cli".to_string()),
        };
        let config = build_link_config(&key, &binaries).expect("constructible without a mesh");
        assert_eq!(config.mesh.tailscaled_path, PathBuf::new());
        assert_eq!(config.mesh.tailscale_cli_path, PathBuf::new());
        assert!(!config.control.allow_remote_execution);

        let found = MeshBinaries {
            tailscaled: Ok(PathBuf::from("/app/bin/tailscaled")),
            tailscale: Ok(PathBuf::from("/app/bin/tailscale")),
        };
        let config = build_link_config(&key, &found).unwrap();
        assert_eq!(
            config.mesh.tailscaled_path,
            PathBuf::from("/app/bin/tailscaled")
        );
        assert_eq!(
            config.mesh.tailscale_cli_path,
            PathBuf::from("/app/bin/tailscale")
        );
    }

    /// R-H3: the switch defaults OFF when unset and fails CLOSED on an unreadable value;
    /// only an explicit `true` opts the node in.
    #[test]
    fn remote_execution_switch_defaults_off_and_fails_closed() {
        assert!(!allow_from_config(Err(ConfigError::NotFound(
            ALLOW_REMOTE_EXECUTION_KEY.to_string()
        ))));
        assert!(allow_from_config(Ok(true)));
        assert!(!allow_from_config(Ok(false)));
        let unparseable: Result<bool, ConfigError> =
            serde_json::from_value::<bool>(serde_json::json!("yes-ish")).map_err(Into::into);
        assert!(
            !allow_from_config(unparseable),
            "an unparseable value must not open the node"
        );
    }

    /// FH#8: the loopback proxy's body is the contract or an error — a hole in it is never
    /// read as "no peers".
    #[test]
    fn swarm_nodes_body_missing_a_key_is_an_error_not_an_empty_roster() {
        let full = parse_swarm_nodes_body(serde_json::json!({
            "self": {"node_id": "a"},
            "peers": [{"node_id": "b"}]
        }))
        .unwrap();
        assert_eq!(full.self_node["node_id"], "a");
        assert_eq!(full.peers.len(), 1);

        let no_peers = parse_swarm_nodes_body(serde_json::json!({ "self": {"node_id": "a"} }))
            .expect_err("a body without 'peers' is not a roster");
        assert!(no_peers.contains("'peers'"), "{no_peers}");

        let no_self = parse_swarm_nodes_body(serde_json::json!({ "peers": [] }))
            .expect_err("a body without 'self' is not a roster");
        assert!(no_self.contains("'self'"), "{no_self}");

        let peers_not_array =
            parse_swarm_nodes_body(serde_json::json!({ "self": {}, "peers": "nope" }))
                .expect_err("a non-array 'peers' is not a roster");
        assert!(peers_not_array.contains("'peers'"), "{peers_not_array}");
    }

    #[test]
    fn nodes_response_uses_self_key() {
        let response = LeanzeroLinkNodesResponse {
            self_node: serde_json::json!({"node_id": "node-a"}),
            peers: vec![serde_json::json!({"node_id": "node-b"})],
        };
        let value = serde_json::to_value(&response).unwrap();
        assert_eq!(value["self"]["node_id"], "node-a");
        assert_eq!(value["peers"][0]["node_id"], "node-b");
        assert!(value.get("self_node").is_none());
    }

    #[test]
    fn remote_execute_request_uses_camelcase() {
        let req = LeanzeroLinkRemoteExecuteRequest {
            target_node_id: "studio-ab12cd".to_string(),
            prompt: "build the thing".to_string(),
            working_dir: Some("/tmp/proj".to_string()),
        };
        let value = serde_json::to_value(&req).unwrap();
        assert_eq!(value["targetNodeId"], "studio-ab12cd");
        assert_eq!(value["prompt"], "build the thing");
        assert_eq!(value["workingDir"], "/tmp/proj");
        assert!(value.get("target_node_id").is_none());
        assert!(value.get("working_dir").is_none());

        // Round-trips, and an absent workingDir stays absent (skip_serializing_if).
        let back: LeanzeroLinkRemoteExecuteRequest = serde_json::from_value(value).unwrap();
        assert_eq!(back.target_node_id, "studio-ab12cd");
        assert_eq!(back.working_dir.as_deref(), Some("/tmp/proj"));

        let minimal: LeanzeroLinkRemoteExecuteRequest =
            serde_json::from_value(serde_json::json!({ "targetNodeId": "n", "prompt": "p" }))
                .unwrap();
        assert!(minimal.working_dir.is_none());
        let minimal_value = serde_json::to_value(&minimal).unwrap();
        assert!(
            minimal_value.get("workingDir").is_none(),
            "an absent workingDir must not serialize as null"
        );
    }

    #[test]
    fn remote_execute_response_uses_camelcase() {
        let resp = LeanzeroLinkRemoteExecuteResponse {
            session_id: "sess-9".to_string(),
        };
        let value = serde_json::to_value(&resp).unwrap();
        assert_eq!(value["sessionId"], "sess-9");
        assert!(value.get("session_id").is_none());
        let back: LeanzeroLinkRemoteExecuteResponse = serde_json::from_value(value).unwrap();
        assert_eq!(back.session_id, "sess-9");
    }

    fn invalid_params_code() -> agent_client_protocol::ErrorCode {
        agent_client_protocol::Error::invalid_params().code
    }

    fn internal_error_code() -> agent_client_protocol::ErrorCode {
        agent_client_protocol::Error::internal_error().code
    }

    fn err_text(error: &agent_client_protocol::Error) -> String {
        error
            .data
            .as_ref()
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string()
    }

    #[test]
    fn remote_execute_err_surfaces_device_state_verbatim_as_invalid_params() {
        // A busy peer (409 → LinkError::Execute(Busy)) must reach the UI as its own text.
        let busy = remote_execute_err(LinkError::Execute(ExecuteError::Busy));
        assert_eq!(busy.code, invalid_params_code());
        assert_eq!(err_text(&busy), "node is busy");

        let disabled = remote_execute_err(LinkError::Execute(ExecuteError::Disabled));
        assert_eq!(disabled.code, invalid_params_code());
        assert_eq!(
            err_text(&disabled),
            "remote execution disabled on this node"
        );

        let bad = remote_execute_err(LinkError::Execute(ExecuteError::BadRequest(
            "empty prompt".to_string(),
        )));
        assert_eq!(bad.code, invalid_params_code());
        assert_eq!(err_text(&bad), "bad request: empty prompt");

        // The self short-circuit with no injected executor.
        let unwired = remote_execute_err(LinkError::ExecutorUnavailable);
        assert_eq!(unwired.code, invalid_params_code());
        assert_eq!(
            err_text(&unwired),
            "remote execution is not wired on this node"
        );

        // A target id that is not a known mesh peer — a selection error, user-actionable.
        let unknown = remote_execute_err(LinkError::UnknownPeer("ghost".to_string()));
        assert_eq!(unknown.code, invalid_params_code());
        assert_eq!(
            err_text(&unknown),
            "no known mesh peer with node id 'ghost'"
        );
    }

    #[test]
    fn remote_execute_err_maps_transport_and_internal_to_internal_error() {
        // A transport failure reaching the peer is not a user knob.
        let transport =
            remote_execute_err(LinkError::RemoteExecute("connection refused".to_string()));
        assert_eq!(transport.code, internal_error_code());
        assert_eq!(
            err_text(&transport),
            "remote execute request to a peer failed: connection refused"
        );

        // The peer's own internal error rides through as internal too.
        let internal = remote_execute_err(LinkError::Execute(ExecuteError::Internal(
            "boom".to_string(),
        )));
        assert_eq!(internal.code, internal_error_code());
        assert_eq!(err_text(&internal), "internal error: boom");
    }

    #[test]
    fn mlx_remote_target_treats_absent_and_self_identically_and_names_peers() {
        // Absent nodeId → local (None): the byte-identical local path.
        assert_eq!(mlx_remote_target("studio-ab12cd", None), None);
        // nodeId == self → local (None), so a handler with nodeId=self behaves exactly like
        // nodeId absent — the short-circuit the contract requires.
        assert_eq!(
            mlx_remote_target("studio-ab12cd", Some("studio-ab12cd")),
            None
        );
        // A different id → forward to that peer.
        assert_eq!(
            mlx_remote_target("studio-ab12cd", Some("laptop-99")),
            Some("laptop-99".to_string())
        );
    }

    #[test]
    fn mlx_proxy_err_preserves_the_local_error_class_and_text() {
        // A peer's invalid_params-class failure (mount gate BLOCK, malformed repo id) rides
        // through as invalid_params with its text verbatim.
        let bad = mlx_proxy_err(LinkError::MlxControl(MlxControlError::BadRequest(
            "memory gate BLOCK: model needs 40GB, 12GB free".to_string(),
        )));
        assert_eq!(bad.code, invalid_params_code());
        assert_eq!(
            err_text(&bad),
            "memory gate BLOCK: model needs 40GB, 12GB free"
        );

        // Target-selection errors are user-actionable → invalid_params.
        let unknown = mlx_proxy_err(LinkError::UnknownPeer("ghost".to_string()));
        assert_eq!(unknown.code, invalid_params_code());
        assert_eq!(
            err_text(&unknown),
            "no known mesh peer with node id 'ghost'"
        );

        let unwired = mlx_proxy_err(LinkError::MlxControlUnavailable);
        assert_eq!(unwired.code, invalid_params_code());
        assert_eq!(err_text(&unwired), "mlx control is not wired on this node");

        // A peer's own internal failure (disk read, HF fetch → 500) → internal_error, verbatim.
        let failed = mlx_proxy_err(LinkError::MlxControl(MlxControlError::Failed(
            "reading local models failed: permission denied".to_string(),
        )));
        assert_eq!(failed.code, internal_error_code());
        assert_eq!(
            err_text(&failed),
            "reading local models failed: permission denied"
        );

        // A transport/proxy failure reaching the peer → internal_error, verbatim.
        let transport = mlx_proxy_err(LinkError::MlxProxy("connection refused".to_string()));
        assert_eq!(transport.code, internal_error_code());
        assert_eq!(
            err_text(&transport),
            "mlx proxy request to a peer failed: connection refused"
        );
    }

    #[test]
    fn not_connected_manager_yields_the_connect_first_message() {
        // The handler's pre-check (manager not Connected) surfaces exactly this.
        let error = not_connected_to_mesh_err();
        assert_eq!(error.code, invalid_params_code());
        assert_eq!(err_text(&error), "not connected to the mesh");
    }

    async fn seeded_source() -> (tempfile::TempDir, GoosedSwarmStateSource) {
        use crate::agents::{AgentConfig, GoosePlatform};
        use crate::config::permission::PermissionManager;
        use crate::config::GooseMode;

        let temp = tempfile::TempDir::new().unwrap();
        let session_manager = Arc::new(SessionManager::new(temp.path().to_path_buf()));
        let agent_config = AgentConfig::new(
            session_manager.clone(),
            PermissionManager::instance(),
            None,
            GooseMode::default(),
            false,
            GoosePlatform::GooseDesktop,
        );
        let agent_manager = Arc::new(AgentManager::new(agent_config, Some(100)).await.unwrap());
        let source = GoosedSwarmStateSource::new(agent_manager, session_manager);
        (temp, source)
    }

    #[tokio::test]
    async fn local_sessions_maps_the_session_index() {
        let (_temp, source) = seeded_source().await;

        source
            .session_manager
            .create_session(
                PathBuf::from("/tmp/project"),
                "First".to_string(),
                SessionType::User,
                GooseMode::default(),
            )
            .await
            .unwrap();
        source
            .session_manager
            .create_session(
                PathBuf::from("/tmp/other"),
                "Second".to_string(),
                SessionType::User,
                GooseMode::default(),
            )
            .await
            .unwrap();

        let sessions = source
            .local_sessions()
            .await
            .expect("the seeded store is readable");
        assert_eq!(sessions.len(), 2);
        for summary in &sessions {
            assert_eq!(summary.origin_node_id, source.node_id);
            assert!(!summary.live, "no agent is running, so nothing is busy");
            assert_eq!(summary.message_count, 0);
        }
        let names: HashSet<&str> = sessions.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains("First") && names.contains("Second"));
        let dirs: HashSet<&str> = sessions.iter().map(|s| s.working_dir.as_str()).collect();
        assert!(dirs.contains("/tmp/project") && dirs.contains("/tmp/other"));
    }

    #[tokio::test]
    async fn a_registered_cancel_token_makes_the_session_live_and_the_node_busy() {
        let (_temp, source) = seeded_source().await;
        let session = source
            .session_manager
            .create_session(
                PathBuf::from("/tmp/project"),
                "First".to_string(),
                SessionType::User,
                GooseMode::default(),
            )
            .await
            .unwrap();

        source
            .agent_manager
            .try_register_cancel_token(&session.id, CancellationToken::new())
            .await
            .unwrap();

        let node = source.local_node().await;
        assert_eq!(
            node.status,
            NodeStatus::Busy {
                session_id: session.id.clone()
            },
            "a session holding a cancel token is the busy one"
        );
        assert_eq!(node.sessions_active, 1);
        let live: Vec<String> = source
            .local_sessions()
            .await
            .expect("the seeded store is readable")
            .into_iter()
            .filter(|s| s.live)
            .map(|s| s.session_id)
            .collect();
        assert_eq!(live, vec![session.id.clone()]);

        source
            .agent_manager
            .unregister_cancel_token(&session.id)
            .await;
        assert_eq!(source.local_node().await.status, NodeStatus::Idle);
    }

    /// A source whose session store cannot open: the `sessions/` directory the storage
    /// created is replaced by a plain file before the lazy pool's first connection, so
    /// every store read fails with the real sqlite open error. The agent manager (and its
    /// token map) is untouched.
    async fn source_with_unreadable_store() -> (tempfile::TempDir, GoosedSwarmStateSource) {
        use crate::agents::{AgentConfig, GoosePlatform};
        use crate::config::permission::PermissionManager;
        use crate::config::GooseMode;

        let temp = tempfile::TempDir::new().unwrap();
        let session_manager = Arc::new(SessionManager::new(temp.path().to_path_buf()));
        let sessions_dir = temp.path().join("sessions");
        std::fs::remove_dir_all(&sessions_dir).unwrap();
        std::fs::write(&sessions_dir, b"not a directory").unwrap();

        let agent_config = AgentConfig::new(
            session_manager.clone(),
            PermissionManager::instance(),
            None,
            GooseMode::default(),
            false,
            GoosePlatform::GooseDesktop,
        );
        let agent_manager = Arc::new(AgentManager::new(agent_config, Some(100)).await.unwrap());
        let source = GoosedSwarmStateSource::new(agent_manager, session_manager);
        (temp, source)
    }

    /// R-M2 end to end on a real broken store: `local_node` stays Busy from the token
    /// map, `local_sessions_checked` carries the error verbatim, and the poller publishes
    /// NOTHING (no NodeStateChanged, no upsert, no retain-wipe) for the unreadable tick.
    #[tokio::test]
    async fn an_unreadable_store_keeps_the_node_busy_and_the_poller_silent() {
        let (_temp, source) = source_with_unreadable_store().await;
        source
            .agent_manager
            .try_register_cancel_token("s-running", CancellationToken::new())
            .await
            .unwrap();

        let error = source
            .local_sessions_checked()
            .await
            .expect_err("a store that cannot open is an error, never an empty index");
        assert!(
            error.starts_with("session index unreadable: "),
            "got {error}"
        );

        let node = source.local_node().await;
        assert_eq!(
            node.status,
            NodeStatus::Busy {
                session_id: "s-running".to_string()
            }
        );
        assert_eq!(node.sessions_active, 1);

        let mut stream = source.local_deltas_with(None);
        let first = tokio::time::timeout(Duration::from_millis(400), stream.next()).await;
        assert!(
            first.is_err(),
            "the poller must publish nothing while the index is unreadable, got {:?}",
            first.ok().flatten()
        );
    }

    /// A scheduler that schedules nothing: `GooseAcpAgent::new` requires one and the
    /// ACP-door test below never touches it.
    struct NoScheduler;

    #[async_trait::async_trait]
    impl crate::scheduler_trait::SchedulerTrait for NoScheduler {
        async fn add_scheduled_job(
            &self,
            job: crate::scheduler::ScheduledJob,
            _copy_recipe: bool,
        ) -> Result<(), crate::scheduler::SchedulerError> {
            Err(crate::scheduler::SchedulerError::JobNotFound(job.id))
        }
        async fn schedule_recipe(
            &self,
            recipe_path: PathBuf,
            _cron_schedule: Option<String>,
        ) -> Result<(), crate::scheduler::SchedulerError> {
            Err(crate::scheduler::SchedulerError::JobNotFound(
                recipe_path.display().to_string(),
            ))
        }
        async fn list_scheduled_jobs(&self) -> Vec<crate::scheduler::ScheduledJob> {
            Vec::new()
        }
        async fn remove_scheduled_job(
            &self,
            id: &str,
            _remove_recipe: bool,
        ) -> Result<(), crate::scheduler::SchedulerError> {
            Err(crate::scheduler::SchedulerError::JobNotFound(
                id.to_string(),
            ))
        }
        async fn pause_schedule(&self, id: &str) -> Result<(), crate::scheduler::SchedulerError> {
            Err(crate::scheduler::SchedulerError::JobNotFound(
                id.to_string(),
            ))
        }
        async fn unpause_schedule(&self, id: &str) -> Result<(), crate::scheduler::SchedulerError> {
            Err(crate::scheduler::SchedulerError::JobNotFound(
                id.to_string(),
            ))
        }
        async fn run_now(&self, id: &str) -> Result<String, crate::scheduler::SchedulerError> {
            Err(crate::scheduler::SchedulerError::JobNotFound(
                id.to_string(),
            ))
        }
        async fn sessions(
            &self,
            sched_id: &str,
            _limit: usize,
        ) -> Result<Vec<(String, crate::session::Session)>, crate::scheduler::SchedulerError>
        {
            Err(crate::scheduler::SchedulerError::JobNotFound(
                sched_id.to_string(),
            ))
        }
        async fn update_schedule(
            &self,
            sched_id: &str,
            _new_cron: String,
        ) -> Result<(), crate::scheduler::SchedulerError> {
            Err(crate::scheduler::SchedulerError::JobNotFound(
                sched_id.to_string(),
            ))
        }
        async fn kill_running_job(
            &self,
            sched_id: &str,
        ) -> Result<(), crate::scheduler::SchedulerError> {
            Err(crate::scheduler::SchedulerError::JobNotFound(
                sched_id.to_string(),
            ))
        }
        async fn get_running_job_info(
            &self,
            sched_id: &str,
        ) -> Result<Option<(String, DateTime<Utc>)>, crate::scheduler::SchedulerError> {
            Err(crate::scheduler::SchedulerError::JobNotFound(
                sched_id.to_string(),
            ))
        }
    }

    async fn acp_agent() -> (tempfile::TempDir, GooseAcpAgent) {
        use crate::agents::GoosePlatform;
        let temp = tempfile::TempDir::new().unwrap();
        let provider_factory: AcpProviderFactory = Arc::new(|_name, _extensions, _dir| {
            Box::pin(async { Err(anyhow::anyhow!("no provider in this test")) })
        });
        let agent = GooseAcpAgent::new(GooseAcpAgentOptions {
            provider_factory,
            builtins: Vec::new(),
            data_dir: temp.path().to_path_buf(),
            config_dir: temp.path().to_path_buf(),
            disable_session_naming: true,
            goose_platform: GoosePlatform::GooseCli,
            additional_source_roots: Vec::new(),
            scheduler: Arc::new(NoScheduler),
        })
        .await
        .unwrap();
        (temp, agent)
    }

    /// The ACP `on_prompt` door: `start_active_run` registers the run's token with the
    /// AgentManager (the busy set the link source and the idle guard read) and
    /// `clear_active_run` releases it — the hole that left every desktop chat Idle.
    #[tokio::test]
    async fn the_acp_prompt_door_registers_and_releases_the_busy_token() {
        let (_temp, agent) = acp_agent().await;
        let session = agent
            .session_manager
            .create_session(
                PathBuf::from("/tmp/project"),
                "Chat".to_string(),
                SessionType::User,
                GooseMode::default(),
            )
            .await
            .unwrap();
        let source = agent.link_source();

        agent
            .start_active_run(&session.id, "run_1".to_string(), CancellationToken::new())
            .await
            .unwrap();
        assert!(agent.agent_manager.is_session_busy(&session.id).await);
        assert_eq!(
            source.local_node().await.status,
            NodeStatus::Busy {
                session_id: session.id.clone()
            },
            "an ACP prompt run is visible to the link source as Busy"
        );

        agent.clear_active_run(&session.id, "run_1").await;
        assert!(!agent.agent_manager.is_session_busy(&session.id).await);
        assert_eq!(source.local_node().await.status, NodeStatus::Idle);
    }

    /// A token held by the OTHER door (goose-server's reply route, a subagent run) refuses
    /// the ACP prompt instead of running a second reply on the session, and leaves no
    /// half-registered active run behind.
    #[tokio::test]
    async fn the_acp_prompt_door_refuses_a_session_busy_in_another_run() {
        let (_temp, agent) = acp_agent().await;
        agent
            .agent_manager
            .try_register_cancel_token("s-other-door", CancellationToken::new())
            .await
            .unwrap();

        let error = agent
            .start_active_run(
                "s-other-door",
                "run_2".to_string(),
                CancellationToken::new(),
            )
            .await
            .expect_err("a session busy in another run is refused");
        assert!(
            err_text(&error).contains("busy in another run"),
            "got {}",
            err_text(&error)
        );
        assert!(
            agent.active_prompt_runs.lock().await.is_empty(),
            "a refused run must not be recorded as active"
        );
        // The other door's token is untouched by the refusal.
        assert!(agent.agent_manager.is_session_busy("s-other-door").await);
    }

    #[tokio::test]
    async fn local_node_is_idle_with_no_running_agents() {
        let (_temp, source) = seeded_source().await;
        source
            .session_manager
            .create_session(
                PathBuf::from("/tmp/project"),
                "First".to_string(),
                SessionType::User,
                GooseMode::default(),
            )
            .await
            .unwrap();

        let node = source.local_node().await;
        assert_eq!(node.status, NodeStatus::Idle);
        assert_eq!(node.sessions_active, 0);
        assert_eq!(node.node_id, source.node_id);
        assert!(!node.hostname.is_empty());
    }

    /// A scripted [`DeltaSource`] that yields exactly the `DeltaInput`s it was handed,
    /// then ends — the goose-server tap's stand-in for classifying `MessageEvent`s.
    struct ScriptedDeltaSource(std::sync::Mutex<Option<Vec<DeltaInput>>>);

    impl ScriptedDeltaSource {
        fn new(inputs: Vec<DeltaInput>) -> Self {
            Self(std::sync::Mutex::new(Some(inputs)))
        }
    }

    impl DeltaSource for ScriptedDeltaSource {
        fn subscribe(&self) -> BoxStream<'static, DeltaInput> {
            let inputs = self.0.lock().unwrap().take().unwrap_or_default();
            Box::pin(futures::stream::iter(inputs))
        }
    }

    fn delta_input(id: &str, seq: u64, kind: SessionDeltaKind) -> DeltaInput {
        DeltaInput {
            session_id: id.to_string(),
            seq,
            kind,
            payload: serde_json::json!({ "session_id": id, "seq": seq }),
        }
    }

    #[tokio::test]
    async fn injected_delta_source_yields_session_deltas_of_each_kind() {
        let (_temp, source) = seeded_source().await;

        let scripted = ScriptedDeltaSource::new(vec![
            delta_input("s1", 1, SessionDeltaKind::Message),
            delta_input("s1", 2, SessionDeltaKind::ToolUpdate),
            delta_input("s1", 3, SessionDeltaKind::Finish),
            delta_input("s2", 1, SessionDeltaKind::Error),
        ]);

        let mut stream = source.local_deltas_with(Some(Arc::new(scripted)));

        // Collect the SessionDeltas out of the merged stream (the poller half may also
        // interleave NodeStateChanged/SessionUpserted; we assert the delta half is exact).
        let mut deltas = Vec::new();
        while deltas.len() < 4 {
            match stream.next().await.expect("stream ended before all deltas") {
                LinkEvent::SessionDelta {
                    session_id,
                    seq,
                    kind,
                    payload,
                } => deltas.push((session_id, seq, kind, payload)),
                LinkEvent::NodeStateChanged(_) | LinkEvent::SessionUpserted(_) => {}
            }
        }

        assert_eq!(deltas[0].0, "s1");
        assert_eq!(deltas[0].1, 1);
        assert_eq!(deltas[0].2, SessionDeltaKind::Message);
        assert_eq!(deltas[0].3["seq"], 1);
        assert_eq!(deltas[1].2, SessionDeltaKind::ToolUpdate);
        assert_eq!(deltas[2].2, SessionDeltaKind::Finish);
        assert_eq!(deltas[3].0, "s2");
        assert_eq!(deltas[3].2, SessionDeltaKind::Error);
    }

    #[tokio::test]
    async fn without_injection_only_node_and_session_events_flow() {
        let (_temp, source) = seeded_source().await;
        source
            .session_manager
            .create_session(
                PathBuf::from("/tmp/project"),
                "First".to_string(),
                SessionType::User,
                GooseMode::default(),
            )
            .await
            .unwrap();

        let mut stream = source.local_deltas_with(None);

        // The very first tick emits NodeStateChanged then a SessionUpserted; no
        // SessionDelta can appear without an injected source.
        for _ in 0..2 {
            match stream.next().await.expect("poller produced an event") {
                LinkEvent::NodeStateChanged(_) | LinkEvent::SessionUpserted(_) => {}
                LinkEvent::SessionDelta { .. } => {
                    panic!("no SessionDelta may flow without an injected DeltaSource")
                }
            }
        }
    }
}
