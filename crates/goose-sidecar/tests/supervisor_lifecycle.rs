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
    );
    config.startup_timeout = Duration::from_secs(20);
    config.backoff_initial = Duration::from_millis(100);
    config.backoff_cap = Duration::from_millis(200);
    config
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
