#[allow(dead_code)]
#[path = "acp_fixtures/mod.rs"]
mod fixtures;

use fixtures::server::AcpServerConnection;
use fixtures::{send_custom, Connection, OpenAiFixture, TestConnectionConfig};
use goose_test_support::IgnoreSessionId;
use serde_json::json;
use std::sync::Arc;

// Do not use fixtures::run_test: its enlarged stacks hide desktop startup regressions.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn custom_requests_round_trip_on_default_worker_stack() {
    let root = tempfile::tempdir().unwrap();
    std::env::set_var("GOOSE_PATH_ROOT", root.path());
    std::env::set_var("GOOSE_DISABLE_KEYRING", "1");
    let openai = OpenAiFixture::new(vec![], Arc::new(IgnoreSessionId)).await;
    let connection = AcpServerConnection::new(TestConnectionConfig::default(), openai).await;

    let extensions = send_custom(
        connection.cx(),
        "_goose/unstable/config/extensions/list",
        json!({}),
    )
    .await
    .unwrap();
    assert!(extensions["extensions"].is_array());
    assert!(extensions["warnings"].is_array());

    let prompt = send_custom(
        connection.cx(),
        "_goose/unstable/config/prompts/get",
        json!({"name": "system.md"}),
    )
    .await
    .unwrap();
    assert_eq!(prompt["name"], "system.md");
    assert!(prompt["content"]
        .as_str()
        .is_some_and(|content| !content.is_empty()));
    assert_eq!(prompt["isCustomized"], false);

    let status = send_custom(
        connection.cx(),
        "_goose/unstable/controller/status",
        json!({}),
    )
    .await
    .unwrap();
    assert_eq!(
        status,
        json!({"state": "signed_out", "device": null, "error": null})
    );

    let malformed = send_custom(
        connection.cx(),
        "_goose/unstable/config/prompts/get",
        json!({"name": 17}),
    )
    .await
    .unwrap_err();
    assert_eq!(
        malformed.code,
        agent_client_protocol::Error::invalid_params().code
    );
    let missing = send_custom(connection.cx(), "_goose/stack-test-missing", json!({}))
        .await
        .unwrap_err();
    assert_eq!(
        missing.code,
        agent_client_protocol::Error::method_not_found().code
    );
}
