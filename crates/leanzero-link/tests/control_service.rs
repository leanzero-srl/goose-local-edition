//! Integration tests for the `/v1/swarm` control service: a scriptable
//! `FakeStateSource` + real axum services on ephemeral loopback ports, driven
//! with reqwest and a tokio-tungstenite ws client (dev-side only). No goosed,
//! no tailscaled, no tailnet contact — the mesh crate's own tests own that. Peer
//! fabric traffic goes through [`support::fake_tailnet`], a SOCKS5 stand-in for the
//! daemon's proxy: peers carry mesh-looking IPs only that proxy can reach.

mod support;

use std::future::Future;
use std::net::{IpAddr, Ipv6Addr};
use std::sync::atomic::{AtomicBool, Ordering};
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
    LeaveReason, LinkEvent, NodeState, NodeStatus, SessionDeltaKind, SessionSummary, StreamFrame,
    SwarmNodesResponse,
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
    /// When set, `local_sessions` fails with this text — the session store is
    /// unreadable right now. `local_node` keeps answering from the live set.
    store_error: StdMutex<Option<String>>,
    delta_tx: broadcast::Sender<LinkEvent>,
    /// How many times `/nodes` read this node — a peer's polls, counted.
    node_reads: std::sync::atomic::AtomicUsize,
}

impl FakeStateSource {
    fn new(node_id: &str) -> Arc<Self> {
        let (delta_tx, _) = broadcast::channel(2048);
        Arc::new(Self {
            node_id: node_id.to_string(),
            hostname: format!("{node_id}-host"),
            updated_at: StdMutex::new(ts(1_700_000_000)),
            sessions: StdMutex::new(Vec::new()),
            store_error: StdMutex::new(None),
            delta_tx,
            node_reads: std::sync::atomic::AtomicUsize::new(0),
        })
    }

    fn set_sessions(&self, sessions: Vec<SessionSummary>) {
        *self.sessions.lock().unwrap() = sessions;
        *self.updated_at.lock().unwrap() = Utc::now();
    }

    fn set_store_error(&self, error: Option<&str>) {
        *self.store_error.lock().unwrap() = error.map(str::to_string);
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
        self.node_reads.fetch_add(1, Ordering::SeqCst);
        let sessions = self.sessions.lock().unwrap().clone();
        NodeState {
            node_id: self.node_id.clone(),
            hostname: self.hostname.clone(),
            mesh_ip: None,
            status: NodeStatus::from_sessions(&sessions),
            sessions_active: sessions.iter().filter(|s| s.live).count() as u32,
            updated_at: *self.updated_at.lock().unwrap(),
            last_poll_error: None,
            computer_name: None,
            allows: None,
            leaving: None,
            load: None,
        }
    }

    async fn local_sessions(&self) -> Result<Vec<SessionSummary>, String> {
        if let Some(error) = self.store_error.lock().unwrap().clone() {
            return Err(error);
        }
        Ok(self.sessions.lock().unwrap().clone())
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
    config.peer_proxy = Some(support::fake_tailnet().proxy());
    ControlService::start(config, source, executor, None, None, None)
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

// ── R-M6: an Origin header is a browser context — refused outright ──────

#[tokio::test]
async fn any_request_carrying_an_origin_header_is_403_even_with_a_valid_token() {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;

    let source = FakeStateSource::new("node-a");
    let executor = FakeExecutor::accepting("unused");
    let handle = spawn_node_full(source.clone(), mesh_v6(), Some(executor.clone()), true).await;
    let base = base_url(&handle);
    let client = reqwest::Client::new();

    // GET with the correct bearer + Origin → 403 (a page fetching the mirror).
    for path in ["/v1/swarm/nodes", "/v1/swarm/sessions"] {
        let resp = client
            .get(format!("{base}{path}"))
            .bearer_auth(TOKEN)
            .header("origin", "http://evil.example")
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 403, "{path} with an Origin header");
    }
    // The ?token= form (the ws-client path) gets no exemption either.
    let resp = client
        .get(format!("{base}/v1/swarm/nodes?token={TOKEN}"))
        .header("origin", "http://127.0.0.1:41226")
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        403,
        "same-origin-looking Origin is still a browser"
    );

    // POST /execute with Origin → 403 and the executor never runs.
    let resp = client
        .post(format!("{base}/v1/swarm/execute"))
        .bearer_auth(TOKEN)
        .header("origin", "null")
        .json(&serde_json::json!({"prompt": "x"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 403);
    assert_eq!(executor.call_count(), 0);

    // The WebSocket upgrade with Origin → the handshake itself fails with 403.
    let url = format!(
        "ws://127.0.0.1:{}/v1/swarm/stream?token={TOKEN}",
        handle.local_addr().port()
    );
    let mut request = url.clone().into_client_request().unwrap();
    request
        .headers_mut()
        .insert("origin", "http://evil.example".parse().unwrap());
    match tokio_tungstenite::connect_async(request).await {
        Err(tokio_tungstenite::tungstenite::Error::Http(response)) => {
            assert_eq!(response.status(), 403, "ws upgrade with Origin refused")
        }
        Ok(_) => panic!("a browser-shaped ws upgrade must not be accepted"),
        Err(other) => panic!("expected an HTTP 403 handshake failure, got {other:?}"),
    }

    // Negative control: the same requests WITHOUT Origin succeed (peer / native shape).
    let resp = client
        .get(format!("{base}/v1/swarm/nodes"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let (ws, _) = tokio_tungstenite::connect_async(url)
        .await
        .expect("a native ws client (no Origin) still connects");
    drop(ws);

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

    let b_mesh_ip = support::fake_tailnet().expose(b.local_addr().port());
    a.set_peers(vec![PeerTarget {
        hostname: "node-b-host".to_string(),
        mesh_ip: Some(b_mesh_ip.clone()),
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
    assert!(
        support::fake_tailnet().connects_to(&b_mesh_ip) > 0,
        "A reached B's mesh IP through the mesh proxy (no host route exists to it)"
    );

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

    // A delta published on B arrives on A's /stream (the union) — B's delta travels
    // A's `/stream` subscription, a WebSocket dialed through the same proxy.
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

// ── R-M2: an unreadable session index is 503, never `[]` ────────────────

#[tokio::test]
async fn sessions_answers_503_when_the_index_is_unreadable_never_an_empty_list() {
    let source = FakeStateSource::new("node-a");
    source.set_sessions(vec![summary("s1", "node-a", true, 100)]);
    let handle = spawn_node(source.clone(), mesh_v6()).await;
    let base = base_url(&handle);
    let client = reqwest::Client::new();

    source.set_store_error(Some("sqlx: database is locked (SQLITE_BUSY)"));
    for path in ["/v1/swarm/sessions", "/v1/swarm/sessions?scope=local"] {
        let resp = client
            .get(format!("{base}{path}"))
            .bearer_auth(TOKEN)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 503, "{path} over an unreadable store");
        let text = resp.text().await.unwrap();
        assert_eq!(
            text,
            "session index unreadable: sqlx: database is locked (SQLITE_BUSY)"
        );
    }
    // /nodes stays truthful: the busy set is store-independent.
    let nodes = get_json(&client, &format!("{base}/v1/swarm/nodes")).await;
    assert_eq!(
        nodes["self"]["status"],
        serde_json::json!({"type": "Busy", "session_id": "s1"})
    );

    // Recovery: the same request answers the real list, not a cached empty one.
    source.set_store_error(None);
    let sessions = get_json(&client, &format!("{base}/v1/swarm/sessions")).await;
    assert_eq!(sessions.as_array().unwrap().len(), 1);
    handle.shutdown();
}

#[tokio::test]
async fn execute_answers_503_when_the_index_is_unreadable_and_never_calls_the_executor() {
    let source = FakeStateSource::new("node-a"); // Idle — the guard alone would admit
    let executor = FakeExecutor::accepting("unused");
    let handle = spawn_node_full(source.clone(), mesh_v6(), Some(executor.clone()), true).await;
    let base = base_url(&handle);
    let client = reqwest::Client::new();

    source.set_store_error(Some("sqlx: pool timed out"));
    let resp = post_execute(&client, &base, TOKEN, serde_json::json!({"prompt": "x"})).await;
    assert_eq!(resp.status(), 503);
    assert_eq!(
        resp.text().await.unwrap(),
        "session index unreadable: sqlx: pool timed out"
    );
    assert_eq!(
        executor.call_count(),
        0,
        "a node that cannot see its own sessions takes no work"
    );

    source.set_store_error(None);
    let resp = post_execute(&client, &base, TOKEN, serde_json::json!({"prompt": "x"})).await;
    assert_eq!(resp.status(), 202, "negative control: readable → accepted");
    handle.shutdown();
}

/// A peer whose `/sessions` answers 503 is alive and identified: its node state is
/// folded, its MIRROR IS KEPT (never purged over an answer that carried no sessions),
/// and the failure rides in `last_poll_error` until the index is readable again.
#[tokio::test]
async fn peer_keeps_its_mirror_while_the_peers_index_is_unreadable() {
    let source_a = FakeStateSource::new("node-a");
    let source_b = FakeStateSource::new("node-b");
    source_b.set_sessions(vec![summary("sb1", "node-b", true, 500)]);
    let b = spawn_node(source_b.clone(), mesh_v6()).await;
    let a = spawn_node(source_a.clone(), mesh_v6()).await;
    let base_a = base_url(&a);
    let client = reqwest::Client::new();
    a.set_peers(vec![PeerTarget {
        hostname: "node-b-host".to_string(),
        mesh_ip: Some(support::fake_tailnet().expose(b.local_addr().port())),
        port: b.local_addr().port(),
    }]);

    wait_until("A to mirror B's session", || {
        let client = &client;
        let base_a = &base_a;
        async move {
            let sessions = get_json(client, &format!("{base_a}/v1/swarm/sessions")).await;
            sessions
                .as_array()
                .unwrap()
                .iter()
                .any(|s| s["session_id"] == "sb1")
        }
    })
    .await;

    // B's store goes unreadable: B's /sessions → 503; B's /nodes still says Busy.
    source_b.set_store_error(Some("sqlx: database is locked (SQLITE_BUSY)"));
    wait_until("A to record B's 503 without dropping the mirror", || {
        let client = &client;
        let base_a = &base_a;
        async move {
            let nodes = get_json(client, &format!("{base_a}/v1/swarm/nodes")).await;
            peer_row(&nodes, "node-b").is_some_and(|p| {
                p["last_poll_error"]
                    .as_str()
                    .is_some_and(|e| e.contains("503") && e.contains("session index unreadable"))
                    && p["status"] == serde_json::json!({"type": "Busy", "session_id": "sb1"})
            })
        }
    })
    .await;
    // Several more polls (100 ms each) — the mirror must survive every one of them.
    tokio::time::sleep(Duration::from_millis(400)).await;
    let sessions = get_json(&client, &format!("{base_a}/v1/swarm/sessions")).await;
    assert!(
        sessions
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["session_id"] == "sb1" && s["origin_node_id"] == "node-b"),
        "the mirror of B's session was purged over a 503: {sessions}"
    );

    // Recovery clears the record.
    source_b.set_store_error(None);
    wait_until("the poll error to clear once B's index is readable", || {
        let client = &client;
        let base_a = &base_a;
        async move {
            let nodes = get_json(client, &format!("{base_a}/v1/swarm/nodes")).await;
            peer_row(&nodes, "node-b").is_some_and(|p| p.get("last_poll_error").is_none())
        }
    })
    .await;

    b.shutdown();
    a.shutdown();
}

// ── FH#12: a peer that ANSWERS 4xx is alive — status kept, error recorded ──

/// A minimal stand-in for a peer whose control service answers, but — once
/// `unauthorized` flips — with `401` (a token-derivation skew between nodes). Not this
/// crate's service on purpose: the point is what the POLLER does with an answer it
/// cannot use.
#[derive(Clone)]
struct StubPeer {
    node: NodeState,
    unauthorized: Arc<AtomicBool>,
}

async fn stub_nodes(
    axum::extract::State(stub): axum::extract::State<StubPeer>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    if stub.unauthorized.load(Ordering::SeqCst) {
        return (axum::http::StatusCode::UNAUTHORIZED, "token mismatch").into_response();
    }
    axum::Json(SwarmNodesResponse {
        self_node: stub.node.clone(),
        peers: Vec::new(),
    })
    .into_response()
}

async fn stub_sessions(
    axum::extract::State(stub): axum::extract::State<StubPeer>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    if stub.unauthorized.load(Ordering::SeqCst) {
        return (axum::http::StatusCode::UNAUTHORIZED, "token mismatch").into_response();
    }
    axum::Json(Vec::<SessionSummary>::new()).into_response()
}

/// Serve the stub on an ephemeral loopback port; aborting the returned task closes it.
/// Makes the stub vanish the way a quit peer does: the listener AND every open connection close. Aborting
/// the serve task alone stops only the accept loop — axum serves each keep-alive connection on its own
/// task, so the poller's pooled connection kept getting 401s and the "vanished" peer never went Offline
/// (a CI flake on a loaded runner, 2026-09-26).
struct StubPeerHandle {
    shutdown: tokio::sync::oneshot::Sender<()>,
    task: tokio::task::JoinHandle<()>,
}

impl StubPeerHandle {
    async fn vanish(self) {
        let _ = self.shutdown.send(());
        self.task
            .await
            .expect("the stub's server ends once its connections close");
    }
}

async fn spawn_stub_peer(node_id: &str, unauthorized: Arc<AtomicBool>) -> (u16, StubPeerHandle) {
    let stub = StubPeer {
        node: NodeState {
            node_id: node_id.to_string(),
            hostname: format!("{node_id}-host"),
            mesh_ip: Some("127.0.0.1".to_string()),
            status: NodeStatus::Idle,
            sessions_active: 0,
            updated_at: ts(1_700_000_000),
            last_poll_error: None,
            computer_name: None,
            allows: None,
            leaving: None,
            load: None,
        },
        unauthorized,
    };
    let router = axum::Router::new()
        .route("/v1/swarm/nodes", axum::routing::get(stub_nodes))
        .route("/v1/swarm/sessions", axum::routing::get(stub_sessions))
        .with_state(stub);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (shutdown, stopped) = tokio::sync::oneshot::channel::<()>();
    let task = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                let _ = stopped.await;
            })
            .await
            .unwrap();
    });
    (port, StubPeerHandle { shutdown, task })
}

fn peer_row(nodes: &serde_json::Value, node_id: &str) -> Option<serde_json::Value> {
    nodes["peers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["node_id"] == node_id)
        .cloned()
}

#[tokio::test]
async fn a_peer_answering_4xx_keeps_its_last_status_and_carries_the_error() {
    let source = FakeStateSource::new("node-a");
    let handle = spawn_node(source.clone(), mesh_v6()).await;
    let base = base_url(&handle);
    let client = reqwest::Client::new();

    let unauthorized = Arc::new(AtomicBool::new(false));
    let (stub_port, stub) = spawn_stub_peer("node-stub", unauthorized.clone()).await;
    handle.set_peers(vec![PeerTarget {
        hostname: "node-stub-host".to_string(),
        mesh_ip: Some(support::fake_tailnet().expose(stub_port)),
        port: stub_port,
    }]);

    // Healthy first: the poller learns the stub's identity and Idle status.
    wait_until("A to see the stub Idle", || {
        let client = &client;
        let base = &base;
        async move {
            let nodes = get_json(client, &format!("{base}/v1/swarm/nodes")).await;
            peer_row(&nodes, "node-stub").is_some_and(|p| {
                p["status"] == serde_json::json!({"type": "Idle"})
                    && p.get("last_poll_error").is_none()
            })
        }
    })
    .await;

    // The stub starts answering 401: it is ALIVE — status stays Idle, the error rides.
    unauthorized.store(true, Ordering::SeqCst);
    wait_until("the 401 to be recorded without flipping Offline", || {
        let client = &client;
        let base = &base;
        async move {
            let nodes = get_json(client, &format!("{base}/v1/swarm/nodes")).await;
            peer_row(&nodes, "node-stub").is_some_and(|p| {
                p["last_poll_error"]
                    .as_str()
                    .is_some_and(|e| e.contains("401") && e.contains("token mismatch"))
            })
        }
    })
    .await;
    let nodes = get_json(&client, &format!("{base}/v1/swarm/nodes")).await;
    let row = peer_row(&nodes, "node-stub").unwrap();
    assert_eq!(
        row["status"],
        serde_json::json!({"type": "Idle"}),
        "an answering peer is never fabricated Offline: {row}"
    );

    // Negative control: a TRANSPORT failure (the stub is gone) is Offline, text kept — the
    // transport error naming the stub's URL, not the 401 answer before it. Q-420: this once read
    // `!e.contains("401")`, and the transport text carries the stub's ephemeral PORT, so a port
    // like 40117 or 54012 held the check false for good and the test timed out on a correct row.
    stub.vanish().await;
    let stub_url = format!(":{stub_port}/v1/swarm/");
    wait_until("the vanished stub to flip Offline", || {
        let client = &client;
        let base = &base;
        let stub_url = &stub_url;
        async move {
            let nodes = get_json(client, &format!("{base}/v1/swarm/nodes")).await;
            peer_row(&nodes, "node-stub").is_some_and(|p| {
                p["status"] == serde_json::json!({"type": "Offline"})
                    && p["last_poll_error"].as_str().is_some_and(|e| {
                        e.contains(stub_url.as_str())
                            && !e.contains(" answered ")
                            && !e.contains("token mismatch")
                    })
            })
        }
    })
    .await;

    handle.shutdown();
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
        mesh_ip: Some(support::fake_tailnet().expose(dead_port)),
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

// ── No mesh proxy: a peer is never dialed directly ─────────────────────

/// A service started with NO mesh proxy must not fall back to a direct dial — even when
/// the peer IS directly reachable (negative control: a plain client reaches B). The peer
/// shows `Offline` with the named refusal on record.
#[tokio::test]
async fn without_a_mesh_proxy_a_reachable_peer_is_offline_never_dialed_directly() {
    let b = spawn_node(FakeStateSource::new("node-b"), mesh_v6()).await;
    let client = reqwest::Client::new();
    let direct = get_json(&client, &format!("{}/v1/swarm/nodes", base_url(&b))).await;
    assert_eq!(
        direct["self"]["node_id"], "node-b",
        "negative control: B answers a direct dial on loopback"
    );

    let mut config = ControlConfig::new(TOKEN.to_string(), mesh_v6());
    config.port = 0;
    config.poll_interval = Duration::from_millis(100);
    config.reconnect_backoff = Duration::from_millis(100);
    assert!(config.peer_proxy.is_none(), "the template carries no proxy");
    let a = ControlService::start(
        config,
        FakeStateSource::new("node-a"),
        None,
        None,
        None,
        None,
    )
    .await
    .expect("A starts");
    a.set_peers(vec![PeerTarget {
        hostname: "node-b-host".to_string(),
        mesh_ip: Some("127.0.0.1".to_string()),
        port: b.local_addr().port(),
    }]);

    let base_a = base_url(&a);
    wait_until("A to record the refused dial", || {
        let client = &client;
        let base_a = &base_a;
        async move {
            let nodes = get_json(client, &format!("{base_a}/v1/swarm/nodes")).await;
            nodes["peers"].as_array().unwrap().iter().any(|p| {
                p["hostname"] == "node-b-host"
                    && p["status"] == serde_json::json!({"type": "Offline"})
                    && p["last_poll_error"]
                        .as_str()
                        .is_some_and(|e| e.contains("refusing a direct dial"))
            })
        }
    })
    .await;
    let nodes = get_json(&client, &format!("{base_a}/v1/swarm/nodes")).await;
    assert!(
        !nodes["peers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["node_id"] == "node-b"),
        "B's self-reported identity was never folded — no poll reached it"
    );

    a.shutdown();
    b.shutdown();
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
    spawn_node_mlx_allowing(source, mesh_ip, mlx_control, true).await
}

async fn spawn_node_mlx_allowing(
    source: Arc<FakeStateSource>,
    mesh_ip: Option<IpAddr>,
    mlx_control: Option<Arc<dyn MlxControl>>,
    allow_remote_execution: bool,
) -> ControlHandle {
    let mut config = ControlConfig::new(TOKEN.to_string(), mesh_ip);
    config.port = 0;
    config.poll_interval = Duration::from_millis(100);
    config.heartbeat_interval = Duration::from_secs(5);
    config.reconnect_backoff = Duration::from_millis(100);
    config.allow_remote_execution = allow_remote_execution;
    ControlService::start(config, source, None, mlx_control, None, None)
        .await
        .expect("control service starts")
}

/// The switch is OFF unless the host passes the user's own setting: a freshly built
/// config is observe-only for both acting surfaces.
#[test]
fn control_config_defaults_to_observe_only() {
    let config = ControlConfig::new(TOKEN.to_string(), None);
    assert!(
        !config.allow_remote_execution,
        "allow_remote_execution must default to false (observe-only)"
    );
}

#[test]
fn destructive_mlx_ops_include_settings_update_and_unmount() {
    for op in [
        MlxOp::ModelDelete,
        MlxOp::DownloadCancel,
        MlxOp::SettingsUpdate,
        MlxOp::Unmount,
        MlxOp::ReplicaCancel,
    ] {
        assert!(op.is_destructive(), "{op:?} reshapes the node; must warn");
    }
    for op in [
        MlxOp::Status,
        MlxOp::SettingsRead,
        MlxOp::ModelsList,
        MlxOp::DownloadProgress,
        MlxOp::LinkFacts,
        MlxOp::ReplicaTargets,
        MlxOp::ReplicaProgress,
    ] {
        assert!(!op.is_destructive(), "{op:?} is a read");
    }
}

#[test]
fn every_mlx_op_path_round_trips_and_is_unique() {
    let mut seen = std::collections::HashSet::new();
    for op in MlxOp::ALL {
        assert_eq!(MlxOp::from_path(op.path()), Some(op), "{op:?}");
        assert!(seen.insert(op.path()), "duplicate path {}", op.path());
    }
    assert_eq!(MlxOp::from_path("replicaPull"), Some(MlxOp::ReplicaPull));
    assert_eq!(MlxOp::from_path("replica_pull"), None);
}

#[tokio::test]
async fn mlx_proxy_403_when_remote_execution_disabled_and_never_calls_control() {
    let source = FakeStateSource::new("node-a");
    let control = FakeMlxControl::returning(serde_json::json!({"unused": true}));
    // The DEFAULT config (allow_remote_execution = false), exactly what a node gets when
    // the host has not opted it in.
    let handle =
        spawn_node_mlx_allowing(source.clone(), mesh_v6(), Some(control.clone()), false).await;
    let base = base_url(&handle);
    let client = reqwest::Client::new();

    // A read AND a destructive write: both refused; the observe-only switch is not
    // per-op.
    for (op, body) in [
        ("status", serde_json::json!({})),
        ("modelDelete", serde_json::json!({"modelId": "org/model"})),
    ] {
        let resp = post_mlx(&client, &base, TOKEN, op, body).await;
        assert_eq!(resp.status(), 403, "{op} on an observe-only node");
        let text = resp.text().await.unwrap();
        assert_eq!(text, "remote model management is disabled on this node");
    }
    assert_eq!(
        control.call_count(),
        0,
        "the control seam is never reached on an observe-only node"
    );

    // Negative control: the same node with the switch on serves the read.
    handle.shutdown();
    let handle = spawn_node_mlx_allowing(source, mesh_v6(), Some(control.clone()), true).await;
    let base = base_url(&handle);
    let resp = post_mlx(&client, &base, TOKEN, "status", serde_json::json!({})).await;
    assert_eq!(resp.status(), 200);
    assert_eq!(control.call_count(), 1);
    handle.shutdown();
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

// ── Q-51: a peer that goes away ON PURPOSE says so; the receiver knows at once ──

async fn post_leaving(
    client: &reqwest::Client,
    base: &str,
    token: Option<&str>,
    body: serde_json::Value,
) -> (reqwest::StatusCode, String) {
    let mut request = client
        .post(format!("{base}/v1/swarm/peer-leaving"))
        .json(&body);
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    let response = request.send().await.expect("request sends");
    let status = response.status();
    (status, response.text().await.unwrap_or_default())
}

/// A (the MacBook) polls B (the Studio). B's goose quits: it marks itself leaving and tells A.
/// A's row for B is `Offline` + `leaving` in the very next read — no poll waited on — and the
/// change is on A's `/stream`. While B's own report still says it is leaving, A's polls keep
/// the mark; B's relaunched goose (a fresh service at the same mesh address, not leaving)
/// clears it on A's next poll.
#[tokio::test]
async fn a_peer_leaving_on_purpose_is_marked_at_once_and_cleared_when_it_answers_again() {
    let a = spawn_node(FakeStateSource::new("node-a"), mesh_v6()).await;
    let source_b = FakeStateSource::new("node-b");
    let b = spawn_node(source_b.clone(), mesh_v6()).await;
    let base_a = base_url(&a);
    let client = reqwest::Client::new();
    let b_port = b.local_addr().port();
    let b_ip = support::fake_tailnet().expose(b_port);
    a.set_peers(vec![PeerTarget {
        hostname: "node-b-host".to_string(),
        mesh_ip: Some(b_ip.clone()),
        port: b_port,
    }]);
    wait_until("A to see B Idle", || {
        let (client, base_a) = (&client, &base_a);
        async move {
            let nodes = get_json(client, &format!("{base_a}/v1/swarm/nodes")).await;
            peer_row(&nodes, "node-b")
                .is_some_and(|p| p["status"] == serde_json::json!({"type": "Idle"}))
        }
    })
    .await;
    let mut ws_a = connect_stream(&a, None).await;

    // The route's refusals first: no bearer 401, an unknown peer 404, a bad body 400.
    let notice = serde_json::json!({
        "node_id": "node-b", "hostname": "node-b-host", "mesh_ip": b_ip, "reason": "quitting"
    });
    let (status, _) = post_leaving(&client, &base_a, None, notice.clone()).await;
    assert_eq!(status, 401);
    let (status, body) = post_leaving(
        &client,
        &base_a,
        Some(TOKEN),
        serde_json::json!({"node_id": "node-z", "hostname": "z", "mesh_ip": null, "reason": "quitting"}),
    )
    .await;
    assert_eq!(status, 404, "{body}");
    assert!(body.contains("node-z"), "{body}");
    let (status, _) = post_leaving(
        &client,
        &base_a,
        Some(TOKEN),
        serde_json::json!({"node_id": "node-b", "reason": "exploding"}),
    )
    .await;
    assert_eq!(status, 400);
    let nodes = get_json(&client, &format!("{base_a}/v1/swarm/nodes")).await;
    assert!(
        peer_row(&nodes, "node-b").unwrap().get("leaving").is_none(),
        "a refused notice marks nothing"
    );

    // B leaves: its own report says so from here on, then it tells A.
    b.mark_leaving(LeaveReason::Quitting);
    let b_nodes = get_json(&client, &format!("{}/v1/swarm/nodes", base_url(&b))).await;
    assert_eq!(b_nodes["self"]["leaving"]["reason"], "quitting");
    let (status, body) = post_leaving(&client, &base_a, Some(TOKEN), notice).await;
    assert_eq!(status, 204, "{body}");

    let nodes = get_json(&client, &format!("{base_a}/v1/swarm/nodes")).await;
    let row = peer_row(&nodes, "node-b").unwrap();
    assert_eq!(
        row["status"],
        serde_json::json!({"type": "Offline"}),
        "{row}"
    );
    assert_eq!(row["leaving"]["reason"], "quitting", "{row}");
    assert_eq!(row["last_poll_error"], "it said it quit goose", "{row}");
    tokio::time::timeout(DEADLINE, async {
        loop {
            if let LinkEvent::NodeStateChanged(node) = next_frame(&mut ws_a).await.event {
                if node.node_id == "node-b" && node.leaving.is_some() {
                    assert_eq!(node.status, NodeStatus::Offline);
                    return;
                }
            }
        }
    })
    .await
    .expect("A's /stream carries B leaving");

    // B still answers (its Link has not stopped yet), reporting itself leaving: A's polls —
    // 100 ms apart — keep the mark.
    let polls_before = source_b.node_reads.load(Ordering::SeqCst);
    wait_until("A to poll B three more times after the notice", || async {
        source_b.node_reads.load(Ordering::SeqCst) >= polls_before + 3
    })
    .await;
    let nodes = get_json(&client, &format!("{base_a}/v1/swarm/nodes")).await;
    let row = peer_row(&nodes, "node-b").unwrap();
    assert_eq!(
        row["status"],
        serde_json::json!({"type": "Offline"}),
        "{row}"
    );
    assert_eq!(row["leaving"]["reason"], "quitting", "{row}");

    // B's goose comes back: a fresh service behind the same mesh address, not leaving.
    b.shutdown();
    let b2 = spawn_node(FakeStateSource::new("node-b"), mesh_v6()).await;
    support::fake_tailnet().revive(&b_ip, b_port, b2.local_addr().port());
    wait_until("A to clear B's mark once B answers as itself", || {
        let (client, base_a) = (&client, &base_a);
        async move {
            let nodes = get_json(client, &format!("{base_a}/v1/swarm/nodes")).await;
            peer_row(&nodes, "node-b").is_some_and(|p| {
                p["status"] == serde_json::json!({"type": "Idle"}) && p.get("leaving").is_none()
            })
        }
    })
    .await;

    b2.shutdown();
    a.shutdown();
}

// ---------------------------------------------------------------------------------------------
// /v1/swarm/jobs over HTTP (step 2): every refusal code, group cancel, ledger content, the
// watchdog's timeout and memory kill, and the shared heavy-run lock in both directions.

mod jobs_http {
    use super::*;
    use leanzero_link::jobs::{JobsConfig, JobsSwitch, UserActivePolicy};
    use leanzero_link::load::{HeavySlot, NodeLimits, OwnLoad};
    use std::path::{Path, PathBuf};

    /// A calm, idle, plugged-in 128 GB machine.
    fn calm() -> OwnLoad {
        OwnLoad {
            user_idle_s: Some(9999.0),
            user_active: false,
            cpu_load_per_core: Some(0.1),
            ncpu: 16,
            mem_free_pct: Some(80.0),
            mem_total_gb: Some(128.0),
            on_ac: Some(true),
            thermal_limit: Some(0),
            ..Default::default()
        }
    }

    struct Node {
        handle: ControlHandle,
        base: String,
        own: Arc<StdMutex<OwnLoad>>,
        root: PathBuf,
        lock: PathBuf,
        source: Arc<FakeStateSource>,
        _tmp: tempfile::TempDir,
    }

    async fn spawn_jobs_node(
        on: bool,
        limits: NodeLimits,
        executor: Option<Arc<dyn RemoteExecutor>>,
    ) -> Node {
        let tmp = tempfile::tempdir().unwrap();
        start_jobs_node(tmp, on, limits, executor).await
    }

    /// A node over an existing temp dir (its `jobs/` ledger and lock): the same node after a
    /// goose restart or a Link reconnect.
    async fn start_jobs_node(
        tmp: tempfile::TempDir,
        on: bool,
        limits: NodeLimits,
        executor: Option<Arc<dyn RemoteExecutor>>,
    ) -> Node {
        let root = tmp.path().join("jobs");
        let lock = tmp.path().join("heavy-run.lock");
        let own = Arc::new(StdMutex::new(calm()));
        let sampler_own = own.clone();
        let mut config = ControlConfig::new(TOKEN.to_string(), None);
        config.port = 0;
        config.poll_interval = Duration::from_millis(100);
        config.jobs = JobsConfig {
            switch: JobsSwitch::fixed(on),
            root: root.clone(),
            heavy_lock: lock.clone(),
            limits: Some(limits),
            min_free: Some(20.0),
            user_active: UserActivePolicy::LastResort,
            sampler: Arc::new(move || sampler_own.lock().unwrap().clone()),
            tick: Duration::from_millis(200),
            kill_grace: Duration::from_secs(1),
        };
        let source = FakeStateSource::new("jobs-node");
        let handle = ControlService::start(config, source.clone(), executor, None, None, None)
            .await
            .expect("control service starts");
        let base = base_url(&handle);
        Node {
            handle,
            base,
            own,
            root,
            lock,
            source,
            _tmp: tmp,
        }
    }

    /// Limits for tests: no system-free kill line (the real machine's free % is not ours).
    fn test_limits() -> NodeLimits {
        NodeLimits {
            kill_free: 0.0,
            ..NodeLimits::default()
        }
    }

    async fn submit(n: &Node, body: serde_json::Value) -> (u16, serde_json::Value) {
        let r = reqwest::Client::new()
            .post(format!("{}/v1/swarm/jobs", n.base))
            .bearer_auth(TOKEN)
            .json(&body)
            .send()
            .await
            .unwrap();
        let s = r.status().as_u16();
        (s, r.json().await.unwrap_or(serde_json::Value::Null))
    }

    fn shell(cmd: &str) -> serde_json::Value {
        serde_json::json!({"kind": "shell", "command": cmd, "origin_node": "laptop"})
    }

    async fn expect_refusal(n: &Node, body: serde_json::Value, status: u16, code: &str) {
        let (s, v) = submit(n, body).await;
        assert_eq!((s, v["code"].as_str()), (status, Some(code)), "body {v}");
        assert!(v["reason"].as_str().is_some_and(|r| !r.is_empty()), "{v}");
    }

    async fn job(n: &Node, id: &str) -> (u16, serde_json::Value) {
        let r = reqwest::Client::new()
            .get(format!("{}/v1/swarm/jobs/{id}", n.base))
            .bearer_auth(TOKEN)
            .send()
            .await
            .unwrap();
        let s = r.status().as_u16();
        (s, r.json().await.unwrap_or(serde_json::Value::Null))
    }

    async fn wait_terminal(n: &Node, id: &str, within: Duration) -> serde_json::Value {
        let deadline = tokio::time::Instant::now() + within;
        loop {
            let (_, v) = job(n, id).await;
            if !matches!(v["state"].as_str(), Some("running") | Some("queued") | None) {
                return v;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "job {id} never ended: {v}"
            );
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    fn ledger(n: &Node, id: &str) -> serde_json::Value {
        let raw = std::fs::read_to_string(n.root.join(id).join("job.json")).unwrap();
        serde_json::from_str(&raw).unwrap()
    }

    fn group_alive(pgid: i32) -> bool {
        // SAFETY: kill(-pgid, 0) only probes.
        unsafe { libc::kill(-pgid, 0) == 0 }
    }

    #[tokio::test]
    async fn every_refusal_code_answers_its_status_over_http() {
        // jobs_disabled: the switch is OFF by default.
        let off = spawn_jobs_node(false, test_limits(), None).await;
        expect_refusal(&off, shell("true"), 403, "jobs_disabled").await;
        off.handle.shutdown();

        let n = spawn_jobs_node(true, test_limits(), None).await;
        expect_refusal(&n, shell("   "), 400, "bad_request").await;
        let mut bad_cwd = shell("true");
        bad_cwd["cwd"] = "/definitely/not/here".into();
        expect_refusal(&n, bad_cwd, 400, "bad_request").await;
        // A body that is not a JobRequest at all is a 400 too, in the same {code, reason} shape.
        expect_refusal(&n, serde_json::json!({"nope": 1}), 400, "bad_request").await;
        // prompt without goose's executor wired here.
        expect_refusal(
            &n,
            serde_json::json!({"kind": "prompt", "command": "hi"}),
            501,
            "executor_missing",
        )
        .await;

        let set = |f: &dyn Fn(&mut OwnLoad)| {
            let mut o = calm();
            f(&mut o);
            *n.own.lock().unwrap() = o;
        };
        set(&|o| o.benchmark = Some("gauntlet run".into()));
        expect_refusal(&n, shell("true"), 503, "benchmark_running").await;
        set(&|o| o.mem_free_pct = None);
        expect_refusal(&n, shell("true"), 503, "memory_unreadable").await;
        set(&|_| {});
        let mut big = shell("true");
        big["need_mem_gb"] = 500.0.into();
        expect_refusal(&n, big, 409, "memory_headroom").await;
        set(&|o| o.mem_free_pct = Some(10.0));
        expect_refusal(&n, shell("true"), 409, "memory_headroom").await;
        let mut heavy = shell("true");
        heavy["heavy"] = true.into();
        set(&|o| o.on_ac = Some(false));
        expect_refusal(&n, heavy.clone(), 409, "on_battery").await;
        set(&|o| o.thermal_limit = Some(4));
        expect_refusal(&n, heavy.clone(), 409, "thermal").await;
        set(&|o| o.lid_closed_sleeps = true);
        expect_refusal(&n, shell("true"), 409, "lid_sleeps").await;
        set(&|o| {
            o.user_active = true;
            o.user_idle_s = Some(3.0);
        });
        expect_refusal(&n, shell("true"), 409, "user_active").await;
        // ... and the placer's explicit last-resort flag admits it under the default policy.
        let mut last = shell("true");
        last["allow_user_active"] = true.into();
        let (s, v) = submit(&n, last).await;
        assert_eq!(s, 202, "{v}");
        set(&|_| {});
        // heavy_slot_held: someone else holds the lock file (the sampled flag is ignored: the
        // slot itself is the truth).
        let other = HeavySlot::try_take_at(&n.lock, "test holder")
            .unwrap()
            .unwrap();
        expect_refusal(&n, heavy.clone(), 409, "heavy_slot_held").await;
        drop(other);
        let (s, v) = submit(&n, heavy).await;
        assert_eq!(s, 202, "slot free again: {v}");
        // not_found on every per-job route.
        let c = reqwest::Client::new();
        for (m, path) in [
            ("GET", "/v1/swarm/jobs/nope"),
            ("GET", "/v1/swarm/jobs/nope/log"),
            ("POST", "/v1/swarm/jobs/nope/cancel"),
        ] {
            let r = c
                .request(m.parse().unwrap(), format!("{}{path}", n.base))
                .bearer_auth(TOKEN)
                .send()
                .await
                .unwrap();
            assert_eq!(r.status().as_u16(), 404, "{m} {path}");
            let v: serde_json::Value = r.json().await.unwrap();
            assert_eq!(v["code"], "not_found");
        }
        // already_finished: cancel a job that has ended.
        let (_, v) = submit(&n, shell("true")).await;
        let id = v["job_id"].as_str().unwrap().to_string();
        wait_terminal(&n, &id, Duration::from_secs(20)).await;
        let r = c
            .post(format!("{}/v1/swarm/jobs/{id}/cancel", n.base))
            .bearer_auth(TOKEN)
            .send()
            .await
            .unwrap();
        assert_eq!(r.status().as_u16(), 409);
        let v: serde_json::Value = r.json().await.unwrap();
        assert_eq!(v["code"], "already_finished");
        // No token = 401 like every other route.
        let r = c
            .post(format!("{}/v1/swarm/jobs", n.base))
            .json(&shell("true"))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status().as_u16(), 401);
        n.handle.shutdown();
    }

    #[tokio::test]
    async fn prompt_jobs_run_through_the_executor_and_cancel_is_an_honest_501() {
        let exec = FakeExecutor::accepting("sess-job-1");
        let n = spawn_jobs_node(true, test_limits(), Some(exec.clone())).await;
        let (s, v) = submit(
            &n,
            serde_json::json!({"kind": "prompt", "command": "write hello", "origin_node": "laptop"}),
        )
        .await;
        assert_eq!(s, 202, "{v}");
        let id = v["job_id"].as_str().unwrap().to_string();
        assert_eq!(exec.call_count(), 1);
        let (_, view) = job(&n, &id).await;
        assert_eq!(view["session_id"], "sess-job-1", "{view}");
        assert_eq!(view["state"], "running", "{view}");
        let r = reqwest::Client::new()
            .post(format!("{}/v1/swarm/jobs/{id}/cancel", n.base))
            .bearer_auth(TOKEN)
            .send()
            .await
            .unwrap();
        assert_eq!(r.status().as_u16(), 501);
        let v: serde_json::Value = r.json().await.unwrap();
        assert_eq!(v["code"], "cancel_unsupported");
        // The job follows its session: live, then not live = done.
        n.source
            .set_sessions(vec![summary("sess-job-1", "laptop", true, 1)]);
        tokio::time::sleep(Duration::from_millis(600)).await;
        assert_eq!(job(&n, &id).await.1["state"], "running");
        n.source
            .set_sessions(vec![summary("sess-job-1", "laptop", false, 2)]);
        let end = wait_terminal(&n, &id, Duration::from_secs(5)).await;
        assert_eq!(end["state"], "done", "{end}");
        n.handle.shutdown();
    }

    #[tokio::test]
    async fn cancel_kills_the_whole_process_group_and_the_ledger_keeps_origin_and_command() {
        let n = spawn_jobs_node(true, test_limits(), None).await;
        let cmd = "sleep 300 & sleep 300 & echo started; wait";
        let (s, v) = submit(&n, shell(cmd)).await;
        assert_eq!(s, 202, "{v}");
        let id = v["job_id"].as_str().unwrap().to_string();
        // Wait for the children to exist (the log line is printed after both backgrounded).
        let c = reqwest::Client::new();
        wait_until("job log shows started", || {
            let c = c.clone();
            let url = format!("{}/v1/swarm/jobs/{id}/log?from=0", n.base);
            async move {
                let r = c.get(url).bearer_auth(TOKEN).send().await.unwrap();
                let v: serde_json::Value = r.json().await.unwrap();
                v["data"].as_str().is_some_and(|d| d.contains("started"))
            }
        })
        .await;
        let rec = ledger(&n, &id);
        let pgid = rec["pgid"].as_i64().expect("ledger has pgid") as i32;
        assert!(pgid > 1);
        assert!(group_alive(pgid), "group should be running");
        // Ledger: origin and the command are recorded locally.
        assert_eq!(rec["origin_node"], "laptop");
        assert_eq!(rec["command"], cmd);
        // The wire never carries the command: not the job view, not the list, not /nodes.
        let (_, view) = job(&n, &id).await;
        let list = get_json(&c, &format!("{}/v1/swarm/jobs", n.base)).await;
        let nodes = get_json(&c, &format!("{}/v1/swarm/nodes", n.base)).await;
        for (what, body) in [("view", &view), ("list", &list), ("nodes", &nodes)] {
            let s = body.to_string();
            assert!(!s.contains("sleep 300"), "{what} leaked the command: {s}");
            assert!(s.contains(&id), "{what} should carry the job id: {s}");
            assert!(s.contains("laptop"), "{what} should carry the origin: {s}");
        }
        assert_eq!(view["origin_node"], "laptop");
        assert_eq!(view["kind"], "shell");

        let r = c
            .post(format!("{}/v1/swarm/jobs/{id}/cancel", n.base))
            .bearer_auth(TOKEN)
            .send()
            .await
            .unwrap();
        assert_eq!(r.status().as_u16(), 202, "cancel is accepted");
        // D3 (3.0.128 live test): the answer is the FINAL view, not the pre-cancel "running".
        let body: serde_json::Value = r.json().await.unwrap();
        assert_eq!(body["state"], "cancelled", "cancel answered {body}");
        assert_eq!(body["exit_code"], 143, "cancel answered {body}");
        let end = wait_terminal(&n, &id, Duration::from_secs(20)).await;
        assert_eq!(end["state"], "cancelled", "{end}");
        assert_eq!(end["exit_code"], 143, "{end}");
        wait_until("every process of the group is gone", || async move {
            !group_alive(pgid)
        })
        .await;
        let rec = ledger(&n, &id);
        assert_eq!(rec["state"], "cancelled");
        assert_eq!(rec["exit_code"], 143);
        n.handle.shutdown();
    }

    #[tokio::test]
    async fn watchdog_times_out_and_kills_a_memory_hog_by_footprint() {
        // ~100 MB cap on this machine's RAM: a 400 MB hog must die with 137.
        let ram = {
            let mut v: u64 = 0;
            let mut len = std::mem::size_of::<u64>();
            let name = std::ffi::CString::new("hw.memsize").unwrap();
            // SAFETY: sysctlbyname writes at most len bytes into v.
            unsafe {
                libc::sysctlbyname(
                    name.as_ptr(),
                    &mut v as *mut u64 as *mut libc::c_void,
                    &mut len,
                    std::ptr::null_mut(),
                    0,
                )
            };
            v
        };
        assert!(ram > 0);
        let limits = NodeLimits {
            max_rss_pct: 100.0 * (100u64 << 20) as f64 / ram as f64,
            ..test_limits()
        };
        let n = spawn_jobs_node(true, limits, None).await;
        let (s, v) = submit(
            &n,
            shell("python3 -c \"a=b'x'*(400<<20); import time; time.sleep(60)\""),
        )
        .await;
        assert_eq!(s, 202, "{v}");
        let hog = v["job_id"].as_str().unwrap().to_string();
        let mut t = shell("sleep 30");
        t["timeout_s"] = 1.into();
        let (s, v) = submit(&n, t).await;
        assert_eq!(s, 202, "{v}");
        let slow = v["job_id"].as_str().unwrap().to_string();

        let end = wait_terminal(&n, &hog, Duration::from_secs(30)).await;
        assert_eq!(end["state"], "killed", "{end}");
        assert_eq!(end["exit_code"], 137, "{end}");
        assert!(
            end["reason"].as_str().unwrap_or("").contains("of RAM"),
            "{end}"
        );
        let end = wait_terminal(&n, &slow, Duration::from_secs(20)).await;
        assert_eq!(end["exit_code"], 124, "{end}");
        n.handle.shutdown();
    }

    fn heavy_run() -> Option<PathBuf> {
        let p = dirs::home_dir()?.join("Projects/machine-sync/bin/heavy-run");
        p.is_file().then_some(p)
    }

    fn heavy_run_cmd(bin: &Path, lock: &Path, ledger: &Path) -> std::process::Command {
        let mut c = std::process::Command::new(bin);
        c.env("HEAVY_RUN_LOCK", lock)
            .env("NODE_LIMITS_FILE", ledger.join("absent-limits.json"))
            .args([
                "--lock-wait",
                "0",
                "--min-free",
                "0",
                "--wait",
                "0",
                "--ledger",
            ])
            .arg(ledger);
        c
    }

    #[tokio::test]
    async fn goose_jobs_and_heavy_run_exclude_each_other_on_one_lock() {
        let Some(bin) = heavy_run() else {
            eprintln!("SKIP: ~/Projects/machine-sync/bin/heavy-run absent");
            return;
        };
        let n = spawn_jobs_node(true, test_limits(), None).await;
        // 1) A goose heavy job holds the slot -> heavy-run --lock-wait 0 -- true exits 75.
        let mut heavy = shell("sleep 30");
        heavy["heavy"] = true.into();
        let (s, v) = submit(&n, heavy.clone()).await;
        assert_eq!(s, 202, "{v}");
        let id = v["job_id"].as_str().unwrap().to_string();
        let led1 = n.root.join("hr1");
        std::fs::create_dir_all(&led1).unwrap();
        let out = heavy_run_cmd(&bin, &n.lock, &led1)
            .args(["--", "true"])
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(75),
            "heavy-run must refuse while a goose job holds the lock: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        reqwest::Client::new()
            .post(format!("{}/v1/swarm/jobs/{id}/cancel", n.base))
            .bearer_auth(TOKEN)
            .send()
            .await
            .unwrap();
        wait_terminal(&n, &id, Duration::from_secs(20)).await;

        // 2) A real heavy-run holds the lock -> a heavy goose job is refused heavy_slot_held.
        let led2 = n.root.join("hr2");
        std::fs::create_dir_all(&led2).unwrap();
        let mut hr = heavy_run_cmd(&bin, &n.lock, &led2)
            .args(["--", "sleep", "20"])
            .spawn()
            .unwrap();
        let lock = n.lock.clone();
        wait_until("heavy-run took the lock", || {
            let lock = lock.clone();
            async move { matches!(HeavySlot::try_take_at(&lock, "probe"), Ok(None)) }
        })
        .await;
        expect_refusal(&n, heavy.clone(), 409, "heavy_slot_held").await;
        // A light job is not affected by the heavy slot.
        let (s, v) = submit(&n, shell("true")).await;
        assert_eq!(s, 202, "{v}");
        // SAFETY: TERM our own child; heavy-run then kills its job group itself.
        unsafe { libc::kill(hr.id() as i32, libc::SIGTERM) };
        let _ = hr.wait();
        n.handle.shutdown();
    }

    async fn cancel(n: &Node, id: &str) -> (u16, serde_json::Value) {
        let r = reqwest::Client::new()
            .post(format!("{}/v1/swarm/jobs/{id}/cancel", n.base))
            .bearer_auth(TOKEN)
            .send()
            .await
            .unwrap();
        let s = r.status().as_u16();
        (s, r.json().await.unwrap_or(serde_json::Value::Null))
    }

    async fn listed(n: &Node, id: &str) -> Option<serde_json::Value> {
        let c = reqwest::Client::new();
        let list = get_json(&c, &format!("{}/v1/swarm/jobs", n.base)).await;
        list.as_array()?.iter().find(|j| j["job_id"] == id).cloned()
    }

    /// D1 (3.0.128 live test): a Link reconnect rebuilt the control service and with it an
    /// EMPTY job registry: the running job vanished from the list, cancel said not_found, and
    /// the process kept running (and holding the heavy slot). The registry must outlive it.
    #[tokio::test]
    async fn a_running_job_survives_a_control_service_restart_and_cancel_kills_its_group() {
        let n = spawn_jobs_node(true, test_limits(), None).await;
        let (s, v) = submit(&n, shell("sleep 300")).await;
        assert_eq!(s, 202, "{v}");
        let id = v["job_id"].as_str().unwrap().to_string();
        let pgid = ledger(&n, &id)["pgid"].as_i64().unwrap() as i32;
        assert!(group_alive(pgid));

        let Node { handle, _tmp, .. } = n;
        handle.shutdown();
        let n = start_jobs_node(_tmp, true, test_limits(), None).await;

        let row = listed(&n, &id)
            .await
            .unwrap_or_else(|| panic!("job {id} vanished after the restart"));
        assert_eq!(row["state"], "running", "{row}");
        let (s, v) = cancel(&n, &id).await;
        assert_eq!(s, 202, "cancel must find the job: {v}");
        assert_eq!(v["state"], "cancelled", "{v}");
        assert_eq!(v["exit_code"], 143, "{v}");
        assert!(!group_alive(pgid), "the group must be dead after cancel");
        n.handle.shutdown();
    }

    fn running_record(id: &str, pgid: i32, heavy: bool) -> serde_json::Value {
        serde_json::json!({
            "job_id": id, "origin_node": "laptop", "kind": "shell", "command": "sleep 60",
            "cwd": null, "heavy": heavy, "need_mem_gb": null, "timeout_s": 600,
            "state": "running", "started_at": chrono::Utc::now().to_rfc3339(),
            "ended_at": null, "exit_code": null, "reason": null, "session_id": null,
            "pgid": pgid,
        })
    }

    fn seed(root: &Path, rec: &serde_json::Value) {
        let dir = root.join(rec["job_id"].as_str().unwrap());
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("job.json"), rec.to_string()).unwrap();
    }

    /// D1, the goose-restart half: a fresh registry over a ledger adopts a `running` job whose
    /// group is alive (listed, holds the heavy slot it asked for, cancellable by group kill)
    /// and marks one whose group is gone `lost`, never leaving it "running".
    #[tokio::test]
    async fn a_fresh_registry_adopts_a_live_ledger_job_and_marks_a_dead_one_lost() {
        use std::os::unix::process::CommandExt;
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("jobs");
        let live_id = "j20261025000000-aaaaaa";
        let dead_id = "j20261025000000-bbbbbb";
        // The live record is written BEFORE its process starts, as submit does.
        let mut live_rec = running_record(live_id, 0, true);
        // The leader exits at once and its `sleep` lives on in the group, reparented to launchd
        // (as a job's group is after the goose that started it died).
        let mut leader = std::process::Command::new("/bin/sh")
            .args(["-c", "/bin/sleep 60 >/dev/null 2>&1 & exit 0"])
            .process_group(0)
            .spawn()
            .unwrap();
        let live_pgid = leader.id() as i32;
        leader.wait().unwrap();
        assert!(group_alive(live_pgid), "the group outlives its leader");
        live_rec["pgid"] = live_pgid.into();
        seed(&root, &live_rec);
        let mut dead = std::process::Command::new("/usr/bin/true")
            .process_group(0)
            .spawn()
            .unwrap();
        let dead_pgid = dead.id() as i32;
        dead.wait().unwrap();
        seed(&root, &running_record(dead_id, dead_pgid, false));

        let n = start_jobs_node(tmp, true, test_limits(), None).await;
        let row = listed(&n, live_id).await.expect("live ledger job adopted");
        assert_eq!(row["state"], "running", "{row}");
        let row = listed(&n, dead_id).await.expect("dead ledger job listed");
        assert_eq!(row["state"], "lost", "{row}");
        assert!(row["ended_at"].is_string(), "{row}");
        assert_eq!(ledger(&n, dead_id)["state"], "lost");
        // The adopted heavy job holds the slot again, and the lock names it.
        assert!(
            matches!(HeavySlot::try_take_at(&n.lock, "probe"), Ok(None)),
            "an adopted heavy job must hold the heavy slot"
        );
        let holder = std::fs::read_to_string(&n.lock).unwrap();
        assert!(holder.contains(live_id), "lock holder line: {holder}");

        let (s, v) = cancel(&n, live_id).await;
        assert_eq!(s, 202, "{v}");
        assert_eq!(v["state"], "cancelled", "{v}");
        assert_eq!(v["exit_code"], 143, "{v}");
        assert!(
            !group_alive(live_pgid),
            "the adopted group is gone after cancel"
        );
        assert!(
            matches!(HeavySlot::try_take_at(&n.lock, "probe"), Ok(Some(_))),
            "the slot is free once the adopted job is gone"
        );
        n.handle.shutdown();
    }

    /// D10: a heavy refusal names the holder and for how long, from the lock's holder line.
    #[tokio::test]
    async fn heavy_slot_refusal_names_the_holder_and_how_long() {
        let n = spawn_jobs_node(true, test_limits(), None).await;
        let _held = HeavySlot::try_take_at(&n.lock, "test")
            .unwrap()
            .expect("slot");
        let since = (chrono::Local::now() - chrono::Duration::seconds(372))
            .format("%F %T")
            .to_string();
        std::fs::write(
            &n.lock,
            format!("pid 4321 since {since}: /opt/homebrew/bin/cargo test -p goose\n"),
        )
        .unwrap();
        let mut heavy = shell("true");
        heavy["heavy"] = true.into();
        let (s, v) = submit(&n, heavy).await;
        assert_eq!(
            (s, v["code"].as_str()),
            (409, Some("heavy_slot_held")),
            "{v}"
        );
        let reason = v["reason"].as_str().unwrap();
        assert!(
            reason.starts_with(
                "heavy slot held by pid 4321 (heavy-run: cargo test -p goose) for 6m 1"
            ),
            "{reason}"
        );
        n.handle.shutdown();
    }
}
