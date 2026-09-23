//! The OpenAI-compatible endpoint flow the desktop's Cloud Providers tab drives, end to end against a
//! real HTTP server: create the endpoint with NO models, ask the ENGINE for its model list, save a
//! default (the engine runs it once — the proof), then write the list back. Every request the stub
//! answers must carry the endpoint's key and custom header, because that is what chat will send.

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
use wiremock::matchers::{body_partial_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn sse_reply(model: &str, text: &str) -> String {
    let chunk = serde_json::json!({
        "id": "chatcmpl-stub", "object": "chat.completion.chunk", "created": 1, "model": model,
        "choices": [{"index": 0, "delta": {"role": "assistant", "content": text}, "finish_reason": null}]
    });
    let done = serde_json::json!({
        "id": "chatcmpl-stub", "object": "chat.completion.chunk", "created": 1, "model": model,
        "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}],
        "usage": {"prompt_tokens": 5, "completion_tokens": 1, "total_tokens": 6}
    });
    format!("data: {chunk}\n\ndata: {done}\n\ndata: [DONE]\n\n")
}

/// The ACP server's provider door backed by the REAL registry (the harness defaults to a recorded
/// OpenAI fixture), so a listing reaches the endpoint the way it does in the app.
fn registry_factory() -> AcpProviderFactory {
    Arc::new(|name, extensions, _working_dir| {
        Box::pin(async move { goose::providers::create(&name, extensions).await })
    })
}

#[test]
#[serial]
fn an_openai_compatible_endpoint_is_created_listed_proven_and_selectable() {
    let root = tempfile::tempdir().unwrap();
    let root_path = root.path().to_string_lossy().to_string();
    let _env = env_lock::lock_env([
        ("GOOSE_PATH_ROOT", Some(root_path.as_str())),
        ("GOOSE_DISABLE_KEYRING", Some("1")),
        ("CUSTOM_TEAM_GATEWAY_API_KEY", None),
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
        let stub = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .and(header("Authorization", "Bearer gateway-key"))
            .and(header("X-Team", "platform"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "object": "list",
                "data": [{"id": "qwen3-coder"}, {"id": "llama-4-scout"}]
            })))
            .mount(&stub)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .and(header("Authorization", "Bearer gateway-key"))
            .and(header("X-Team", "platform"))
            .and(body_partial_json(
                serde_json::json!({"model": "qwen3-coder"}),
            ))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(sse_reply("qwen3-coder", "OK")),
            )
            .expect(1)
            .mount(&stub)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .and(body_partial_json(
                serde_json::json!({"model": "llama-4-scout"}),
            ))
            .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
                "error": {"message": "model llama-4-scout is not enabled for this key"}
            })))
            .mount(&stub)
            .await;

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

        let created = send_custom(
            conn.cx(),
            "_goose/unstable/providers/custom/create",
            serde_json::json!({
                "engine": "openai_compatible",
                "displayName": "Team Gateway",
                "apiUrl": format!("{}/v1", stub.uri()),
                "apiKey": "gateway-key",
                "models": [],
                "headers": {"X-Team": "platform"},
                "requiresAuth": true
            }),
        )
        .await
        .expect("an endpoint is created before its models are known");
        let provider_id = created["providerId"].as_str().unwrap().to_string();
        assert_eq!(provider_id, "custom_team_gateway");

        let listed = send_custom(
            conn.cx(),
            "_goose/unstable/providers/supported-models/list",
            serde_json::json!({ "providerId": provider_id }),
        )
        .await
        .expect("the engine lists the endpoint's models with its key and header");
        assert_eq!(
            listed["models"],
            serde_json::json!(["llama-4-scout", "qwen3-coder"])
        );

        let rejected = send_custom(
            conn.cx(),
            "_goose/unstable/providers/config/save",
            serde_json::json!({ "providerId": provider_id, "fields": [], "testModel": "llama-4-scout" }),
        )
        .await
        .expect_err("a default the endpoint refuses to run is not saved");
        let rejected = format!("{rejected:?}");
        assert!(
            rejected.contains("not enabled for this key"),
            "the endpoint's own words reach the dialog: {rejected}"
        );

        send_custom(
            conn.cx(),
            "_goose/unstable/providers/config/save",
            serde_json::json!({ "providerId": provider_id, "fields": [], "testModel": "qwen3-coder" }),
        )
        .await
        .expect("the chosen default runs once and is saved");

        let status = send_custom(
            conn.cx(),
            "_goose/unstable/providers/config/status",
            serde_json::json!({ "providerIds": [provider_id] }),
        )
        .await
        .unwrap();
        assert_eq!(
            status["statuses"][0],
            serde_json::json!({
                "providerId": provider_id,
                "isConfigured": true,
                "testModel": "qwen3-coder",
                "connectionChecked": true,
                "connectionError": null,
            }),
            "the proven default is what every model picker starts from"
        );

        send_custom(
            conn.cx(),
            "_goose/unstable/providers/custom/update",
            serde_json::json!({
                "providerId": provider_id,
                "engine": "openai_compatible",
                "displayName": "Team Gateway",
                "apiUrl": format!("{}/v1", stub.uri()),
                "models": ["qwen3-coder", "llama-4-scout"],
                "headers": {"X-Team": "platform"},
                "requiresAuth": true
            }),
        )
        .await
        .expect("the listed models are written back with the stored key kept");

        let inventory = send_custom(
            conn.cx(),
            "_goose/unstable/providers/list",
            serde_json::json!({ "providerIds": [provider_id] }),
        )
        .await
        .unwrap();
        let entry = &inventory["entries"][0];
        assert_eq!(entry["providerType"], "Custom");
        assert_eq!(entry["configured"], true);
        assert_eq!(entry["defaultModel"], "qwen3-coder");
        assert_eq!(
            Config::global()
                .get_secret::<String>("CUSTOM_TEAM_GATEWAY_API_KEY")
                .unwrap(),
            "gateway-key"
        );

        let local = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"data": [{"id": "local-model"}]})),
            )
            .mount(&local)
            .await;

        let keyless = send_custom(
            conn.cx(),
            "_goose/unstable/providers/custom/create",
            serde_json::json!({
                "engine": "openai_compatible",
                "displayName": "Desk Server",
                "apiUrl": local.uri(),
                "models": [],
                "headers": {},
                "requiresAuth": false
            }),
        )
        .await
        .expect("a keyless endpoint is created");
        let keyless_id = keyless["providerId"].as_str().unwrap().to_string();
        assert_eq!(keyless["status"]["isConfigured"], true);

        let listed = send_custom(
            conn.cx(),
            "_goose/unstable/providers/supported-models/list",
            serde_json::json!({ "providerId": keyless_id }),
        )
        .await
        .expect("a base URL without /v1 still lists from <base>/v1/models");
        assert_eq!(listed["models"], serde_json::json!(["local-model"]));
        let requests = local.received_requests().await.unwrap();
        assert!(requests
            .iter()
            .all(|request| !request.headers.contains_key("authorization")));

        send_custom(
            conn.cx(),
            "_goose/unstable/providers/custom/delete",
            serde_json::json!({ "providerId": provider_id }),
        )
        .await
        .expect("an endpoint can be removed");
        assert!(!Paths::config_dir()
            .join("custom_providers")
            .join(format!("{provider_id}.json"))
            .exists());
    });
}
