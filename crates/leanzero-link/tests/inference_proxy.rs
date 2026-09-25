//! The chat inference proxy end to end, hermetically: a scripted OpenAI-compatible engine on
//! loopback, node B's real control service serving it under `/v1/swarm/inference/*`, and the
//! requester's real [`InferenceRelay`] dialing B through [`support::fake_tailnet`] (a SOCKS5
//! stand-in for tailscaled — B carries a mesh-looking IP only that proxy reaches). No goosed, no
//! tailscaled, no model.

mod support;

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::http::{header, StatusCode};
use axum::response::Response;
use axum::routing::{get, post};
use axum::Router;
use futures::stream::BoxStream;
use futures::StreamExt;
use leanzero_link::control::{ControlConfig, ControlHandle, ControlService};
use leanzero_link::inference::{
    InferenceRelay, PeerCall, PeerCallResolver, ENGINE_UNREACHABLE, RELAY_FAILED,
};
use leanzero_link::manager::MESH_POLL_FAILURE_LOOKS;
use leanzero_link::state::{ChatServing, SwarmStateSource};
use leanzero_link::wire::{LinkEvent, NodeState, NodeStatus};

const TOKEN: &str = "inference-node-token";
const HOSTNAME: &str = "WorksMacStudio.lan";
const DEADLINE: Duration = Duration::from_secs(10);

const CHUNK_1: &str =
    "data: {\"id\":\"c1\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Par\"}}]}\n\n";
const CHUNK_2: &str =
    "data: {\"id\":\"c1\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"is\"}}]}\n\n";
const CHUNK_END: &str =
    "data: {\"id\":\"c1\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n";
const DONE: &str = "data: [DONE]\n\n";
const MODELS: &str =
    r#"{"object":"list","data":[{"id":"qwen3.8-27b","object":"model","context_window":262144}]}"#;
const STATUS: &str = r#"{"num_running":1,"num_waiting":0}"#;

// ---------------------------------------------------------------------------------------------
// The scripted engine
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum Script {
    /// chunks, finish_reason, [DONE]
    Complete,
    /// two chunks, then a clean end — no finish_reason, no [DONE]
    CutClean,
    /// two chunks, then the body errors (the engine process died mid-stream)
    DiesMidStream,
    /// a chunk every 20 ms until the client leaves
    Endless,
    /// never answers — not even a response head (a long prefill, a non-streamed answer)
    Silent,
    /// a slow, HEALTHY generation: no response head for `head_after`, one chunk, then
    /// `gap` of silence, then the rest — the case the in-flight watch must never cut
    SlowAlive { head_after: Duration, gap: Duration },
}

struct Engine {
    script: Script,
    received: StdMutex<Vec<Bytes>>,
    received_content_type: StdMutex<Option<String>>,
    /// Set when the streamed body is dropped by the server — the client left.
    stream_dropped: Arc<AtomicBool>,
    chunks_sent: Arc<AtomicUsize>,
}

struct DropFlag(Arc<AtomicBool>);
impl Drop for DropFlag {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

fn sse(stream: BoxStream<'static, Result<Bytes, std::io::Error>>) -> Response {
    Response::builder()
        .status(200)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-cache")
        .body(Body::from_stream(stream))
        .unwrap()
}

async fn engine_chat(
    axum::extract::State(engine): axum::extract::State<Arc<Engine>>,
    headers: axum::http::HeaderMap,
    body: Bytes,
) -> Response {
    engine.received.lock().unwrap().push(body);
    *engine.received_content_type.lock().unwrap() = headers
        .get(header::CONTENT_TYPE)
        .map(|v| v.to_str().unwrap().to_string());
    let ok = |s: &'static str| Ok::<Bytes, std::io::Error>(Bytes::from_static(s.as_bytes()));
    match engine.script {
        Script::Complete => {
            sse(
                futures::stream::iter(vec![ok(CHUNK_1), ok(CHUNK_2), ok(CHUNK_END), ok(DONE)])
                    .boxed(),
            )
        }
        Script::CutClean => sse(futures::stream::iter(vec![ok(CHUNK_1), ok(CHUNK_2)]).boxed()),
        // Paced like a real generation, so the headers and the first chunks are on the wire
        // before the death (an immediately-failing body never sends a response at all).
        Script::DiesMidStream => sse(futures::stream::iter(vec![
            ok(CHUNK_1),
            ok(CHUNK_2),
            Err(std::io::Error::other("rank died")),
        ])
        .then(|item| async move {
            tokio::time::sleep(Duration::from_millis(30)).await;
            item
        })
        .boxed()),
        Script::Endless => {
            let guard = DropFlag(engine.stream_dropped.clone());
            let sent = engine.chunks_sent.clone();
            sse(futures::stream::unfold(guard, move |guard| {
                let sent = sent.clone();
                async move {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                    sent.fetch_add(1, Ordering::SeqCst);
                    Some((Ok(Bytes::from_static(CHUNK_1.as_bytes())), guard))
                }
            })
            .boxed())
        }
        Script::Silent => std::future::pending().await,
        Script::SlowAlive { head_after, gap } => {
            tokio::time::sleep(head_after).await;
            sse(futures::stream::iter(vec![
                (Duration::ZERO, CHUNK_1),
                (gap, CHUNK_2),
                (Duration::ZERO, CHUNK_END),
                (Duration::ZERO, DONE),
            ])
            .then(|(wait, chunk)| async move {
                tokio::time::sleep(wait).await;
                Ok::<Bytes, std::io::Error>(Bytes::from_static(chunk.as_bytes()))
            })
            .boxed())
        }
    }
}

async fn start_engine(script: Script) -> (Arc<Engine>, String) {
    let engine = Arc::new(Engine {
        script,
        received: StdMutex::new(Vec::new()),
        received_content_type: StdMutex::new(None),
        stream_dropped: Arc::new(AtomicBool::new(false)),
        chunks_sent: Arc::new(AtomicUsize::new(0)),
    });
    let router = Router::new()
        .route("/v1/chat/completions", post(engine_chat))
        .route(
            "/v1/models",
            get(|| async { ([(header::CONTENT_TYPE, "application/json")], MODELS) }),
        )
        .route(
            "/v1/status",
            get(|| async { ([(header::CONTENT_TYPE, "application/json")], STATUS) }),
        )
        .with_state(engine.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    (engine, format!("http://{addr}"))
}

// ---------------------------------------------------------------------------------------------
// Node B (serving) and the requester's relay
// ---------------------------------------------------------------------------------------------

struct FakeServing {
    allowed: AtomicBool,
    base: Result<String, String>,
}

impl ChatServing for FakeServing {
    fn serving_allowed(&self) -> bool {
        self.allowed.load(Ordering::SeqCst)
    }
    fn engine_base_url(&self) -> Result<String, String> {
        self.base.clone()
    }
}

struct Source;

#[async_trait::async_trait]
impl SwarmStateSource for Source {
    async fn local_node(&self) -> NodeState {
        NodeState {
            node_id: "node-b".to_string(),
            hostname: HOSTNAME.to_string(),
            mesh_ip: None,
            status: NodeStatus::Idle,
            sessions_active: 0,
            updated_at: chrono::Utc::now(),
            last_poll_error: None,
            computer_name: None,
            allows: None,
        }
    }
    async fn local_sessions(&self) -> Result<Vec<leanzero_link::wire::SessionSummary>, String> {
        Ok(Vec::new())
    }
    fn subscribe_local_deltas(&self) -> BoxStream<'static, LinkEvent> {
        futures::stream::pending().boxed()
    }
}

async fn start_node_b(serving: Option<Arc<FakeServing>>) -> (ControlHandle, String) {
    let mut config = ControlConfig::new(TOKEN.to_string(), None);
    config.port = 0;
    let handle = ControlService::start(
        config,
        Arc::new(Source),
        None,
        None,
        None,
        serving.map(|s| s as Arc<dyn ChatServing>),
    )
    .await
    .expect("node B starts");
    let port = handle.local_addr().port();
    let mesh_ip = support::fake_tailnet().expose(port);
    (handle, format!("http://{mesh_ip}:{port}"))
}

struct Resolver(Result<PeerCall, String>);

#[async_trait::async_trait]
impl PeerCallResolver for Resolver {
    async fn peer_call(&self, _peer: &str) -> Result<PeerCall, String> {
        self.0.clone()
    }
}

/// Every relay in this file is watched at a fast cadence, so every test here also proves the
/// in-flight watch leaves a healthy request alone.
const LOOK_INTERVAL: Duration = Duration::from_millis(50);
/// The kill tests' connect timeout: short, so a dead peer's unanswered dials resolve fast.
const FAST_CONNECT: Duration = Duration::from_millis(300);

fn call_to(base_url: &str) -> PeerCall {
    PeerCall {
        base_url: base_url.to_string(),
        token: TOKEN.to_string(),
        proxy: Some(support::fake_tailnet().proxy()),
        connect_timeout: Duration::from_secs(5),
        liveness_interval: LOOK_INTERVAL,
    }
}

fn fast_call_to(base_url: &str) -> PeerCall {
    PeerCall {
        connect_timeout: FAST_CONNECT,
        ..call_to(base_url)
    }
}

struct Rig {
    engine: Arc<Engine>,
    serving: Arc<FakeServing>,
    b_base: String,
    relay: InferenceRelay,
    _b: ControlHandle,
}

async fn rig(script: Script) -> Rig {
    let (engine, engine_base) = start_engine(script).await;
    let serving = Arc::new(FakeServing {
        allowed: AtomicBool::new(true),
        base: Ok(engine_base),
    });
    let (b, b_base) = start_node_b(Some(serving.clone())).await;
    let relay = InferenceRelay::start(
        "node-b".to_string(),
        Arc::new(Resolver(Ok(call_to(&b_base)))),
    )
    .await
    .expect("relay starts");
    Rig {
        engine,
        serving,
        b_base,
        relay,
        _b: b,
    }
}

const REQUEST: &str = r#"{"model":"qwen3.8-27b","stream":true,"messages":[{"role":"user","content":"Capital of France?"}]}"#;

fn client() -> reqwest::Client {
    reqwest::Client::builder().no_proxy().build().unwrap()
}

async fn post_chat(base: &str) -> reqwest::Response {
    client()
        .post(format!("{base}/v1/chat/completions"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(REQUEST)
        .send()
        .await
        .expect("the relay answers")
}

// ---------------------------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------------------------

#[tokio::test]
async fn a_chat_stream_crosses_both_hops_byte_for_byte_through_the_mesh_proxy() {
    let rig = rig(Script::Complete).await;
    let mesh_ip = rig
        .b_base
        .trim_start_matches("http://")
        .split(':')
        .next()
        .unwrap()
        .to_string();
    let before = support::fake_tailnet().connects_to(&mesh_ip);

    let response = post_chat(rig.relay.base_url()).await;
    assert_eq!(response.status(), 200);
    assert_eq!(
        response.headers().get(header::CONTENT_TYPE).unwrap(),
        "text/event-stream"
    );
    assert_eq!(
        response.headers().get(header::CACHE_CONTROL).unwrap(),
        "no-cache"
    );
    let body = response.bytes().await.expect("the whole stream");
    assert_eq!(
        body,
        Bytes::from(format!("{CHUNK_1}{CHUNK_2}{CHUNK_END}{DONE}")),
        "every SSE byte arrives unchanged"
    );

    let received = rig.engine.received.lock().unwrap().clone();
    assert_eq!(
        received,
        vec![Bytes::from_static(REQUEST.as_bytes())],
        "the request body too"
    );
    assert_eq!(
        rig.engine.received_content_type.lock().unwrap().as_deref(),
        Some("application/json")
    );
    assert!(
        support::fake_tailnet().connects_to(&mesh_ip) > before,
        "the relay reached B only through the mesh proxy"
    );
}

#[tokio::test]
async fn models_and_status_pass_through_for_the_router_probe() {
    let rig = rig(Script::Complete).await;
    for (path, expected) in [("v1/models", MODELS), ("v1/status", STATUS)] {
        let response = client()
            .get(format!("{}/{path}", rig.relay.base_url()))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200, "{path}");
        assert_eq!(response.text().await.unwrap(), expected, "{path}");
    }
}

#[tokio::test]
async fn a_stream_the_engine_ends_without_its_marker_still_ends_without_it() {
    let rig = rig(Script::CutClean).await;
    let body = post_chat(rig.relay.base_url()).await.bytes().await.unwrap();
    let text = String::from_utf8(body.to_vec()).unwrap();
    assert_eq!(text, format!("{CHUNK_1}{CHUNK_2}"));
    assert!(
        !text.contains("[DONE]") && !text.contains("finish_reason"),
        "nothing is appended: the requester's parser must see the cut and name it"
    );
}

#[tokio::test]
async fn an_engine_that_dies_mid_stream_is_a_body_error_never_a_clean_end() {
    let rig = rig(Script::DiesMidStream).await;
    let response = post_chat(rig.relay.base_url()).await;
    assert_eq!(response.status(), 200);
    let mut stream = response.bytes_stream();
    let mut got = Vec::new();
    let outcome = loop {
        match tokio::time::timeout(DEADLINE, stream.next())
            .await
            .expect("the stream ends")
        {
            Some(Ok(chunk)) => got.extend_from_slice(&chunk),
            Some(Err(err)) => break Err(err),
            None => break Ok(()),
        }
    };
    assert!(
        outcome.is_err(),
        "a clean EOF would impersonate a finished answer; got {:?} after {:?}",
        outcome,
        String::from_utf8_lossy(&got)
    );
}

#[tokio::test]
async fn a_requester_that_disconnects_ends_the_engine_stream() {
    let rig = rig(Script::Endless).await;
    let response = post_chat(rig.relay.base_url()).await;
    let mut stream = response.bytes_stream();
    let first = tokio::time::timeout(DEADLINE, stream.next())
        .await
        .expect("a first chunk")
        .expect("not ended")
        .expect("not an error");
    assert!(!first.is_empty());
    drop(stream);

    let deadline = tokio::time::Instant::now() + DEADLINE;
    while !rig.engine.stream_dropped.load(Ordering::SeqCst) {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the engine kept streaming to nobody ({} chunks)",
            rig.engine.chunks_sent.load(Ordering::SeqCst)
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let settled = rig.engine.chunks_sent.load(Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        rig.engine.chunks_sent.load(Ordering::SeqCst),
        settled,
        "no chunk is generated after the client left"
    );
}

#[tokio::test]
async fn the_owners_switch_off_is_a_named_403_carried_verbatim_by_the_relay() {
    let rig = rig(Script::Complete).await;
    rig.serving.allowed.store(false, Ordering::SeqCst);

    let response = post_chat(rig.relay.base_url()).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let text = response.text().await.unwrap();
    assert_eq!(
        text,
        format!(
            "chatServingDisabled: \"Let my other Macs use this Mac › Answer chat\" is off on {HOSTNAME}"
        )
    );
    assert!(
        rig.engine.received.lock().unwrap().is_empty(),
        "the engine saw nothing"
    );

    rig.serving.allowed.store(true, Ordering::SeqCst);
    assert_eq!(
        post_chat(rig.relay.base_url()).await.status(),
        200,
        "read per request"
    );
}

#[tokio::test]
async fn the_route_is_bearer_gated_and_refuses_browsers() {
    let rig = rig(Script::Complete).await;
    let direct = |token: Option<&str>, origin: bool| {
        let mut req = reqwest::Client::builder()
            .proxy(reqwest::Proxy::all(support::fake_tailnet().proxy().proxy_url()).unwrap())
            .build()
            .unwrap()
            .get(format!("{}/v1/swarm/inference/v1/models", rig.b_base));
        if let Some(token) = token {
            req = req.bearer_auth(token);
        }
        if origin {
            req = req.header(header::ORIGIN, "https://evil.example");
        }
        req.send()
    };
    assert_eq!(direct(None, false).await.unwrap().status(), 401);
    assert_eq!(
        direct(Some("not-the-token"), false).await.unwrap().status(),
        401
    );
    assert_eq!(direct(Some(TOKEN), true).await.unwrap().status(), 403);
    assert_eq!(direct(Some(TOKEN), false).await.unwrap().status(), 200);

    // The relay: the capability segment is the gate; a browser is refused before it.
    let base = rig.relay.base_url();
    let wrong = format!("{}0000/v1/models", base.rsplit_once('/').unwrap().0);
    assert_eq!(client().get(wrong).send().await.unwrap().status(), 404);
    let browser = client()
        .get(format!("{base}/v1/models"))
        .header(header::ORIGIN, "http://localhost:3000")
        .send()
        .await
        .unwrap();
    assert_eq!(browser.status(), 403);
    assert!(rig.relay.local_addr().ip().is_loopback());
}

#[tokio::test]
async fn nothing_mounted_on_the_peer_is_a_named_502_and_the_proxy_does_not_mount() {
    let unused = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let dead = format!("http://{}", unused.local_addr().unwrap());
    drop(unused);
    let serving = Arc::new(FakeServing {
        allowed: AtomicBool::new(true),
        base: Ok(dead.clone()),
    });
    let (_b, b_base) = start_node_b(Some(serving)).await;
    let relay = InferenceRelay::start("node-b".into(), Arc::new(Resolver(Ok(call_to(&b_base)))))
        .await
        .unwrap();
    let response = post_chat(relay.base_url()).await;
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    let text = response.text().await.unwrap();
    assert!(text.starts_with(ENGINE_UNREACHABLE), "{text}");
    assert!(text.contains(&dead) && text.contains(HOSTNAME), "{text}");
}

#[tokio::test]
async fn an_unwired_or_unnamable_engine_is_loud() {
    let (_b, b_base) = start_node_b(None).await;
    let relay = InferenceRelay::start("node-b".into(), Arc::new(Resolver(Ok(call_to(&b_base)))))
        .await
        .unwrap();
    assert_eq!(
        post_chat(relay.base_url()).await.status(),
        StatusCode::NOT_IMPLEMENTED
    );

    let serving = Arc::new(FakeServing {
        allowed: AtomicBool::new(true),
        base: Err("the mlx_engine config block is unreadable".to_string()),
    });
    let (_b2, b2_base) = start_node_b(Some(serving)).await;
    let relay = InferenceRelay::start("node-b".into(), Arc::new(Resolver(Ok(call_to(&b2_base)))))
        .await
        .unwrap();
    let response = post_chat(relay.base_url()).await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(response.text().await.unwrap().contains("unreadable"));
}

#[tokio::test]
async fn a_peer_the_relay_cannot_reach_is_a_named_502_never_a_direct_dial() {
    let relay = InferenceRelay::start(
        "node-gone".into(),
        Arc::new(Resolver(Err(
            "no known mesh peer with node id 'node-gone'".into()
        ))),
    )
    .await
    .unwrap();
    let response = post_chat(relay.base_url()).await;
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    let text = response.text().await.unwrap();
    assert!(
        text.starts_with(RELAY_FAILED) && text.contains("node-gone"),
        "{text}"
    );

    // A resolved peer without a mesh proxy is refused, not dialed around the mesh.
    let mut call = call_to("http://100.64.9.9:41226");
    call.proxy = None;
    let relay = InferenceRelay::start("node-b".into(), Arc::new(Resolver(Ok(call))))
        .await
        .unwrap();
    let text = post_chat(relay.base_url()).await.text().await.unwrap();
    assert!(text.contains("refusing a direct dial"), "{text}");
}

// ---------------------------------------------------------------------------------------------
// Q-32: the peer's Link dies under a request in flight
// ---------------------------------------------------------------------------------------------

// ---------------------------------------------------------------------------------------------
// Q-32: the peer's Link dies under a request in flight (r3-1.log 12:14:59)
// ---------------------------------------------------------------------------------------------

fn mesh_addr_of(base: &str) -> (String, u16) {
    let (ip, port) = base.trim_start_matches("http://").rsplit_once(':').unwrap();
    (ip.to_string(), port.parse().unwrap())
}

/// The bound the watch promises for a peer that went away and stayed away: every look is one
/// fresh dial that the dead peer never answers (the connect timeout), and the request ends at
/// the `MESH_POLL_FAILURE_LOOKS`th in a row. Transport facts only.
fn unreachable_bound(call: &PeerCall) -> Duration {
    (call.liveness_interval + call.connect_timeout) * MESH_POLL_FAILURE_LOOKS
        + Duration::from_secs(2)
}

async fn relay_to(call: PeerCall) -> InferenceRelay {
    InferenceRelay::start("node-b".into(), Arc::new(Resolver(Ok(call))))
        .await
        .unwrap()
}

async fn serving(engine_base: String) -> Arc<FakeServing> {
    Arc::new(FakeServing {
        allowed: AtomicBool::new(true),
        base: Ok(engine_base),
    })
}

/// Read a streamed body to its end: the bytes, and whether it ended in an error (an aborted
/// body) or cleanly.
async fn drain(response: reqwest::Response) -> (String, Result<(), String>) {
    let mut stream = response.bytes_stream();
    let mut got = Vec::new();
    let outcome = loop {
        match stream.next().await {
            Some(Ok(chunk)) => got.extend_from_slice(&chunk),
            Some(Err(err)) => break Err(err.to_string()),
            None => break Ok(()),
        }
    };
    (String::from_utf8_lossy(&got).into_owned(), outcome)
}

/// The one error event the relay appends when it ends an SSE stream: OpenAI-shaped, so the
/// requester's stream parser raises `error.message` verbatim.
fn relay_error_event(body: &str) -> serde_json::Value {
    let line = body
        .lines()
        .rev()
        .find(|line| line.starts_with("data: {\"error\""))
        .unwrap_or_else(|| panic!("no relay error event in {body:?}"));
    let event: serde_json::Value = serde_json::from_str(&line["data: ".len()..]).unwrap();
    assert_eq!(event["error"]["type"], RELAY_FAILED, "{event}");
    event
}

#[tokio::test]
async fn a_stream_in_flight_when_the_peers_link_dies_ends_with_a_named_error() {
    let (_engine, engine_base) = start_engine(Script::Endless).await;
    let (_b, b_base) = start_node_b(Some(serving(engine_base).await)).await;
    let call = fast_call_to(&b_base);
    let bound = unreachable_bound(&call);
    let relay = relay_to(call).await;

    let response = post_chat(relay.base_url()).await;
    assert_eq!(response.status(), 200);
    let mut stream = response.bytes_stream();
    stream.next().await.unwrap().expect("the stream is live");
    let killed_at = tokio::time::Instant::now();
    support::fake_tailnet().kill(&mesh_addr_of(&b_base).0);

    let mut got = Vec::new();
    let outcome = tokio::time::timeout(DEADLINE, async {
        loop {
            match stream.next().await {
                Some(Ok(chunk)) => got.extend_from_slice(&chunk),
                Some(Err(err)) => return Err(err.to_string()),
                None => return Ok(()),
            }
        }
    })
    .await
    .expect("the stream ended — never a silent hang");
    let waited = killed_at.elapsed();
    assert!(
        outcome.is_err(),
        "a clean end would impersonate a finished answer"
    );
    assert!(waited <= bound, "ended after {waited:?}, bound {bound:?}");
    let body = String::from_utf8_lossy(&got);
    let message = relay_error_event(&body)["error"]["message"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        message.starts_with(&format!(
            "{RELAY_FAILED}: Link peer 'node-b' lost this request in flight: "
        )) && message.contains("could not reach the peer"),
        "{message}"
    );
}

#[tokio::test]
async fn a_request_awaiting_its_answer_when_the_peers_link_dies_is_a_named_502() {
    let (engine, engine_base) = start_engine(Script::Silent).await;
    let (_b, b_base) = start_node_b(Some(serving(engine_base).await)).await;
    let call = fast_call_to(&b_base);
    let bound = unreachable_bound(&call);
    let relay = relay_to(call).await;

    let base = relay.base_url().to_string();
    let pending = tokio::spawn(async move { post_chat(&base).await });
    let deadline = tokio::time::Instant::now() + DEADLINE;
    while engine.received.lock().unwrap().is_empty() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the engine never saw the request"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let killed_at = tokio::time::Instant::now();
    support::fake_tailnet().kill(&mesh_addr_of(&b_base).0);

    let response = tokio::time::timeout(DEADLINE, pending)
        .await
        .expect("answered — never a silent hang")
        .unwrap();
    let waited = killed_at.elapsed();
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    let text = response.text().await.unwrap();
    assert!(
        text.starts_with(&format!(
            "{RELAY_FAILED}: Link peer 'node-b' lost this request in flight: "
        )) && text.contains("could not reach the peer"),
        "{text}"
    );
    assert!(
        waited <= bound,
        "answered after {waited:?}, bound {bound:?}"
    );
}

/// Q-31 and Q-32 together: the peer's Link comes back (a fresh control service, a new epoch)
/// before its absence could end the request — the look that reaches it finds the request
/// gone and says so, instead of waiting on a connection nothing will ever answer again.
#[tokio::test]
async fn a_peer_whose_link_restarted_under_the_stream_names_the_drop() {
    let (engine, engine_base) = start_engine(Script::Endless).await;
    let serving = serving(engine_base).await;
    let (_b, b_base) = start_node_b(Some(serving.clone())).await;
    let relay = relay_to(fast_call_to(&b_base)).await;

    let response = post_chat(relay.base_url()).await;
    let mut stream = response.bytes_stream();
    stream.next().await.unwrap().expect("the stream is live");
    tokio::time::sleep(LOOK_INTERVAL * 4).await; // looks that find it held

    let (mesh_ip, mesh_port) = mesh_addr_of(&b_base);
    support::fake_tailnet().kill(&mesh_ip);
    let (b2, _) = start_node_b(Some(serving)).await;
    support::fake_tailnet().revive(&mesh_ip, mesh_port, b2.local_addr().port());

    let mut got = Vec::new();
    let outcome = tokio::time::timeout(DEADLINE, async {
        loop {
            match stream.next().await {
                Some(Ok(chunk)) => got.extend_from_slice(&chunk),
                Some(Err(err)) => return Err(err.to_string()),
                None => return Ok(()),
            }
        }
    })
    .await
    .expect("the stream ended — never a silent hang");
    assert!(outcome.is_err());
    let body = String::from_utf8_lossy(&got);
    let message = relay_error_event(&body)["error"]["message"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        message.contains("no longer holds it") && message.contains("LeanZero Link restarts"),
        "the reachable peer's answer decided, not its absence: {message}"
    );
    assert!(
        engine.stream_dropped.load(Ordering::SeqCst),
        "the old node's engine stream was released when its Link died"
    );
}

/// The negative control: silence is not death. No response head for many looks, then a gap
/// of many more between chunks — every look finds the request held, and every byte arrives.
#[tokio::test]
async fn a_slow_but_alive_generation_is_never_cut() {
    let slow = Script::SlowAlive {
        head_after: LOOK_INTERVAL * 20,
        gap: LOOK_INTERVAL * 30,
    };
    let (_engine, engine_base) = start_engine(slow).await;
    let (_b, b_base) = start_node_b(Some(serving(engine_base).await)).await;
    let relay = relay_to(fast_call_to(&b_base)).await;

    let response = post_chat(relay.base_url()).await;
    assert_eq!(response.status(), 200);
    let (body, outcome) = drain(response).await;
    assert_eq!(outcome, Ok(()));
    assert_eq!(
        body,
        format!("{CHUNK_1}{CHUNK_2}{CHUNK_END}{DONE}"),
        "byte for byte, nothing appended"
    );
}

/// A peer running an older goose has no look route (`404`): the watch still ends a request
/// whose peer went away, and still never cuts a slow one.
#[tokio::test]
async fn an_older_peer_without_the_look_route_is_ended_only_by_its_absence() {
    let slow = Script::SlowAlive {
        head_after: LOOK_INTERVAL * 10,
        gap: LOOK_INTERVAL * 10,
    };
    for (script, dies) in [(slow, false), (Script::Endless, true)] {
        let (engine, _) = start_engine(script).await;
        let old_peer = Router::new()
            .route("/v1/swarm/inference/v1/chat/completions", post(engine_chat))
            .with_state(engine.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move { axum::serve(listener, old_peer).await.unwrap() });
        let base = format!("http://{}:{port}", support::fake_tailnet().expose(port));
        let call = fast_call_to(&base);
        let bound = unreachable_bound(&call);
        let relay = relay_to(call).await;

        let response = post_chat(relay.base_url()).await;
        if !dies {
            let (body, outcome) = drain(response).await;
            assert_eq!(outcome, Ok(()));
            assert_eq!(body, format!("{CHUNK_1}{CHUNK_2}{CHUNK_END}{DONE}"));
            continue;
        }
        let mut stream = response.bytes_stream();
        stream.next().await.unwrap().expect("the stream is live");
        let killed_at = tokio::time::Instant::now();
        support::fake_tailnet().kill(&mesh_addr_of(&base).0);
        let mut got = Vec::new();
        let ended = tokio::time::timeout(DEADLINE, async {
            while let Some(item) = stream.next().await {
                match item {
                    Ok(chunk) => got.extend_from_slice(&chunk),
                    Err(_) => return,
                }
            }
        })
        .await;
        assert!(ended.is_ok(), "never a silent hang");
        assert!(killed_at.elapsed() <= bound);
        let body = String::from_utf8_lossy(&got);
        assert!(relay_error_event(&body)["error"]["message"]
            .as_str()
            .unwrap()
            .contains("could not reach the peer"));
    }
}
