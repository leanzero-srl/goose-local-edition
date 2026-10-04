#[allow(dead_code)]
#[path = "acp_common_tests/mod.rs"]
mod common_tests;

use common_tests::fixtures::server::AcpServerConnection;
use common_tests::fixtures::{run_test, send_custom, Connection, TestConnectionConfig};
use goose::config::paths::Paths;
use goose_test_support::EnforceSessionId;
use serde_json::json;
use serial_test::serial;
use std::sync::Arc;
use wiremock::matchers::{body_partial_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// The four OpenRouter host methods end to end over ACP: the pin the desktop picker shows and
/// writes is config.yaml's OPENROUTER_PARAMETERS, and the listing and the probe carry the saved key
/// as the bearer (the key never crosses into the renderer).
#[test]
#[serial]
fn the_host_picker_reads_and_writes_the_benchmark_drivers_pin_and_probes_with_the_saved_key() {
    let root = tempfile::tempdir().unwrap();
    let root_path = root.path().to_string_lossy().to_string();
    let _env = env_lock::lock_env([
        ("GOOSE_PATH_ROOT", Some(root_path.as_str())),
        ("GOOSE_DISABLE_KEYRING", Some("1")),
        ("OPENROUTER_API_KEY", Some("or-saved-key")),
        ("OPENROUTER_HOST", None),
        ("OPENROUTER_PARAMETERS", None),
    ]);
    let config_dir = Paths::config_dir();
    std::fs::create_dir_all(&config_dir).unwrap();
    let config_path = config_dir.join(goose::config::base::CONFIG_YAML_NAME);

    run_test(async move {
        let openrouter = MockServer::start().await;
        std::fs::write(
            &config_path,
            format!(
                "GOOSE_PROVIDER: openrouter\nGOOSE_MODEL: qwen/qwen3.8-27b\nOPENROUTER_HOST: {}\n",
                openrouter.uri()
            ),
        )
        .unwrap();
        Mock::given(method("GET"))
            .and(path("/api/v1/models/qwen/qwen3.8-27b/endpoints"))
            .and(header("Authorization", "Bearer or-saved-key"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {"endpoints": [
                {"provider_name": "Wafer", "tag": "wafer", "quantization": "fp8",
                 "context_length": 262144, "supported_parameters": ["tools"], "uptime_last_30m": 99.2},
                {"provider_name": "DeepInfra", "tag": "deepinfra/bf16",
                 "supported_parameters": ["max_tokens"]}
            ]}})))
            .mount(&openrouter)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/v1/chat/completions"))
            .and(header("Authorization", "Bearer or-saved-key"))
            .and(body_partial_json(
                json!({"provider": {"order": ["wafer"], "allow_fallbacks": false}}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{"finish_reason": "tool_calls", "message": {"role": "assistant",
                    "tool_calls": [{"id": "c1", "type": "function",
                        "function": {"name": "ping", "arguments": "{\"x\":1}"}}]}}],
                "usage": {"completion_tokens": 80}
            })))
            .expect(1)
            .mount(&openrouter)
            .await;

        let fixture = common_tests::fixtures::OpenAiFixture::new(
            vec![],
            Arc::new(EnforceSessionId::default()),
        )
        .await;
        let conn = AcpServerConnection::new(
            TestConnectionConfig {
                data_root: config_dir.clone(),
                ..Default::default()
            },
            fixture,
        )
        .await;

        let read = send_custom(conn.cx(), "_goose/unstable/openrouter/pin/read", json!({}))
            .await
            .unwrap();
        assert_eq!(read, json!({"pin": {}}));

        let set = send_custom(
            conn.cx(),
            "_goose/unstable/openrouter/pin/set",
            json!({"tag": "wafer"}),
        )
        .await
        .unwrap();
        assert_eq!(set["pin"]["tag"], "wafer");
        let text = std::fs::read_to_string(&config_path).unwrap();
        assert!(
            text.lines().any(|line| line
                == r#"OPENROUTER_PARAMETERS: '{"provider":{"order":["wafer"],"allow_fallbacks":false}}'"#),
            "{text}"
        );

        let hosts = send_custom(
            conn.cx(),
            "_goose/unstable/openrouter/hosts/list",
            json!({"model": "qwen/qwen3.8-27b"}),
        )
        .await
        .unwrap();
        assert_eq!(hosts["hosts"][0]["tag"], "wafer");
        assert_eq!(hosts["hosts"][0]["supportsTools"], true);
        assert_eq!(hosts["hosts"][0]["contextLength"], 262144);
        assert_eq!(hosts["hosts"][1]["supportsTools"], false);

        let probe = send_custom(
            conn.cx(),
            "_goose/unstable/openrouter/hosts/probe",
            json!({"model": "qwen/qwen3.8-27b", "tag": "wafer"}),
        )
        .await
        .unwrap();
        assert_eq!(probe["toolCall"], true, "{probe}");
        assert_eq!(probe["finishReason"], "tool_calls");
        assert_eq!(probe["completionTokens"], 80);
        assert!(probe.get("error").is_none(), "{probe}");

        let refused = send_custom(
            conn.cx(),
            "_goose/unstable/openrouter/hosts/list",
            json!({"model": "../../v1/keys"}),
        )
        .await;
        assert!(refused.is_err(), "{refused:?}");

        let cleared = send_custom(conn.cx(), "_goose/unstable/openrouter/pin/set", json!({}))
            .await
            .unwrap();
        assert_eq!(cleared, json!({"pin": {}}));
        let text = std::fs::read_to_string(&config_path).unwrap();
        assert!(!text.contains("OPENROUTER_PARAMETERS"), "{text}");
        assert!(text.contains("OPENROUTER_HOST"), "{text}");
    });
}
