//! The `/v1/swarm` control service: the node-to-node API every LeanZero Link
//! node serves, and the one the desktop UI and the companion app consume.
//!
//! Endpoints (all bearer-token authenticated, see [`ControlConfig::node_token`]):
//! - `GET /v1/swarm/nodes` → [`SwarmNodesResponse`] — this node + last-known peers.
//! - `GET /v1/swarm/sessions` → `Vec<SessionSummary>` — the mirror index
//!   (`?scope=local` restricts to locally originated sessions; peers poll with it).
//! - `GET /v1/swarm/stream` (WebSocket) → [`StreamFrame`]s; `?since=<seq>` replay
//!   cursor, `?scope=local|all`, heartbeat as ws ping. An evicted cursor gets a
//!   close frame `code=4408, reason="ClientTooFarBehind"`.
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

use axum::extract::ws::{CloseFrame, Message as WsMessage, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, Request, State};
use axum::http::{header, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
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
use crate::state::{PeerRegistry, PeerRegistryConfig, PeerTarget, SwarmStateSource};
use crate::wire::{NodeStatus, SessionSummary, StreamFrame, SwarmNodesResponse};

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
    /// Injected shared secret (account JWT or a derived per-tailnet secret).
    /// This crate never mints or verifies it against any backend — the mesh is
    /// already account-isolated; this is defense in depth.
    pub node_token: String,
    pub port: u16,
    /// `MeshStatus.self_ip`; `None` = mesh down.
    pub mesh_ip: Option<IpAddr>,
    pub poll_interval: Duration,
    pub heartbeat_interval: Duration,
    pub request_timeout: Duration,
    pub reconnect_backoff: Duration,
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
            reconnect_backoff: Duration::from_secs(2),
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
}

pub struct ControlService;

impl ControlService {
    /// Bind the listeners, start the peer fabric and the local delta pump, and
    /// return a handle. Peers are attached afterwards via
    /// [`ControlHandle::set_peers`] as mesh status evolves.
    pub async fn start(
        config: ControlConfig,
        source: Arc<dyn SwarmStateSource>,
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
async fn require_token(
    State(token): State<Arc<String>>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
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

async fn sessions(
    Query(query): Query<SessionsQuery>,
    State(ctx): State<Ctx>,
) -> Json<Vec<SessionSummary>> {
    let mut all = ctx.source.local_sessions().await;
    if query.scope == StreamScope::All && ctx.mesh_ip.is_some() {
        all.extend(ctx.registry.peer_sessions());
    }
    all.sort_by(|a, b| {
        b.updated_at
            .cmp(&a.updated_at)
            .then_with(|| a.session_id.cmp(&b.session_id))
    });
    Json(all)
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
