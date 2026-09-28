//! Q-240: a start that finds its port held names the holder — pid, command line, whether it is
//! ours — and stops it only on proof it is an engine a goose sidecar started for this name and
//! port whose starter is gone. Real processes stand in for every shape: a foreign listener, the
//! leftover `uv` + engine pair of a dead goosed, and an engine whose starter is still alive — and
//! the same proof behind Unmount (Q-252), the one step (Q-251) and the status cache (Q-253).
#![cfg(unix)]

use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::time::Duration;

use goose_sidecar::engine::{engine_marker, EngineSettings, MlxEngineManager};
use goose_sidecar::port_holder::{listener_pids, HoldersCache};
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

/// The single engine's manager on `port`, supervising nothing.
fn manager_on(port: u16) -> MlxEngineManager {
    let manager = MlxEngineManager::new();
    manager.set_settings(EngineSettings {
        port,
        ..Default::default()
    });
    manager
}

/// A goose older than the marker, still running, and the unmarked engine it started in a process
/// group of its own — the `goosed` → `uv` + python shape (Q-251). The starter runs THIS process's
/// own program (its argv[0] is this test binary), which is how a goose starter is recognised: a
/// `/bin/sh` under that name, job control on so the engine it backgrounds leads its own group.
/// (A python starter cannot stand in: the hermit `python3` shim re-execs under its own argv[0].)
/// Returns (starter, engine).
fn older_goose_engine(port: u16) -> (u32, u32) {
    let own = std::env::current_exe().unwrap();
    let mut starter = Command::new("/bin/sh")
        .arg0(&own)
        .args([
            "-c",
            r#"set -m; python3 -c "$1" "$2" & echo $!; wait"#,
            "sh",
            FAKE_ENGINE,
            &port.to_string(),
        ])
        .env_remove(SIDECAR_MARKER_ENV)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .unwrap();
    let mut line = String::new();
    BufReader::new(starter.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let engine: u32 = line.trim().parse().unwrap();
    wait_listening(port, engine);
    let starter = starter.id();
    let read = |pid| goose_sidecar::port_holder::read_process(pid).unwrap();
    let (starter_read, engine_read) = (read(starter), read(engine));
    assert_eq!(
        starter_read.argv.first().map(String::as_str),
        own.to_str(),
        "the stand-in starter runs as this program"
    );
    assert_eq!(engine_read.parent, Some(starter), "the starter is its parent");
    assert_ne!(
        engine_read.group, starter_read.group,
        "the engine leads a group of its own, as `uv` does"
    );
    assert_eq!(engine_read.marker, None, "an older goose marks nothing");
    (starter, engine)
}

/// Q-252: another goose's live engine — marked for this very port, its starter alive — is named by
/// Unmount with the step to quit that starter, and left serving. Before Q-252 Unmount's reclaim
/// SIGTERMed every LISTEN pid on the port, this engine included.
#[tokio::test]
async fn unmount_leaves_another_gooses_live_engine_serving_and_names_its_starter() {
    let port = free_port();
    let (starter, engine) = orphaned_pair(port, Some(&engine_marker(port)), "new_group");

    let outcome = manager_on(port).unmount().await;
    let untouched = alive(engine) && alive(starter) && listening(port);
    stop_after(&[starter, engine], || {
        let refused = match outcome {
            Ok(()) => panic!("Unmount stopped another goose's live engine {engine}"),
            Err(refused) => refused.to_string(),
        };
        eprintln!("the refusal: {refused}");
        assert!(untouched, "the engine or its starter was signalled");
        assert!(refused.starts_with("Unmount stopped nothing: "), "{refused}");
        assert!(refused.contains(&format!("pid {engine}")), "{refused}");
        assert!(refused.contains("nothing was signalled"), "{refused}");
        assert!(
            refused.contains(&format!("quit what started it (pid {starter})")),
            "the step: {refused}"
        );
        assert!(!refused.contains("kill"), "{refused}");
    });
}

/// This goose's own leftover — the launcher and engine a dead goosed left, marked for this port —
/// is what Unmount still reclaims: both pids stopped, the port free.
#[tokio::test]
async fn unmount_stops_this_gooses_own_leftover_pair_per_pid() {
    let port = free_port();
    let (launcher, engine) = orphaned_pair(port, Some(&engine_marker(port)), "same_group");

    let outcome = manager_on(port).unmount().await;
    let (engine_alive, launcher_alive) = (alive(engine), alive(launcher));
    stop_after(&[launcher, engine], || {
        if let Err(refused) = outcome {
            panic!("{refused}");
        }
        assert!(!engine_alive, "the leftover engine {engine} still ran");
        assert!(
            !launcher_alive,
            "the leftover launcher {launcher} still ran"
        );
        assert!(!listening(port));
    });
}

/// Q-251: an engine a goose older than the marker mounted, that goose still running. Every surface
/// says one step — restart that goose — from one derivation: the start's refusal (Q-240), a
/// refused Unmount (Q-252), and the status the Engine panel reads (Q-249). Never `kill <pid>`.
#[tokio::test]
async fn an_older_gooses_engine_is_told_to_restart_that_goose_everywhere() {
    let port = free_port();
    let (starter, engine) = older_goose_engine(port);
    let manager = manager_on(port);

    let unmount = manager.unmount().await.map_err(|e| e.to_string());
    let start = goose_sidecar::port_holder::claim_port(port, &engine_marker(port))
        .await
        .map(|_| ())
        .map_err(|e| format!("{e:#}"));
    let status = manager.status().await;
    let untouched = alive(engine) && alive(starter) && listening(port);
    stop_after(&[starter, engine], || {
        let step = format!("restart the goose that started it (pid {starter})");
        for (surface, said) in [("Unmount", unmount), ("a start", start)] {
            let said = match said {
                Ok(_) => panic!("{surface} stopped the older goose's engine {engine}"),
                Err(said) => said,
            };
            eprintln!("{surface}: {said}");
            assert!(said.contains(&format!("pid {engine}")), "{said}");
            assert!(said.contains(&format!("carries no {SIDECAR_MARKER_ENV}")), "{said}");
            assert!(said.contains(&step), "{surface}'s step: {said}");
            assert!(!said.contains("kill"), "{surface}: {said}");
        }
        let status_step = status.stray_listener_step.as_ref().unwrap_or_else(|| {
            panic!("no step: {:?}", status.stray_listener_holders_error)
        });
        assert_eq!(status_step.kind, "restartGoose");
        assert_eq!(status_step.pid, Some(starter));
        assert!(status_step.text.contains(&step), "{}", status_step.text);
        let holders = status.stray_listener_holders.as_ref().unwrap();
        assert_eq!(holders[0].not_ours_rule.as_deref(), Some("noMarker"));
        assert_eq!(holders[0].live_starter_pid, Some(starter));
        assert!(untouched, "the older goose or its engine was signalled");
    });
}

/// Q-253: between reads that change nothing, the holders are served from the cache; a change in
/// what they were judged from — the starter a verdict named exits, a listener is replaced — reads
/// them again, so a cached verdict never names a process that has gone.
#[tokio::test]
async fn cached_holders_are_read_again_when_what_they_named_is_gone() {
    let port = free_port();
    let marker = sidecar_marker("fake-engine", &format!("http://127.0.0.1:{port}"));
    let cache = HoldersCache::new();
    let (starter, engine) = orphaned_pair(port, Some(&marker), "new_group");

    let full = std::time::Instant::now();
    let first = cache.read(port, &marker).await.unwrap();
    let full = full.elapsed();
    let cached = std::time::Instant::now();
    let second = cache.read(port, &marker).await.unwrap();
    let cached = cached.elapsed();
    eprintln!("a full read took {full:?}, a cached one {cached:?}");
    let reads_while_unchanged = cache.full_reads();

    // The starter the verdict names exits: the engine is re-parented to init, and now ours.
    stop(&[starter]);
    while alive(starter) {
        std::thread::sleep(goose_sidecar::GRACE_TICK);
    }
    let after_starter = cache.read(port, &marker).await.unwrap();
    let reads_after_starter = cache.full_reads();

    // The listener itself is replaced on the same port.
    stop(&[engine]);
    while listening(port) {
        std::thread::sleep(goose_sidecar::GRACE_TICK);
    }
    let mut replacement = Command::new("python3")
        .args(["-c", FAKE_ENGINE, &port.to_string()])
        .spawn()
        .unwrap();
    let replacement_pid = replacement.id();
    wait_listening(port, replacement_pid);
    let after_replace = cache.read(port, &marker).await.unwrap();
    let reads_after_replace = cache.full_reads();

    stop_after(&[starter, engine, replacement_pid], || {
        assert_eq!(first, second, "an unchanged port serves the same facts");
        assert_eq!(reads_while_unchanged, 1, "the second read was served cached");
        assert_eq!(
            first[0].verdict.as_ref().unwrap_err().live_starter.as_ref().map(|s| s.pid),
            Some(starter)
        );
        assert_eq!(reads_after_starter, 2, "the starter's exit read them again");
        assert!(
            after_starter[0].verdict.is_ok(),
            "its starter gone, the engine is this goose's leftover: {}",
            after_starter[0]
        );
        assert_eq!(reads_after_replace, 3, "a new listener read them again");
        assert_eq!(after_replace.len(), 1);
        assert_eq!(after_replace[0].pid, replacement_pid, "never the gone {engine}");
    });
    replacement.wait().unwrap();
}

/// The in-process listener read answers what lsof answers: this process's own listener, not a
/// client connection to it (the negative control), and nothing once it closes.
#[tokio::test]
async fn listener_pids_agree_with_lsof() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let mut client = Command::new("python3")
        .args([
            "-c",
            "import socket, sys, time; s = socket.create_connection(('127.0.0.1', int(sys.argv[1]))); \
             print('connected', flush=True); time.sleep(600)",
            &port.to_string(),
        ])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut line = String::new();
    BufReader::new(client.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let (_accepted, _) = listener.accept().unwrap();
    let client_pid = client.id();

    let libproc = listener_pids(port).unwrap();
    let lsof = String::from_utf8(
        Command::new(goose_sidecar::resolve_lsof().unwrap())
            .args(["-ti", &format!("TCP:{port}"), "-sTCP:LISTEN"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    let lsof: Vec<u32> = lsof.split_whitespace().map(|p| p.parse().unwrap()).collect();
    drop(listener);
    let closed = listener_pids(port).unwrap();
    stop_after(&[client_pid], || {
        assert_eq!(libproc, vec![std::process::id()]);
        assert_eq!(libproc, lsof);
        assert!(!libproc.contains(&client_pid), "a client is no listener");
        assert_eq!(closed, Vec::<u32>::new());
    });
    client.wait().unwrap();
}
