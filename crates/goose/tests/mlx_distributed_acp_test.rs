//! The distributed MLX engine through goose's ACP surface. The first tests need nothing external;
//! the `#[ignore]` one is the live run on the two Macs (this MacBook = rank 0, `workhorse` over
//! ssh, the owner's 27B on both): ACP start → status (which switches the omlx provider to the
//! distributed port) → a completion through goose's `omlx` provider → the single engine's mount
//! refused while distributed owns the Mac → ACP stop, verified. Config lives in a temp
//! GOOSE_PATH_ROOT; nothing is downloaded, port 8090 is never used.

#[allow(dead_code)]
#[path = "acp_common_tests/mod.rs"]
mod common_tests;

use std::sync::Arc;
use std::time::{Duration, Instant};

use common_tests::fixtures::server::AcpServerConnection;
use common_tests::fixtures::{run_test, send_custom, Connection, OpenAiFixture, TestConnectionConfig};
use goose::conversation::message::Message;
use goose_test_support::IgnoreSessionId;
use serial_test::serial;

fn node(
    name: &str,
    ssh: Option<&str>,
    ip: &str,
    service: &str,
    python: String,
    model: String,
) -> serde_json::Value {
    let mut node = serde_json::json!({
        "name": name,
        "tbIp": ip,
        "tbNetmask": "255.255.255.252",
        "tbInterface": "en3",
        "tbService": service,
        "rdmaDevice": "rdma_en3",
        "python": python,
        "modelDir": model,
    });
    if let Some(ssh) = ssh {
        node["ssh"] = serde_json::json!(ssh);
    }
    node
}

fn recorded_config(port: u16) -> serde_json::Value {
    let home = dirs::home_dir().unwrap();
    serde_json::json!({
        "modelId": "Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx",
        "backend": std::env::var("GOOSE_DIST_BACKEND").unwrap_or_else(|_| "jaccl".to_string()),
        "port": port,
        "coordinatorPort": 32323,
        "restartOnFailure": false,
        "nodes": [
            node(
                "MacBook Pro",
                None,
                "192.168.0.1",
                "EXO Thunderbolt 3",
                home.join("goose-builds/jaccl-smoke/.venv/bin/python").display().to_string(),
                home.join(".goose/models/Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx")
                    .display()
                    .to_string(),
            ),
            node(
                "workhorse",
                Some("workhorse"),
                "192.168.0.2",
                "EXO Thunderbolt 2",
                "/Users/workhorse/jaccl-smoke/.venv/bin/python".to_string(),
                "/Users/workhorse/jaccl-smoke/models/Qwen3.8-27B-Atlassian-Q8-mlx".to_string(),
            ),
        ],
    })
}

async fn connection() -> AcpServerConnection {
    let openai = OpenAiFixture::new(vec![], Arc::new(IgnoreSessionId)).await;
    AcpServerConnection::new(TestConnectionConfig::default(), openai).await
}

#[test]
#[serial]
fn distributed_status_reports_single_mode_when_nothing_runs() {
    run_test(async move {
        let conn = connection().await;
        let status = send_custom(conn.cx(), "_goose/unstable/mlxEngine/distributedStatus", serde_json::json!({}))
            .await
            .unwrap();
        assert_eq!(status["status"]["mode"], "single");
        assert_eq!(status["status"]["state"], "stopped");
        assert_eq!(status["status"]["admissionOpen"], true);
    });
}

#[test]
#[serial]
fn the_single_engines_port_and_a_missing_config_are_refused_by_name() {
    run_test(async move {
        let conn = connection().await;
        let err = send_custom(
            conn.cx(),
            "_goose/unstable/mlxEngine/distributedPreflight",
            serde_json::json!({ "config": recorded_config(8090) }),
        )
        .await
        .unwrap_err();
        assert!(format!("{err:?}").contains("single MLX engine's port"), "{err:?}");

        let err = send_custom(
            conn.cx(),
            "_goose/unstable/mlxEngine/distributedStart",
            serde_json::json!({}),
        )
        .await
        .unwrap_err();
        assert!(format!("{err:?}").contains("no distributed config"), "{err:?}");
    });
}

#[test]
#[serial]
fn a_saved_config_comes_back_on_status() {
    run_test(async move {
        let conn = connection().await;
        let saved = send_custom(
            conn.cx(),
            "_goose/unstable/mlxEngine/distributedConfigUpdate",
            serde_json::json!({ "config": recorded_config(8190) }),
        )
        .await
        .unwrap();
        assert_eq!(saved["config"]["port"], 8190);
        let status = send_custom(conn.cx(), "_goose/unstable/mlxEngine/distributedStatus", serde_json::json!({}))
            .await
            .unwrap();
        assert_eq!(status["status"]["config"]["nodes"][1]["ssh"], "workhorse");
    });
}

#[test]
#[serial]
#[ignore = "needs both Macs and the owner's 27B on each; loads ~18 GB per Mac"]
fn live_acp_start_complete_through_the_omlx_provider_stop() {
    run_test(async move {
        let conn = connection().await;
        let started = send_custom(
            conn.cx(),
            "_goose/unstable/mlxEngine/distributedStart",
            serde_json::json!({ "config": recorded_config(8190) }),
        )
        .await
        .unwrap();
        println!("start: started={} refusal={}", started["started"], started["refusal"]);
        assert_eq!(started["started"], true, "{started:#}");

        let t0 = Instant::now();
        let status = loop {
            let status = send_custom(conn.cx(), "_goose/unstable/mlxEngine/distributedStatus", serde_json::json!({}))
                .await
                .unwrap();
            match status["status"]["state"].as_str().unwrap() {
                "ready" | "serving" => break status,
                "failed" | "stopped" => panic!("start failed: {status:#}"),
                _ => tokio::time::sleep(Duration::from_millis(500)).await,
            }
        };
        println!("ready in {:?}: mode {} base {}", t0.elapsed(), status["status"]["mode"], status["status"]["baseUrl"]);
        assert_eq!(status["status"]["mode"], "distributed");
        assert_eq!(
            std::env::var("OMLX_HOST").unwrap(),
            "http://127.0.0.1:8190",
            "the omlx provider targets the distributed engine while it owns the Mac"
        );

        let refused = send_custom(
            conn.cx(),
            "_goose/unstable/mlxEngine/mount",
            serde_json::json!({ "modelId": "Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx" }),
        )
        .await
        .unwrap_err();
        println!("single mount while distributed: {refused:?}");
        assert!(format!("{refused:?}").contains("distributedEngineActive"));

        let provider = goose::providers::create("omlx", vec![]).await.unwrap();
        let models = provider.fetch_supported_models().await.unwrap();
        println!("omlx lists: {models:?}");
        assert_eq!(models, vec!["Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx".to_string()]);
        let model_config =
            goose_providers::model::ModelConfig::new("Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx");
        let t1 = Instant::now();
        let (reply, usage) = goose::session_context::with_session_id(
            Some("mlx-distributed-live".to_string()),
            provider.complete(
                &model_config,
                "You are terse.",
                &[Message::user().with_text("What is the capital of France? One word.")],
                &[],
            ),
        )
        .await
        .unwrap();
        println!(
            "omlx provider reply in {:?}: {:?} usage {:?}",
            t1.elapsed(),
            reply.as_concat_text(),
            usage
        );
        assert!(reply.as_concat_text().to_lowercase().contains("paris"));

        let stopped = send_custom(conn.cx(), "_goose/unstable/mlxEngine/distributedStop", serde_json::json!({}))
            .await
            .unwrap();
        println!("stop: {:#}", stopped["stop"]);
        assert_eq!(stopped["stop"]["verified"], true);
        assert_eq!(stopped["status"]["mode"], "single");
        assert_eq!(
            std::env::var("OMLX_HOST").unwrap(),
            "http://127.0.0.1:8090",
            "the switch returns to the single engine's port"
        );
        for event in stopped["status"]["events"].as_array().unwrap() {
            println!("event {} {}", event["kind"], event["message"]);
        }
        let leftover = std::process::Command::new("/usr/bin/ssh")
            .args([
                "workhorse",
                "/bin/ps -axo pid=,command= | /usr/bin/grep goose-distributed-rank | /usr/bin/grep -v grep",
            ])
            .output()
            .unwrap();
        assert!(String::from_utf8_lossy(&leftover.stdout).trim().is_empty());
    });
}
