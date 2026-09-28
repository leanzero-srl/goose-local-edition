//! Q-240: a start that finds its port held names the holder — pid, command line, whether it is
//! ours — and stops it only on proof it is an engine a goose sidecar started for this name and
//! port whose starter is gone. Real processes stand in for every shape: a foreign listener, the
//! leftover `uv` + engine pair of a dead goosed, and an engine whose starter is still alive.
#![cfg(unix)]

use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::time::Duration;

use goose_sidecar::{sidecar_marker, Sidecar, SidecarConfig, SIDECAR_MARKER_ENV};

const FAKE_ENGINE: &str = r#"
import http.server, sys
class H(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        body = b'{"object":"list","data":[{"id":"fake"}]}'
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)
    def log_message(self, *a):
        pass
http.server.HTTPServer(("127.0.0.1", int(sys.argv[1])), H).serve_forever()
"#;

/// A launcher that starts the engine and waits on it — the `uv` → python pair. `new_group` puts
/// the engine in a group of its own (the shape of an engine another goosed started).
const LAUNCHER: &str = r#"
import subprocess, sys
engine = subprocess.Popen([sys.executable, "-c", sys.argv[1], sys.argv[2]],
                          start_new_session=sys.argv[3] == "new_group")
print(engine.pid, flush=True)
engine.wait()
"#;

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn config(port: u16) -> SidecarConfig {
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
    config
}

fn alive(pid: u32) -> bool {
    goose_sidecar::machine::process_start(pid).is_some_and(|(_, zombie)| !zombie)
}

fn listening(port: u16) -> bool {
    std::net::TcpStream::connect(("127.0.0.1", port)).is_ok()
}

/// Until `port` accepts — or the process that should bind it is gone, which fails the test.
fn wait_listening(port: u16, binder: u32) {
    while !listening(port) {
        assert!(alive(binder), "pid {binder} ended before it bound {port}");
        std::thread::sleep(goose_sidecar::GRACE_TICK);
    }
}

/// `sh`, leading a group of its own, backgrounds the launcher and exits at once: the launcher is
/// re-parented to init — a goosed that died and left its engine behind. Returns (launcher, engine).
fn orphaned_pair(port: u16, marker: Option<&str>, group: &str) -> (u32, u32) {
    let mut sh = Command::new("/bin/sh");
    sh.args([
        "-c",
        r#"python3 -c "$1" "$2" "$3" "$4" & echo $!"#,
        "sh",
        LAUNCHER,
        FAKE_ENGINE,
        &port.to_string(),
        group,
    ])
    .stdout(Stdio::piped())
    .stderr(Stdio::null())
    .process_group(0);
    if let Some(marker) = marker {
        sh.env(SIDECAR_MARKER_ENV, marker);
    }
    let mut sh = sh.spawn().unwrap();
    let mut out = BufReader::new(sh.stdout.take().unwrap());
    let mut line = String::new();
    out.read_line(&mut line).unwrap();
    let launcher: u32 = line.trim().parse().unwrap();
    line.clear();
    out.read_line(&mut line).unwrap();
    let engine: u32 = line.trim().parse().unwrap();
    sh.wait().unwrap();
    wait_listening(port, engine);
    (launcher, engine)
}

fn stop(pids: &[u32]) {
    for pid in pids {
        // SAFETY: a process this test started, signalled by its pid alone.
        unsafe { libc::kill(*pid as libc::pid_t, libc::SIGKILL) };
    }
}

/// A listener nobody marked — the owner's own server, or an engine from a goose older than the
/// marker — is named and left alone: the start fails before it spawns anything, and never takes
/// the stand-in's catalog for its own readiness. Before Q-240 the start answered Ok here: the
/// stand-in serves the expected id, so the probe read it as this child being ready.
#[tokio::test]
async fn a_start_over_a_listener_it_did_not_start_names_it_and_signals_nothing() {
    let port = free_port();
    let mut stand_in = Command::new("python3")
        .args(["-c", FAKE_ENGINE, &port.to_string()])
        .spawn()
        .unwrap();
    let stand_in_pid = stand_in.id();
    wait_listening(port, stand_in_pid);

    let err = match Sidecar::start(config(port)).await {
        Ok(sidecar) => {
            let pid = sidecar.pid().await;
            sidecar.shutdown().await;
            stand_in.kill().unwrap();
            panic!("the start took pid {stand_in_pid}'s catalog for its own child {pid:?}");
        }
        Err(e) => format!("{e:#}"),
    };
    eprintln!("the refusal: {err}");
    let untouched = alive(stand_in_pid) && listening(port);
    stop_after(&[stand_in_pid], || {
        assert!(err.contains(&format!("port {port}")), "{err}");
        assert!(err.contains(&format!("pid {stand_in_pid}")), "{err}");
        assert!(
            err.contains("http.server"),
            "the command line is named: {err}"
        );
        assert!(err.contains("not this goose's"), "{err}");
        assert!(
            err.contains(&format!("carries no {SIDECAR_MARKER_ENV}")),
            "{err}"
        );
        assert!(err.contains("nothing was signalled"), "{err}");
        assert!(untouched, "the stand-in was touched");
    });
    stand_in.wait().unwrap();
}

/// The Q-240 shape: a goosed died and left its launcher (`uv`, re-parented to init) and the engine
/// under it holding the port, both carrying the marker this sidecar stamps. The start stops both,
/// per pid, and serves from its own child.
#[tokio::test]
async fn a_start_stops_the_launcher_and_engine_a_dead_goose_left_on_its_port() {
    let port = free_port();
    let marker = sidecar_marker("fake-engine", &format!("http://127.0.0.1:{port}"));
    let (launcher, engine) = orphaned_pair(port, Some(&marker), "same_group");

    let sidecar = Sidecar::start(config(port)).await.unwrap_or_else(|e| {
        stop(&[launcher, engine]);
        panic!("{e:#}")
    });
    let own = sidecar.pid().await.unwrap();
    let healthy = sidecar.healthy().await;
    let (engine_alive, launcher_alive) = (alive(engine), alive(launcher));
    sidecar.shutdown().await;
    stop_after(&[launcher, engine], || {
        assert!(!engine_alive, "the leftover engine {engine} still ran");
        assert!(
            !launcher_alive,
            "the leftover launcher {launcher} still ran"
        );
        assert_ne!(own, engine);
        assert!(healthy, "its own child serves");
        assert!(!listening(port));
    });
}

/// A marked engine whose starter is ALIVE is another goose's: named with the starter's pid, and
/// left serving.
#[tokio::test]
async fn an_engine_whose_starter_lives_is_another_gooses_and_is_left_serving() {
    let port = free_port();
    let marker = sidecar_marker("fake-engine", &format!("http://127.0.0.1:{port}"));
    let (starter, engine) = orphaned_pair(port, Some(&marker), "new_group");

    let err = match Sidecar::start(config(port)).await {
        Ok(sidecar) => {
            sidecar.shutdown().await;
            stop(&[starter, engine]);
            panic!("the start went ahead over another goose's engine");
        }
        Err(e) => format!("{e:#}"),
    };
    eprintln!("the refusal: {err}");
    stop_after(&[starter, engine], || {
        assert!(err.contains(&format!("pid {engine}")), "{err}");
        assert!(
            err.contains(&format!("pid {starter}")) && err.contains("is alive"),
            "the live starter is named: {err}"
        );
        assert!(err.contains("nothing was signalled"), "{err}");
        assert!(alive(engine) && alive(starter) && listening(port));
    });
}

/// A marker for another name or port is not this sidecar's engine.
#[tokio::test]
async fn an_engine_marked_for_another_port_is_not_ours() {
    let port = free_port();
    let (launcher, engine) =
        orphaned_pair(port, Some("fake-engine@http://127.0.0.1:1"), "same_group");
    let err = match Sidecar::start(config(port)).await {
        Ok(sidecar) => {
            sidecar.shutdown().await;
            stop(&[launcher, engine]);
            panic!("the start went ahead over an engine marked for another port");
        }
        Err(e) => format!("{e:#}"),
    };
    stop_after(&[launcher, engine], || {
        assert!(err.contains("fake-engine@http://127.0.0.1:1"), "{err}");
        assert!(alive(engine) && listening(port));
    });
}

fn stop_after(pids: &[u32], checks: impl FnOnce()) {
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(checks));
    stop(pids);
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}
