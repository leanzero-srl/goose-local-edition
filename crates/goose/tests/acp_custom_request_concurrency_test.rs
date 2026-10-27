//! One slow custom request must not hold the others (2026-10-27).
//!
//! agent-client-protocol runs every `cx.spawn` task of a connection inside ONE future, so a
//! handler doing synchronous work (the models-folder walk over the NFS-linked SSD on Gabee)
//! froze every other request of the desktop: a 0.7 s Hugging Face browse answered after 42 s,
//! Link status polls after 30 s. Here a model's safetensors index is a FIFO, so
//! `mlxEngine/modelsList` blocks in a synchronous `read` until the test writes to it — and a
//! cheap request sent meanwhile must still answer at once.
#[allow(dead_code)]
#[path = "acp_common_tests/mod.rs"]
mod common_tests;

use common_tests::fixtures::server::AcpServerConnection;
use common_tests::fixtures::{run_test, send_custom, Connection, TestConnectionConfig};
use goose_test_support::EnforceSessionId;
use serial_test::serial;
use std::sync::Arc;
use std::time::{Duration, Instant};

use common_tests::fixtures::OpenAiFixture;

#[test]
#[serial]
fn a_blocked_models_list_does_not_hold_other_requests() {
    let root = tempfile::tempdir().unwrap();
    std::env::set_var("GOOSE_PATH_ROOT", root.path());
    std::env::set_var("GOOSE_DISABLE_KEYRING", "1");
    let models = root.path().join("models");
    let model = models.join("pub/blocked");
    std::fs::create_dir_all(&model).unwrap();
    std::fs::write(model.join("config.json"), "{}").unwrap();
    let fifo = model.join("model.safetensors.index.json");
    let made = std::process::Command::new("/usr/bin/mkfifo")
        .arg(&fifo)
        .status()
        .unwrap();
    assert!(made.success());
    // The in-process server reads <data_root>/config.yaml and keeps what is already there.
    let data_root = root.path().join("data");
    std::fs::create_dir_all(&data_root).unwrap();
    std::fs::write(
        data_root.join(goose::config::base::CONFIG_YAML_NAME),
        format!(
            "GOOSE_MODEL: gpt-4o\nGOOSE_PROVIDER: openai\nGOOSE_DISABLE_KEYRING: true\nmlx_engine:\n  models_dir: {}\n",
            models.display()
        ),
    )
    .unwrap();

    let models_dir = models.display().to_string();
    run_test(async move {
        let openai = OpenAiFixture::new(vec![], Arc::new(EnforceSessionId::default())).await;
        let conn = AcpServerConnection::new(
            TestConnectionConfig {
                data_root,
                ..Default::default()
            },
            openai,
        )
        .await;
        let cx = conn.cx().clone();

        let settings = send_custom(
            &cx,
            "_goose/unstable/mlxEngine/settingsRead",
            serde_json::json!({}),
        )
        .await
        .unwrap();
        assert_eq!(
            settings["settings"]["modelsDir"].as_str(),
            Some(models_dir.as_str()),
            "the isolated config must be the one read: {settings}"
        );
        let blocked = {
            let cx = cx.clone();
            tokio::spawn(async move {
                send_custom(
                    &cx,
                    "_goose/unstable/mlxEngine/modelsList",
                    serde_json::json!({}),
                )
                .await
            })
        };
        // Let the models list reach the FIFO read.
        tokio::time::sleep(Duration::from_millis(500)).await;
        if blocked.is_finished() {
            panic!(
                "the FIFO should hold the models list, got: {:?}",
                blocked.await
            );
        }

        let started = Instant::now();
        let quick = tokio::time::timeout(
            Duration::from_secs(5),
            send_custom(
                &cx,
                "_goose/unstable/mlxEngine/settingsRead",
                serde_json::json!({}),
            ),
        )
        .await;
        let waited = started.elapsed();

        // Release the FIFO whatever happened, so a failure cannot hang the suite.
        let writer = fifo.clone();
        tokio::task::spawn_blocking(move || std::fs::write(writer, "{\"weight_map\":{}}"))
            .await
            .unwrap()
            .unwrap();
        let listed = tokio::time::timeout(Duration::from_secs(10), blocked)
            .await
            .expect("models list answers once the FIFO is written")
            .unwrap()
            .expect("models list ok");

        let quick = quick.expect("settingsRead answered while modelsList was blocked");
        assert!(quick.is_ok(), "settingsRead failed: {quick:?}");
        assert!(
            waited < Duration::from_secs(2),
            "settingsRead waited {waited:?}"
        );
        let ids: Vec<&str> = listed["models"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|m| m["id"].as_str())
            .collect();
        assert_eq!(ids, ["pub/blocked"]);
    });
}
