//! Per-model custom fields over ACP, end to end against a stub of OpenRouter's models listing: the
//! desktop asks the ENGINE for a model's fields, saves a value, the store refuses a value the model
//! does not declare, and the saved effort becomes that model's request param — the one OpenRouter's
//! request carries as `reasoning: {effort}` (the wire half is
//! `providers::openrouter::model_field_tests::a_saved_effort_reaches_the_wire_as_reasoning_effort`).

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
use serde_json::json;
use serial_test::serial;
use std::sync::Arc;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn registry_factory() -> AcpProviderFactory {
    Arc::new(|name, extensions, _working_dir| {
        Box::pin(async move { goose::providers::create(&name, extensions).await })
    })
}

const MODEL: &str = "deepseek/deepseek-v4.1-flash";

#[test]
#[serial]
fn a_models_fields_come_from_the_listing_save_per_model_and_reach_its_request_params() {
    let root = tempfile::tempdir().unwrap();
    let root_path = root.path().to_string_lossy().to_string();
    let _env = env_lock::lock_env([
        ("GOOSE_PATH_ROOT", Some(root_path.as_str())),
        ("GOOSE_DISABLE_KEYRING", Some("1")),
        ("OPENROUTER_API_KEY", Some("or-test-key")),
        ("OPENROUTER_HOST", None),
        ("GOOSE_MODEL_FIELDS", None),
        ("GOOSE_THINKING_EFFORT", None),
    ]);

    run_test(async move {
        let stub = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/models"))
            .and(header("Authorization", "Bearer or-test-key"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": [
                {"id": MODEL, "context_length": 1048576,
                 "supported_parameters": ["include_reasoning", "max_tokens", "reasoning",
                                          "temperature", "tools", "top_k", "top_p"],
                 "default_parameters": {},
                 "reasoning": {"mandatory": false, "supported_efforts": ["max", "high", "low"],
                               "default_effort": "high"}},
                {"id": "vendor/plain", "supported_parameters": ["tools"]}
            ]})))
            .mount(&stub)
            .await;
        let config_dir = Paths::config_dir();
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join(CONFIG_YAML_NAME),
            format!(
                "GOOSE_MODEL: gpt-4o\nGOOSE_PROVIDER: openai\nGOOSE_DISABLE_KEYRING: true\n\
                 OPENROUTER_HOST: {}\n",
                stub.uri()
            ),
        )
        .unwrap();
        Config::global().invalidate_secrets_cache();

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

        let listed = send_custom(
            conn.cx(),
            "_goose/unstable/providers/model-fields/list",
            json!({ "providerId": "openrouter", "modelId": MODEL }),
        )
        .await
        .expect("the engine reads the model's fields from OpenRouter's listing");
        assert_eq!(listed["source"], json!("provider_metadata"));
        let ids: Vec<&str> = listed["fields"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, vec!["effort", "temperature", "top_p", "top_k"]);
        assert_eq!(
            listed["fields"][0]["kind"],
            json!({"type": "select", "options": ["max", "high", "low"]})
        );
        assert_eq!(listed["fields"][0]["modelDefault"], json!("high"));
        assert_eq!(listed["values"], json!({}));

        let plain = send_custom(
            conn.cx(),
            "_goose/unstable/providers/model-fields/list",
            json!({ "providerId": "openrouter", "modelId": "vendor/plain" }),
        )
        .await
        .unwrap();
        assert_eq!(plain["fields"], json!([]));
        assert_eq!(plain["source"], json!("none"));

        let unlisted = send_custom(
            conn.cx(),
            "_goose/unstable/providers/model-fields/list",
            json!({ "providerId": "openrouter", "modelId": "vendor/typo" }),
        )
        .await
        .unwrap();
        assert_eq!(unlisted["source"], json!("unlisted_model"));

        let refused = send_custom(
            conn.cx(),
            "_goose/unstable/providers/model-fields/save",
            json!({ "providerId": "openrouter", "modelId": MODEL,
                    "values": {"effort": "medium"} }),
        )
        .await
        .expect_err("a level the listing does not declare for this model is refused");
        assert!(format!("{refused:?}").contains("not one of"), "{refused:?}");

        let saved = send_custom(
            conn.cx(),
            "_goose/unstable/providers/model-fields/save",
            json!({ "providerId": "openrouter", "modelId": MODEL,
                    "values": {"effort": "low", "top_k": 40, "temperature": null} }),
        )
        .await
        .expect("declared values are saved");
        assert_eq!(saved["values"], json!({"effort": "low", "top_k": 40}));

        let relisted = send_custom(
            conn.cx(),
            "_goose/unstable/providers/model-fields/list",
            json!({ "providerId": "openrouter", "modelId": MODEL }),
        )
        .await
        .unwrap();
        assert_eq!(relisted["values"], json!({"effort": "low", "top_k": 40}));

        let yaml = std::fs::read_to_string(Paths::config_dir().join(CONFIG_YAML_NAME)).unwrap();
        assert!(yaml.contains("GOOSE_MODEL_FIELDS"), "{yaml}");

        let model = goose::model_config::model_config_from_user_config("openrouter", MODEL)
            .expect("the model's config builds with its saved fields");
        let params = model
            .request_params
            .expect("saved fields become request params");
        assert_eq!(params["reasoning"], json!({"effort": "low"}));
        assert_eq!(params["top_k"], json!(40));

        let other =
            goose::model_config::model_config_from_user_config("openrouter", "vendor/plain")
                .unwrap();
        assert!(
            other
                .request_params
                .as_ref()
                .is_none_or(|p| !p.contains_key("reasoning")),
            "a value saved for one model never rides another's requests"
        );
    });
}
