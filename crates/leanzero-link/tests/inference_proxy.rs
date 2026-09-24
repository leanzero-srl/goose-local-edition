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

fn call_to(base_url: &str) -> PeerCall {
    PeerCall {
        base_url: base_url.to_string(),
        token: TOKEN.to_string(),
        proxy: Some(support::fake_tailnet().proxy()),
        connect_timeout: Duration::from_secs(5),
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
            "chatServingDisabled: \"Allow this Mac to serve chat to linked devices\" is off on {HOSTNAME}"
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
