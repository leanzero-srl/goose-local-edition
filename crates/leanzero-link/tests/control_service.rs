//! Integration tests for the `/v1/swarm` control service: a scriptable
//! `FakeStateSource` + real axum services on ephemeral loopback ports, driven
//! with reqwest and a tokio-tungstenite ws client (dev-side only). No goosed,
//! no tailscaled, no tailnet contact — the mesh crate's own tests own that.

use std::future::Future;
use std::net::{IpAddr, Ipv6Addr};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use chrono::{DateTime, TimeZone, Utc};
use futures::stream::BoxStream;
use futures::StreamExt;
use leanzero_link::control::{
    execute_error_status, mlx_control_error_status, ControlConfig, ControlHandle, ControlService,
    MeshBind, CLOSE_CODE_CLIENT_TOO_FAR_BEHIND, CLOSE_REASON_CLIENT_TOO_FAR_BEHIND,
};
use leanzero_link::pubsub::REPLAY_BUFFER_CAPACITY;
use leanzero_link::state::{
    ExecuteAccepted, ExecuteError, ExecuteRequest, MlxControl, MlxControlError, MlxOp, PeerTarget,
    RemoteExecutor, SwarmStateSource,
};
use leanzero_link::wire::{
    LinkEvent, NodeState, NodeStatus, SessionDeltaKind, SessionSummary, StreamFrame,
};
use tokio::net::TcpStream;
use tokio::sync::broadcast;
use tokio_tungstenite::tungstenite::Message as TsMessage;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

type WsClient = WebSocketStream<MaybeTlsStream<TcpStream>>;

const TOKEN: &str = "test-node-token";
const DEADLINE: Duration = Duration::from_secs(15);

fn ts(secs: i64) -> DateTime<Utc> {
    Utc.timestamp_opt(secs, 0).unwrap()
}

fn summary(id: &str, origin: &str, live: bool, updated: i64) -> SessionSummary {
    SessionSummary {
        session_id: id.to_string(),
        origin_node_id: origin.to_string(),
        working_dir: format!("/work/{id}"),
        name: format!("session {id}"),
        updated_at: ts(updated),
        message_count: 4,
        live,
    }
}

struct FakeStateSource {
    node_id: String,
    hostname: String,
    updated_at: StdMutex<DateTime<Utc>>,
    sessions: StdMutex<Vec<SessionSummary>>,
    delta_tx: broadcast::Sender<LinkEvent>,
}

impl FakeStateSource {
    fn new(node_id: &str) -> Arc<Self> {
        let (delta_tx, _) = broadcast::channel(2048);
        Arc::new(Self {
            node_id: node_id.to_string(),
            hostname: format!("{node_id}-host"),
            updated_at: StdMutex::new(ts(1_700_000_000)),
            sessions: StdMutex::new(Vec::new()),
            delta_tx,
        })
    }

    fn set_sessions(&self, sessions: Vec<SessionSummary>) {
        *self.sessions.lock().unwrap() = sessions;
        *self.updated_at.lock().unwrap() = Utc::now();
    }

    fn emit(&self, event: LinkEvent) {
        let _ = self.delta_tx.send(event);
    }

    fn delta(&self, session_id: &str, seq: u64) -> LinkEvent {
        LinkEvent::SessionDelta {
            session_id: session_id.to_string(),
            seq,
            kind: SessionDeltaKind::Message,
            payload: serde_json::json!({"from": self.node_id, "seq": seq}),
        }
    }
}

#[async_trait::async_trait]
impl SwarmStateSource for FakeStateSource {
    async fn local_node(&self) -> NodeState {
        let sessions = self.sessions.lock().unwrap().clone();
        NodeState {
            node_id: self.node_id.clone(),
            hostname: self.hostname.clone(),
            mesh_ip: None,
            status: NodeStatus::from_sessions(&sessions),
            sessions_active: sessions.iter().filter(|s| s.live).count() as u32,
            updated_at: *self.updated_at.lock().unwrap(),
        }
    }

    async fn local_sessions(&self) -> Vec<SessionSummary> {
        self.sessions.lock().unwrap().clone()
    }

    fn subscribe_local_deltas(&self) -> BoxStream<'static, LinkEvent> {
        let rx = self.delta_tx.subscribe();
        Box::pin(futures::stream::unfold(rx, |mut rx| async move {
            loop {
                match rx.recv().await {
                    Ok(event) => return Some((event, rx)),
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => return None,
                }
            }
        }))
    }
}

async fn spawn_node(source: Arc<FakeStateSource>, mesh_ip: Option<IpAddr>) -> ControlHandle {
    spawn_node_full(source, mesh_ip, None, true).await
}

async fn spawn_node_full(
    source: Arc<FakeStateSource>,
    mesh_ip: Option<IpAddr>,
    executor: Option<Arc<dyn RemoteExecutor>>,
    allow_remote_execution: bool,
) -> ControlHandle {
    let mut config = ControlConfig::new(TOKEN.to_string(), mesh_ip);
    config.port = 0;
    config.poll_interval = Duration::from_millis(100);
    config.heartbeat_interval = Duration::from_secs(5);
    config.reconnect_backoff = Duration::from_millis(100);
    config.allow_remote_execution = allow_remote_execution;
    ControlService::start(config, source, executor, None)
        .await
        .expect("control service starts")
}

fn mesh_v6() -> Option<IpAddr> {
    Some(IpAddr::V6(Ipv6Addr::LOCALHOST))
}

async fn wait_until<F, Fut>(what: &str, mut probe: F)
where
    F: FnMut() -> Fut,
    Fut: Future<Output = bool>,
{
    let deadline = tokio::time::Instant::now() + DEADLINE;
    loop {
        if probe().await {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for {what}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn base_url(handle: &ControlHandle) -> String {
    format!("http://127.0.0.1:{}", handle.local_addr().port())
}

async fn get_json(client: &reqwest::Client, url: &str) -> serde_json::Value {
    client
        .get(url)
        .bearer_auth(TOKEN)
        .send()
        .await
        .expect("request sends")
        .error_for_status()
        .expect("request succeeds")
        .json()
        .await
        .expect("body parses")
}

async fn connect_stream(handle: &ControlHandle, since: Option<u64>) -> WsClient {
    let mut url = format!(
        "ws://127.0.0.1:{}/v1/swarm/stream?token={TOKEN}",
        handle.local_addr().port()
    );
    if let Some(since) = since {
        url.push_str(&format!("&since={since}"));
    }
    let (ws, _) = tokio_tungstenite::connect_async(url)
        .await
        .expect("ws connects");
    ws
}

/// Next JSON frame, skipping heartbeat pings/pongs.
async fn next_frame(ws: &mut WsClient) -> StreamFrame {
    tokio::time::timeout(DEADLINE, async {
        loop {
            match ws.next().await {
                Some(Ok(TsMessage::Text(text))) => {
                    return serde_json::from_str::<StreamFrame>(&text).expect("frame parses")
                }
                Some(Ok(TsMessage::Ping(_) | TsMessage::Pong(_))) => continue,
                other => panic!("unexpected ws message while waiting for a frame: {other:?}"),
            }
        }
    })
    .await
    .expect("frame arrives within the deadline")
}

// ── nodes/sessions shape + auth ─────────────────────────────────────────

#[tokio::test]
async fn nodes_sessions_shape_and_auth() {
    let source = FakeStateSource::new("node-a");
    source.set_sessions(vec![summary("s1", "node-a", true, 100)]);
    let handle = spawn_node(source.clone(), mesh_v6()).await;
    let base = base_url(&handle);
    let client = reqwest::Client::new();

    assert!(
        matches!(handle.mesh_bind(), MeshBind::Direct(addr) if addr.port() == handle.local_addr().port()),
        "expected a direct mesh bind on ::1, got {:?}",
        handle.mesh_bind()
    );

    // 401: no token, bad bearer, bad query token — on every route.
    for path in ["/v1/swarm/nodes", "/v1/swarm/sessions", "/v1/swarm/stream"] {
        let missing = client.get(format!("{base}{path}")).send().await.unwrap();
        assert_eq!(missing.status(), 401, "{path} without a token");
        let bad_header = client
            .get(format!("{base}{path}"))
            .bearer_auth("wrong")
            .send()
            .await
            .unwrap();
        assert_eq!(bad_header.status(), 401, "{path} with a bad bearer");
        let bad_query = client
            .get(format!("{base}{path}?token=wrong"))
            .send()
            .await
            .unwrap();
        assert_eq!(bad_query.status(), 401, "{path} with a bad ?token");
    }

    // Query-token auth works (the ws-client path).
    let via_query = client
        .get(format!("{base}/v1/swarm/nodes?token={TOKEN}"))
        .send()
        .await
        .unwrap();
    assert_eq!(via_query.status(), 200);

    // Shape asserted on RAW JSON, not through this crate's own types.
    let nodes = get_json(&client, &format!("{base}/v1/swarm/nodes")).await;
    let self_node = nodes.get("self").expect("nodes carries a `self` key");
    assert_eq!(self_node["node_id"], "node-a");
    assert_eq!(self_node["hostname"], "node-a-host");
    assert_eq!(self_node["mesh_ip"], "::1");
    assert_eq!(self_node["sessions_active"], 1);
    assert_eq!(
        self_node["status"],
        serde_json::json!({"type": "Busy", "session_id": "s1"})
    );
    assert!(self_node["updated_at"].is_string());
    assert!(nodes["peers"]
        .as_array()
        .expect("peers is an array")
        .is_empty());

    let sessions = get_json(&client, &format!("{base}/v1/swarm/sessions")).await;
    let list = sessions.as_array().expect("sessions is a bare array");
    assert_eq!(list.len(), 1);
    let s = &list[0];
    for key in [
        "session_id",
        "origin_node_id",
        "working_dir",
        "name",
        "updated_at",
        "message_count",
        "live",
    ] {
        assert!(s.get(key).is_some(), "session summary carries `{key}`");
    }
    assert_eq!(s["session_id"], "s1");
    assert_eq!(s["origin_node_id"], "node-a");
    assert_eq!(s["live"], true);

    handle.shutdown();
}

// ── Busy vs Idle from active sessions ───────────────────────────────────

#[tokio::test]
async fn busy_vs_idle_follows_active_sessions() {
    let source = FakeStateSource::new("node-a");
    let handle = spawn_node(source.clone(), mesh_v6()).await;
    let base = base_url(&handle);
    let client = reqwest::Client::new();

    let nodes = get_json(&client, &format!("{base}/v1/swarm/nodes")).await;
    assert_eq!(nodes["self"]["status"], serde_json::json!({"type": "Idle"}));
    assert_eq!(nodes["self"]["sessions_active"], 0);

    // Two live sessions: Busy carries the most recently updated one.
    source.set_sessions(vec![
        summary("older", "node-a", true, 100),
        summary("newer", "node-a", true, 200),
    ]);
    let nodes = get_json(&client, &format!("{base}/v1/swarm/nodes")).await;
    assert_eq!(
        nodes["self"]["status"],
        serde_json::json!({"type": "Busy", "session_id": "newer"})
    );
    assert_eq!(nodes["self"]["sessions_active"], 2);

    // A dead session alone does not make the node Busy.
    source.set_sessions(vec![summary("done", "node-a", false, 300)]);
    let nodes = get_json(&client, &format!("{base}/v1/swarm/nodes")).await;
    assert_eq!(nodes["self"]["status"], serde_json::json!({"type": "Idle"}));
    assert_eq!(nodes["self"]["sessions_active"], 0);

    handle.shutdown();
}

// ── /stream: delivery, ?since replay, eviction ──────────────────────────

#[tokio::test]
async fn stream_delivers_replays_and_evicts() {
    let source = FakeStateSource::new("node-a");
    let handle = spawn_node(source.clone(), mesh_v6()).await;

    let mut ws = connect_stream(&handle, None).await;
    source.emit(source.delta("s1", 7));
    let first = next_frame(&mut ws).await;
    match &first.event {
        LinkEvent::SessionDelta {
            session_id,
            seq,
            kind,
            payload,
        } => {
            assert_eq!(session_id, "s1");
            assert_eq!(*seq, 7);
            assert_eq!(*kind, SessionDeltaKind::Message);
            assert_eq!(payload["from"], "node-a");
        }
        other => panic!("expected the scripted SessionDelta, got {other:?}"),
    }

    source.emit(source.delta("s1", 8));
    let second = next_frame(&mut ws).await;
    assert_eq!(second.seq, first.seq + 1);
    drop(ws);

    // Replay: a reconnect with ?since=<first> starts at the second frame.
    let mut ws = connect_stream(&handle, Some(first.seq)).await;
    let replayed = next_frame(&mut ws).await;
    assert_eq!(replayed.seq, second.seq);
    assert_eq!(replayed.event, second.event);
    drop(ws);

    // Eviction: overflow the replay buffer, then present the evicted cursor.
    for n in 0..(REPLAY_BUFFER_CAPACITY as u64 + 16) {
        source.emit(source.delta("s1", 100 + n));
    }
    wait_until(
        "the evicted cursor to be refused with a close frame",
        || {
            let handle = &handle;
            async move {
                let mut ws = connect_stream(handle, Some(first.seq)).await;
                match tokio::time::timeout(DEADLINE, ws.next()).await {
                    Ok(Some(Ok(TsMessage::Close(Some(frame))))) => {
                        assert_eq!(u16::from(frame.code), CLOSE_CODE_CLIENT_TOO_FAR_BEHIND);
                        assert_eq!(frame.reason.as_str(), CLOSE_REASON_CLIENT_TOO_FAR_BEHIND);
                        true
                    }
                    // Pump still catching up: the cursor is not evicted yet and the
                    // server replays instead of closing. Try again.
                    Ok(Some(Ok(TsMessage::Text(_)))) => false,
                    other => panic!("expected a close or replay frame, got {other:?}"),
                }
            }
        },
    )
    .await;

    handle.shutdown();
}

// ── Peer fabric: two services, then one goes unreachable ────────────────

#[tokio::test]
async fn peering_folds_remote_state_and_deltas_then_flips_offline() {
    let source_a = FakeStateSource::new("node-a");
    let source_b = FakeStateSource::new("node-b");
    source_b.set_sessions(vec![summary("sb1", "node-b", true, 500)]);

    let b = spawn_node(source_b.clone(), mesh_v6()).await;
    let a = spawn_node(source_a.clone(), mesh_v6()).await;
    let base_a = base_url(&a);
    let client = reqwest::Client::new();

    a.set_peers(vec![PeerTarget {
        hostname: "node-b-host".to_string(),
        mesh_ip: Some("127.0.0.1".to_string()),
        port: b.local_addr().port(),
    }]);

    // A's /nodes learns B's real identity and Busy state via polling.
    wait_until("A to see B Busy on sb1", || {
        let client = &client;
        let base_a = &base_a;
        async move {
            let nodes = get_json(client, &format!("{base_a}/v1/swarm/nodes")).await;
            let peers = nodes["peers"].as_array().unwrap();
            peers.iter().any(|p| {
                p["node_id"] == "node-b"
                    && p["status"] == serde_json::json!({"type": "Busy", "session_id": "sb1"})
            })
        }
    })
    .await;

    // A's /sessions mirrors B's session with its origin intact.
    wait_until("A to mirror B's session", || {
        let client = &client;
        let base_a = &base_a;
        async move {
            let sessions = get_json(client, &format!("{base_a}/v1/swarm/sessions")).await;
            sessions.as_array().unwrap().iter().any(|s| {
                s["session_id"] == "sb1" && s["origin_node_id"] == "node-b" && s["live"] == true
            })
        }
    })
    .await;

    // A delta published on B arrives on A's /stream (the union).
    let mut ws_a = connect_stream(&a, None).await;
    source_b.emit(source_b.delta("sb1", 42));
    tokio::time::timeout(DEADLINE, async {
        loop {
            let frame = next_frame(&mut ws_a).await;
            if let LinkEvent::SessionDelta {
                session_id,
                seq,
                payload,
                ..
            } = &frame.event
            {
                if session_id == "sb1" && *seq == 42 {
                    assert_eq!(payload["from"], "node-b");
                    return;
                }
            }
        }
    })
    .await
    .expect("B's delta reaches A's /stream");
    drop(ws_a);

    // B goes away: A flips it Offline — a state, never an erroring endpoint.
    b.shutdown();
    wait_until("A to flip B Offline", || {
        let client = &client;
        let base_a = &base_a;
        async move {
            let response = client
                .get(format!("{base_a}/v1/swarm/nodes"))
                .bearer_auth(TOKEN)
                .send()
                .await
                .expect("A's endpoint keeps answering");
            assert_eq!(
                response.status(),
                200,
                "peer loss must never 5xx the endpoint"
            );
            let nodes: serde_json::Value = response.json().await.unwrap();
            nodes["peers"].as_array().unwrap().iter().any(|p| {
                p["node_id"] == "node-b" && p["status"] == serde_json::json!({"type": "Offline"})
            })
        }
    })
    .await;

    a.shutdown();
}

// ── Unreachable-from-birth peer: Offline row, endpoint healthy ──────────

#[tokio::test]
async fn unreachable_peer_is_offline_not_an_error() {
    let source = FakeStateSource::new("node-a");
    let handle = spawn_node(source.clone(), mesh_v6()).await;
    let base = base_url(&handle);
    let client = reqwest::Client::new();

    // A port that answers nothing: bind, take the number, drop the listener.
    let dead_port = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    };
    handle.set_peers(vec![PeerTarget {
        hostname: "ghost-host".to_string(),
        mesh_ip: Some("127.0.0.1".to_string()),
        port: dead_port,
    }]);

    // Give the poller time to fail at least once, then assert the contract.
    wait_until("the ghost peer to appear Offline", || {
        let client = &client;
        let base = &base;
        async move {
            let response = client
                .get(format!("{base}/v1/swarm/nodes"))
                .bearer_auth(TOKEN)
                .send()
                .await
                .expect("endpoint answers despite the unreachable peer");
            assert_eq!(response.status(), 200);
            let nodes: serde_json::Value = response.json().await.unwrap();
            nodes["peers"].as_array().unwrap().iter().any(|p| {
                p["hostname"] == "ghost-host"
                    && p["status"] == serde_json::json!({"type": "Offline"})
            })
        }
    })
    .await;

    handle.shutdown();
}

// ── Mesh down: loopback only, self Offline, peers=[] ────────────────────

#[tokio::test]
async fn no_mesh_ip_binds_loopback_only_and_reports_offline() {
    let source = FakeStateSource::new("node-a");
    source.set_sessions(vec![summary("s1", "node-a", true, 100)]);
    let handle = spawn_node(source.clone(), None).await;
    let base = base_url(&handle);
    let client = reqwest::Client::new();

    assert!(
        matches!(handle.mesh_bind(), MeshBind::LoopbackOnly),
        "expected LoopbackOnly, got {:?}",
        handle.mesh_bind()
    );

    let nodes = get_json(&client, &format!("{base}/v1/swarm/nodes")).await;
    // Offline is the mesh verdict even while sessions run — loud, not fabricated Idle/Busy.
    assert_eq!(
        nodes["self"]["status"],
        serde_json::json!({"type": "Offline"})
    );
    assert_eq!(nodes["self"]["mesh_ip"], serde_json::Value::Null);
    assert_eq!(nodes["self"]["sessions_active"], 1);
    assert!(nodes["peers"].as_array().unwrap().is_empty());

    // Local sessions still served — the desktop keeps working with the mesh down.
    let sessions = get_json(&client, &format!("{base}/v1/swarm/sessions")).await;
    assert_eq!(sessions.as_array().unwrap().len(), 1);

    handle.shutdown();
}

// ── POST /v1/swarm/execute: gates + executor seam ───────────────────────

/// A scripted [`RemoteExecutor`]: records the requests it is handed and returns a fixed
/// outcome. Stands in for goose-server's `GoosedRemoteExecutor` — no agent, no model.
struct FakeExecutor {
    recorded: StdMutex<Vec<ExecuteRequest>>,
    outcome: Result<ExecuteAccepted, ExecuteError>,
}

impl FakeExecutor {
    fn accepting(session_id: &str) -> Arc<Self> {
        Arc::new(Self {
            recorded: StdMutex::new(Vec::new()),
            outcome: Ok(ExecuteAccepted {
                session_id: session_id.to_string(),
            }),
        })
    }

    fn failing(error: ExecuteError) -> Arc<Self> {
        Arc::new(Self {
            recorded: StdMutex::new(Vec::new()),
            outcome: Err(error),
        })
    }

    fn call_count(&self) -> usize {
        self.recorded.lock().unwrap().len()
    }
}

#[async_trait::async_trait]
impl RemoteExecutor for FakeExecutor {
    async fn execute(&self, req: ExecuteRequest) -> Result<ExecuteAccepted, ExecuteError> {
        self.recorded.lock().unwrap().push(req);
        self.outcome.clone()
    }
}

async fn post_execute(
    client: &reqwest::Client,
    base: &str,
    bearer: &str,
    body: serde_json::Value,
) -> reqwest::Response {
    client
        .post(format!("{base}/v1/swarm/execute"))
        .bearer_auth(bearer)
        .json(&body)
        .send()
        .await
        .expect("execute request sends")
}

#[tokio::test]
async fn execute_returns_202_when_idle_enabled_and_wired() {
    let source = FakeStateSource::new("node-a"); // no sessions → Idle
    let executor = FakeExecutor::accepting("sess-remote-1");
    let handle = spawn_node_full(source.clone(), mesh_v6(), Some(executor.clone()), true).await;
    let base = base_url(&handle);
    let client = reqwest::Client::new();

    let resp = post_execute(
        &client,
        &base,
        TOKEN,
        serde_json::json!({"prompt": "build me a thing", "working_dir": "/tmp/x"}),
    )
    .await;
    assert_eq!(resp.status(), 202);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["session_id"], "sess-remote-1");

    let recorded = executor.recorded.lock().unwrap();
    assert_eq!(recorded.len(), 1, "the executor ran exactly once");
    assert_eq!(recorded[0].prompt, "build me a thing");
    assert_eq!(recorded[0].working_dir.as_deref(), Some("/tmp/x"));
    assert_eq!(recorded[0].session_id, None);

    handle.shutdown();
}

#[tokio::test]
async fn execute_409_when_node_busy_and_never_calls_executor() {
    let source = FakeStateSource::new("node-a");
    source.set_sessions(vec![summary("s1", "node-a", true, 100)]); // live → Busy
    let executor = FakeExecutor::accepting("unused");
    let handle = spawn_node_full(source.clone(), mesh_v6(), Some(executor.clone()), true).await;
    let base = base_url(&handle);
    let client = reqwest::Client::new();

    let resp = post_execute(&client, &base, TOKEN, serde_json::json!({"prompt": "x"})).await;
    assert_eq!(
        resp.status(),
        409,
        "receive-side idle guard refuses a busy node"
    );
    assert_eq!(
        executor.call_count(),
        0,
        "a busy node never reaches the executor"
    );

    handle.shutdown();
}

#[tokio::test]
async fn execute_403_when_remote_execution_disabled() {
    let source = FakeStateSource::new("node-a");
    let executor = FakeExecutor::accepting("unused");
    let handle = spawn_node_full(source.clone(), mesh_v6(), Some(executor.clone()), false).await;
    let base = base_url(&handle);
    let client = reqwest::Client::new();

    let resp = post_execute(&client, &base, TOKEN, serde_json::json!({"prompt": "x"})).await;
    assert_eq!(resp.status(), 403);
    assert_eq!(executor.call_count(), 0, "observe-only node never executes");

    handle.shutdown();
}

#[tokio::test]
async fn execute_501_when_no_executor_injected() {
    let source = FakeStateSource::new("node-a");
    let handle = spawn_node_full(source.clone(), mesh_v6(), None, true).await;
    let base = base_url(&handle);
    let client = reqwest::Client::new();

    let resp = post_execute(&client, &base, TOKEN, serde_json::json!({"prompt": "x"})).await;
    assert_eq!(
        resp.status(),
        501,
        "no executor wired is loud-absent, never a fake accept"
    );

    handle.shutdown();
}

#[tokio::test]
async fn execute_401_on_bad_bearer_and_never_calls_executor() {
    let source = FakeStateSource::new("node-a");
    let executor = FakeExecutor::accepting("unused");
    let handle = spawn_node_full(source.clone(), mesh_v6(), Some(executor.clone()), true).await;
    let base = base_url(&handle);
    let client = reqwest::Client::new();

    let resp = post_execute(
        &client,
        &base,
        "wrong-token",
        serde_json::json!({"prompt": "x"}),
    )
    .await;
    assert_eq!(resp.status(), 401);
    assert_eq!(executor.call_count(), 0);

    handle.shutdown();
}

#[tokio::test]
async fn execute_maps_executor_internal_error_to_500() {
    let source = FakeStateSource::new("node-a");
    let executor = FakeExecutor::failing(ExecuteError::Internal("session store exploded".into()));
    let handle = spawn_node_full(source.clone(), mesh_v6(), Some(executor.clone()), true).await;
    let base = base_url(&handle);
    let client = reqwest::Client::new();

    let resp = post_execute(&client, &base, TOKEN, serde_json::json!({"prompt": "x"})).await;
    assert_eq!(resp.status(), 500);
    assert_eq!(
        executor.call_count(),
        1,
        "the executor ran and its error mapped"
    );

    handle.shutdown();
}

#[tokio::test]
async fn execute_400_on_unparseable_body() {
    let source = FakeStateSource::new("node-a");
    let executor = FakeExecutor::accepting("unused");
    let handle = spawn_node_full(source.clone(), mesh_v6(), Some(executor.clone()), true).await;
    let base = base_url(&handle);
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{base}/v1/swarm/execute"))
        .bearer_auth(TOKEN)
        .header("content-type", "application/json")
        .body("this is not json")
        .send()
        .await
        .expect("request sends");
    assert_eq!(resp.status(), 400);
    assert_eq!(executor.call_count(), 0);

    handle.shutdown();
}

#[test]
fn execute_error_status_mapping_is_exact() {
    assert_eq!(execute_error_status(&ExecuteError::Busy).as_u16(), 409);
    assert_eq!(execute_error_status(&ExecuteError::Disabled).as_u16(), 403);
    assert_eq!(
        execute_error_status(&ExecuteError::BadRequest("bad".into())).as_u16(),
        400
    );
    assert_eq!(
        execute_error_status(&ExecuteError::Internal("boom".into())).as_u16(),
        500
    );
}

// ── POST /v1/swarm/mlx/<op>: model-management proxy + control seam ───────

/// A scripted [`MlxControl`]: records the (op, request) pairs it is handed and returns a
/// fixed outcome. Stands in for goose's `GoosedMlxControl` — no engine, no sidecar.
struct FakeMlxControl {
    recorded: StdMutex<Vec<(MlxOp, serde_json::Value)>>,
    outcome: Result<serde_json::Value, MlxControlError>,
}

impl FakeMlxControl {
    fn returning(value: serde_json::Value) -> Arc<Self> {
        Arc::new(Self {
            recorded: StdMutex::new(Vec::new()),
            outcome: Ok(value),
        })
    }

    fn failing(error: MlxControlError) -> Arc<Self> {
        Arc::new(Self {
            recorded: StdMutex::new(Vec::new()),
            outcome: Err(error),
        })
    }

    fn call_count(&self) -> usize {
        self.recorded.lock().unwrap().len()
    }
}

#[async_trait::async_trait]
impl MlxControl for FakeMlxControl {
    async fn dispatch(
        &self,
        op: MlxOp,
        request: serde_json::Value,
    ) -> Result<serde_json::Value, MlxControlError> {
        self.recorded.lock().unwrap().push((op, request));
        self.outcome.clone()
    }
}

async fn spawn_node_mlx(
    source: Arc<FakeStateSource>,
    mesh_ip: Option<IpAddr>,
    mlx_control: Option<Arc<dyn MlxControl>>,
) -> ControlHandle {
    let mut config = ControlConfig::new(TOKEN.to_string(), mesh_ip);
    config.port = 0;
    config.poll_interval = Duration::from_millis(100);
    config.heartbeat_interval = Duration::from_secs(5);
    config.reconnect_backoff = Duration::from_millis(100);
    ControlService::start(config, source, None, mlx_control)
        .await
        .expect("control service starts")
}

async fn post_mlx(
    client: &reqwest::Client,
    base: &str,
    bearer: &str,
    op: &str,
    body: serde_json::Value,
) -> reqwest::Response {
    client
        .post(format!("{base}/v1/swarm/mlx/{op}"))
        .bearer_auth(bearer)
        .json(&body)
        .send()
        .await
        .expect("mlx request sends")
}

#[tokio::test]
async fn mlx_proxy_dispatches_to_control_and_returns_its_payload_even_when_busy() {
    // A busy node (a session is live) — model management has NO idle guard, unlike execute.
    let source = FakeStateSource::new("node-a");
    source.set_sessions(vec![summary("s1", "node-a", true, 100)]);
    let payload = serde_json::json!({
        "status": {
            "state": "running",
            "modelId": "org/model",
            "availableMemoryGb": 12.0,
            "totalMemoryGb": 64.0,
            "restartRequired": false
        }
    });
    let control = FakeMlxControl::returning(payload.clone());
    let handle = spawn_node_mlx(source.clone(), mesh_v6(), Some(control.clone())).await;
    let base = base_url(&handle);
    let client = reqwest::Client::new();

    let resp = post_mlx(
        &client,
        &base,
        TOKEN,
        "status",
        serde_json::json!({"nodeId": "node-a"}),
    )
    .await;
    assert_eq!(
        resp.status(),
        200,
        "a busy node still serves model management"
    );
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        body, payload,
        "the control's response DTO is returned verbatim"
    );

    let recorded = control.recorded.lock().unwrap();
    assert_eq!(recorded.len(), 1, "the control ran exactly once");
    assert_eq!(
        recorded[0].0,
        MlxOp::Status,
        "the op was routed from the path"
    );
    assert_eq!(
        recorded[0].1["nodeId"], "node-a",
        "the request body passed through"
    );

    handle.shutdown();
}

#[tokio::test]
async fn mlx_proxy_501_when_no_control_injected() {
    let source = FakeStateSource::new("node-a");
    let handle = spawn_node_mlx(source.clone(), mesh_v6(), None).await;
    let base = base_url(&handle);
    let client = reqwest::Client::new();

    let resp = post_mlx(&client, &base, TOKEN, "status", serde_json::json!({})).await;
    assert_eq!(
        resp.status(),
        501,
        "no mlx control wired is loud-absent, never a fabricated result"
    );

    handle.shutdown();
}

#[tokio::test]
async fn mlx_proxy_401_on_bad_bearer_and_never_calls_control() {
    let source = FakeStateSource::new("node-a");
    let control = FakeMlxControl::returning(serde_json::json!({}));
    let handle = spawn_node_mlx(source.clone(), mesh_v6(), Some(control.clone())).await;
    let base = base_url(&handle);
    let client = reqwest::Client::new();

    let resp = post_mlx(
        &client,
        &base,
        "wrong-token",
        "modelDelete",
        serde_json::json!({"modelId": "x"}),
    )
    .await;
    assert_eq!(resp.status(), 401);
    assert_eq!(
        control.call_count(),
        0,
        "auth is checked before the control seam"
    );

    handle.shutdown();
}

#[tokio::test]
async fn mlx_proxy_surfaces_a_peer_failure_verbatim() {
    let source = FakeStateSource::new("node-a");

    // A node's own internal failure (disk read) → 500 with the text intact.
    let failing = FakeMlxControl::failing(MlxControlError::Failed(
        "reading local models failed: permission denied".to_string(),
    ));
    let handle = spawn_node_mlx(source.clone(), mesh_v6(), Some(failing)).await;
    let base = base_url(&handle);
    let client = reqwest::Client::new();
    let resp = post_mlx(&client, &base, TOKEN, "modelsList", serde_json::json!({})).await;
    assert_eq!(resp.status(), 500);
    let text = resp.text().await.unwrap();
    assert!(
        text.contains("reading local models failed: permission denied"),
        "the peer's failure text is surfaced verbatim, got: {text}"
    );
    handle.shutdown();

    // A memory-gate BLOCK on mount is the local invalid_params class → 400, text intact.
    let blocked = FakeMlxControl::failing(MlxControlError::BadRequest(
        "memory gate BLOCK: model needs 40GB, 12GB free".to_string(),
    ));
    let handle = spawn_node_mlx(source, mesh_v6(), Some(blocked)).await;
    let base = base_url(&handle);
    let resp = post_mlx(
        &client,
        &base,
        TOKEN,
        "mount",
        serde_json::json!({"modelId": "org/big"}),
    )
    .await;
    assert_eq!(resp.status(), 400);
    let text = resp.text().await.unwrap();
    assert!(
        text.contains("memory gate BLOCK: model needs 40GB, 12GB free"),
        "the gate BLOCK is surfaced verbatim, got: {text}"
    );
    handle.shutdown();
}

#[tokio::test]
async fn mlx_proxy_unknown_op_is_404() {
    let source = FakeStateSource::new("node-a");
    let control = FakeMlxControl::returning(serde_json::json!({}));
    let handle = spawn_node_mlx(source.clone(), mesh_v6(), Some(control.clone())).await;
    let base = base_url(&handle);
    let client = reqwest::Client::new();

    let resp = post_mlx(&client, &base, TOKEN, "bogusOp", serde_json::json!({})).await;
    assert_eq!(
        resp.status(),
        404,
        "an unknown op is loud, not a silent no-op"
    );
    assert_eq!(control.call_count(), 0);

    handle.shutdown();
}

#[tokio::test]
async fn mlx_proxy_400_on_unparseable_body() {
    let source = FakeStateSource::new("node-a");
    let control = FakeMlxControl::returning(serde_json::json!({}));
    let handle = spawn_node_mlx(source.clone(), mesh_v6(), Some(control.clone())).await;
    let base = base_url(&handle);
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{base}/v1/swarm/mlx/download"))
        .bearer_auth(TOKEN)
        .header("content-type", "application/json")
        .body("this is not json")
        .send()
        .await
        .expect("request sends");
    assert_eq!(resp.status(), 400);
    assert_eq!(control.call_count(), 0);

    handle.shutdown();
}

#[test]
fn mlx_control_error_status_mapping_is_exact() {
    assert_eq!(
        mlx_control_error_status(&MlxControlError::BadRequest("bad".into())).as_u16(),
        400
    );
    assert_eq!(
        mlx_control_error_status(&MlxControlError::Failed("boom".into())).as_u16(),
        500
    );
}
