//! Lifecycle tests against a real child process: a python3 stdlib HTTP server standing in
//! for an engine (macOS ships python3; the repo's test suites already shell out freely).
#![cfg(unix)]

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
    goose_sidecar::unshared_loopback_port().unwrap()
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

/// Q-499: a start's readiness is its OWN child's answer. While the child loads, its port is free,
/// and another process can bind it and serve the very catalog the probe expects — on Linux CI a
/// sibling test's stand-in drew the same port and the slow start returned "ready" at t < 4 s,
/// before its own engine had bound anything. The sibling here answers one probe; only then may
/// the child try its bind, which fails, and the start must say so — never succeed on the
/// sibling's answer.
#[tokio::test]
async fn a_listener_outside_the_childs_tree_is_not_its_readiness() {
    use tokio::io::AsyncBufReadExt;

    let port = free_port();
    let dir = tempfile::tempdir().unwrap();
    let probed = dir.path().join("the-sibling-was-probed");
    let marker = format!("readiness-owner-marker-{port}");
    let mut config = fake_engine_config(port);
    config.command = vec![
        "python3".to_string(),
        "-c".to_string(),
        format!(
            "import os, sys, time  # {marker}\nwhile not os.path.exists({probed:?}):\n    \
             print('loading', file=sys.stderr, flush=True); time.sleep(0.1)\n{FAKE_ENGINE}"
        ),
        port.to_string(),
    ];

    let sibling = tokio::spawn(async move {
        // The child exists only after the start claimed the free port; bind it only then.
        loop {
            let found = std::process::Command::new("pgrep")
                .args(["-f", &marker])
                .output()
                .unwrap();
            if !found.stdout.is_empty() {
                break;
            }
            tokio::time::sleep(goose_sidecar::GRACE_TICK).await;
        }
        let mut sibling = tokio::process::Command::new("python3")
            .args(["-u", "-c", FAKE_ENGINE, &port.to_string()])
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let mut lines = tokio::io::BufReader::new(sibling.stderr.take().unwrap()).lines();
        while let Some(line) = lines.next_line().await.unwrap() {
            if line.contains("served /v1/models") {
                break;
            }
        }
        std::fs::write(&probed, b"").unwrap();
        sibling
    });

    let outcome = Sidecar::start(config).await;
    let mut sibling = sibling.await.unwrap();
    sibling.kill().await.unwrap();
    let err = match outcome {
        Ok(sidecar) => {
            sidecar.shutdown().await;
            panic!("the start took a listener outside its child's tree for the child's readiness");
        }
        Err(e) => e.to_string(),
    };
    assert!(err.contains("exited during startup"), "err was: {err}");
    assert!(err.contains("Address already in use"), "err was: {err}");
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

/// The line Rapid-MLX logs when a stream's generator raises, before it sends the client only
/// "Internal error during streaming" (helpers.py, disconnect_guard) — shaped as the engine prints
/// it; the exception is a stand-in.
const STREAM_ERROR_LINE: &str = "ERROR:rapid_mlx.service.helpers:[disconnect_guard] generator \
     raised RuntimeError: [metal::malloc] Unable to allocate 18874368000 bytes., 1 chunks, \
     elapsed=1108.4s";

fn engine_logs(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    std::fs::read_dir(dir)
        .map(|entries| entries.flatten().map(|e| e.path()).collect())
        .unwrap_or_default()
}

/// Q-423: the Studio's single engine logged why a 191k-token answer failed mid-stream, and the
/// only copy was goosed's 200-line memory tail. Every stderr line now lands, stamped, in the
/// engine's own log file under `log_dir` — the stream's ERROR line with it.
#[tokio::test]
async fn every_stderr_line_is_kept_in_the_engines_durable_log() {
    let port = free_port();
    let dir = tempfile::tempdir().unwrap();
    let mut config = fake_engine_config(port);
    config.command = vec![
        "python3".to_string(),
        "-c".to_string(),
        format!(
            "import sys\nprint({STREAM_ERROR_LINE:?}, file=sys.stderr, flush=True)\n{FAKE_ENGINE}"
        ),
        port.to_string(),
    ];
    config.log_dir = Some(dir.path().to_path_buf());
    let sidecar = Sidecar::start(config).await.unwrap();

    let asked = Instant::now();
    let kept = loop {
        let logs = engine_logs(dir.path());
        let text: String = logs
            .iter()
            .filter_map(|p| std::fs::read_to_string(p).ok())
            .collect();
        if text.contains(STREAM_ERROR_LINE) || asked.elapsed() > Duration::from_secs(10) {
            break (logs, text);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    sidecar.shutdown().await;

    let (logs, text) = kept;
    assert_eq!(logs.len(), 1, "one log per spawned engine: {logs:?}");
    let name = logs[0].file_name().unwrap().to_string_lossy().into_owned();
    assert!(
        name.starts_with("fake-engine-") && name.ends_with(".log"),
        "{name}"
    );
    let line = text
        .lines()
        .find(|l| l.contains(STREAM_ERROR_LINE))
        .unwrap_or_else(|| panic!("the engine's ERROR line is not in its log:\n{text}"));
    assert!(
        line.ends_with(&format!("Z err {STREAM_ERROR_LINE}")),
        "stamped with the instant goosed read it and the stream: {line}"
    );
}

/// A log that cannot be opened is said in the stderr tail — the words a failed start reports —
/// never a silent absence.
#[tokio::test]
async fn an_engine_log_that_cannot_be_opened_is_said_in_the_tail() {
    let port = free_port();
    let dir = tempfile::tempdir().unwrap();
    let not_a_dir = dir.path().join("a-file");
    std::fs::write(&not_a_dir, b"").unwrap();
    let mut config = fake_engine_config(port);
    config.command = vec![
        "python3".to_string(),
        "-c".to_string(),
        "import sys\nprint('ValueError: no weights', file=sys.stderr, flush=True)\nsys.exit(3)"
            .to_string(),
    ];
    config.log_dir = Some(not_a_dir.join("mlx-engine"));
    let err = match Sidecar::start(config).await {
        Ok(_) => panic!("a child that exits at once must not count as started"),
        Err(e) => format!("{e:#}"),
    };
    assert!(
        err.contains("goose: this engine's durable log is unavailable"),
        "{err}"
    );
    assert!(err.contains("ValueError: no weights"), "{err}");
}

/// An engine that answers `GET /fail` the way Rapid-MLX ends a failed stream: it logs the
/// exception on stderr FIRST, then sends the client only the sanitized words.
fn failing_engine(port: u16) -> SidecarConfig {
    let mut config = fake_engine_config(port);
    let script = format!(
        r#"
import http.server, sys
class H(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path == "/v1/models":
            body = b'{{"object":"list","data":[{{"id":"fake"}}]}}'
        elif self.path == "/fail":
            print("INFO:rapid_mlx.service.helpers:[disconnect_guard] poll #10", file=sys.stderr, flush=True)
            print({STREAM_ERROR_LINE:?}, file=sys.stderr, flush=True)
            print("Traceback (most recent call last):", file=sys.stderr, flush=True)
            body = b'data: {{"error":{{"message":"Internal error during streaming","type":"internal_error"}}}}'
        else:
            body = b'{{}}'
        self.send_response(200)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)
    def log_message(self, *a):
        pass
http.server.HTTPServer(("127.0.0.1", int(sys.argv[1])), H).serve_forever()
"#
    );
    config.command = vec![
        "python3".to_string(),
        "-c".to_string(),
        script,
        port.to_string(),
    ];
    config
}

/// Q-423: a stream's failure is matched with the exception its engine logged for it — asked the
/// moment the client holds the error, with no clock — and a request that logged nothing is
/// answered `None` at once (the stderr is read up to now), never waited on.
#[tokio::test]
async fn a_failed_request_is_matched_with_the_error_its_engine_logged() {
    let port = free_port();
    let dir = tempfile::tempdir().unwrap();
    let mut config = failing_engine(port);
    config.log_dir = Some(dir.path().to_path_buf());
    let sidecar = Sidecar::start(config).await.unwrap();
    let client = reqwest::Client::new();

    for _ in 0..20 {
        let mark = sidecar.error_mark().expect("a running engine has a mark");
        let body = client
            .get(format!("http://127.0.0.1:{port}/fail"))
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        assert!(body.contains("Internal error during streaming"), "{body}");
        let logged = sidecar.error_logged_since(mark).await.unwrap();
        assert_eq!(logged.line.as_deref(), Some(STREAM_ERROR_LINE));
        assert!(logged.log.contains("fake-engine-"), "{}", logged.log);
    }

    let mark = sidecar.error_mark().unwrap();
    client
        .get(format!("http://127.0.0.1:{port}/v1/models"))
        .send()
        .await
        .unwrap();
    let logged = sidecar.error_logged_since(mark).await.unwrap();
    assert_eq!(logged.line, None, "no error was logged after this mark");

    let stale = goose_sidecar::ErrorMark {
        pid: mark.pid + 1,
        errors: 0,
    };
    let refused = sidecar.error_logged_since(stale).await.unwrap_err();
    assert!(refused.contains("is gone"), "{refused}");
    sidecar.shutdown().await;
}
