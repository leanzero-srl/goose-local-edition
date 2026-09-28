//! Lifecycle tests against a real child process: a python3 stdlib HTTP server standing in
//! for an engine (macOS ships python3; the repo's test suites already shell out freely).
#![cfg(unix)]

use std::net::TcpListener;
use std::sync::Arc;
use std::time::{Duration, Instant};

use goose_sidecar::engine::start_phase;
use goose_sidecar::{Ensured, Sidecar, SidecarConfig, StartCancel, StartupWatch};

const FAKE_ENGINE: &str = r#"
import http.server, sys
class H(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path == "/v1/models":
            body = b'{"object":"list","data":[{"id":"fake"}]}'
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
        else:
            self.send_response(404); self.end_headers()
    def log_message(self, *a):
        print("served", self.path, file=sys.stderr)
http.server.HTTPServer(("127.0.0.1", int(sys.argv[1])), H).serve_forever()
"#;

fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

fn fake_engine_config(port: u16) -> SidecarConfig {
    let mut config = SidecarConfig::new(
        "fake-engine",
        vec![
            "python3".to_string(),
            "-c".to_string(),
            FAKE_ENGINE.to_string(),
            port.to_string(),
        ],
        format!("http://127.0.0.1:{port}"),
        "fake",
    );
    config.startup_stall_window = Duration::from_secs(20);
    config.backoff_initial = Duration::from_millis(100);
    config.backoff_cap = Duration::from_millis(200);
    config
}

/// The id net (S-H3): a 200 whose catalog serves some OTHER id is not readiness. The fake
/// serves "fake"; expecting "other" must fail the start and say what was served.
#[tokio::test]
async fn a_catalog_serving_another_id_is_not_ready() {
    let port = free_port();
    let mut config = fake_engine_config(port);
    config.expected_model_id = "other".to_string();
    config.startup_stall_window = Duration::from_secs(3);
    let err = match Sidecar::start(config).await {
        Ok(_) => panic!("start accepted a catalog serving a different id"),
        Err(e) => e.to_string(),
    };
    assert!(
        err.contains("serves 'fake', expected 'other'"),
        "err was: {err}"
    );
    // No sleep: the failed start releases the port BEFORE it returns (release_port waits for it).
    assert!(
        std::net::TcpStream::connect(("127.0.0.1", port)).is_err(),
        "the failed start must not leave its child listening"
    );
}

/// S-L8: the startup terminator is progress, not a clock. An engine that keeps reporting
/// (a stderr line every 500 ms for 4 s, then serving) must START under a 2 s stall window
/// — a 2 s startup *timeout* would have failed it at t=2.
#[tokio::test]
async fn a_slow_engine_that_keeps_progressing_is_never_failed_by_the_clock() {
    let port = free_port();
    let mut config = fake_engine_config(port);
    config.command = vec![
        "python3".to_string(),
        "-c".to_string(),
        format!(
            "import sys, time\nfor i in range(8):\n    print('loading shard', i, \
             file=sys.stderr, flush=True); time.sleep(0.5)\n{FAKE_ENGINE}"
        ),
        port.to_string(),
    ];
    config.startup_stall_window = Duration::from_secs(2);
    let started = std::time::Instant::now();
    let sidecar = Sidecar::start(config).await.unwrap();
    assert!(
        started.elapsed() >= Duration::from_secs(4),
        "the engine served before its 4 s of loading — the test proves nothing"
    );
    assert!(sidecar.healthy().await);
    sidecar.shutdown().await;
}

/// The other half: a child that never serves AND never progresses (no stderr, no CPU, no
/// memory movement) fails after the stall window, loudly, and is not left behind.
#[tokio::test]
async fn a_silent_engine_that_never_serves_fails_by_stall() {
    let port = free_port();
    let marker = format!("stall-marker-{port}");
    let mut config = fake_engine_config(port);
    config.command = vec![
        "python3".to_string(),
        "-c".to_string(),
        format!("import time  # {marker}\ntime.sleep(600)"),
    ];
    config.startup_stall_window = Duration::from_secs(2);
    let err = match Sidecar::start(config).await {
        Ok(_) => panic!("a silent child must not count as started"),
        Err(e) => e.to_string(),
    };
    assert!(
        err.contains("stalled during startup: no progress for"),
        "err was: {err}"
    );
    assert!(
        err.contains("connection refused") || err.contains("GET http"),
        "err was: {err}"
    );
    let leftover = std::process::Command::new("pgrep")
        .args(["-f", &marker])
        .output()
        .unwrap();
    assert!(
        leftover.stdout.is_empty(),
        "stalled child left running: {}",
        String::from_utf8_lossy(&leftover.stdout)
    );
}

/// A zombie has exited: the kernel answers no process info for it (`machine::process_start`).
fn process_alive(pid: u32) -> bool {
    goose_sidecar::machine::process_start(pid).is_some_and(|(_, zombie)| !zombie)
}

/// Until the kernel says `pid` has finished exiting — `ps` shows it a zombie (waitable, so the
/// supervisor's next `try_wait` sees it) or not at all. After a SIGKILL, which cannot be caught,
/// that is the only thing left to wait on (Q-245: fixed 200/300 ms sleeps stood in for it).
async fn wait_exited(pid: u32) {
    loop {
        let stat = std::process::Command::new("ps")
            .args(["-o", "stat=", "-p", &pid.to_string()])
            .output()
            .unwrap();
        let stat = String::from_utf8_lossy(&stat.stdout);
        if stat.trim().is_empty() || stat.contains('Z') {
            return;
        }
        tokio::time::sleep(goose_sidecar::GRACE_TICK).await;
    }
}

#[tokio::test]
async fn starts_becomes_healthy_and_shuts_down_without_orphans() {
    let port = free_port();
    let sidecar = Sidecar::start(fake_engine_config(port)).await.unwrap();
    assert!(sidecar.healthy().await);
    let pid = sidecar.pid().await.unwrap();
    assert!(process_alive(pid));

    // No sleep: shutdown reaps its child before it returns.
    sidecar.shutdown().await;
    assert!(!process_alive(pid), "engine pid {pid} survived shutdown");
}

#[tokio::test]
async fn restarts_after_the_engine_is_killed() {
    let port = free_port();
    let sidecar = Sidecar::start(fake_engine_config(port)).await.unwrap();
    let first_pid = sidecar.pid().await.unwrap();

    unsafe {
        libc::kill(first_pid as libc::pid_t, libc::SIGKILL);
    }
    wait_exited(first_pid).await;

    sidecar.ensure_running().await.unwrap();
    let second_pid = sidecar.pid().await.unwrap();
    assert_ne!(first_pid, second_pid);
    assert!(sidecar.healthy().await);

    sidecar.shutdown().await;
}

#[tokio::test]
async fn startup_failure_reports_exit_and_stderr() {
    let port = free_port();
    let mut config = fake_engine_config(port);
    config.command = vec![
        "python3".to_string(),
        "-c".to_string(),
        "import sys; print('boom: no metal device', file=sys.stderr); sys.exit(3)".to_string(),
    ];
    let err = match Sidecar::start(config).await {
        Ok(_) => panic!("start unexpectedly succeeded"),
        Err(e) => e.to_string(),
    };
    assert!(err.contains("exited during startup"), "err was: {err}");
    assert!(
        err.contains("boom: no metal device"),
        "stderr tail missing: {err}"
    );
}

#[tokio::test]
async fn circuit_breaker_opens_after_repeated_deaths() {
    let port = free_port();
    let sidecar = Sidecar::start(fake_engine_config(port)).await.unwrap();

    let mut opened = false;
    for _ in 0..6 {
        let pid = sidecar.pid().await.unwrap();
        unsafe {
            libc::kill(pid as libc::pid_t, libc::SIGKILL);
        }
        wait_exited(pid).await;
        match sidecar.ensure_running().await {
            Ok(()) => continue,
            Err(e) => {
                assert!(
                    e.to_string().contains("circuit breaker"),
                    "unexpected error: {e}"
                );
                opened = true;
                break;
            }
        }
    }
    assert!(opened, "circuit breaker never opened after repeated kills");
    sidecar.shutdown().await;
}

/// A stand-in that writes the Loading and Warming lines in ONE `write` before it serves: the
/// reader takes both from one pipe read, so on this single-threaded runtime no startup look can
/// run between them — Loading lasts zero looks, deterministically.
fn engine_with_instant_phases(port: u16) -> SidecarConfig {
    let mut config = fake_engine_config(port);
    config.command[2] = format!(
        "import os\nos.write(2, b'Loading model with BatchedEngine\\nWarming up (compiling Metal shaders)\\n')\n{FAKE_ENGINE}"
    );
    config
}

fn phase_names(watch: &StartupWatch) -> Vec<&'static str> {
    watch
        .phase_marks()
        .iter()
        .map(|(phase, _)| *phase)
        .collect()
}

/// Q-275: a phase is marked at the line that shows it, never only at a startup look. Marked at
/// the 400 ms looks, a Loading superseded before the next look was lost (Linux CI read a
/// restart's phases as `loading: None`).
#[tokio::test]
async fn a_phase_superseded_before_any_look_is_still_marked() {
    let port = free_port();
    let watch = Arc::new(StartupWatch::with_phases(start_phase));
    let mut config = engine_with_instant_phases(port);
    config.startup_watch = Some(Arc::clone(&watch));
    let sidecar = Sidecar::start(config).await.unwrap();
    assert_eq!(phase_names(&watch), ["starting", "loading", "warming"]);
    sidecar.shutdown().await;
}

/// Q-275 on the restart path (Q-256): the restart marks ITS phases on the watch the caller
/// handed it — every one, the first sighted at the respawn, after the supervisor's backoff — and
/// the first start's watch is left as it was.
#[tokio::test]
async fn a_restart_marks_its_own_phases_from_its_respawn() {
    let port = free_port();
    let first = Arc::new(StartupWatch::with_phases(start_phase));
    let mut config = engine_with_instant_phases(port);
    config.startup_watch = Some(Arc::clone(&first));
    let backoff = config.backoff_initial;
    let sidecar = Sidecar::start(config).await.unwrap();
    let first_marks = first.phase_marks();

    let pid = sidecar.pid().await.unwrap();
    unsafe {
        libc::kill(pid as libc::pid_t, libc::SIGKILL);
    }
    wait_exited(pid).await;
    let restart = Arc::new(StartupWatch::with_phases(start_phase));
    let asked = Instant::now();
    let ensured = sidecar
        .ensure_running_unless(
            &Arc::new(StartCancel::default()),
            Some(Arc::clone(&restart)),
        )
        .await
        .unwrap();
    assert_eq!(ensured, Ensured::Restarted);
    assert_eq!(phase_names(&restart), ["starting", "loading", "warming"]);
    assert!(
        restart.phase_marks()[0].1 >= asked + backoff,
        "`starting` is sighted at the respawn, after the backoff"
    );
    assert_eq!(first.phase_marks(), first_marks);
    sidecar.shutdown().await;
}
