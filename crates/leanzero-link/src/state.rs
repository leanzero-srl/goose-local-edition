//! The decoupling seam ([`SwarmStateSource`]) and the peer fabric
//! ([`PeerRegistry`]): polls each mesh peer's `/v1/swarm/nodes` + `/sessions`
//! and subscribes to its `/v1/swarm/stream`, folding remote node states,
//! session summaries, and deltas into the local view.
//!
//! Fabric invariants:
//! - The fabric is full-mesh: each node folds only what a peer ORIGINATES
//!   (`?scope=local` subscriptions, origin-filtered session folds), never what a
//!   peer merely relays — no echo loops, no duplicate fan-out.
//! - A peer that cannot be REACHED (transport failure) is `NodeStatus::Offline` — a
//!   state in the view, never a dropped request and never a fabricated `Idle`. A peer
//!   that ANSWERS but not as a LeanZero Link node should (a 401, a 503, an unparseable
//!   body) is alive: its `status` keeps the last known value and the failure text
//!   rides in `NodeState::last_poll_error` — never a fabricated `Offline` either.
//! - Peer tasks are aborted individually per peer (the per-pid discipline);
//!   they hold only a `Weak` handle to the registry so a dropped registry ends
//!   its tasks instead of leaking them.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex as StdMutex, Weak};
use std::time::Duration;

use chrono::Utc;
use futures::stream::BoxStream;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::task::JoinHandle;

use crate::mesh::MeshPeer;
use crate::pubsub::{EventOrigin, PubSub};
use crate::wire::{
    LinkEvent, NodeState, NodeStatus, SessionSummary, StreamFrame, SwarmNodesResponse,
};

/// What the control service needs from the process hosting it. goose-server will
/// implement this over its `SessionManager` + `session_event_bus`; tests script a
/// fake. The stream carries every locally originated [`LinkEvent`]
/// (`NodeStateChanged` / `SessionUpserted` / `SessionDelta`).
#[async_trait::async_trait]
pub trait SwarmStateSource: Send + Sync + 'static {
    async fn local_node(&self) -> NodeState;
    async fn local_sessions(&self) -> Vec<SessionSummary>;
    fn subscribe_local_deltas(&self) -> BoxStream<'static, LinkEvent>;
}

/// Body of `POST /v1/swarm/execute`: one node's request that THIS node run a goose
/// prompt. Field names are the wire contract the iOS companion and the swarm dispatcher
/// send. `working_dir`/`session_id` are optional: absent `working_dir` defaults to the
/// node's link workspace (the executor decides — goose-server uses `$HOME`), and absent
/// `session_id` creates a fresh session; a present one reuses that session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecuteRequest {
    pub prompt: String,
    #[serde(default)]
    pub working_dir: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
}

/// `202` body of `POST /v1/swarm/execute`: the run was accepted and is now streaming its
/// per-message deltas over `/v1/swarm/stream` (the P4 mirror) — there is no separate
/// result channel. `session_id` is the session the run drives (created or reused).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecuteAccepted {
    pub session_id: String,
}

/// A remote-execute failure. Each variant maps to one HTTP status on the control route
/// (see `control::execute_error_status`): `Busy` → 409, `Disabled` → 403,
/// `BadRequest` → 400, `Internal` → 500. Never an empty-success or a fabricated accept.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ExecuteError {
    #[error("node is busy")]
    Busy,
    #[error("remote execution disabled on this node")]
    Disabled,
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("internal error: {0}")]
    Internal(String),
}

/// What the control service needs to actually RUN a remote prompt on this machine.
/// goose-server implements it (`GoosedRemoteExecutor`) over its reply machinery; the
/// control route calls it after the receive-side gates pass. The trait is the seam —
/// this crate never spawns an agent itself. Injected into the control service beside the
/// [`SwarmStateSource`]; `None` there means the route answers `501` (never a fake accept).
#[async_trait::async_trait]
pub trait RemoteExecutor: Send + Sync + 'static {
    async fn execute(&self, req: ExecuteRequest) -> Result<ExecuteAccepted, ExecuteError>;
}

/// One mlxEngine operation the mesh proxy forwards. Each variant maps 1:1 to a
/// `_goose/unstable/mlxEngine/<op>` ACP method AND to the control route
/// `POST /v1/swarm/mlx/<path()>`. The wire carries the [`Self::path`] string as the last
/// path segment; the request/response bodies are the mlxEngine DTOs passed through as
/// opaque JSON (this crate never re-types the 16 shapes). Adding an op is: a variant here,
/// a route in [`crate::control`], and an arm in the goose-side `MlxControl` impl.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MlxOp {
    Status,
    Mount,
    Unmount,
    SettingsRead,
    SettingsUpdate,
    ModelsList,
    ModelDelete,
    HfSearch,
    Browse,
    BrowseFilters,
    ModelCard,
    Download,
    DownloadProgress,
    DownloadPause,
    DownloadResume,
    DownloadCancel,
}

impl MlxOp {
    /// The last path segment of the control route and the ACP method suffix (camelCase),
    /// so `/v1/swarm/mlx/settingsUpdate` mirrors `mlxEngine/settingsUpdate` exactly.
    pub fn path(self) -> &'static str {
        match self {
            Self::Status => "status",
            Self::Mount => "mount",
            Self::Unmount => "unmount",
            Self::SettingsRead => "settingsRead",
            Self::SettingsUpdate => "settingsUpdate",
            Self::ModelsList => "modelsList",
            Self::ModelDelete => "modelDelete",
            Self::HfSearch => "hfSearch",
            Self::Browse => "browse",
            Self::BrowseFilters => "browseFilters",
            Self::ModelCard => "modelCard",
            Self::Download => "download",
            Self::DownloadProgress => "downloadProgress",
            Self::DownloadPause => "downloadPause",
            Self::DownloadResume => "downloadResume",
            Self::DownloadCancel => "downloadCancel",
        }
    }

    pub fn from_path(path: &str) -> Option<Self> {
        Some(match path {
            "status" => Self::Status,
            "mount" => Self::Mount,
            "unmount" => Self::Unmount,
            "settingsRead" => Self::SettingsRead,
            "settingsUpdate" => Self::SettingsUpdate,
            "modelsList" => Self::ModelsList,
            "modelDelete" => Self::ModelDelete,
            "hfSearch" => Self::HfSearch,
            "browse" => Self::Browse,
            "browseFilters" => Self::BrowseFilters,
            "modelCard" => Self::ModelCard,
            "download" => Self::Download,
            "downloadProgress" => Self::DownloadProgress,
            "downloadPause" => Self::DownloadPause,
            "downloadResume" => Self::DownloadResume,
            "downloadCancel" => Self::DownloadCancel,
            _ => return None,
        })
    }

    /// True for ops that destroy on-disk state or a running engine over the mesh: model
    /// delete, download-cancel (which also deletes the partial repo), settings-update
    /// (persists into the node's own config and can change what it serves) and unmount
    /// (stops the engine a local session may be using). These are logged at `warn` on the
    /// executing node so a remote destructive action is never silent.
    pub fn is_destructive(self) -> bool {
        matches!(
            self,
            Self::ModelDelete | Self::DownloadCancel | Self::SettingsUpdate | Self::Unmount
        )
    }
}

/// A local mlxEngine failure surfaced to a remote caller through the proxy. The two
/// variants preserve the local ACP error CLASS so node A can re-raise it with the same
/// currency: [`Self::BadRequest`] is a user-actionable / params problem (mounts that hit a
/// memory-gate BLOCK, a malformed repo id — the local `invalid_params` bucket) → `400`;
/// [`Self::Failed`] is the node's own internal failure (disk read, HF fetch — the local
/// `internal_error` bucket) → `500`. Never an empty-success. See
/// [`crate::control::mlx_control_error_status`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum MlxControlError {
    #[error("{0}")]
    BadRequest(String),
    #[error("{0}")]
    Failed(String),
}

/// What the control service needs to actually RUN a mlxEngine operation on THIS machine.
/// goose implements it (`GoosedMlxControl`) over the very code path its local
/// `mlxEngine/*` ACP handlers call (`goose_sidecar`'s engine manager + HF tracker), so a
/// download/delete/settings-change reached over the mesh is byte-for-byte the local
/// operation. The trait is the seam — this crate never touches `goose_sidecar`. Injected
/// into the control service beside the [`SwarmStateSource`]; `None` there means the
/// `/v1/swarm/mlx/*` routes answer `501` (loud-absent, never a fabricated result).
///
/// `request` is the op's request DTO as opaque JSON and the `Ok` value is the op's
/// response DTO as opaque JSON — the exact `mlxEngine/<op>` shapes; the implementor is the
/// only party that types them.
#[async_trait::async_trait]
pub trait MlxControl: Send + Sync + 'static {
    async fn dispatch(
        &self,
        op: MlxOp,
        request: serde_json::Value,
    ) -> Result<serde_json::Value, MlxControlError>;
}

/// One mesh peer as a polling/subscription target. `mesh_ip: None` (tailscaled
/// knows the peer but reports no IP) yields a permanently `Offline` row — shown
/// loudly, never silently skipped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerTarget {
    pub hostname: String,
    pub mesh_ip: Option<String>,
    pub port: u16,
}

impl PeerTarget {
    pub fn from_mesh_peer(peer: &MeshPeer, port: u16) -> Self {
        Self {
            hostname: peer.hostname.clone(),
            mesh_ip: peer.ip.clone(),
            port,
        }
    }

    fn authority(&self) -> Option<String> {
        self.mesh_ip.as_ref().map(|ip| {
            if ip.contains(':') {
                format!("[{ip}]:{}", self.port)
            } else {
                format!("{ip}:{}", self.port)
            }
        })
    }

    pub fn base_url(&self) -> Option<String> {
        self.authority().map(|a| format!("http://{a}"))
    }

    fn ws_base(&self) -> Option<String> {
        self.authority().map(|a| format!("ws://{a}"))
    }
}

#[derive(Debug, Clone)]
pub struct PeerRegistryConfig {
    pub node_token: String,
    pub poll_interval: Duration,
    pub request_timeout: Duration,
    pub reconnect_backoff: Duration,
}

struct PeerEntry {
    target: PeerTarget,
    state: NodeState,
    tasks: Vec<JoinHandle<()>>,
}

struct RegistryInner {
    config: PeerRegistryConfig,
    pubsub: Arc<PubSub>,
    http: reqwest::Client,
    /// Keyed by mesh hostname (the identity the tailnet gives us before a peer
    /// ever answers). Lock order where both are held: `peers` before `sessions`.
    peers: StdMutex<HashMap<String, PeerEntry>>,
    /// Mirror index of peer-originated sessions, keyed by session_id.
    sessions: StdMutex<HashMap<String, SessionSummary>>,
}

impl Drop for RegistryInner {
    fn drop(&mut self) {
        if let Ok(peers) = self.peers.lock() {
            for entry in peers.values() {
                for task in &entry.tasks {
                    task.abort();
                }
            }
        }
    }
}

#[derive(Clone)]
pub struct PeerRegistry {
    inner: Arc<RegistryInner>,
}

impl PeerRegistry {
    pub fn new(config: PeerRegistryConfig, pubsub: Arc<PubSub>) -> Result<Self, reqwest::Error> {
        let http = reqwest::Client::builder()
            .timeout(config.request_timeout)
            .build()?;
        Ok(Self {
            inner: Arc::new(RegistryInner {
                config,
                pubsub,
                http,
                peers: StdMutex::new(HashMap::new()),
                sessions: StdMutex::new(HashMap::new()),
            }),
        })
    }

    /// Reconcile the fabric against the current mesh peer list: removed peers
    /// have their tasks aborted and their sessions dropped from the mirror;
    /// new/changed peers start `Offline` and get a poll task + a stream task.
    pub fn set_peers(&self, targets: Vec<PeerTarget>) {
        let mut peers = self.inner.peers.lock().unwrap();

        let keep: HashSet<&str> = targets.iter().map(|t| t.hostname.as_str()).collect();
        let removed: Vec<String> = peers
            .keys()
            .filter(|k| !keep.contains(k.as_str()))
            .cloned()
            .collect();
        for hostname in removed {
            if let Some(entry) = peers.remove(&hostname) {
                for task in &entry.tasks {
                    task.abort();
                }
                let node_id = entry.state.node_id;
                self.inner
                    .sessions
                    .lock()
                    .unwrap()
                    .retain(|_, s| s.origin_node_id != node_id);
            }
        }

        for target in targets {
            if let Some(existing) = peers.get(&target.hostname) {
                if existing.target == target {
                    continue;
                }
            }
            if let Some(old) = peers.remove(&target.hostname) {
                for task in &old.tasks {
                    task.abort();
                }
            }
            let state = NodeState {
                node_id: target.hostname.clone(),
                hostname: target.hostname.clone(),
                mesh_ip: target.mesh_ip.clone(),
                status: NodeStatus::Offline,
                sessions_active: 0,
                updated_at: Utc::now(),
                last_poll_error: None,
            };
            let mut tasks = Vec::new();
            match (target.base_url(), target.ws_base()) {
                (Some(base_url), Some(ws_base)) => {
                    let weak = Arc::downgrade(&self.inner);
                    tasks.push(tokio::spawn(poll_peer_loop(
                        weak.clone(),
                        target.clone(),
                        base_url,
                    )));
                    tasks.push(tokio::spawn(stream_peer_loop(
                        weak,
                        target.clone(),
                        ws_base,
                    )));
                }
                _ => {
                    tracing::warn!(
                        hostname = %target.hostname,
                        "mesh peer reports no IP; shown Offline, not polled"
                    );
                }
            }
            peers.insert(
                target.hostname.clone(),
                PeerEntry {
                    target,
                    state,
                    tasks,
                },
            );
        }
    }

    /// Convenience over [`Self::set_peers`] straight from `MeshStatus.peers`.
    pub fn set_mesh_peers(&self, peers: &[MeshPeer], control_port: u16) {
        self.set_peers(
            peers
                .iter()
                .map(|p| PeerTarget::from_mesh_peer(p, control_port))
                .collect(),
        );
    }

    /// Last-known states of all mesh peers, sorted by hostname.
    pub fn peer_nodes(&self) -> Vec<NodeState> {
        let peers = self.inner.peers.lock().unwrap();
        let mut nodes: Vec<NodeState> = peers.values().map(|e| e.state.clone()).collect();
        nodes.sort_by(|a, b| {
            a.hostname
                .cmp(&b.hostname)
                .then_with(|| a.node_id.cmp(&b.node_id))
        });
        nodes
    }

    /// The peer-originated slice of the mirror index, most recent first.
    pub fn peer_sessions(&self) -> Vec<SessionSummary> {
        let sessions = self.inner.sessions.lock().unwrap();
        let mut all: Vec<SessionSummary> = sessions.values().cloned().collect();
        all.sort_by(|a, b| {
            b.updated_at
                .cmp(&a.updated_at)
                .then_with(|| a.session_id.cmp(&b.session_id))
        });
        all
    }

    /// Base URL (`http://<mesh_ip>:<port>`) of the peer identified by `node_id` — the
    /// address [`LinkManager::remote_execute`](crate::manager::LinkManager::remote_execute)
    /// POSTs `/v1/swarm/execute` to. Matches on the peer's folded `node_id` first, then on
    /// the registry key / reported hostname (so a target is reachable even before the first
    /// poll has folded its self-reported id). `None` when no such peer is known or it
    /// reports no mesh IP (a permanently `Offline` row is not a POST target).
    pub fn peer_base_url(&self, node_id: &str) -> Option<String> {
        let peers = self.inner.peers.lock().unwrap();
        peers
            .values()
            .find(|entry| {
                entry.state.node_id == node_id
                    || entry.target.hostname == node_id
                    || entry.state.hostname == node_id
            })
            .and_then(|entry| entry.target.base_url())
    }

    /// Abort every peer task and clear the view.
    pub fn shutdown(&self) {
        let mut peers = self.inner.peers.lock().unwrap();
        for entry in peers.values() {
            for task in &entry.tasks {
                task.abort();
            }
        }
        peers.clear();
        self.inner.sessions.lock().unwrap().clear();
    }
}

/// Why one poll of a peer produced no snapshot. The two classes are folded
/// differently: a peer we cannot REACH is `Offline`; a peer that ANSWERS wrongly is
/// alive with an error on record.
enum PollFailure {
    /// No HTTP exchange completed (refused, reset, timeout, DNS).
    Transport(String),
    /// The peer answered — with a non-2xx status or a body this crate cannot parse.
    Answered(String),
}

async fn poll_peer_loop(weak: Weak<RegistryInner>, target: PeerTarget, base_url: String) {
    loop {
        let Some(inner) = weak.upgrade() else { return };
        let poll_interval = inner.config.poll_interval;
        match poll_peer_once(&inner, &target, &base_url).await {
            Ok(events) => {
                for event in events {
                    inner.pubsub.publish(EventOrigin::Peer, event).await;
                }
            }
            Err(PollFailure::Transport(error)) => {
                if let Some(event) = mark_peer_offline(&inner, &target.hostname, &error) {
                    inner.pubsub.publish(EventOrigin::Peer, event).await;
                }
            }
            Err(PollFailure::Answered(error)) => {
                if let Some(event) = mark_peer_poll_error(&inner, &target.hostname, &error) {
                    inner.pubsub.publish(EventOrigin::Peer, event).await;
                }
            }
        }
        drop(inner);
        tokio::time::sleep(poll_interval).await;
    }
}

/// One authenticated GET of a peer route, classified: a transport error is
/// [`PollFailure::Transport`]; a non-2xx status (with a body snippet) or an
/// undecodable body is [`PollFailure::Answered`].
async fn fetch_json<T: serde::de::DeserializeOwned>(
    inner: &Arc<RegistryInner>,
    url: String,
) -> Result<T, PollFailure> {
    let response = inner
        .http
        .get(&url)
        .bearer_auth(&inner.config.node_token)
        .send()
        .await
        .map_err(|e| PollFailure::Transport(e.to_string()))?;
    let status = response.status();
    if !status.is_success() {
        let body = response
            .text()
            .await
            .unwrap_or_else(|e| format!("<body unreadable: {e}>"));
        let snippet: String = body.chars().take(200).collect();
        return Err(PollFailure::Answered(format!(
            "{url} answered {status}: {snippet}"
        )));
    }
    response.json::<T>().await.map_err(|e| {
        PollFailure::Answered(format!(
            "{url} answered 2xx but its body did not parse: {e}"
        ))
    })
}

async fn poll_peer_once(
    inner: &Arc<RegistryInner>,
    target: &PeerTarget,
    base_url: &str,
) -> Result<Vec<LinkEvent>, PollFailure> {
    let nodes: SwarmNodesResponse = fetch_json(inner, format!("{base_url}/v1/swarm/nodes")).await?;
    let sessions: Vec<SessionSummary> =
        fetch_json(inner, format!("{base_url}/v1/swarm/sessions?scope=local")).await?;
    Ok(fold_peer_snapshot(
        inner,
        &target.hostname,
        nodes.self_node,
        sessions,
    ))
}

/// Fold a successful poll into the view. Returns the change events to publish
/// (computed under the locks, published outside them). `last_poll_error` is the
/// POLLER's field: a clean poll clears it whatever the peer put in its own report.
fn fold_peer_snapshot(
    inner: &Arc<RegistryInner>,
    hostname: &str,
    mut remote_self: NodeState,
    remote_sessions: Vec<SessionSummary>,
) -> Vec<LinkEvent> {
    remote_self.last_poll_error = None;
    let mut events = Vec::new();
    let mut peers = inner.peers.lock().unwrap();
    let Some(entry) = peers.get_mut(hostname) else {
        return events;
    };
    if entry.state != remote_self {
        entry.state = remote_self.clone();
        events.push(LinkEvent::NodeStateChanged(remote_self.clone()));
    }
    let remote_id = remote_self.node_id;

    let mut sessions = inner.sessions.lock().unwrap();
    let fresh: HashSet<&str> = remote_sessions
        .iter()
        .filter(|s| s.origin_node_id == remote_id)
        .map(|s| s.session_id.as_str())
        .collect();
    sessions.retain(|_, s| s.origin_node_id != remote_id || fresh.contains(s.session_id.as_str()));
    for summary in remote_sessions
        .into_iter()
        .filter(|s| s.origin_node_id == remote_id)
    {
        let changed = sessions.get(&summary.session_id) != Some(&summary);
        if changed {
            sessions.insert(summary.session_id.clone(), summary.clone());
            events.push(LinkEvent::SessionUpserted(summary));
        }
    }
    events
}

/// A transport failure: the peer is unreachable → `Offline`, the error text on record.
fn mark_peer_offline(inner: &Arc<RegistryInner>, hostname: &str, error: &str) -> Option<LinkEvent> {
    let mut peers = inner.peers.lock().unwrap();
    let entry = peers.get_mut(hostname)?;
    if entry.state.status == NodeStatus::Offline
        && entry.state.last_poll_error.as_deref() == Some(error)
    {
        return None;
    }
    tracing::warn!(hostname, error, "mesh peer unreachable; marked Offline");
    entry.state.status = NodeStatus::Offline;
    entry.state.last_poll_error = Some(error.to_string());
    entry.state.updated_at = Utc::now();
    Some(LinkEvent::NodeStateChanged(entry.state.clone()))
}

/// An answered-but-wrong poll (4xx/5xx, undecodable body): the peer is ALIVE, so its
/// `status` keeps its last known value; only the error text (and `updated_at`) change.
fn mark_peer_poll_error(
    inner: &Arc<RegistryInner>,
    hostname: &str,
    error: &str,
) -> Option<LinkEvent> {
    let mut peers = inner.peers.lock().unwrap();
    let entry = peers.get_mut(hostname)?;
    if entry.state.last_poll_error.as_deref() == Some(error) {
        return None;
    }
    tracing::warn!(
        hostname,
        error,
        status = ?entry.state.status,
        "mesh peer answered but not as a LeanZero Link node should; status kept, error recorded"
    );
    entry.state.last_poll_error = Some(error.to_string());
    entry.state.updated_at = Utc::now();
    Some(LinkEvent::NodeStateChanged(entry.state.clone()))
}

async fn stream_peer_loop(weak: Weak<RegistryInner>, target: PeerTarget, ws_base: String) {
    let mut since: Option<u64> = None;
    loop {
        let Some(inner) = weak.upgrade() else { return };
        let backoff = inner.config.reconnect_backoff;
        let url = stream_url(&ws_base, &inner.config.node_token, since);
        drop(inner);

        match tokio_tungstenite::connect_async(&url).await {
            Err(error) => {
                tracing::debug!(hostname = %target.hostname, %error, "peer stream connect failed; will retry");
            }
            Ok((mut ws, _response)) => {
                tracing::debug!(hostname = %target.hostname, "peer stream connected");
                use tokio_tungstenite::tungstenite::Message as TsMessage;
                while let Some(message) = ws.next().await {
                    let Some(inner) = weak.upgrade() else { return };
                    match message {
                        Ok(TsMessage::Text(text)) => {
                            match serde_json::from_str::<StreamFrame>(&text) {
                                Ok(frame) => {
                                    since = Some(frame.seq);
                                    let events =
                                        fold_stream_event(&inner, &target.hostname, frame.event);
                                    for event in events {
                                        inner.pubsub.publish(EventOrigin::Peer, event).await;
                                    }
                                }
                                Err(error) => {
                                    tracing::warn!(
                                        hostname = %target.hostname,
                                        %error,
                                        frame = %text,
                                        "unparseable peer stream frame; skipped"
                                    );
                                }
                            }
                        }
                        Ok(TsMessage::Close(frame)) => {
                            let evicted = frame
                                .as_ref()
                                .is_some_and(|f| f.reason.as_str() == "ClientTooFarBehind");
                            if evicted {
                                tracing::warn!(
                                    hostname = %target.hostname,
                                    "peer stream cursor evicted; resubscribing from scratch"
                                );
                                since = None;
                            }
                            break;
                        }
                        Ok(_) => {}
                        Err(error) => {
                            tracing::debug!(hostname = %target.hostname, %error, "peer stream read failed");
                            break;
                        }
                    }
                }
            }
        }
        tokio::time::sleep(backoff).await;
    }
}

fn stream_url(ws_base: &str, token: &str, since: Option<u64>) -> String {
    let mut query = url::form_urlencoded::Serializer::new(String::new());
    query.append_pair("scope", "local");
    query.append_pair("token", token);
    if let Some(since) = since {
        query.append_pair("since", &since.to_string());
    }
    format!("{ws_base}/v1/swarm/stream?{}", query.finish())
}

/// Fold one live event from a peer's `?scope=local` stream. Returns the events
/// to republish into the local hub (deduplicated against the current view).
fn fold_stream_event(
    inner: &Arc<RegistryInner>,
    hostname: &str,
    event: LinkEvent,
) -> Vec<LinkEvent> {
    match event {
        LinkEvent::NodeStateChanged(node) => {
            let mut peers = inner.peers.lock().unwrap();
            let Some(entry) = peers.get_mut(hostname) else {
                return Vec::new();
            };
            if entry.state == node {
                return Vec::new();
            }
            entry.state = node.clone();
            vec![LinkEvent::NodeStateChanged(node)]
        }
        LinkEvent::SessionUpserted(summary) => {
            let mut sessions = inner.sessions.lock().unwrap();
            if sessions.get(&summary.session_id) == Some(&summary) {
                return Vec::new();
            }
            sessions.insert(summary.session_id.clone(), summary.clone());
            vec![LinkEvent::SessionUpserted(summary)]
        }
        delta @ LinkEvent::SessionDelta { .. } => vec![delta],
    }
}
