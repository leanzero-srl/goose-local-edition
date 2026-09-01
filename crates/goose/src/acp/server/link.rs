//! ACP surface for LeanZero Link: bridges the `_goose/unstable/leanzeroLink/*`
//! custom methods to a process-wide [`LinkManager`], and implements the
//! [`SwarmStateSource`] the control service reads from goosed's live session state.
//!
//! ## What lives here
//! - [`GoosedSwarmStateSource`] — the decoupling seam. It sources the node's own
//!   [`NodeState`], the local [`SessionSummary`] index, and a live delta stream from
//!   goosed's [`AgentManager`] + [`SessionManager`]. No mesh, no network.
//! - The lazily-constructed global [`LinkManager`] (an `OnceLock` holder, mirroring
//!   `goose_sidecar::engine::global_manager`), rebuilt when the on-disk identity email
//!   changes so the account-derived control `node_token` stays current.
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

use std::sync::{Mutex as StdMutex, OnceLock};
use std::time::Duration;

use chrono::{DateTime, Utc};
use futures::stream::BoxStream;
use futures::StreamExt;
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

/// How often the delta poller re-snapshots node/session state. Matches the fabric's own
/// peer poll cadence (`ControlConfig::poll_interval` default) so a busy/idle transition
/// surfaces to peers on the same order of latency whether via our stream or their poll.
const LINK_DELTA_POLL_INTERVAL: Duration = Duration::from_secs(3);

/// Compile-time app constant folded into the control `node_token`. The token is
/// `HMAC-SHA256(key = account_email_lowercased, message = this constant)`, hex-encoded.
/// It is shared per-account (every node of one account derives the same value) and is
/// defense-in-depth behind the already account-isolated tailnet — never the primary
/// auth boundary. The iOS companion must derive it identically.
const LINK_NODE_TOKEN_APP_SECRET: &str = "leanzero-link/v1/node-token";

/// Fallback binary paths used only when discovery fails, so the auth flows
/// (health/requestCode/verify/status/logout) still work with no Tailscale installed. A
/// `connect()` then fails loudly with a spawn error naming the path (the discovery
/// warning is already logged), rather than the whole manager refusing to construct.
const TAILSCALED_FALLBACK: &str = "/opt/homebrew/bin/tailscaled";
const TAILSCALE_CLI_FALLBACK: &str = "/opt/homebrew/bin/tailscale";

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
        let sessions =
            snapshot_sessions(&self.agent_manager, &self.session_manager, &self.node_id).await;
        derive_node(&self.node_id, &sessions, Utc::now())
    }

    async fn local_sessions(&self) -> Vec<SessionSummary> {
        snapshot_sessions(&self.agent_manager, &self.session_manager, &self.node_id).await
    }

    fn subscribe_local_deltas(&self) -> BoxStream<'static, LinkEvent> {
        self.local_deltas_with(current_delta_source())
    }
}

impl GoosedSwarmStateSource {
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
                let sessions = snapshot_sessions(&agent_manager, &session_manager, &node_id).await;

                let node = derive_node(&node_id, &sessions, Utc::now());
                let node_key = (node.status.clone(), node.sessions_active);
                if last_node_key.as_ref() != Some(&node_key) {
                    if tx.send(LinkEvent::NodeStateChanged(node)).await.is_err() {
                        return;
                    }
                    last_node_key = Some(node_key);
                }

                for summary in &sessions {
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

/// Snapshot the local session index, mapping each non-archived [`Session`] to a
/// [`SessionSummary`]. `live` is `is_session_busy` (an in-flight reply); a read failure
/// is logged LOUDLY and yields an empty index for this tick — never a faked list.
async fn snapshot_sessions(
    agent_manager: &AgentManager,
    session_manager: &SessionManager,
    node_id: &str,
) -> Vec<SessionSummary> {
    let sessions = match session_manager.list_sessions().await {
        Ok(sessions) => sessions,
        Err(error) => {
            error!(%error, "leanzeroLink: reading local sessions failed; empty index this tick");
            return Vec::new();
        }
    };

    // A session can only be busy (hold an in-flight cancel token) while its agent is
    // loaded, so the busy set is a subset of the active (loaded) ids — probe only those,
    // not every session on disk.
    let mut busy: HashSet<String> = HashSet::new();
    for id in agent_manager.list_active_session_ids().await {
        if agent_manager.is_session_busy(&id).await {
            busy.insert(id);
        }
    }

    sessions
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
        .collect()
}

/// The node's own [`NodeState`]. `mesh_ip` is left `None`: the source does not know the
/// mesh IP (that is the mesh layer's knowledge); the control service's `/nodes` handler
/// fills it for the direct response, and peers learn it from the tailnet + their `/nodes`
/// polls.
fn derive_node(node_id: &str, sessions: &[SessionSummary], now: DateTime<Utc>) -> NodeState {
    NodeState {
        node_id: node_id.to_string(),
        hostname: hostname_string(),
        mesh_ip: None,
        status: NodeStatus::from_sessions(sessions),
        sessions_active: sessions.iter().filter(|s| s.live).count() as u32,
        updated_at: now,
    }
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
/// (`~/.leanzero/node-id`). Absent until the first connect; read-only here.
fn read_persisted_node_suffix() -> Option<String> {
    let path = identity::default_identity_path().ok()?;
    let dir = path.parent()?;
    let content = std::fs::read_to_string(dir.join("node-id")).ok()?;
    let trimmed = content.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// `HMAC-SHA256(key = account_email_lowercased, message = app constant)`, hex-encoded.
/// A `None` email (logged out) folds an empty key — a non-empty placeholder never used,
/// since `connect()` requires a verified identity and the manager is rebuilt with the
/// real email before then.
fn node_token_from_email(email: Option<&str>) -> String {
    use hmac::{Hmac, KeyInit, Mac};
    use sha2::Sha256;
    let key = email.map(str::to_lowercase).unwrap_or_default();
    let mut mac =
        <Hmac<Sha256>>::new_from_slice(key.as_bytes()).expect("HMAC accepts any key length");
    mac.update(LINK_NODE_TOKEN_APP_SECRET.as_bytes());
    to_hex(&mac.finalize().into_bytes())
}

fn to_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
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

// ---------------------------------------------------------------------------
// The process-wide LinkManager (lazily built, rebuilt on identity-email change).
// ---------------------------------------------------------------------------

struct LinkHolder {
    /// The account email (lowercased) the current manager's `node_token` was derived
    /// from, so a login/identity change forces a rebuild.
    email_key: Option<String>,
    manager: Arc<LinkManager>,
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
    email: Option<&str>,
) -> Result<LinkManagerConfig, agent_client_protocol::Error> {
    let worker_base_url = resolve_worker_base_url();
    let identity_path = identity::default_identity_path()
        .internal_err_ctx("resolving the LeanZero Link identity path")?;

    let tailscaled = discovery::find_tailscaled().unwrap_or_else(|error| {
        warn!(%error, "leanzeroLink: tailscaled not found; connect() will fail loudly until it is installed");
        PathBuf::from(TAILSCALED_FALLBACK)
    });
    let tailscale_cli = discovery::find_tailscale_cli().unwrap_or_else(|error| {
        warn!(%error, "leanzeroLink: tailscale CLI not found; connect() will fail loudly until it is installed");
        PathBuf::from(TAILSCALE_CLI_FALLBACK)
    });
    let mesh = MeshConfig::new(tailscaled, tailscale_cli, stable_node_id())
        .internal_err_ctx("building the LeanZero Link mesh config")?;

    let control = ControlConfig::new(node_token_from_email(email), None);

    Ok(LinkManagerConfig {
        worker_base_url,
        identity_path,
        mesh,
        control,
    })
}

/// Proxy the local control service's `GET /v1/swarm/nodes` (loopback, bearer token),
/// returning its `{ self, peers }` body verbatim so the snake_case wire shape survives.
async fn fetch_local_swarm_nodes() -> Result<LeanzeroLinkNodesResponse, String> {
    let email = current_identity_email()
        .ok_or_else(|| "no identity on disk to derive the control node token".to_string())?;
    let token = node_token_from_email(Some(&email));
    let url = format!("http://127.0.0.1:{DEFAULT_CONTROL_PORT}/v1/swarm/nodes");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| e.to_string())?;
    let body: serde_json::Value = client
        .get(&url)
        .bearer_auth(&token)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    let self_node = body.get("self").cloned().unwrap_or(serde_json::Value::Null);
    let peers = body
        .get("peers")
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default();
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

fn link_state_to_dto(state: LinkState) -> LeanzeroLinkStateResponse {
    LeanzeroLinkStateResponse {
        auth: auth_state_to_dto(state.auth),
        mesh: state.mesh.map(mesh_status_to_dto),
        node_count: state.node_count,
        last_error: state.last_error,
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
        email: Option<&str>,
    ) -> Result<Arc<LinkManager>, agent_client_protocol::Error> {
        let config = build_link_config(email)?;
        let source = self.link_source();
        let mut manager = LinkManager::new(config, source)
            .internal_err_ctx("constructing the LeanZero Link manager")?;
        // Attach the process-wide remote executor if goose-server injected one at boot; a
        // node built without it serves `/v1/swarm/execute` as `501` (execution not wired).
        if let Some(executor) = current_executor() {
            manager = manager.with_executor(executor);
        }
        Ok(Arc::new(manager))
    }

    /// The process-wide [`LinkManager`], built on first use in whatever auth state the
    /// identity file implies (it does NOT auto-connect). Rebuilt when the on-disk
    /// identity email changes so the account-derived `node_token` is always current — a
    /// change that can only happen while NOT connected (verify/connect are refused once
    /// Connecting/Connected, and logout clears the identity), so a rebuild never drops a
    /// live mesh.
    fn link_manager(&self) -> Result<Arc<LinkManager>, agent_client_protocol::Error> {
        let current = current_identity_email();

        if let Some(holder) = LINK.get() {
            {
                let guard = holder.lock().unwrap();
                if guard.email_key == current {
                    return Ok(guard.manager.clone());
                }
            }
            let manager = self.build_link_manager(current.as_deref())?;
            let mut guard = holder.lock().unwrap();
            if guard.email_key != current {
                guard.email_key = current;
                guard.manager = manager;
            }
            return Ok(guard.manager.clone());
        }

        let manager = self.build_link_manager(current.as_deref())?;
        let holder = StdMutex::new(LinkHolder {
            email_key: current,
            manager,
        });
        match LINK.set(holder) {
            Ok(()) => Ok(LINK.get().unwrap().lock().unwrap().manager.clone()),
            // Lost the init race; the fast path now resolves against the winner.
            Err(_) => self.link_manager(),
        }
    }

    pub(super) async fn on_leanzero_link_health(
        &self,
        _req: LeanzeroLinkHealthRequest,
    ) -> Result<LeanzeroLinkHealthResponse, agent_client_protocol::Error> {
        let manager = self.link_manager()?;
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
        let manager = self.link_manager()?;
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
        let manager = self.link_manager()?;
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
        let manager = self.link_manager()?;
        manager.connect().await.map_err(link_err)?;
        Ok(link_state_to_dto(manager.status().await))
    }

    pub(super) async fn on_leanzero_link_status(
        &self,
        _req: LeanzeroLinkStatusRequest,
    ) -> Result<LeanzeroLinkStateResponse, agent_client_protocol::Error> {
        let manager = self.link_manager()?;
        Ok(link_state_to_dto(manager.status().await))
    }

    pub(super) async fn on_leanzero_link_logout(
        &self,
        req: LeanzeroLinkLogoutRequest,
    ) -> Result<LeanzeroLinkStateResponse, agent_client_protocol::Error> {
        let manager = self.link_manager()?;
        manager.logout(req.wipe).await.map_err(link_err)?;
        Ok(link_state_to_dto(manager.status().await))
    }

    pub(super) async fn on_leanzero_link_nodes(
        &self,
        _req: LeanzeroLinkNodesRequest,
    ) -> Result<LeanzeroLinkNodesResponse, agent_client_protocol::Error> {
        let manager = self.link_manager()?;
        if let AuthState::Connected { .. } = manager.status().await.auth {
            match fetch_local_swarm_nodes().await {
                Ok(response) => return Ok(response),
                Err(error) => {
                    warn!(%error, "leanzeroLink: proxying the local control service /v1/swarm/nodes failed; returning the local node only");
                }
            }
        }
        // Not connected (or the loopback proxy failed): self only, no peers.
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
        let manager = self.link_manager()?;
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_token_is_deterministic_case_insensitive_and_hex() {
        let a = node_token_from_email(Some("Mihai@Wolfaenpak.com"));
        let b = node_token_from_email(Some("mihai@wolfaenpak.com"));
        assert_eq!(a, b, "email casing must not change the token");
        assert_eq!(a.len(), 64, "SHA-256 HMAC is 32 bytes = 64 hex chars");
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));

        let other = node_token_from_email(Some("someone@else.com"));
        assert_ne!(a, other, "different accounts derive different tokens");

        let logged_out = node_token_from_email(None);
        assert_eq!(logged_out.len(), 64);
        assert_ne!(
            logged_out, a,
            "the empty-key placeholder is not an account token"
        );
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

    #[test]
    fn derive_node_reports_busy_idle_and_active_count() {
        let now = Utc::now();

        let idle = derive_node("node-a", &[summary("s1", false, 100)], now);
        assert_eq!(idle.status, NodeStatus::Idle);
        assert_eq!(idle.sessions_active, 0);
        assert!(idle.mesh_ip.is_none());
        assert_eq!(idle.node_id, "node-a");

        let busy = derive_node(
            "node-a",
            &[summary("old", true, 100), summary("new", true, 200)],
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
        };
        let value = serde_json::to_value(&state).unwrap();
        assert_eq!(value["auth"]["state"], "loggedIn");
        assert_eq!(value["nodeCount"], 2);
        assert_eq!(value["lastError"], "boom");
        let back: LeanzeroLinkStateResponse = serde_json::from_value(value).unwrap();
        assert_eq!(back.node_count, 2);
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

        let sessions = source.local_sessions().await;
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
