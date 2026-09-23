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
    assert!(
        report.repairs.is_empty(),
        "a dry run changes no network setting"
    );
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
                panic!(
                    "start failed: {:?}\n{:#?}",
                    status.last_error, status.events
                )
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

    // The rank wrapper's own surface: the catalog, the progress counter, the wrong-model refusal
    // and the admission gate the memory watchdog closes.
    let http = reqwest::Client::new();
    let base = config.base_url();
    let get = |path: &'static str| {
        let http = http.clone();
        let base = base.clone();
        async move {
            let resp = http.get(format!("{base}{path}")).send().await.unwrap();
            (resp.status().as_u16(), resp.text().await.unwrap())
        }
    };
    let post = |path: &'static str, body: serde_json::Value| {
        let http = http.clone();
        let base = base.clone();
        async move {
            let resp = http
                .post(format!("{base}{path}"))
                .header("content-type", "application/json")
                .body(body.to_string())
                .send()
                .await
                .unwrap();
            (resp.status().as_u16(), resp.text().await.unwrap())
        }
    };
    let chat = |model: &str| {
        serde_json::json!({
            "model": model,
            "messages": [{"role": "user", "content": "Say ok."}],
            "max_tokens": 2,
            "temperature": 0,
            "chat_template_kwargs": {"enable_thinking": false},
        })
    };
    let (code, models) = get("/v1/models").await;
    println!("/v1/models {code}: {models}");
    assert!(models.contains(&config.model_id) && models.contains("context_window"));
    let (code, progress) = get("/goose/progress").await;
    println!("/goose/progress {code}: {progress}");
    assert_eq!(code, 200);
    let (code, wrong) = post("/v1/chat/completions", chat("mlx-community/other-model")).await;
    println!("wrong model {code}: {wrong}");
    assert_eq!(code, 404);
    let (code, _) = post(
        "/goose/admission",
        serde_json::json!({"open": false, "reason": "live test"}),
    )
    .await;
    assert_eq!(code, 200);
    let (code, held) = post("/v1/chat/completions", chat(&config.model_id)).await;
    println!("admission closed {code}: {held}");
    assert_eq!(code, 503);
    assert!(held.contains("not admitting") && held.contains("live test"));
    post("/goose/admission", serde_json::json!({"open": true})).await;
    let (code, ok) = post("/v1/chat/completions", chat(&config.model_id)).await;
    println!("admission reopened {code}: {}", &ok[..ok.len().min(160)]);
    assert_eq!(code, 200);
    assert!(
        ok.contains(&format!("\"model\": \"{}\"", config.model_id)),
        "the response echoes the goose id"
    );

    tokio::time::sleep(Duration::from_secs(6)).await;
    let status = manager.status();
    println!("liveness {:?}", status.liveness);
    for node in &status.nodes {
        println!(
            "node {} avail {:?} peak {:?} active {:?} pressure {:?} caps memory {:?} wired {:?} cache {:?}",
            node.name,
            node.available_bytes,
            node.peak_bytes,
            node.active_bytes,
            node.pressure,
            node.memory_limit_bytes,
            node.wired_limit_bytes,
            node.cache_limit_bytes
        );
        assert!(node.memory_limit_bytes.is_some() && node.wired_limit_bytes.is_some());
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

async fn wait_serving(manager: &DistributedManager, previous_pid: Option<u32>) -> u32 {
    loop {
        let status = manager.status();
        let pid = status.nodes.get(1).and_then(|n| n.pid);
        match status.state {
            RunState::Ready | RunState::Serving if pid.is_some() && pid != previous_pid => {
                return pid.unwrap()
            }
            RunState::Failed | RunState::Stopped => {
                panic!("run ended: {:?}\n{:#?}", status.last_error, status.events)
            }
            _ => tokio::time::sleep(Duration::from_millis(250)).await,
        }
    }
}

fn ssh_signal(signal: &str, pid: u32) {
    let out = std::process::Command::new("/usr/bin/ssh")
        .args(["workhorse", &format!("/bin/kill -{signal} {pid}")])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
}

/// The soak's two failure classes injected on the real pair, per pid on the peer rank: a FROZEN
/// rank (SIGSTOP — silent forever without a supervisor) and a DEAD rank (SIGKILL). Each must be
/// detected, the pair stopped (verified) and restarted by the policy.
#[tokio::test]
#[ignore = "needs both Macs and the owner's 27B on each; restarts the pair twice"]
async fn live_supervisor_detects_a_frozen_and_a_dead_peer_and_restarts() {
    let mut config = recorded_config();
    config.restart_on_failure = true;
    let manager = DistributedManager::new(Arc::new(SystemExec));
    match manager.start(config).await.unwrap() {
        StartOutcome::Started { .. } => {}
        StartOutcome::Refused { code, message, .. } => panic!("refused {code:?}: {message}"),
    }
    let first = wait_serving(&manager, None).await;
    println!("serving, peer rank pid {first}");

    let t = Instant::now();
    ssh_signal("STOP", first);
    let second = wait_serving(&manager, Some(first)).await;
    println!(
        "SIGSTOP {first} → restarted with peer pid {second} in {:?}",
        t.elapsed()
    );

    let t = Instant::now();
    ssh_signal("KILL", second);
    let third = wait_serving(&manager, Some(second)).await;
    println!(
        "SIGKILL {second} → restarted with peer pid {third} in {:?}",
        t.elapsed()
    );

    let stop = manager.stop().await;
    let status = manager.status();
    for event in &status.events {
        println!(
            "event {:?}: {}",
            event.kind,
            event.message.lines().next().unwrap_or_default()
        );
    }
    assert!(stop.verified, "{stop:#?}");
    assert_eq!(status.restarts, 2);
    let kinds: Vec<_> = status.events.iter().map(|e| e.kind).collect();
    assert!(kinds.contains(&goose_sidecar::distributed::EventKind::RankFrozen));
    assert!(kinds.contains(&goose_sidecar::distributed::EventKind::RankDied));
    for pid in [first, second, third] {
        let out = std::process::Command::new("/usr/bin/ssh")
            .args(["workhorse", &format!("/bin/ps -o pid=,stat= -p {pid}")])
            .output()
            .unwrap();
        assert!(
            String::from_utf8_lossy(&out.stdout).trim().is_empty(),
            "peer pid {pid} survived"
        );
    }
}

/// The qwen4_exp runner as a DRY RUN on the real pair with Flash (headers only, nothing loads):
/// the fork's planner splits the layers on the measured memory, and the start is refused by name
/// because the fork offers no `serve` entry.
#[tokio::test]
#[ignore = "needs both Macs, the fork venvs and Flash on each"]
async fn live_flash_pipeline_is_planned_and_refused_by_name() {
    let home = dirs::home_dir().unwrap();
    let mut config = recorded_config();
    config.model_id = "rapid-mlx/Qwen3.8-Flash-Next-4bit".to_string();
    config.context = Some(8_192);
    let local_fork = home
        .join("Projects/Rapid-MLX/.venv/bin/python")
        .display()
        .to_string();
    config.nodes[0].python = local_fork.clone();
    config.nodes[0].pipeline_python = Some(local_fork);
    config.nodes[0].model_dir = home
        .join(".goose/models/rapid-mlx/Qwen3.8-Flash-Next-4bit")
        .display()
        .to_string();
    let remote_fork = "/Users/workhorse/Projects/Rapid-MLX-pipeline/.venv/bin/python".to_string();
    config.nodes[1].python = remote_fork.clone();
    config.nodes[1].pipeline_python = Some(remote_fork);
    config.nodes[1].model_dir =
        "/Users/workhorse/.goose/models/rapid-mlx/Qwen3.8-Flash-Next-4bit".to_string();

    let manager = DistributedManager::new(Arc::new(SystemExec));
    let report = manager.preflight(&config, false).await.unwrap();
    for check in &report.checks {
        println!(
            "cluster {:?} {}: {}",
            check.verdict, check.id, check.message
        );
    }
    for node in &report.nodes {
        for check in &node.checks {
            println!(
                "{} {:?} {}: {}",
                node.name, check.verdict, check.id, check.message
            );
        }
        println!("{} plan {:?}", node.name, node.plan);
    }
    assert!(!report.ok);
    let failures = report.failures();
    assert!(
        failures
            .iter()
            .all(|f| f.contains("runner:") && f.contains("no `serve` entry")),
        "the only refusal is the missing server entry: {failures:#?}"
    );
    let rank0 = report.nodes[0].plan.as_ref().unwrap();
    let rank1 = report.nodes[1].plan.as_ref().unwrap();
    assert_eq!(rank0.layer_start, 0);
    assert_eq!(rank0.layer_end, rank1.layer_start);
    assert_eq!(rank1.layer_end, 48);
    assert!(rank0.fits && rank1.fits);
}
