//! The connection check the desktop runs at every launch (`providers/config/status` with
//! `checkConnections`) against saved settings. 2026-10-20: every Mac's config points OPENAI_HOST at
//! goose's own MLX engine on 127.0.0.1:8090, which is up only while a local model is loaded, and
//! every launch toasted "OpenAI could not list models: Network error: Could not connect to
//! 127.0.0.1:8090". A loopback server that is not started is a state, not a failure; a broken key,
//! a loopback server that answers with a refusal, and a remote host that cannot be reached still are.

#[allow(dead_code)]
#[path = "acp_common_tests/mod.rs"]
mod common_tests;

use common_tests::fixtures::server::AcpServerConnection;
use common_tests::fixtures::{run_test, send_custom, Connection, TestConnectionConfig};
use goose::acp::server::AcpProviderFactory;
use goose::config::base::CONFIG_YAML_NAME;
use goose::config::paths::Paths;
use goose::config::Config;
use goose_test_support::EnforceSessionId;
use serial_test::serial;
use std::sync::Arc;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn registry_factory() -> AcpProviderFactory {
    Arc::new(|name, extensions, _working_dir| {
        Box::pin(async move { goose::providers::create(&name, extensions).await })
    })
}

/// A loopback port nothing listens on: bound by the OS, then released.
fn closed_loopback_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

#[test]
#[serial]
fn a_local_server_that_is_not_started_never_reads_as_a_failed_connection() {
    let root = tempfile::tempdir().unwrap();
    let root_path = root.path().to_string_lossy().to_string();
    let engine_port = closed_loopback_port();
    let desk_port = closed_loopback_port();
    let openai_host = format!("http://127.0.0.1:{engine_port}");
    let _env = env_lock::lock_env([
        ("GOOSE_PATH_ROOT", Some(root_path.as_str())),
        ("GOOSE_DISABLE_KEYRING", Some("1")),
        ("GOOSE_DISABLE_SESSION_NAMING", Some("true")),
        ("OPENAI_HOST", Some(openai_host.as_str())),
        ("OPENAI_BASE_URL", None),
        ("OPENAI_API_KEY", Some("sk-local-engine")),
        ("ANTHROPIC_API_KEY", Some("sk-ant-revoked")),
        ("ANTHROPIC_HOST", None),
    ]);
    let config_dir = Paths::config_dir();
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(
        config_dir.join(CONFIG_YAML_NAME),
        "GOOSE_MODEL: gpt-4o\nGOOSE_PROVIDER: openai\nGOOSE_DISABLE_KEYRING: true\n",
    )
    .unwrap();
    Config::global().invalidate_secrets_cache();

    run_test(async move {
        let refusing = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
                "error": {"type": "authentication_error", "message": "invalid x-api-key"}
            })))
            .mount(&refusing)
            .await;
        Config::global()
            .set_param("ANTHROPIC_HOST", refusing.uri())
            .unwrap();

        let openai = common_tests::fixtures::OpenAiFixture::new(
            vec![],
            Arc::new(EnforceSessionId::default()),
        )
        .await;
        let conn = AcpServerConnection::new(
            TestConnectionConfig {
                data_root: Paths::config_dir(),
                provider_factory: Some(registry_factory()),
                ..Default::default()
            },
            openai,
        )
        .await;

        let mut endpoints = Vec::new();
        for (name, api_url) in [
            ("Desk vLLM", format!("http://localhost:{desk_port}/v1")),
            ("Running Gateway", format!("{}/v1", refusing.uri())),
            (
                "Remote Gateway",
                "http://goose-endpoint-down.invalid/v1".to_string(),
            ),
        ] {
            let created = send_custom(
                conn.cx(),
                "_goose/unstable/providers/custom/create",
                serde_json::json!({
                    "engine": "openai_compatible",
                    "displayName": name,
                    "apiUrl": api_url,
                    "apiKey": "endpoint-key",
                    "models": [],
                    "requiresAuth": true
                }),
            )
            .await
            .expect("an endpoint is created before its server is reachable");
            endpoints.push(created["providerId"].as_str().unwrap().to_string());
        }
        let [desk, running, remote] = endpoints.clone().try_into().unwrap();

        let mut ids = vec!["openai".to_string(), "anthropic".to_string()];
        ids.extend(endpoints);
        let status = send_custom(
            conn.cx(),
            "_goose/unstable/providers/config/status",
            serde_json::json!({ "providerIds": ids, "checkConnections": true }),
        )
        .await
        .unwrap();
        let of = |id: &str| -> serde_json::Value {
            status["statuses"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["providerId"] == id)
                .unwrap_or_else(|| panic!("no status for {id}: {status}"))
                .clone()
        };

        let engine = of("openai");
        assert_eq!(engine["isConfigured"], true, "{engine}");
        assert_eq!(engine["connectionChecked"], true, "{engine}");
        assert_eq!(
            engine["connectionError"],
            serde_json::Value::Null,
            "a stopped local engine is not a failed connection: {engine}"
        );
        assert_eq!(
            engine["localServerNotRunning"],
            serde_json::json!(format!("http://127.0.0.1:{engine_port}")),
            "{engine}"
        );

        let desk = of(&desk);
        assert_eq!(desk["connectionError"], serde_json::Value::Null, "{desk}");
        assert_eq!(
            desk["localServerNotRunning"],
            serde_json::json!(format!("http://localhost:{desk_port}")),
            "{desk}"
        );

        let anthropic = of("anthropic");
        let refused = anthropic["connectionError"].as_str().unwrap_or_default();
        assert!(
            refused.contains("invalid x-api-key"),
            "a revoked cloud key still fails at startup: {anthropic}"
        );
        assert!(
            anthropic.get("localServerNotRunning").is_none(),
            "{anthropic}"
        );

        let running = of(&running);
        assert!(
            running["connectionError"]
                .as_str()
                .is_some_and(|error| error.contains("401")),
            "a loopback server that answers with a refusal is a failure: {running}"
        );
        assert!(running.get("localServerNotRunning").is_none(), "{running}");

        let remote = of(&remote);
        assert!(
            remote["connectionError"]
                .as_str()
                .is_some_and(|error| error.contains("Could not connect")),
            "a remote endpoint that cannot be reached is a failure: {remote}"
        );
        assert!(remote.get("localServerNotRunning").is_none(), "{remote}");

        let rejected = send_custom(
            conn.cx(),
            "_goose/unstable/providers/config/save",
            serde_json::json!({ "providerId": desk["providerId"], "fields": [] }),
        )
        .await
        .expect_err("a save still proves the connection, local or not");
        assert!(
            format!("{rejected:?}").contains("Could not connect"),
            "{rejected:?}"
        );
    });
}
