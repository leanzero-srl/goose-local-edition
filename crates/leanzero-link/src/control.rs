//! The `/v1/swarm` control service: the node-to-node API every LeanZero Link
//! node serves, and the one the desktop UI and the companion app consume.
//!
//! Endpoints (all bearer-token authenticated, see [`ControlConfig::node_token`]):
//! - `GET /v1/swarm/nodes` → [`SwarmNodesResponse`] — this node + last-known peers.
//! - `GET /v1/swarm/sessions` → `Vec<SessionSummary>` — the mirror index
//!   (`?scope=local` restricts to locally originated sessions; peers poll with it).
//!   `503 session index unreadable: <err>` when the local store cannot be read right
//!   now — never `200 []` over a failed read (peers keep their mirror on a 503).
//! - `GET /v1/swarm/stream` (WebSocket) → [`StreamFrame`]s; `?since=<seq>` replay
//!   cursor, `?scope=local|all`, heartbeat as ws ping. An evicted cursor gets a
//!   close frame `code=4408, reason="ClientTooFarBehind"`.
//! - `POST /v1/swarm/execute` → `202 {session_id}` — cross-node remote execution: one
//!   same-account device tells this node's goose to run a prompt; results stream back
//!   over `/v1/swarm/stream` (the P4 mirror), not a separate channel. SECURITY: a remote
//!   execute runs goose (shell/file tools) on THIS machine. It is gated by (a) tailnet
//!   membership — the mesh is account-isolated, joined via the worker's ephemeral key —
//!   (b) the [`ControlConfig::node_token`] bearer (derived on every device of the account
//!   from the worker-issued account secret, see [`crate::token`]), and (c) the user's own
//!   switch: [`ControlConfig::allow_remote_execution`] is `false` by DEFAULT (observe-only;
//!   the host reads the user's setting and passes `true`), and while `false` the route
//!   answers `403`. The receive-side idle guard is real: a node whose
//!   [`SwarmStateSource::local_node`] reports non-`Idle` answers `409` and refuses the
//!   work now. No executor injected → `501` (loud-absent, never a fake accept). The
//!   bearer check is never weakened here.
//! - `POST /v1/swarm/mlx/<op>` → the remote MODEL-MANAGEMENT proxy: one same-account
//!   device runs/pauses/cancels a model download, deletes a model, reads status/models, or
//!   changes sampling/mount settings on THIS node's local MLX engine. Each `<op>` maps 1:1
//!   to a `mlxEngine/<op>` ACP method ([`MlxOp`]); the request/response bodies are the
//!   mlxEngine DTOs passed through as opaque JSON. The op is executed by the injected
//!   [`MlxControl`] against the node's own `goose_sidecar` engine — so a remote node's disk
//!   space, downloads, mounted model and browse results are all THAT node's truth. Gates:
//!   the same tailnet-isolation + node_token bearer as `/execute`, AND the same
//!   [`ControlConfig::allow_remote_execution`] switch — an observe-only node answers `403`
//!   ("remote model management is disabled on this node") to every `/mlx/*` op, read or
//!   write, because a peer that may not run prompts here may not reshape this node's
//!   engine either. NO idle guard — model management runs while the node is busy. No
//!   `MlxControl` injected → `501`. Destructive ops (`modelDelete`, `downloadCancel`,
//!   `settingsUpdate`, `unmount`) log at `warn` on the executing node. A peer's own
//!   failure (memory-gate BLOCK, disk-full, its 501) surfaces verbatim as `400`/`500`,
//!   never swallowed or faked.
//!
//! Listeners: 127.0.0.1 always (the local desktop), plus the mesh IP when one is
//! up. Under `--tun=userspace-networking` (this crate's only tailscaled mode) the
//! mesh IP is not a kernel address, so that bind fails and is reported as
//! [`MeshBind::UserspaceForwarded`]: tailscaled forwards inbound tailnet TCP to
//! the same port on loopback, so the loopback listener serves peers. No mesh IP
//! at all means [`MeshBind::LoopbackOnly`]: the node reports itself `Offline`
//! with `peers: []` — loud, never an error dressed as an empty result.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use axum::extract::rejection::JsonRejection;
use axum::extract::ws::{CloseFrame, Message as WsMessage, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, Request, State};
use axum::http::{header, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use bytes::Bytes;
use futures::stream::{SplitSink, StreamExt};
use futures::SinkExt;
use serde::Deserialize;
use subtle::ConstantTimeEq;
use thiserror::Error;
use tokio::net::TcpListener;
use tokio::sync::broadcast::error::RecvError;
use tokio::task::JoinHandle;

use crate::pubsub::{EventOrigin, PubSub, StampedEvent, SubscribeError};
use crate::state::{
    ExecuteError, ExecuteRequest, MlxControl, MlxControlError, MlxOp, PeerRegistry,
    PeerRegistryConfig, PeerTarget, RemoteExecutor, SwarmStateSource,
};
use crate::wire::{NodeStatus, StreamFrame, SwarmNodesResponse};

/// Fixed high default for the control service; every node on a tailnet must
/// serve the same port so peers can derive each other's URL from the mesh IP
/// alone. Override via [`ControlConfig::port`] (0 = ephemeral, tests only).
pub const DEFAULT_CONTROL_PORT: u16 = 41226;

/// WebSocket close code sent when a `?since=` cursor has been evicted from the
/// replay buffer (application range 4000-4999).
pub const CLOSE_CODE_CLIENT_TOO_FAR_BEHIND: u16 = 4408;
pub const CLOSE_REASON_CLIENT_TOO_FAR_BEHIND: &str = "ClientTooFarBehind";

#[derive(Debug, Error)]
pub enum ControlError {
    #[error("empty node token — the control service only starts with an injected token")]
    EmptyToken,
    #[error("cannot bind control listener on {addr}: {source}")]
    Bind {
        addr: SocketAddr,
        source: std::io::Error,
    },
    #[error("cannot build the peer-fabric HTTP client: {0}")]
    HttpClient(#[from] reqwest::Error),
}

#[derive(Debug, Clone)]
pub struct ControlConfig {
    /// The `/v1/swarm` bearer: [`crate::token::node_token_from_secret`] over the
    /// worker-issued per-account secret, so every device of the account derives the
    /// same value and no other party can. This crate never verifies it against any
    /// backend — the mesh is already account-isolated; this is defense in depth. A
    /// TEMPLATE config may leave it empty; `LinkManager::connect` fills it before
    /// `start`, and `start` refuses an empty one ([`ControlError::EmptyToken`]).
    pub node_token: String,
    pub port: u16,
    /// `MeshStatus.self_ip`; `None` = mesh down.
    pub mesh_ip: Option<IpAddr>,
    pub poll_interval: Duration,
    pub heartbeat_interval: Duration,
    /// TOTAL timeout of the fabric's `/nodes` + `/sessions` polls — small, bounded
    /// reads whose slowness IS the signal.
    pub request_timeout: Duration,
    /// CONNECT timeout of the `/execute` and `/mlx/*` proxy POSTs — and their only
    /// timeout. Those ops run as long as the peer's work runs (a model delete of tens
    /// of GB, an HF fetch); a total cap would report failure while the peer completes.
    pub connect_timeout: Duration,
    pub reconnect_backoff: Duration,
    /// Whether a same-account peer may ACT on this node: `POST /v1/swarm/execute` (run
    /// goose here) and every `POST /v1/swarm/mlx/<op>` (reshape this node's MLX engine).
    /// Default `false` — OBSERVE-ONLY: the node still mirrors sessions and serves
    /// `/nodes` `/sessions` `/stream`, but both acting surfaces answer `403` and no
    /// remote prompt or model op ever runs here. The host reads the user's own setting
    /// and passes `true` to opt a node in; nothing in this crate flips it.
    pub allow_remote_execution: bool,
}

impl ControlConfig {
    pub fn new(node_token: String, mesh_ip: Option<IpAddr>) -> Self {
        Self {
            node_token,
            port: DEFAULT_CONTROL_PORT,
            mesh_ip,
            poll_interval: Duration::from_secs(3),
            heartbeat_interval: Duration::from_secs(20),
            request_timeout: Duration::from_secs(5),
            connect_timeout: Duration::from_secs(5),
            reconnect_backoff: Duration::from_secs(2),
            allow_remote_execution: false,
        }
    }
}

/// How this node is reachable from the tailnet. Never a silent fallback — the
/// degraded variants carry the evidence.
#[derive(Debug)]
pub enum MeshBind {
    /// Dedicated listener bound directly on the mesh IP (kernel-TUN setups).
    Direct(SocketAddr),
    /// The mesh IP could not be bound (expected under userspace-networking
    /// tailscaled: no kernel interface carries the address). tailscaled forwards
    /// inbound tailnet TCP for this node to the same port on 127.0.0.1, so the
    /// loopback listener serves peers. The bind error is carried, not swallowed.
    UserspaceForwarded {
        attempted: SocketAddr,
        bind_error: String,
    },
    /// Mesh down (no `self_ip`): loopback only; `/nodes` reports this node
    /// `Offline` with `peers: []`.
    LoopbackOnly,
}

#[derive(Clone)]
struct Ctx {
    source: Arc<dyn SwarmStateSource>,
    pubsub: Arc<PubSub>,
    registry: PeerRegistry,
    mesh_ip: Option<IpAddr>,
    heartbeat_interval: Duration,
    /// `None` → `POST /v1/swarm/execute` answers `501` (execution not wired). Injected
    /// beside `source`, exactly as [`SwarmStateSource`] is — the executor is the seam
    /// goose-server implements; this crate never spawns an agent.
    executor: Option<Arc<dyn RemoteExecutor>>,
    /// `None` → the `/v1/swarm/mlx/*` proxy routes answer `501` (mlx control not wired).
    /// Injected beside `executor`; the seam goose implements over its local mlxEngine
    /// code path. This crate never touches `goose_sidecar`.
    mlx_control: Option<Arc<dyn MlxControl>>,
    allow_remote_execution: bool,
}

pub struct ControlService;

impl ControlService {
    /// Bind the listeners, start the peer fabric and the local delta pump, and
    /// return a handle. Peers are attached afterwards via
    /// [`ControlHandle::set_peers`] as mesh status evolves. `executor` is injected beside
    /// `source` (mirroring how [`SwarmStateSource`] is threaded); `None` leaves the
    /// execute route answering `501`.
    pub async fn start(
        config: ControlConfig,
        source: Arc<dyn SwarmStateSource>,
        executor: Option<Arc<dyn RemoteExecutor>>,
        mlx_control: Option<Arc<dyn MlxControl>>,
    ) -> Result<ControlHandle, ControlError> {
        if config.node_token.trim().is_empty() {
            return Err(ControlError::EmptyToken);
        }

        let pubsub = Arc::new(PubSub::new());
        let registry = PeerRegistry::new(
            PeerRegistryConfig {
                node_token: config.node_token.clone(),
                poll_interval: config.poll_interval,
                request_timeout: config.request_timeout,
                reconnect_backoff: config.reconnect_backoff,
            },
            pubsub.clone(),
        )?;

        let ctx = Ctx {
            source: source.clone(),
            pubsub: pubsub.clone(),
            registry: registry.clone(),
            mesh_ip: config.mesh_ip,
            heartbeat_interval: config.heartbeat_interval,
            executor,
            mlx_control,
            allow_remote_execution: config.allow_remote_execution,
        };
        let router = swarm_router(ctx, Arc::new(config.node_token));

        let loopback_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), config.port);
        let local_listener =
            TcpListener::bind(loopback_addr)
                .await
                .map_err(|source| ControlError::Bind {
                    addr: loopback_addr,
                    source,
                })?;
        let local_addr = local_listener
            .local_addr()
            .map_err(|source| ControlError::Bind {
                addr: loopback_addr,
                source,
            })?;

        let mut tasks = Vec::new();
        let mesh_bind = match config.mesh_ip {
            None => {
                tracing::warn!(
                    "mesh down (no self_ip): control service binds loopback only; \
                     node reports itself Offline with no peers"
                );
                MeshBind::LoopbackOnly
            }
            Some(ip) => {
                let mesh_addr = SocketAddr::new(ip, local_addr.port());
                match TcpListener::bind(mesh_addr).await {
                    Ok(listener) => {
                        let bound = listener.local_addr().unwrap_or(mesh_addr);
                        tasks.push(tokio::spawn(serve_listener(listener, router.clone())));
                        tracing::info!(%bound, "control service listening on the mesh IP");
                        MeshBind::Direct(bound)
                    }
                    Err(err) => {
                        tracing::warn!(
                            %mesh_addr,
                            error = %err,
                            "cannot bind the mesh IP directly (expected under \
                             userspace-networking tailscaled, which forwards inbound \
                             tailnet TCP to loopback); peers are served via the \
                             loopback listener"
                        );
                        MeshBind::UserspaceForwarded {
                            attempted: mesh_addr,
                            bind_error: err.to_string(),
                        }
                    }
                }
            }
        };
        tracing::info!(%local_addr, "control service listening on loopback");
        tasks.push(tokio::spawn(serve_listener(local_listener, router)));

        let mut deltas = source.subscribe_local_deltas();
        tasks.push(tokio::spawn(async move {
            while let Some(event) = deltas.next().await {
                pubsub.publish(EventOrigin::Local, event).await;
            }
            tracing::warn!(
                "local delta stream ended; /v1/swarm/stream now carries peer events only"
            );
        }));

        Ok(ControlHandle {
            local_addr,
            mesh_bind,
            registry,
            tasks,
        })
    }
}

pub struct ControlHandle {
    local_addr: SocketAddr,
    mesh_bind: MeshBind,
    registry: PeerRegistry,
    tasks: Vec<JoinHandle<()>>,
}

impl ControlHandle {
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    pub fn mesh_bind(&self) -> &MeshBind {
        &self.mesh_bind
    }

    pub fn registry(&self) -> &PeerRegistry {
        &self.registry
    }

    /// Reconcile the peer fabric; call whenever `MeshStatus.peers` changes.
    pub fn set_peers(&self, targets: Vec<PeerTarget>) {
        self.registry.set_peers(targets);
    }

    /// Stop accepting, abort the serve/pump tasks (each individually), and tear
    /// down the peer fabric.
    pub fn shutdown(self) {
        for task in &self.tasks {
            task.abort();
        }
        self.registry.shutdown();
    }
}

impl Drop for ControlHandle {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}

async fn serve_listener(listener: TcpListener, router: Router) {
    if let Err(error) = axum::serve(listener, router).await {
        tracing::error!(%error, "control listener failed");
    }
}

fn swarm_router(ctx: Ctx, token: Arc<String>) -> Router {
    Router::new()
        .route("/v1/swarm/nodes", get(nodes))
        .route("/v1/swarm/sessions", get(sessions))
        .route("/v1/swarm/stream", get(stream))
        .route("/v1/swarm/execute", post(execute))
        // One authenticated proxy route per mlxEngine op. `{op}` is validated against
        // `MlxOp` in the handler — an unknown op is a loud `404`, never a silent no-op.
        .route("/v1/swarm/mlx/{op}", post(mlx_proxy))
        .layer(axum::middleware::from_fn_with_state(token, require_token))
        .with_state(ctx)
}

fn token_matches(candidate: Option<&str>, expected: &str) -> bool {
    candidate
        .map(|c| bool::from(c.as_bytes().ct_eq(expected.as_bytes())))
        .unwrap_or(false)
}

/// `Authorization: Bearer <node_token>`, or `?token=` for WebSocket clients that
/// cannot set headers (mirrors goose's `/acp` transport choice). Constant-time
/// comparison via `subtle`.
///
/// Any request carrying an `Origin` header is refused with `403` BEFORE the token is
/// looked at: browsers attach `Origin` to every cross-origin fetch and every WebSocket
/// upgrade, while peers, the host's loopback proxy and native clients never send one —
/// so a web page on the same machine cannot drive the loopback listener even with a
/// leaked bearer (R-M6). This applies to the WS upgrade too, since the middleware wraps
/// `/v1/swarm/stream` like every other route.
async fn require_token(
    State(token): State<Arc<String>>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    if request.headers().contains_key(header::ORIGIN) {
        return Err(StatusCode::FORBIDDEN);
    }

    let header_token = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));

    let query_token = request.uri().query().and_then(|query| {
        url::form_urlencoded::parse(query.as_bytes())
            .find(|(key, _)| key == "token")
            .map(|(_, value)| value.into_owned())
    });

    if token_matches(header_token, &token) || token_matches(query_token.as_deref(), &token) {
        Ok(next.run(request).await)
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum StreamScope {
    /// Only locally originated events / sessions — what peers subscribe with,
    /// so relayed events are never re-relayed (no echo loops in the fabric).
    Local,
    #[default]
    All,
}

impl StreamScope {
    fn admits(self, origin: EventOrigin) -> bool {
        match self {
            Self::All => true,
            Self::Local => origin == EventOrigin::Local,
        }
    }
}

async fn nodes(State(ctx): State<Ctx>) -> Json<SwarmNodesResponse> {
    let mut self_node = ctx.source.local_node().await;
    match ctx.mesh_ip {
        Some(ip) => {
            self_node.mesh_ip = Some(ip.to_string());
            Json(SwarmNodesResponse {
                self_node,
                peers: ctx.registry.peer_nodes(),
            })
        }
        None => {
            self_node.mesh_ip = None;
            self_node.status = NodeStatus::Offline;
            Json(SwarmNodesResponse {
                self_node,
                peers: Vec::new(),
            })
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
struct SessionsQuery {
    #[serde(default)]
    scope: StreamScope,
}

fn index_unreadable(err: &str) -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        format!("session index unreadable: {err}"),
    )
        .into_response()
}

async fn sessions(Query(query): Query<SessionsQuery>, State(ctx): State<Ctx>) -> Response {
    let mut all = match ctx.source.local_sessions().await {
        Ok(local) => local,
        Err(err) => return index_unreadable(&err),
    };
    if query.scope == StreamScope::All && ctx.mesh_ip.is_some() {
        all.extend(ctx.registry.peer_sessions());
    }
    all.sort_by(|a, b| {
        b.updated_at
            .cmp(&a.updated_at)
            .then_with(|| a.session_id.cmp(&b.session_id))
    });
    Json(all).into_response()
}

/// The HTTP status each [`ExecuteError`] maps to. The wire contract the companion app,
/// the swarm dispatcher, and [`crate::manager::LinkManager::remote_execute`] all read.
pub fn execute_error_status(error: &ExecuteError) -> StatusCode {
    match error {
        ExecuteError::Busy => StatusCode::CONFLICT,
        ExecuteError::Disabled => StatusCode::FORBIDDEN,
        ExecuteError::BadRequest(_) => StatusCode::BAD_REQUEST,
        ExecuteError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

fn execute_error_response(error: ExecuteError) -> Response {
    (execute_error_status(&error), error.to_string()).into_response()
}

/// `POST /v1/swarm/execute`: run a goose prompt on this node for a same-account peer.
///
/// Gate order (each is loud, none is a fallback): `allow_remote_execution=false` → `403`;
/// an unparseable body → `400`; a session index that cannot be read right now → `503`
/// (a node that cannot see its own sessions does not take work — R-M2); the
/// receive-side idle guard (`local_node().status != Idle`) → `409` (a busy node refuses
/// remote work NOW); no executor injected → `501`; otherwise the executor runs and its
/// `202 {session_id}` (or mapped error) is returned. Auth (bearer / `?token=`,
/// constant-time) is enforced by the router middleware.
async fn execute(
    State(ctx): State<Ctx>,
    body: Result<Json<ExecuteRequest>, JsonRejection>,
) -> Response {
    if !ctx.allow_remote_execution {
        return execute_error_response(ExecuteError::Disabled);
    }

    let Json(req) = match body {
        Ok(json) => json,
        Err(rejection) => {
            return execute_error_response(ExecuteError::BadRequest(format!(
                "invalid execute request body: {rejection}"
            )));
        }
    };

    if let Err(err) = ctx.source.local_sessions().await {
        return index_unreadable(&err);
    }

    // Receive-side idle guard: the node the work lands on decides, from its own live
    // session state, whether it can take work now. `local_node().status` is `Idle`/`Busy`
    // (the source never fabricates `Offline` — that is a mesh-level verdict).
    if ctx.source.local_node().await.status != NodeStatus::Idle {
        return execute_error_response(ExecuteError::Busy);
    }

    let Some(executor) = ctx.executor.as_ref() else {
        return (
            StatusCode::NOT_IMPLEMENTED,
            "remote execution not wired on this node".to_string(),
        )
            .into_response();
    };

    match executor.execute(req).await {
        Ok(accepted) => (StatusCode::ACCEPTED, Json(accepted)).into_response(),
        Err(error) => execute_error_response(error),
    }
}

/// The HTTP status each [`MlxControlError`] maps to on the `/v1/swarm/mlx/*` routes, so the
/// forwarding side ([`crate::manager::LinkManager::mlx_proxy`]) can reconstruct the local
/// ACP error class: `BadRequest` → `400` (the local `invalid_params` bucket), `Failed` →
/// `500` (the local `internal_error` bucket).
pub fn mlx_control_error_status(error: &MlxControlError) -> StatusCode {
    match error {
        MlxControlError::BadRequest(_) => StatusCode::BAD_REQUEST,
        MlxControlError::Failed(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

/// `POST /v1/swarm/mlx/<op>`: run one mlxEngine operation on THIS node for a same-account
/// peer, against the local MLX engine via the injected [`MlxControl`].
///
/// Gate order (each loud, none a fallback): `allow_remote_execution=false` → `403` (the
/// observe-only switch covers model management exactly as it covers execution); an
/// unknown `<op>` → `404`; no `MlxControl` injected → `501`; an unparseable body → `400`;
/// otherwise the op runs and its response DTO (opaque JSON) is returned `200`, or its
/// [`MlxControlError`] maps to `400`/`500` so the peer's own failure (memory-gate BLOCK,
/// disk-full) surfaces verbatim. Auth (bearer / `?token=`, constant-time) is enforced by
/// the router middleware. There is NO idle guard: model management is allowed while the
/// node runs a session. Destructive ops log at `warn`.
async fn mlx_proxy(
    Path(op): Path<String>,
    State(ctx): State<Ctx>,
    body: Result<Json<serde_json::Value>, JsonRejection>,
) -> Response {
    if !ctx.allow_remote_execution {
        return (
            StatusCode::FORBIDDEN,
            "remote model management is disabled on this node".to_string(),
        )
            .into_response();
    }

    let Some(op) = MlxOp::from_path(&op) else {
        return (StatusCode::NOT_FOUND, format!("unknown mlx op '{op}'")).into_response();
    };

    let Some(control) = ctx.mlx_control.as_ref() else {
        return (
            StatusCode::NOT_IMPLEMENTED,
            "mlx control not wired on this node".to_string(),
        )
            .into_response();
    };

    let Json(request) = match body {
        Ok(json) => json,
        Err(rejection) => {
            return (
                StatusCode::BAD_REQUEST,
                format!("invalid mlx request body: {rejection}"),
            )
                .into_response();
        }
    };

    if op.is_destructive() {
        tracing::warn!(
            op = op.path(),
            "leanzeroLink: executing a DESTRUCTIVE remote mlx op over the mesh"
        );
    } else {
        tracing::info!(
            op = op.path(),
            "leanzeroLink: executing a remote mlx op over the mesh"
        );
    }

    match control.dispatch(op, request).await {
        Ok(value) => (StatusCode::OK, Json(value)).into_response(),
        Err(error) => (mlx_control_error_status(&error), error.to_string()).into_response(),
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
struct StreamQuery {
    since: Option<u64>,
    #[serde(default)]
    scope: StreamScope,
}

async fn stream(
    ws: WebSocketUpgrade,
    Query(query): Query<StreamQuery>,
    State(ctx): State<Ctx>,
) -> Response {
    ws.on_upgrade(move |socket| stream_socket(socket, ctx, query))
        .into_response()
}

async fn stream_socket(socket: WebSocket, ctx: Ctx, query: StreamQuery) {
    let (mut sink, mut incoming) = socket.split();

    let (replay, replay_max_seq, mut live_rx) = match ctx.pubsub.subscribe(query.since).await {
        Ok(subscription) => subscription,
        Err(SubscribeError::ClientTooFarBehind) => {
            let _ = sink
                .send(WsMessage::Close(Some(CloseFrame {
                    code: CLOSE_CODE_CLIENT_TOO_FAR_BEHIND,
                    reason: CLOSE_REASON_CLIENT_TOO_FAR_BEHIND.into(),
                })))
                .await;
            return;
        }
    };

    for event in &replay {
        if !query.scope.admits(event.origin) {
            continue;
        }
        if send_frame(&mut sink, event).await.is_err() {
            return;
        }
    }

    let mut heartbeat = tokio::time::interval(ctx.heartbeat_interval);
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            _ = heartbeat.tick() => {
                if sink.send(WsMessage::Ping(Bytes::new())).await.is_err() {
                    return;
                }
            }
            message = incoming.next() => {
                match message {
                    Some(Ok(WsMessage::Close(_))) | Some(Err(_)) | None => return,
                    Some(Ok(_)) => {}
                }
            }
            received = live_rx.recv() => {
                match received {
                    Ok(event) => {
                        if event.seq <= replay_max_seq || !query.scope.admits(event.origin) {
                            continue;
                        }
                        if send_frame(&mut sink, &event).await.is_err() {
                            return;
                        }
                    }
                    Err(RecvError::Lagged(missed)) => {
                        tracing::warn!(
                            missed,
                            "stream subscriber lagged; closing so it reconnects with ?since"
                        );
                        let _ = sink.send(WsMessage::Close(None)).await;
                        return;
                    }
                    Err(RecvError::Closed) => {
                        let _ = sink.send(WsMessage::Close(None)).await;
                        return;
                    }
                }
            }
        }
    }
}

async fn send_frame(
    sink: &mut SplitSink<WebSocket, WsMessage>,
    event: &StampedEvent,
) -> Result<(), axum::Error> {
    let frame = StreamFrame {
        seq: event.seq,
        event: event.event.clone(),
    };
    let json = serde_json::to_string(&frame).map_err(axum::Error::new)?;
    sink.send(WsMessage::Text(json.into())).await
}
