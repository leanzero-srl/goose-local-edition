//! The OpenAI-compatible routes through the real router + auth middleware: 401 without the secret,
//! 200 with `X-Secret-Key`, 200 with `Authorization: Bearer <same secret>`, Bearer refused elsewhere,
//! 400 on a bad body, 404 on an unknown model.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::middleware;
use goose_server::auth::check_token;
use goose_server::routes::configure;
use goose_server::state::AppState;
use tower::ServiceExt;

const SECRET: &str = "test-secret";

async fn app() -> axum::Router {
    std::env::set_var("GOOSE_DISABLE_KEYRING", "1");
    let state = AppState::new(true).await.unwrap();
    configure(state, SECRET.to_string()).layer(middleware::from_fn_with_state(
        SECRET.to_string(),
        check_token,
    ))
}

async fn body_json(response: axum::response::Response) -> serde_json::Value {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn models_requires_the_secret() {
    let response = app()
        .await
        .oneshot(
            Request::builder()
                .uri("/v1/models")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test(flavor = "multi_thread")]
async fn models_accepts_x_secret_key() {
    let response = app()
        .await
        .oneshot(
            Request::builder()
                .uri("/v1/models")
                .header("x-secret-key", SECRET)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    assert_eq!(body["object"], "list");
    let ids: Vec<&str> = body["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_str().unwrap())
        .collect();
    assert!(ids.contains(&"swarm"));
    assert!(ids.contains(&"swarm-build"));
    assert!(body["data"]
        .as_array()
        .unwrap()
        .iter()
        .all(|m| m["object"] == "model" && m["owned_by"] == "goose"));
}

#[tokio::test(flavor = "multi_thread")]
async fn models_accepts_bearer_with_the_same_secret() {
    let response = app()
        .await
        .oneshot(
            Request::builder()
                .uri("/v1/models")
                .header("authorization", format!("Bearer {}", SECRET))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test(flavor = "multi_thread")]
async fn bearer_does_not_unlock_other_routes() {
    let response = app()
        .await
        .oneshot(
            Request::builder()
                .uri("/config")
                .header("authorization", format!("Bearer {}", SECRET))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test(flavor = "multi_thread")]
async fn wrong_bearer_is_rejected() {
    let response = app()
        .await
        .oneshot(
            Request::builder()
                .uri("/v1/models")
                .header("authorization", "Bearer nope")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test(flavor = "multi_thread")]
async fn chat_completions_rejects_a_bad_body_with_400() {
    let response = app()
        .await
        .oneshot(
            Request::builder()
                .uri("/v1/chat/completions")
                .method("POST")
                .header("content-type", "application/json")
                .header("x-secret-key", SECRET)
                .body(Body::from("{not json"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = body_json(response).await;
    assert!(body["error"]["message"].is_string());
}

#[tokio::test(flavor = "multi_thread")]
async fn chat_completions_unknown_model_is_404() {
    let response = app()
            .await
            .oneshot(
                Request::builder()
                    .uri("/v1/chat/completions")
                    .method("POST")
                    .header("content-type", "application/json")
                    .header("x-secret-key", SECRET)
                    .body(Body::from(
                        r#"{"model":"no-such-provider/model","messages":[{"role":"user","content":"hi"}]}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body = body_json(response).await;
    assert_eq!(body["error"]["message"], "model not found");
}
