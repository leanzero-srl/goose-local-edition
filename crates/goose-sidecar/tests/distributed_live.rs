//! Live tests against the real two-Mac setup (this MacBook = rank 0, `workhorse` over ssh, the
//! Thunderbolt 5 /30). Ignored by default: they need both Macs, the owner's 27B on both, and no
//! other MLX engine running on either. They never download, never touch port 8090, and never
//! change a network setting (the link repair is off in the dry run; the start repairs only the
//! configured TB services, and only when a JACCL link check fails).
//!
//! Paths default to the recorded setup (mlx-jaccl-cluster skill) and can be overridden:
//! GOOSE_DIST_LOCAL_PYTHON, GOOSE_DIST_REMOTE_PYTHON, GOOSE_DIST_LOCAL_MODEL, GOOSE_DIST_REMOTE_MODEL,
//! GOOSE_DIST_BACKEND (jaccl|ring), GOOSE_DIST_PORT.

#![cfg(unix)]

use std::sync::Arc;
use std::time::{Duration, Instant};

use goose_sidecar::distributed::{
    Backend, DistributedConfig, DistributedManager, NodeConfig, RunState, StartOutcome, SystemExec,
};

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

/// These runs set no `served_model_name`, so the ranks serve the HF id — through the same rule the
/// app applies (`engine::served_model_id`), never a copy of it.
fn unaliased(config: &DistributedConfig) -> String {
    goose_sidecar::engine::served_model_id(
        &goose_sidecar::engine::EngineSettings::default(),
        &config.model_id,
    )
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
        slots: None,
        restart_on_failure: false,
        hang_ratio_only: false,
        watchdog_warn_ratio: None,
        watchdog_critical_ratio: None,
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
                // These tests measure the engine, not compaction: nothing pressures the Macs.
                free_memory_automatically: false,
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
                free_memory_automatically: false,
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
    let preflight = match manager
        .start(config.clone(), unaliased(&config))
        .await
        .unwrap()
    {
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
    match manager
        .start(config.clone(), unaliased(&config))
        .await
        .unwrap()
    {
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

/// The qwen4_exp runner as a DRY RUN on the real pair with Flash (headers only, nothing loads,
/// no rank is launched): the fork's `plan --json` splits the layers on the measured memory, the
/// runner check finds the pinned fork's `serve` in the goose-managed fork env on both Macs
/// (provision it first: `distributedProvision`), and the plan approves a split that tiles the 48
/// layers.
#[tokio::test]
#[ignore = "needs both Macs, the provisioned fork env and Flash on each"]
async fn live_flash_pipeline_is_planned_from_the_forks_json() {
    use goose_sidecar::distributed::provision::EnvSpec;
    let home = dirs::home_dir().unwrap();
    let mut config = recorded_config();
    config.model_id = "rapid-mlx/Qwen3.8-Flash-Next-4bit".to_string();
    config.context = Some(8_192);
    config.nodes[0].pipeline_python = Some(EnvSpec::pipeline().python(&home.display().to_string()));
    config.nodes[0].model_dir = home
        .join(".goose/models/rapid-mlx/Qwen3.8-Flash-Next-4bit")
        .display()
        .to_string();
    config.nodes[1].pipeline_python = Some(EnvSpec::pipeline().python("/Users/workhorse"));
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
    for node in &report.nodes {
        let runner = node.checks.iter().find(|c| c.id == "runner").unwrap();
        assert!(
            runner.message.contains("\"serve\""),
            "{}: {}",
            node.name,
            runner.message
        );
    }
    let rank0 = report.nodes[0].plan.as_ref().unwrap();
    let rank1 = report.nodes[1].plan.as_ref().unwrap();
    assert_eq!(rank0.layer_start, 0);
    assert_eq!(rank0.layer_end, rank1.layer_start);
    assert_eq!(rank1.layer_end, 48);
    if rank0.fits && rank1.fits {
        assert_eq!(
            report.pipeline_starts,
            Some(vec![0, rank1.layer_start]),
            "the approved split is the one the ranks launch with"
        );
    }
}

/// A streaming completion read chunk by chunk: (chunks received, saw `[DONE]`, how it ended).
async fn stream_completion(
    base: String,
    model: String,
    max_tokens: u32,
    chunks_seen: Arc<std::sync::atomic::AtomicUsize>,
) -> (usize, bool, String) {
    let resp = reqwest::Client::new()
        .post(format!("{base}/v1/chat/completions"))
        .header("content-type", "application/json")
        .body(
            serde_json::json!({
                "model": model,
                "messages": [{"role": "user", "content": "Count from 1 to 3000, one number per line, nothing else."}],
                "max_tokens": max_tokens,
                "temperature": 0,
                "stream": true,
                "chat_template_kwargs": {"enable_thinking": false},
            })
            .to_string(),
        )
        .send()
        .await;
    let mut resp = match resp {
        Ok(resp) => resp,
        Err(e) => return (0, false, format!("send failed: {e}")),
    };
    let status = resp.status();
    let mut text = String::new();
    let mut ended = format!("EOF after HTTP {status}");
    loop {
        match resp.chunk().await {
            Ok(None) => break,
            Ok(Some(bytes)) => {
                text.push_str(&String::from_utf8_lossy(&bytes));
                let chunks = text.matches("data: {").count();
                chunks_seen.store(chunks, std::sync::atomic::Ordering::SeqCst);
            }
            Err(e) => {
                ended = format!("transport error after HTTP {status}: {e}");
                break;
            }
        }
    }
    (
        text.matches("data: {").count(),
        text.contains("data: [DONE]"),
        ended,
    )
}

async fn short_ok(base: &str, model: &str) -> u16 {
    reqwest::Client::new()
        .post(format!("{base}/v1/chat/completions"))
        .header("content-type", "application/json")
        .body(
            serde_json::json!({
                "model": model,
                "messages": [{"role": "user", "content": "Say ok."}],
                "max_tokens": 3,
                "temperature": 0,
                "chat_template_kwargs": {"enable_thinking": false},
            })
            .to_string(),
        )
        .send()
        .await
        .map(|r| r.status().as_u16())
        .unwrap_or(0)
}

fn print_events(manager: &DistributedManager, from: usize) -> usize {
    let events = manager.status().events;
    for event in &events[from.min(events.len())..] {
        println!(
            "  event {:?} {:?}: {}",
            event.kind,
            event.node,
            event.message.lines().next().unwrap_or_default()
        );
    }
    events.len()
}

/// The hang rule proven on a real freeze MID-STREAM: `hang_ratio_only` turns off the ps-stat-T
/// fast path, so only the progress-ratio rule can see the SIGSTOPped peer. Then a stream CUT
/// (SIGKILL mid-stream) surfaces as rankDied + streamWithoutDone. Both restart per policy.
#[tokio::test]
#[ignore = "needs both Macs and the owner's 27B on each; freezes and kills the peer rank"]
async fn live_hang_rule_and_stream_cut_mid_stream() {
    use goose_sidecar::distributed::EventKind;
    use std::sync::atomic::{AtomicUsize, Ordering};
    let mut config = recorded_config();
    config.restart_on_failure = true;
    config.hang_ratio_only = true;
    let base = config.base_url();
    let model = config.model_id.clone();
    let manager = DistributedManager::new(Arc::new(SystemExec));
    match manager
        .start(config.clone(), unaliased(&config))
        .await
        .unwrap()
    {
        StartOutcome::Started { .. } => {}
        StartOutcome::Refused { code, message, .. } => panic!("refused {code:?}: {message}"),
    }
    let first = wait_serving(&manager, None).await;
    while manager.status().liveness.map(|l| l.samples).unwrap_or(0) < 3 {
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    println!(
        "ready, peer pid {first}, liveness {:?}",
        manager.status().liveness
    );

    // 1. SIGSTOP the peer while tokens flow.
    let seen = Arc::new(AtomicUsize::new(0));
    let stream = tokio::spawn(stream_completion(
        base.clone(),
        model.clone(),
        3000,
        Arc::clone(&seen),
    ));
    while seen.load(Ordering::SeqCst) < 30 {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let frozen_at = Instant::now();
    ssh_signal("STOP", first);
    println!(
        "SIGSTOP peer pid {first} after {} chunks",
        seen.load(Ordering::SeqCst)
    );
    let (chunks, done, ended) = stream.await.unwrap();
    println!(
        "client: {chunks} chunks, [DONE]={done}, ended '{ended}' {:?} after the freeze",
        frozen_at.elapsed()
    );
    assert!(!done, "a frozen pair must not complete the stream");
    let second = wait_serving(&manager, Some(first)).await;
    println!(
        "restarted, peer pid {second}, {:?} after the freeze",
        frozen_at.elapsed()
    );
    let mut cursor = print_events(&manager, 0);
    let events = manager.status().events;
    let hang = events
        .iter()
        .find(|e| e.kind == EventKind::Hang)
        .expect("the ratio rule fired");
    assert!(
        hang.message.starts_with("progress-ratio rule: samples"),
        "{}",
        hang.message
    );
    assert!(
        !events.iter().any(|e| e.kind == EventKind::RankFrozen),
        "the ps fast path was off"
    );
    assert!(events
        .iter()
        .any(|e| e.kind == EventKind::StreamWithoutDone));
    assert_eq!(
        short_ok(&base, &model).await,
        200,
        "the restarted pair serves"
    );

    // 2. SIGKILL the peer mid-stream.
    let seen = Arc::new(AtomicUsize::new(0));
    let stream = tokio::spawn(stream_completion(
        base.clone(),
        model.clone(),
        3000,
        Arc::clone(&seen),
    ));
    while seen.load(Ordering::SeqCst) < 30 {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let killed_at = Instant::now();
    ssh_signal("KILL", second);
    println!(
        "SIGKILL peer pid {second} after {} chunks",
        seen.load(Ordering::SeqCst)
    );
    let (chunks, done, ended) = stream.await.unwrap();
    println!(
        "client: {chunks} chunks, [DONE]={done}, ended '{ended}' {:?} after the kill",
        killed_at.elapsed()
    );
    assert!(!done);
    let third = wait_serving(&manager, Some(second)).await;
    println!(
        "restarted, peer pid {third}, {:?} after the kill",
        killed_at.elapsed()
    );
    cursor = print_events(&manager, cursor);
    let events = manager.status().events;
    let cuts = events
        .iter()
        .filter(|e| e.kind == EventKind::StreamWithoutDone)
        .count();
    assert_eq!(cuts, 2, "each failure named the stream it cut");
    assert!(events.iter().any(|e| e.kind == EventKind::RankDied));
    assert_eq!(short_ok(&base, &model).await, 200);

    let stop = manager.stop().await;
    print_events(&manager, cursor);
    assert!(stop.verified, "{stop:#?}");
    assert_eq!(manager.status().restarts, 2);
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

const BALLAST: &str = "import os, sys, time
target = int(sys.argv[1])
held = []
for i in range(target):
    held.append(os.urandom(1 << 30))
    print(f'BALLAST_HELD {i + 1}', flush=True)
    time.sleep(0.5)
print('BALLAST_DONE', flush=True)
while True:
    time.sleep(60)
";

/// A process WE own on the workhorse holding `gib` GiB of incompressible (urandom) anonymous
/// memory, allocated 1 GiB per step. Killed by its own pid; `ssh -tt` also takes it with the
/// session if the test dies.
struct Ballast {
    pid: u32,
    held: Arc<std::sync::atomic::AtomicUsize>,
    _child: tokio::process::Child,
}

impl Ballast {
    async fn start(gib: u64) -> Self {
        use tokio::io::AsyncBufReadExt;
        let script = format!(
            "echo BALLAST_PID=$$; exec /usr/bin/python3 -u -c {} {gib}",
            goose_sidecar::distributed::exec::sh_quote(BALLAST)
        );
        let mut child = tokio::process::Command::new("/usr/bin/ssh")
            .args([
                "-tt",
                "-o",
                "BatchMode=yes",
                "-o",
                "LogLevel=QUIET",
                "workhorse",
                &script,
            ])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let mut lines = tokio::io::BufReader::new(child.stdout.take().unwrap()).lines();
        let pid: u32 = loop {
            let line = lines
                .next_line()
                .await
                .unwrap()
                .expect("ballast printed its pid");
            if let Some(pid) = line.trim().strip_prefix("BALLAST_PID=") {
                break pid.trim().parse().unwrap();
            }
        };
        let held = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = Arc::clone(&held);
        tokio::spawn(async move {
            while let Ok(Some(line)) = lines.next_line().await {
                if let Some(n) = line.trim().strip_prefix("BALLAST_HELD ") {
                    counter.store(n.parse().unwrap_or(0), std::sync::atomic::Ordering::SeqCst);
                }
            }
        });
        Ballast {
            pid,
            held,
            _child: child,
        }
    }

    fn held(&self) -> usize {
        self.held.load(std::sync::atomic::Ordering::SeqCst)
    }

    fn kill(&self) -> bool {
        ssh_signal("TERM", self.pid);
        for _ in 0..50 {
            let out = std::process::Command::new("/usr/bin/ssh")
                .args(["workhorse", &format!("/bin/ps -o pid= -p {}", self.pid)])
                .output()
                .unwrap();
            if String::from_utf8_lossy(&out.stdout).trim().is_empty() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        false
    }
}

impl Drop for Ballast {
    fn drop(&mut self) {
        let _ = std::process::Command::new("/usr/bin/ssh")
            .args([
                "workhorse",
                &format!("/bin/kill -KILL {} 2>/dev/null", self.pid),
            ])
            .output();
    }
}

fn workhorse_memory() -> (u64, u64, u64) {
    let out = std::process::Command::new("/usr/bin/ssh")
        .args([
            "workhorse",
            "/usr/bin/vm_stat; echo @@; /usr/sbin/sysctl -n hw.memsize kern.memorystatus_level kern.memorystatus_vm_pressure_level",
        ])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    let (vm, sys) = text.split_once("@@").unwrap();
    let values: Vec<u64> = sys.split_whitespace().map(|v| v.parse().unwrap()).collect();
    let reading = goose_sidecar::distributed::probe::parse_vm_stat(vm, values[0]).unwrap();
    (reading.available_bytes, values[1], values[2])
}

const GIB_F: f64 = (1u64 << 30) as f64;

/// The memory watchdog on REAL pressure: a ballast we own on the workhorse crosses the WARN
/// reserve (admission closes: a new request is refused by name while the in-flight stream
/// finishes), is released (admission reopens), then a second ballast crosses CRITICAL (a verified
/// stop, never restarted). The reserves are RAISED by config for this run so both crossings leave
/// the 96 GB Mac with tens of GiB available — far from the kernel's own pressure levels.
#[tokio::test]
#[ignore = "needs both Macs and the owner's 27B; allocates up to ~25 GiB of ballast on the workhorse"]
async fn live_watchdog_warn_then_critical_on_real_workhorse_pressure() {
    use goose_sidecar::distributed::{EventKind, RunState};
    let (idle, idle_level, _) = workhorse_memory();
    let total = 96.0 * GIB_F;
    // The rank takes ~18.5 GiB; WARN 8 GiB below where the loaded node will sit, CRITICAL 10 below that.
    let warn_threshold = idle as f64 - 18.5 * GIB_F - 8.0 * GIB_F;
    let critical_threshold = warn_threshold - 10.0 * GIB_F;
    let mut config = recorded_config();
    config.restart_on_failure = true;
    config.watchdog_warn_ratio = Some(warn_threshold / total);
    config.watchdog_critical_ratio = Some(critical_threshold / total);
    println!(
        "workhorse idle: available {:.1} GiB, memorystatus_level {idle_level}%; WARN below {:.1} GiB \
         (ratio {:.4}), CRITICAL below {:.1} GiB (ratio {:.4})",
        idle as f64 / GIB_F,
        warn_threshold / GIB_F,
        warn_threshold / total,
        critical_threshold / GIB_F,
        critical_threshold / total
    );
    let base = config.base_url();
    let model = config.model_id.clone();
    let manager = DistributedManager::new(Arc::new(SystemExec));
    match manager
        .start(config.clone(), unaliased(&config))
        .await
        .unwrap()
    {
        StartOutcome::Started { .. } => {}
        StartOutcome::Refused { code, message, .. } => panic!("refused {code:?}: {message}"),
    }
    wait_serving(&manager, None).await;
    tokio::time::sleep(Duration::from_secs(5)).await;
    let status = manager.status();
    let loaded = status.nodes[1].available_bytes.unwrap() as f64;
    let macbook = status.nodes[0].available_bytes.unwrap() as f64;
    let macbook_warn = warn_threshold / total * 128.0 * GIB_F;
    println!(
        "loaded: workhorse {:.1} GiB, MacBook {:.1} GiB (its WARN line {:.1} GiB)",
        loaded / GIB_F,
        macbook / GIB_F,
        macbook_warn / GIB_F
    );
    assert!(
        loaded > warn_threshold + 3.0 * GIB_F,
        "the loaded workhorse already sits near WARN"
    );
    if macbook < macbook_warn + 5.0 * GIB_F {
        manager.stop().await;
        panic!("the MacBook is too close to the raised WARN line for a clean workhorse-only test");
    }
    let mut cursor = print_events(&manager, 0);

    // WARN: an in-flight stream, then ballast past the line.
    let seen = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let inflight = tokio::spawn(stream_completion(
        base.clone(),
        model.clone(),
        1500,
        Arc::clone(&seen),
    ));
    while seen.load(std::sync::atomic::Ordering::SeqCst) < 5 {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let ballast_gib = ((loaded - warn_threshold) / GIB_F).ceil() as u64 + 3;
    let ballast = Ballast::start(ballast_gib).await;
    println!("ballast 1: pid {} → {ballast_gib} GiB", ballast.pid);
    loop {
        tokio::time::sleep(Duration::from_secs(2)).await;
        let (avail, level, pressure) = workhorse_memory();
        println!(
            "  held {:>2} GiB | workhorse available {:.1} GiB, memorystatus_level {level}%, pressure level {pressure} | admission open {}",
            ballast.held(),
            avail as f64 / GIB_F,
            manager.status().admission_open
        );
        if !manager.status().admission_open {
            break;
        }
        assert!(
            manager.status().state != RunState::Stopped,
            "stopped before WARN"
        );
        assert!(ballast.held() <= ballast_gib as usize);
    }
    cursor = print_events(&manager, cursor);
    let refused = reqwest::Client::new()
        .post(format!("{base}/v1/chat/completions"))
        .header("content-type", "application/json")
        .body(serde_json::json!({"model": model, "messages": [{"role": "user", "content": "hi"}], "max_tokens": 2}).to_string())
        .send()
        .await
        .unwrap();
    let code = refused.status().as_u16();
    let body = refused.text().await.unwrap();
    println!("new request during WARN: {code} {body}");
    assert_eq!(code, 503);
    assert!(body.contains("not admitting") && body.contains("workhorse"));
    let (chunks, done, ended) = inflight.await.unwrap();
    println!("in-flight stream: {chunks} chunks, [DONE]={done}, {ended}");
    assert!(done, "the request admitted before WARN finishes");

    // Release → admission reopens.
    assert!(ballast.kill(), "ballast 1 pid {} did not exit", ballast.pid);
    println!("ballast 1 pid {} killed", ballast.pid);
    while !manager.status().admission_open {
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    cursor = print_events(&manager, cursor);
    let (avail, level, _) = workhorse_memory();
    println!(
        "after release: workhorse available {:.1} GiB, memorystatus_level {level}%",
        avail as f64 / GIB_F
    );
    assert_eq!(short_ok(&base, &model).await, 200, "admission reopened");

    // CRITICAL: second ballast past the critical line → verified stop, no restart.
    let now = manager.status().nodes[1].available_bytes.unwrap() as f64;
    let ballast_gib = ((now - critical_threshold) / GIB_F).ceil() as u64 + 3;
    let ballast = Ballast::start(ballast_gib).await;
    println!("ballast 2: pid {} → {ballast_gib} GiB", ballast.pid);
    loop {
        tokio::time::sleep(Duration::from_secs(2)).await;
        let (avail, level, pressure) = workhorse_memory();
        let state = manager.status().state;
        println!(
            "  held {:>2} GiB | workhorse available {:.1} GiB, memorystatus_level {level}%, pressure level {pressure} | state {state:?}",
            ballast.held(),
            avail as f64 / GIB_F
        );
        if state == RunState::Stopped {
            break;
        }
        assert!(ballast.held() <= ballast_gib as usize);
    }
    print_events(&manager, cursor);
    tokio::time::sleep(Duration::from_secs(6)).await;
    let status = manager.status();
    println!(
        "after CRITICAL: state {:?}, restarts {}, lastError {:?}",
        status.state, status.restarts, status.last_error
    );
    assert_eq!(
        status.state,
        RunState::Stopped,
        "a CRITICAL stop is never restarted"
    );
    assert_eq!(status.restarts, 0);
    assert!(status
        .events
        .iter()
        .any(|e| e.kind == EventKind::WatchdogCritical));
    let stopped = status
        .events
        .iter()
        .rev()
        .find(|e| e.kind == EventKind::Stopped)
        .unwrap();
    assert!(
        stopped.message.contains("gone (verified over ssh)"),
        "{}",
        stopped.message
    );

    assert!(ballast.kill(), "ballast 2 pid {} did not exit", ballast.pid);
    tokio::time::sleep(Duration::from_secs(3)).await;
    let (back, level, pressure) = workhorse_memory();
    println!(
        "ballast 2 pid {} killed; workhorse available {:.1} GiB (idle was {:.1}), memorystatus_level {level}%, pressure level {pressure}",
        ballast.pid,
        back as f64 / GIB_F,
        idle as f64 / GIB_F
    );
    assert!(back as f64 > idle as f64 - 4.0 * GIB_F, "memory returned");
    let leftover = std::process::Command::new("/usr/bin/ssh")
        .args(["workhorse", "/bin/ps -axo pid=,command= | /usr/bin/grep -E 'goose-distributed-rank|BALLAST' | /usr/bin/grep -v grep"])
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&leftover.stdout).trim().is_empty());
}

/// Make room over ssh on a peer that holds NO model (`GOOSE_COMPACT_HOST`, default `workhorse`):
/// Apple's memory_pressure to the kernel's WARN, released at once, sampled until available stopped
/// rising. Nothing is quit. Afterwards no `memory_pressure` may remain on the peer.
#[tokio::test]
#[ignore = "pressures a real Mac to WARN: run only on a peer with no model loaded"]
async fn live_compaction_over_ssh() {
    use goose_sidecar::distributed::compaction::{compact_node, CompactionOutcome};
    let host = env_or("GOOSE_COMPACT_HOST", "workhorse");
    let started = Instant::now();
    let outcome = compact_node(&SystemExec, Some(&host), &host).await.unwrap();
    let CompactionOutcome::Compacted(report) = outcome else {
        panic!("{host} refused: {outcome:?}");
    };
    println!(
        "{} ({:.1} s)",
        report.summary(),
        started.elapsed().as_secs_f64()
    );
    println!("{report:#?}");
    let leftover = std::process::Command::new("/usr/bin/ssh")
        .args([host.as_str(), "/usr/bin/pgrep -fl /usr/bin/memory_pressure"])
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&leftover.stdout).trim().is_empty(),
        "ballast left behind: {}",
        String::from_utf8_lossy(&leftover.stdout)
    );
}

/// The negative control: a Mac with an MLX engine loaded is refused by name, and nothing is
/// pressured (`GOOSE_COMPACT_BUSY_HOST` unset = this Mac).
#[tokio::test]
#[ignore = "needs an MLX engine running on the target Mac"]
async fn live_compaction_refuses_beside_a_loaded_engine() {
    use goose_sidecar::distributed::compaction::{compact_node, CompactionOutcome};
    let host = std::env::var("GOOSE_COMPACT_BUSY_HOST").ok();
    let outcome = compact_node(&SystemExec, host.as_deref(), "target")
        .await
        .unwrap();
    let CompactionOutcome::Refused(refusal) = outcome else {
        panic!("expected a refusal beside a loaded engine: {outcome:?}");
    };
    println!("{}: {}", refusal.code, refusal.message);
    assert_eq!(refusal.code, "engineLoaded");
}

/// Flash split over both Macs at the GPU-ceiling budget rule, with "Free memory automatically" ON:
/// the start's preflight compacts a short node and plans again; then a 2k prompt, a long prompt
/// and two concurrent requests. Sample `kern.memorystatus_vm_pressure_level` on both Macs from
/// outside (the report reads the supervisor's own watchdog events). Stops verified at the end.
/// GOOSE_FLASH_LONG_TOKENS (default 24,000) sizes the long prompt.
#[tokio::test]
#[ignore = "loads Flash on both Macs: nothing else may run an MLX engine on either"]
async fn live_flash_at_the_ceiling_rule_after_compaction() {
    use goose_sidecar::distributed::provision::EnvSpec;
    let home = dirs::home_dir().unwrap();
    let mut config = recorded_config();
    config.model_id = "rapid-mlx/Qwen3.8-Flash-Next-4bit".to_string();
    config.context = None;
    for node in &mut config.nodes {
        node.free_memory_automatically = true;
    }
    config.nodes[0].pipeline_python = Some(EnvSpec::pipeline().python(&home.display().to_string()));
    config.nodes[0].model_dir = home
        .join(".goose/models/rapid-mlx/Qwen3.8-Flash-Next-4bit")
        .display()
        .to_string();
    config.nodes[1].pipeline_python = Some(EnvSpec::pipeline().python("/Users/workhorse"));
    config.nodes[1].model_dir =
        "/Users/workhorse/.goose/models/rapid-mlx/Qwen3.8-Flash-Next-4bit".to_string();
    let served = unaliased(&config);
    let manager = DistributedManager::new(Arc::new(SystemExec));
    let t0 = Instant::now();
    let stamp = |what: &str| println!("[{:>7.1}s] {what}", t0.elapsed().as_secs_f64());
    // "Make room" on every node first (GOOSE_FLASH_COMPACT_FIRST=0 skips it): the budgets are
    // then built on post-compaction readings; the start still compacts a node that is short.
    if env_or("GOOSE_FLASH_COMPACT_FIRST", "1") == "1" {
        for node in &config.nodes {
            stamp(&format!("make room on {}", node.name));
            let record = manager.make_room(&config, &node.name).await.unwrap();
            match (&record.report, &record.refusal, &record.error) {
                (Some(report), _, _) => println!("{}", report.summary()),
                (_, Some(refusal), _) => panic!("{} refused: {refusal:?}", node.name),
                (_, _, Some(error)) => panic!("{} failed: {error}", node.name),
                _ => unreachable!(),
            }
        }
    }
    stamp("start (preflight, compaction when short)");
    let preflight = match manager.start(config.clone(), served.clone()).await.unwrap() {
        StartOutcome::Started { preflight } => preflight,
        StartOutcome::Refused {
            code,
            message,
            preflight,
        } => {
            for e in &manager.status().events {
                println!("event {:?} {:?}: {}", e.kind, e.node, e.message);
            }
            if let Some(p) = preflight {
                for n in &p.nodes {
                    println!(
                        "{} short {:?} checks {:#?}",
                        n.name, n.short_bytes, n.checks
                    );
                }
            }
            panic!("refused {code:?}: {message}");
        }
    };
    let gib = |b: u64| b as f64 / GIB_F;
    for n in &preflight.nodes {
        let plan = n.plan.as_ref().unwrap();
        println!(
            "{}: layers [{}, {}) planned {:.2} GiB of budget {:.2} (available {:.2}, GPU ceiling {:.2}, iogpu.wired_limit_mb {:?})",
            n.name,
            plan.layer_start,
            plan.layer_end,
            gib(plan.with_overhead_bytes),
            gib(plan.budget_bytes),
            gib(n.available_bytes.unwrap()),
            gib(n.ceiling_bytes.unwrap()),
            n.wired_limit_mb
        );
    }
    println!(
        "context {:?} ({:?})",
        preflight.context_limit, preflight.context_source
    );
    for c in &manager.status().compactions {
        println!("compaction {c:?}");
    }
    loop {
        let status = manager.status();
        match status.state {
            RunState::Ready | RunState::Serving => break,
            RunState::Failed | RunState::Stopped => {
                for e in &status.events {
                    println!("event {:?} {:?}: {}", e.kind, e.node, e.message);
                }
                panic!("start failed: {:?}", status.last_error);
            }
            _ => tokio::time::sleep(Duration::from_millis(500)).await,
        }
    }
    stamp("ready");

    let http = reqwest::Client::new();
    let base = config.base_url();
    let paragraph = "The workhorse is a Mac Studio with an M3 Ultra and 96 GB of unified memory; \
                     the MacBook is an M4 Max with 128 GB. They are joined by one Thunderbolt 5 \
                     cable that carries RDMA traffic for the pipeline split. ";
    let prompt_of = |tokens: usize| paragraph.repeat(tokens * 4 / paragraph.len() + 1);
    let ask = |prompt: String, max_tokens: u32| {
        let http = http.clone();
        let base = base.clone();
        let served = served.clone();
        async move {
            let started = Instant::now();
            let resp = http
                .post(format!("{base}/v1/chat/completions"))
                .header("content-type", "application/json")
                .body(
                    serde_json::json!({
                        "model": served,
                        "messages": [{"role": "user", "content": format!("{prompt}\n\nSummarize the text above in one sentence.")}],
                        "max_tokens": max_tokens,
                        "temperature": 0,
                        "stream": false,
                        "chat_template_kwargs": {"enable_thinking": false},
                    })
                    .to_string(),
                )
                .send()
                .await
                .unwrap();
            let status = resp.status().as_u16();
            let text = resp.text().await.unwrap();
            let body: serde_json::Value =
                serde_json::from_str(&text).unwrap_or_else(|_| serde_json::json!({ "raw": text }));
            (status, started.elapsed(), body)
        }
    };
    let report = |label: &str, (status, took, body): (u16, Duration, serde_json::Value)| {
        println!(
            "{label}: HTTP {status} in {:.1} s — usage {} — {}",
            took.as_secs_f64(),
            body["usage"],
            body["choices"][0]["message"]["content"]
                .as_str()
                .unwrap_or("<no content>")
                .chars()
                .take(160)
                .collect::<String>()
        );
        assert_eq!(status, 200, "{body}");
    };
    stamp("2k prompt");
    report("2k", ask(prompt_of(2_000), 128).await);
    let long: usize = env_or("GOOSE_FLASH_LONG_TOKENS", "24000").parse().unwrap();
    stamp(&format!("long prompt ({long} tokens)"));
    report("long", ask(prompt_of(long), 128).await);
    stamp("2 concurrent 2k prompts");
    let (a, b) = tokio::join!(ask(prompt_of(2_000), 128), ask(prompt_of(2_100), 128));
    report("concurrent A", a);
    report("concurrent B", b);
    stamp("done serving");

    let status = manager.status();
    for n in &status.nodes {
        println!(
            "{}: peak {:?} GiB, active {:?}, memory_limit {:?}, wired_limit {:?}, available {:?}, pressure {:?}",
            n.name,
            n.peak_bytes.map(gib),
            n.active_bytes.map(gib),
            n.memory_limit_bytes.map(gib),
            n.wired_limit_bytes.map(gib),
            n.available_bytes.map(gib),
            n.pressure
        );
    }
    for e in &status.events {
        println!("event {:?} {:?}: {}", e.kind, e.node, e.message);
    }
    let stop = manager.stop().await;
    println!("stop verified {}: {:?}", stop.verified, stop.steps);
    assert!(stop.verified);
    stamp("stopped");
}
