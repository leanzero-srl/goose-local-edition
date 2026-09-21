//! CogniRunner task mode through `goose serve`'s router, end to end: a mock OpenAI provider
//! (wiremock, SSE) plays the model, a local axum receiver plays CogniRunner's callback, and the
//! test asserts the pushes arrive in order, with strictly increasing `seq`, each one carrying a
//! valid `x-cognirunner-signature`, ending in `done` — and that the ephemeral session is gone.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::http::{HeaderMap, Method, Request, StatusCode};
use axum::routing::post;
use axum::Router;
use goose::acp::server_factory::{AcpServer, AcpServerFactoryConfig};
use goose::acp::transport::create_router;
use goose::agents::GoosePlatform;
use goose::api::cognirunner::signature;
use serde_json::{json, Value};
use tower::ServiceExt;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const SECRET: &str = "serve-test-secret";
const CALLBACK_SECRET: &str = "cb-secret-42";
const MODEL: &str = "openai/gpt-4o-mini";
const REPLY: &str = "hello from the mock model";

/// One push as the receiver saw it: the signature header and the raw body.
type Push = (Option<String>, Bytes);

#[derive(Clone, Default)]
struct Received(Arc<Mutex<Vec<Push>>>);

async fn callback_receiver() -> (String, Received) {
    let received = Received::default();
    let sink = received.clone();
    let app = Router::new().route(
        "/events",
        post(move |headers: HeaderMap, body: Bytes| {
            let sink = sink.clone();
            async move {
                let sig = headers
                    .get(signature::SIGNATURE_HEADER)
                    .and_then(|v| v.to_str().ok())
                    .map(str::to_string);
                sink.0.lock().unwrap().push((sig, body));
                StatusCode::ACCEPTED
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}/events"), received)
}

async fn mock_openai() -> MockServer {
    let server = MockServer::start().await;
    let sse = format!(
        "data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
        json!({
            "id": "chatcmpl-test",
            "model": "gpt-4o-mini",
            "created": 1755133833,
            "choices": [{ "index": 0, "delta": { "role": "assistant", "content": REPLY } }]
        }),
        json!({
            "id": "chatcmpl-test",
            "model": "gpt-4o-mini",
            "created": 1755133833,
            "choices": [],
            "usage": { "prompt_tokens": 8, "completion_tokens": 10, "total_tokens": 18 }
        })
    );
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(sse)
                .insert_header("content-type", "text/event-stream"),
        )
        .mount(&server)
        .await;
    server
}

fn serve_router(dir: &tempfile::TempDir) -> (Arc<AcpServer>, Router) {
    let server = Arc::new(AcpServer::new(AcpServerFactoryConfig {
        builtins: vec![],
        data_dir: dir.path().join("data"),
        config_dir: dir.path().join("config"),
        goose_platform: GoosePlatform::GooseCli,
        additional_source_roots: Vec::new(),
        scheduler: None,
    }));
    let router = create_router(server.clone(), SECRET.to_string(), true, Vec::new());
    (server, router)
}

async fn send(
    router: &Router,
    method: Method,
    uri: &str,
    auth: bool,
    body: Value,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json");
    if auth {
        builder = builder.header("authorization", format!("Bearer {SECRET}"));
    }
    let response = router
        .clone()
        .oneshot(builder.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    (status, value)
}

#[tokio::test(flavor = "multi_thread")]
async fn a_task_pushes_started_text_done_in_order_with_valid_signatures() {
    let dir = tempfile::tempdir().unwrap();
    let mock = mock_openai().await;
    std::env::set_var("GOOSE_PATH_ROOT", dir.path());
    std::env::set_var("GOOSE_DISABLE_KEYRING", "1");
    std::env::set_var("GOOSE_DISABLE_SESSION_NAMING", "true");
    std::env::set_var("GOOSE_MODE", "auto");
    std::env::set_var("OPENAI_API_KEY", "test-key");
    std::env::set_var("OPENAI_HOST", mock.uri());

    let (server, router) = serve_router(&dir);
    let (callback_url, received) = callback_receiver().await;

    // Refusals first, so a later 202 is known to have passed every gate.
    let (status, _) = send(
        &router,
        Method::POST,
        "/cognirunner/tasks",
        false,
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let task = |model: &str, url: &str| {
        json!({
            "prompt": "Say hello.",
            "model": model,
            "callbackUrl": url,
            "callbackSecret": CALLBACK_SECRET,
            "threadId": "thread-1",
        })
    };
    let (status, body) = send(
        &router,
        Method::POST,
        "/cognirunner/tasks",
        true,
        task("no-such-provider/model", &callback_url),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    let (status, _) = send(
        &router,
        Method::POST,
        "/cognirunner/tasks",
        true,
        task(MODEL, "ftp://nope"),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = send(
        &router,
        Method::GET,
        "/cognirunner/tasks/task_missing",
        true,
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // The task.
    let (status, body) = send(
        &router,
        Method::POST,
        "/cognirunner/tasks",
        true,
        task(MODEL, &callback_url),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let task_id = body["taskId"].as_str().unwrap().to_string();
    let session_id = body["sessionId"].as_str().unwrap().to_string();
    assert!(task_id.starts_with("task_"));

    // Wait for `done` to arrive at the receiver.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    let events = loop {
        let events: Vec<Value> = received
            .0
            .lock()
            .unwrap()
            .iter()
            .flat_map(|(_, body)| serde_json::from_slice::<Vec<Value>>(body).unwrap())
            .collect();
        if events
            .iter()
            .any(|e| e["type"] == "done" || e["type"] == "failed")
        {
            break events;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "no terminal event within 30s; got {events:?}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    };

    // Every push signed over its raw body, and every push a non-empty array.
    let pushes = received.0.lock().unwrap().clone();
    assert!(!pushes.is_empty());
    for (sig, body) in &pushes {
        let sig = sig.as_deref().expect("signature header present");
        assert!(
            signature::verify(CALLBACK_SECRET, body, sig),
            "signature did not verify for {}",
            String::from_utf8_lossy(body)
        );
        assert!(!signature::verify("wrong", body, sig));
        assert!(!serde_json::from_slice::<Vec<Value>>(body)
            .unwrap()
            .is_empty());
    }

    // Order, and seq strictly increasing from 1 across pushes.
    let types: Vec<&str> = events.iter().map(|e| e["type"].as_str().unwrap()).collect();
    assert_eq!(types.first(), Some(&"started"), "{types:?}");
    assert_eq!(types.last(), Some(&"done"), "{types:?}");
    assert!(types.contains(&"text"), "{types:?}");
    for (i, event) in events.iter().enumerate() {
        assert_eq!(event["seq"].as_u64(), Some(i as u64 + 1), "{events:?}");
        assert_eq!(event["taskId"], task_id);
        assert_eq!(event["threadId"], "thread-1");
        assert!(event["at"].is_string());
    }
    let started = &events[0];
    assert_eq!(started["model"], MODEL);
    assert_eq!(started["sessionId"], session_id);
    let text: String = events
        .iter()
        .filter(|e| e["type"] == "text")
        .map(|e| e["text"].as_str().unwrap().to_string())
        .collect();
    assert!(text.contains(REPLY), "{text}");
    let done = events.last().unwrap();
    assert_eq!(done["finishReason"], "stop");
    assert!(done["usage"]["totalTokens"].is_number());

    // Reconciliation reads the same numbers.
    let (status, body) = send(
        &router,
        Method::GET,
        &format!("/cognirunner/tasks/{task_id}"),
        true,
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "done");
    assert_eq!(body["seq"].as_u64(), Some(events.len() as u64));
    assert_eq!(body["sessionId"], session_id);
    assert!(body["lastEventAt"].is_string());

    // A finished task refuses a steer; cancel is idempotent.
    let (status, _) = send(
        &router,
        Method::POST,
        &format!("/cognirunner/tasks/{task_id}/messages"),
        true,
        json!({ "text": "too late" }),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, body) = send(
        &router,
        Method::POST,
        &format!("/cognirunner/tasks/{task_id}/cancel"),
        true,
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(body["status"], "done");

    // The ephemeral session was deleted (no keep header).
    let manager = server.agent_manager().await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(manager
        .session_manager()
        .get_session(&session_id, false)
        .await
        .is_err());
}
