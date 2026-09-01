//! Lifecycle tests against a real child process: a python3 stdlib HTTP server standing in
//! for an engine (macOS ships python3; the repo's test suites already shell out freely).
use std::net::TcpListener;
use std::time::Duration;

use goose_sidecar::{Sidecar, SidecarConfig};

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
    tokio::time::sleep(Duration::from_millis(300)).await;
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

#[cfg(unix)]
fn process_alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

#[tokio::test]
async fn starts_becomes_healthy_and_shuts_down_without_orphans() {
    let port = free_port();
    let sidecar = Sidecar::start(fake_engine_config(port)).await.unwrap();
    assert!(sidecar.healthy().await);
    let pid = sidecar.pid().await.unwrap();
    assert!(process_alive(pid));

    sidecar.shutdown().await;
    tokio::time::sleep(Duration::from_millis(300)).await;
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
    tokio::time::sleep(Duration::from_millis(300)).await;

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
        tokio::time::sleep(Duration::from_millis(200)).await;
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
