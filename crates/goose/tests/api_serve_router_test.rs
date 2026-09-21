//! The OpenAI-compatible routes on `goose serve`'s router — the engine the desktop runs — under
//! the ACP secret: 401 without it, 200 with `X-Secret-Key`, 200 with `Authorization: Bearer`, no
//! Origin header required, Bearer refused on `/acp`, 400 on a bad body, 404 on an unknown model.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use axum::Router;
use goose::acp::server_factory::{AcpServer, AcpServerFactoryConfig};
use goose::acp::transport::create_router;
use goose::agents::GoosePlatform;
use tower::ServiceExt;

const SECRET: &str = "serve-test-secret";

fn router(dir: &tempfile::TempDir) -> Router {
    std::env::set_var("GOOSE_DISABLE_KEYRING", "1");
    let server = Arc::new(AcpServer::new(AcpServerFactoryConfig {
        builtins: vec![],
        data_dir: dir.path().join("data"),
        config_dir: dir.path().join("config"),
        goose_platform: GoosePlatform::GooseCli,
        additional_source_roots: Vec::new(),
        scheduler: None,
    }));
    create_router(server, SECRET.to_string(), true, Vec::new())
}

async fn send(
    router: &Router,
    method: Method,
    uri: &str,
    headers: &[(&str, &str)],
    body: Body,
) -> axum::response::Response {
    let mut builder = Request::builder().method(method).uri(uri);
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    router
        .clone()
        .oneshot(builder.body(body).unwrap())
        .await
        .unwrap()
}

async fn body_json(response: axum::response::Response) -> serde_json::Value {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn models_requires_the_secret_and_takes_either_header() {
    let dir = tempfile::tempdir().unwrap();
    let router = router(&dir);

    let unauthenticated = send(&router, Method::GET, "/v1/models", &[], Body::empty()).await;
    assert_eq!(unauthenticated.status(), StatusCode::UNAUTHORIZED);

    let wrong = send(
        &router,
        Method::GET,
        "/v1/models",
        &[("authorization", "Bearer nope")],
        Body::empty(),
    )
    .await;
    assert_eq!(wrong.status(), StatusCode::UNAUTHORIZED);

    // No Origin header on either: a server-to-server client never sends one.
    let with_secret_key = send(
        &router,
        Method::GET,
        "/v1/models",
        &[("x-secret-key", SECRET)],
        Body::empty(),
    )
    .await;
    assert_eq!(with_secret_key.status(), StatusCode::OK);
    let body = body_json(with_secret_key).await;
    assert_eq!(body["object"], "list");
    let ids: Vec<&str> = body["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_str().unwrap())
        .collect();
    assert!(ids.contains(&"swarm"));
    assert!(ids.contains(&"swarm-build"));

    let with_bearer = send(
        &router,
        Method::GET,
        "/v1/models",
        &[("authorization", &format!("Bearer {SECRET}"))],
        Body::empty(),
    )
    .await;
    assert_eq!(with_bearer.status(), StatusCode::OK);
}

#[tokio::test(flavor = "multi_thread")]
async fn bearer_does_not_unlock_the_acp_endpoint() {
    let dir = tempfile::tempdir().unwrap();
    let router = router(&dir);
    let response = send(
        &router,
        Method::POST,
        "/acp",
        &[
            ("authorization", &format!("Bearer {SECRET}")),
            ("content-type", "application/json"),
        ],
        Body::from("{}"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test(flavor = "multi_thread")]
async fn chat_completions_answers_400_and_404_like_goosed() {
    let dir = tempfile::tempdir().unwrap();
    let router = router(&dir);

    let bad = send(
        &router,
        Method::POST,
        "/v1/chat/completions",
        &[
            ("x-secret-key", SECRET),
            ("content-type", "application/json"),
        ],
        Body::from("{not json"),
    )
    .await;
    assert_eq!(bad.status(), StatusCode::BAD_REQUEST);
    assert!(body_json(bad).await["error"]["message"].is_string());

    let unknown = send(
        &router,
        Method::POST,
        "/v1/chat/completions",
        &[
            ("authorization", &format!("Bearer {SECRET}")),
            ("content-type", "application/json"),
        ],
        Body::from(
            r#"{"model":"no-such-provider/model","messages":[{"role":"user","content":"hi"}]}"#,
        ),
    )
    .await;
    assert_eq!(unknown.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        body_json(unknown).await["error"]["message"],
        "model not found"
    );
}
