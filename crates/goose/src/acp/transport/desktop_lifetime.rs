use super::auth;
use crate::acp::server_factory::AcpServer;
use axum::{
    extract::State,
    http::{header, HeaderMap, StatusCode},
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use serde::Serialize;
use std::sync::Arc;

#[derive(Clone)]
struct LifetimeState {
    server: Arc<AcpServer>,
    secret: String,
    identity_path: Option<std::path::PathBuf>,
}

#[derive(Serialize)]
struct Lifetime {
    link_host_enabled: bool,
    active_run_ids: Vec<String>,
    detached_runs: bool,
}

pub(super) fn routes(server: Arc<AcpServer>, secret: String) -> Router {
    Router::new()
        .route("/desktop/lifetime", get(lifetime))
        .with_state(LifetimeState {
            server,
            secret,
            identity_path: leanzero_link::identity::default_identity_path().ok(),
        })
}

async fn lifetime(
    State(state): State<LifetimeState>,
    headers: HeaderMap,
) -> axum::response::Response {
    if headers.contains_key(header::ORIGIN) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let token = headers.get("x-secret-key").and_then(|v| v.to_str().ok());
    if state.secret.is_empty()
        || !(auth::token_matches(token, &state.secret)
            || auth::token_matches(auth::bearer_token(&headers), &state.secret))
    {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let intent = state.identity_path.as_ref().ok_or(()).and_then(|path| {
        leanzero_link::intent::IntentStore::beside(path)
            .load()
            .map_err(|_| ())
    });
    let link_host_enabled = match intent {
        Ok(intent) => {
            intent.is_some_and(|i| i.intent == leanzero_link::intent::LinkIntent::Connected)
        }
        Err(()) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({"error":"Link hosting intent is unreadable"})),
            )
                .into_response()
        }
    };
    let active_run_ids = state.server.owner_runs().active_run_ids().await;
    (
        [(header::CACHE_CONTROL, "no-store")],
        Json(Lifetime {
            link_host_enabled,
            active_run_ids,
            detached_runs: true,
        }),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{acp::server_factory::AcpServerFactoryConfig, agents::GoosePlatform};
    use axum::{
        body::{to_bytes, Body},
        http::Request,
    };
    use leanzero_link::intent::{IntentCause, IntentRecord, IntentStore, LinkIntent};
    use tower::ServiceExt;

    #[tokio::test]
    async fn lifetime_requires_local_secret_and_reads_persisted_host_intent_without_a_viewer() {
        let directory = tempfile::tempdir().unwrap();
        let identity_path = directory.path().join("identity.json");
        let server = Arc::new(AcpServer::new(AcpServerFactoryConfig {
            builtins: vec![],
            data_dir: directory.path().into(),
            config_dir: directory.path().into(),
            goose_platform: GoosePlatform::GooseCli,
            additional_source_roots: vec![],
            scheduler: None,
        }));
        let router = Router::new()
            .route("/desktop/lifetime", get(lifetime))
            .with_state(LifetimeState {
                server,
                secret: "fixture-secret".into(),
                identity_path: Some(identity_path.clone()),
            });
        let request = |secret: Option<&str>, origin: Option<&str>| {
            let mut request = Request::builder().uri("/desktop/lifetime");
            if let Some(secret) = secret {
                request = request.header("X-Secret-Key", secret);
            }
            if let Some(origin) = origin {
                request = request.header(header::ORIGIN, origin);
            }
            request.body(Body::empty()).unwrap()
        };
        assert_eq!(
            router
                .clone()
                .oneshot(request(None, None))
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            router
                .clone()
                .oneshot(request(Some("wrong"), None))
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            router
                .clone()
                .oneshot(request(Some("fixture-secret"), Some("http://localhost")))
                .await
                .unwrap()
                .status(),
            StatusCode::FORBIDDEN
        );
        let intent = IntentStore::beside(&identity_path);
        intent
            .save(&IntentRecord::new(
                LinkIntent::Connected,
                IntentCause::UserConnect,
            ))
            .unwrap();
        let response = router
            .clone()
            .oneshot(request(Some("fixture-secret"), None))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        let body: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap())
                .unwrap();
        assert_eq!(
            body,
            serde_json::json!({"link_host_enabled":true,"active_run_ids":[],"detached_runs":true})
        );
        intent
            .save(&IntentRecord::new(
                LinkIntent::Disconnected,
                IntentCause::UserDisconnect,
            ))
            .unwrap();
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/desktop/lifetime")
                    .header(header::AUTHORIZATION, "Bearer fixture-secret")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap())
                .unwrap();
        assert_eq!(body["link_host_enabled"], false);
        std::fs::write(intent.path(), "corrupt").unwrap();
        assert_eq!(
            router
                .oneshot(request(Some("fixture-secret"), None))
                .await
                .unwrap()
                .status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
    }
}
