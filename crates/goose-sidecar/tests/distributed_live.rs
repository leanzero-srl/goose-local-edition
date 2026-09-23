//! Live tests against the real two-Mac setup (this MacBook = rank 0, `workhorse` over ssh, the
//! Thunderbolt 5 /30). Ignored by default: they need both Macs, the owner's 27B on both, and no
//! other MLX engine running on either. They never download, never touch port 8090, and never
//! change a network setting (the link repair is off in the dry run; the start repairs only the
//! configured TB services, and only when a JACCL link check fails).
//!
//! Paths default to the recorded setup (mlx-jaccl-cluster skill) and can be overridden:
//! GOOSE_DIST_LOCAL_PYTHON, GOOSE_DIST_REMOTE_PYTHON, GOOSE_DIST_LOCAL_MODEL, GOOSE_DIST_REMOTE_MODEL,
//! GOOSE_DIST_BACKEND (jaccl|ring), GOOSE_DIST_PORT.

use std::sync::Arc;
use std::time::{Duration, Instant};

use goose_sidecar::distributed::{
    Backend, DistributedConfig, DistributedManager, NodeConfig, RunState, StartOutcome, SystemExec,
};

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

pub fn recorded_config() -> DistributedConfig {
    let home = dirs::home_dir().unwrap();
    DistributedConfig {
        model_id: "Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx".to_string(),
        backend: match env_or("GOOSE_DIST_BACKEND", "jaccl").as_str() {
            "ring" => Backend::Ring,
            _ => Backend::Jaccl,
        },
        port: env_or("GOOSE_DIST_PORT", "8190").parse().unwrap(),
        coordinator_port: 32323,
        context: None,
        restart_on_failure: false,
        nodes: vec![
            NodeConfig {
                name: "MacBook Pro".to_string(),
                ssh: None,
                tb_ip: "192.168.0.1".to_string(),
                tb_netmask: "255.255.255.252".to_string(),
                tb_interface: "en3".to_string(),
                tb_service: "EXO Thunderbolt 3".to_string(),
                rdma_device: "rdma_en3".to_string(),
                python: env_or(
                    "GOOSE_DIST_LOCAL_PYTHON",
                    &home
                        .join("goose-builds/jaccl-smoke/.venv/bin/python")
                        .display()
                        .to_string(),
                ),
                pipeline_python: None,
                model_dir: env_or(
                    "GOOSE_DIST_LOCAL_MODEL",
                    &home
                        .join(".goose/models/Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx")
                        .display()
                        .to_string(),
                ),
            },
            NodeConfig {
                name: "workhorse".to_string(),
                ssh: Some("workhorse".to_string()),
                tb_ip: "192.168.0.2".to_string(),
                tb_netmask: "255.255.255.252".to_string(),
                tb_interface: "en3".to_string(),
                tb_service: "EXO Thunderbolt 2".to_string(),
                rdma_device: "rdma_en3".to_string(),
                python: env_or(
                    "GOOSE_DIST_REMOTE_PYTHON",
                    "/Users/workhorse/jaccl-smoke/.venv/bin/python",
                ),
                pipeline_python: None,
                model_dir: env_or(
                    "GOOSE_DIST_REMOTE_MODEL",
                    "/Users/workhorse/jaccl-smoke/models/Qwen3.8-27B-Atlassian-Q8-mlx",
                ),
            },
        ],
    }
}

#[tokio::test]
#[ignore = "needs both Macs and the owner's 27B on each"]
async fn live_preflight_dry_run_on_the_two_macs() {
    let manager = DistributedManager::new(Arc::new(SystemExec));
    let started = Instant::now();
    let report = manager.preflight(&recorded_config(), false).await.unwrap();
    println!(
        "preflight in {:?}:\n{}",
        started.elapsed(),
        serde_json::to_string_pretty(&report).unwrap()
    );
    assert!(report.ok, "failures: {:?}", report.failures());
    assert!(report.repairs.is_empty(), "a dry run changes no network setting");
}

#[tokio::test]
#[ignore = "needs both Macs and the owner's 27B on each; loads ~20 GB per Mac"]
async fn live_start_complete_stop_the_27b_over_the_two_macs() {
    let config = recorded_config();
    let manager = DistributedManager::new(Arc::new(SystemExec));
    let t0 = Instant::now();
    let preflight = match manager.start(config.clone()).await.unwrap() {
        StartOutcome::Started { preflight } => preflight,
        StartOutcome::Refused { code, message, .. } => panic!("refused {code:?}: {message}"),
    };
    println!("preflight ok, context {:?}", preflight.context_limit);
    loop {
        let status = manager.status();
        match status.state {
            RunState::Ready | RunState::Serving => break,
            RunState::Failed | RunState::Stopped => {
                panic!("start failed: {:?}\n{:#?}", status.last_error, status.events)
            }
            _ => tokio::time::sleep(Duration::from_millis(500)).await,
        }
    }
    println!("ready in {:?}", t0.elapsed());
    let status = manager.status();
    for node in &status.nodes {
        println!(
            "node {} rank {} pid {:?} state {:?} planned {:?}",
            node.name, node.rank, node.pid, node.state, node.planned_bytes
        );
    }

    let body = reqwest::Client::new()
        .post(format!("{}/v1/chat/completions", config.base_url()))
        .header("content-type", "application/json")
        .body(
            serde_json::json!({
                "model": config.model_id,
                "messages": [{"role": "user", "content": "Name the capital of France in one word."}],
                "max_tokens": 16,
                "temperature": 0,
                "stream": true,
                "chat_template_kwargs": {"enable_thinking": false},
            })
            .to_string(),
        )
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    println!("stream tail: {}", &body[body.len().saturating_sub(300)..]);
    assert!(body.contains("data: [DONE]"));

    tokio::time::sleep(Duration::from_secs(6)).await;
    let status = manager.status();
    println!("liveness {:?}", status.liveness);
    for node in &status.nodes {
        println!(
            "node {} avail {:?} peak {:?} active {:?} pressure {:?}",
            node.name, node.available_bytes, node.peak_bytes, node.active_bytes, node.pressure
        );
    }

    let stop = manager.stop().await;
    println!("stop: {stop:#?}");
    assert!(stop.verified);
    let leftover = std::process::Command::new("/usr/bin/ssh")
        .args(["workhorse", "/bin/ps -axo pid=,command= | /usr/bin/grep goose-distributed-rank | /usr/bin/grep -v grep"])
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&leftover.stdout).trim().is_empty(),
        "a goose rank survived on the workhorse"
    );
    for event in manager.status().events {
        println!("event {:?} {:?}: {}", event.kind, event.node, event.message);
    }
}
