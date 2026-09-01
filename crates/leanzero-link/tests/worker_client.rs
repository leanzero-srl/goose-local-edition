//! `worker_client` against a mock HTTP server (wiremock): the EXACT request shape
//! (path, method, JSON body, bearer header) each endpoint sends, and every documented
//! status mapped to the right typed error with the worker's `{error}` body carried
//! through verbatim. No live worker, no network.

use leanzero_link::worker_client::{WorkerClient, WorkerError};
use serde_json::json;
use wiremock::matchers::{body_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn client(server: &MockServer) -> WorkerClient {
    WorkerClient::new(server.uri()).expect("client builds")
}

// ── request-code ────────────────────────────────────────────────────────

#[tokio::test]
async fn request_code_success_sends_exact_shape() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/auth/request-code"))
        .and(body_json(json!({ "email": "User@Example.com" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "email": "user@example.com",
            "expiresInSeconds": 600
        })))
        .expect(1)
        .mount(&server)
        .await;

    let result = client(&server)
        .await
        .request_code("User@Example.com")
        .await
        .expect("request-code succeeds");
    assert_eq!(result.email, "user@example.com");
    assert_eq!(result.expires_in_seconds, 600);
}

#[tokio::test]
async fn request_code_429_maps_to_rate_limited_with_body() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/auth/request-code"))
        .respond_with(ResponseTemplate::new(429).set_body_json(json!({
            "error": "rate limited",
            "scope": "email",
            "retryAfterSeconds": 42
        })))
        .mount(&server)
        .await;

    let err = client(&server)
        .await
        .request_code("a@example.com")
        .await
        .expect_err("429 is an error");
    match err {
        WorkerError::RateLimited {
            scope,
            retry_after_seconds,
            error,
        } => {
            assert_eq!(scope, "email");
            assert_eq!(retry_after_seconds, 42);
            assert!(error.contains("rate limited"), "body carried: {error}");
        }
        other => panic!("expected RateLimited, got {other:?}"),
    }
}

#[tokio::test]
async fn request_code_501_maps_to_mail_not_configured() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/auth/request-code"))
        .respond_with(ResponseTemplate::new(501).set_body_json(json!({
            "error": "mail not configured on this deployment"
        })))
        .mount(&server)
        .await;

    let err = client(&server)
        .await
        .request_code("a@example.com")
        .await
        .expect_err("501 is an error");
    match err {
        WorkerError::MailNotConfigured { error } => {
            assert!(
                error.contains("mail not configured"),
                "body carried: {error}"
            )
        }
        other => panic!("expected MailNotConfigured, got {other:?}"),
    }
}

// ── verify ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn verify_success_sends_email_and_code_and_returns_audience_sync() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/auth/verify"))
        .and(body_json(
            json!({ "email": "a@example.com", "code": "123456" }),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "token": "jwt.abc.def",
            "email": "a@example.com",
            "audienceSync": "failed"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let result = client(&server)
        .await
        .verify("a@example.com", "123456")
        .await
        .expect("verify succeeds");
    assert_eq!(result.token, "jwt.abc.def");
    assert_eq!(result.email, "a@example.com");
    // Carried verbatim so the UI can note a sync failure honestly — never dropped.
    assert_eq!(result.audience_sync, "failed");
}

#[tokio::test]
async fn verify_401_maps_to_invalid_code() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/auth/verify"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({
            "error": "invalid or expired code"
        })))
        .mount(&server)
        .await;

    let err = client(&server)
        .await
        .verify("a@example.com", "000000")
        .await
        .expect_err("401 is an error");
    match err {
        WorkerError::InvalidCode { error } => {
            assert!(
                error.contains("invalid or expired code"),
                "body carried: {error}"
            )
        }
        other => panic!("expected InvalidCode, got {other:?}"),
    }
}

#[tokio::test]
async fn verify_429_maps_to_too_many_attempts() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/auth/verify"))
        .respond_with(ResponseTemplate::new(429).set_body_json(json!({
            "error": "too many attempts; request a new code"
        })))
        .mount(&server)
        .await;

    let err = client(&server)
        .await
        .verify("a@example.com", "000000")
        .await
        .expect_err("429 is an error");
    assert!(
        matches!(err, WorkerError::TooManyAttempts { error } if error.contains("too many attempts")),
        "expected TooManyAttempts carrying the body"
    );
}

// ── join-key ────────────────────────────────────────────────────────────

#[tokio::test]
async fn join_key_success_sends_bearer_and_no_body() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/mesh/join-key"))
        .and(header("authorization", "Bearer identity-jwt"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "authKey": "tskey-auth-abc123",
            "expirySeconds": 600
        })))
        .expect(1)
        .mount(&server)
        .await;

    let result = client(&server)
        .await
        .join_key("identity-jwt")
        .await
        .expect("join-key succeeds");
    assert_eq!(result.auth_key, "tskey-auth-abc123");
    assert_eq!(result.expiry_seconds, 600);
}

#[tokio::test]
async fn join_key_401_expired_maps_to_auth_expired() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/mesh/join-key"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({
            "error": "invalid token",
            "reason": "expired"
        })))
        .mount(&server)
        .await;

    let err = client(&server)
        .await
        .join_key("stale-jwt")
        .await
        .expect_err("401 is an error");
    match err {
        WorkerError::AuthExpired { reason, error } => {
            assert_eq!(reason, "expired");
            assert!(error.contains("invalid token"), "body carried: {error}");
        }
        other => panic!("expected AuthExpired, got {other:?}"),
    }
}

#[tokio::test]
async fn join_key_401_bad_signature_maps_to_auth_invalid() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/mesh/join-key"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({
            "error": "invalid token",
            "reason": "bad_signature"
        })))
        .mount(&server)
        .await;

    let err = client(&server)
        .await
        .join_key("forged-jwt")
        .await
        .expect_err("401 is an error");
    match err {
        WorkerError::AuthInvalid { reason, error } => {
            assert_eq!(reason, "bad_signature");
            assert!(error.contains("invalid token"), "body carried: {error}");
        }
        other => panic!("expected AuthInvalid, got {other:?}"),
    }
}

#[tokio::test]
async fn join_key_501_maps_to_mesh_not_configured() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/mesh/join-key"))
        .respond_with(ResponseTemplate::new(501).set_body_json(json!({
            "error": "mesh keys not configured on this deployment"
        })))
        .mount(&server)
        .await;

    let err = client(&server)
        .await
        .join_key("jwt")
        .await
        .expect_err("501 is an error");
    assert!(
        matches!(err, WorkerError::MeshNotConfigured { error } if error.contains("mesh keys not configured")),
        "expected MeshNotConfigured carrying the body"
    );
}

#[tokio::test]
async fn join_key_502_maps_to_unexpected_carrying_status_and_body() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/mesh/join-key"))
        .respond_with(ResponseTemplate::new(502).set_body_json(json!({
            "error": "tailscale key mint failed",
            "status": 0
        })))
        .mount(&server)
        .await;

    let err = client(&server)
        .await
        .join_key("jwt")
        .await
        .expect_err("502 is an error");
    match err {
        WorkerError::Unexpected { status, error, .. } => {
            assert_eq!(status, 502);
            assert!(
                error.contains("tailscale key mint failed"),
                "body carried: {error}"
            );
        }
        other => panic!("expected Unexpected, got {other:?}"),
    }
}

// ── health ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn health_parses_capabilities() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/health"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "version": "0.1.0",
            "capabilities": { "mail": true, "audience": false, "mesh": true }
        })))
        .mount(&server)
        .await;

    let health = client(&server)
        .await
        .health()
        .await
        .expect("health succeeds");
    assert!(health.ok);
    assert_eq!(health.version, "0.1.0");
    assert!(health.capabilities.mail);
    assert!(!health.capabilities.audience);
    assert!(health.capabilities.mesh);
}
