//! Lifecycle tests against real child processes standing in for tailscaled and the
//! tailscale CLI (the goose-sidecar fake-server idiom): a fake daemon that creates the
//! unix-socket file and a fake CLI that answers canned `status --json`, proving
//! spawn -> socket-ready -> status-parse -> join -> logout -> shutdown-with-no-orphan
//! without any real tailnet or any contact with a personal Tailscale daemon.
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::time::Duration;

use leanzero_link::mesh::{BackendState, MeshConfig, MeshEngine, MeshError, DEFAULT_LOGIN_SERVER};

const RUNNING_JSON: &str = include_str!("fixtures/status_running.json");
const NEEDS_LOGIN_JSON: &str = include_str!("fixtures/status_needs_login.json");

/// Verifies the exact argv the engine promises (--tun=userspace-networking, --statedir,
/// --socket, --no-logs-no-support) actually reaches the daemon, then behaves like one:
/// LISTENS on the unix socket after a short delay (a real listener, so the engine's
/// peer-credential proof sees THIS process as the owner), removes it on SIGTERM.
const FAKE_TAILSCALED: &str = r#"#!/usr/bin/env python3
import os, signal, socket, sys, time
args = sys.argv[1:]
def flag(name):
    for a in args:
        if a.startswith(name + "="):
            return a.split("=", 1)[1]
    return None
tun = flag("--tun"); statedir = flag("--statedir"); sock = flag("--socket")
if tun != "userspace-networking" or not statedir or not sock:
    print("fake tailscaled: bad argv: %r" % (args,), file=sys.stderr)
    sys.exit(2)
if "--no-logs-no-support" not in args:
    print("fake tailscaled: missing --no-logs-no-support: %r" % (args,), file=sys.stderr)
    sys.exit(2)
print("fake tailscaled starting", file=sys.stderr)
sys.stderr.flush()
time.sleep(0.3)
srv = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
srv.bind(sock)
srv.listen(8)
srv.settimeout(0.1)
def bye(signum, frame):
    os.unlink(sock)
    sys.exit(0)
signal.signal(signal.SIGTERM, bye)
while True:
    try:
        c, _ = srv.accept()
        c.close()
    except socket.timeout:
        pass
"#;

/// Answers like the real CLI: refuses when the socket file is absent, validates the
/// engine's `up` argv, tracks joined state via a marker file beside the socket, and
/// serves the crate's own fixtures for `status --json`.
const FAKE_TAILSCALE_CLI: &str = r#"#!/usr/bin/env python3
import os, sys
args = sys.argv[1:]
def flag(name):
    for a in args:
        if a.startswith(name + "="):
            return a.split("=", 1)[1]
    return None
sock = flag("--socket")
if not sock:
    print("fake tailscale: --socket missing: %r" % (args,), file=sys.stderr)
    sys.exit(2)
base = os.path.dirname(sock)
marker = os.path.join(base, "joined")
if not os.path.exists(sock):
    print("failed to connect to local tailscaled; is it running?", file=sys.stderr)
    sys.exit(1)
if "up" in args:
    if "--accept-routes=false" not in args or "--reset" not in args:
        print("fake tailscale: missing pinned settings in %r" % (args,), file=sys.stderr)
        sys.exit(3)
    for name in ("--auth-key", "--hostname", "--login-server", "--timeout"):
        if flag(name) is None:
            print("fake tailscale: missing %s in %r" % (name, args), file=sys.stderr)
            sys.exit(3)
    if flag("--auth-key") != "tskey-auth-good":
        print("backend error: invalid key: unable to validate API key", file=sys.stderr)
        sys.exit(1)
    with open(marker, "w") as f:
        f.write("joined")
    sys.exit(0)
if "logout" in args:
    if os.path.exists(marker):
        os.unlink(marker)
    sys.exit(0)
if "status" in args and "--json" in args:
    # Test hook: `<base>/probe-fail-once` makes exactly one probe fail the way a loaded
    # machine's CLI does (transport error), then vanishes.
    fail_once = os.path.join(base, "probe-fail-once")
    if os.path.exists(fail_once):
        os.unlink(fail_once)
        print("dial unix %s: connect: connection refused" % sock, file=sys.stderr)
        sys.exit(1)
    # Test hook: with `<base>/slow-probe` present, record that a probe happened and
    # take a while to answer — lets a daemon die DURING the readiness probe.
    if os.path.exists(os.path.join(base, "slow-probe")):
        with open(os.path.join(base, "probed"), "w") as f:
            f.write("1")
        import time
        time.sleep(0.3)
    name = "running.json" if os.path.exists(marker) else "needs_login.json"
    with open(os.path.join(base, name)) as f:
        sys.stdout.write(f.read())
    sys.exit(0)
print("fake tailscale: unknown args %r" % (args,), file=sys.stderr)
sys.exit(2)
"#;

/// A daemon that creates its socket and then exits the moment the CLI records a probe
/// (`<statedir>/probed`) — i.e. it dies while the readiness probe is being answered by
/// the socket it left behind. Stands in for "another process's daemon owns the socket
/// and ours lost the race".
const FAKE_TAILSCALED_DIES_DURING_PROBE: &str = r#"#!/usr/bin/env python3
import os, socket, sys, time
args = sys.argv[1:]
def flag(name):
    for a in args:
        if a.startswith(name + "="):
            return a.split("=", 1)[1]
    return None
statedir = flag("--statedir"); sock = flag("--socket")
srv = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
srv.bind(sock)
srv.listen(8)
probed = os.path.join(statedir, "probed")
while not os.path.exists(probed):
    time.sleep(0.01)
print("fake tailscaled: lost the socket race, exiting", file=sys.stderr)
sys.exit(0)
"#;

/// A daemon that never owns the socket: it records its pid in `<statedir>/spawned` and
/// idles — so a test can assert it was NEVER run, or that the engine terminated it
/// per-pid after proving the socket belongs to someone else. A shell script, not
/// python: the engine kills a useless spawn within a few ms of the probe, before a
/// python interpreter could even write the marker (measured: 6/6 misses).
const FAKE_TAILSCALED_MARKING: &str = r#"#!/bin/sh
for a in "$@"; do case "$a" in --statedir=*) statedir="${a#--statedir=}";; esac; done
echo $$ > "$statedir/spawned"
exec sleep 1000
"#;

/// Execute a freshly written script once, in a throwaway dir, before the engine does.
/// Measured on macOS 26: the FIRST exec of a newly created script is held ~100-300 ms
/// by the system's exec-time assessment (the process sits in state S having run no
/// line) — long enough for the engine to refuse and terminate it before it writes its
/// marker. Warming makes the marker deterministic; it is a test-environment artifact,
/// not engine behavior.
async fn warm_exec(script: &Path) {
    let warm = tempfile::tempdir().unwrap();
    let mut child = tokio::process::Command::new(script)
        .arg(format!("--statedir={}", warm.path().display()))
        .kill_on_drop(true)
        .spawn()
        .expect("warm exec spawns");
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !warm.path().join("spawned").exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "warm exec never ran {}",
            script.display()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    child.kill().await.unwrap();
}

/// A process that is NOT the engine's child, listening on the engine's socket path —
/// another goosed's daemon. Killed on drop.
async fn spawn_foreign_listener(sock: &Path) -> tokio::process::Child {
    let script = format!(
        "import socket\ns = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)\ns.bind({sock:?})\ns.listen(8)\ns.settimeout(0.1)\nwhile True:\n    try:\n        c, _ = s.accept()\n        c.close()\n    except socket.timeout:\n        pass\n",
        sock = sock.display().to_string()
    );
    let child = tokio::process::Command::new("python3")
        .arg("-c")
        .arg(script)
        .kill_on_drop(true)
        .spawn()
        .expect("foreign listener spawns");
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !sock.exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "foreign listener never bound {}",
            sock.display()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    child
}

fn write_exec(dir: &Path, name: &str, content: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, content).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

fn fake_config(root: &Path) -> MeshConfig {
    let state_dir = root.join("state");
    std::fs::create_dir_all(&state_dir).unwrap();
    std::fs::write(state_dir.join("needs_login.json"), NEEDS_LOGIN_JSON).unwrap();
    std::fs::write(state_dir.join("running.json"), RUNNING_JSON).unwrap();
    MeshConfig {
        tailscaled_path: write_exec(root, "fake-tailscaled", FAKE_TAILSCALED),
        tailscale_cli_path: write_exec(root, "fake-tailscale", FAKE_TAILSCALE_CLI),
        socket_path: state_dir.join("tailscaled.sock"),
        state_dir,
        hostname: "lz-node-self".to_string(),
        login_server: DEFAULT_LOGIN_SERVER.to_string(),
        tag: None,
        startup_timeout: Duration::from_secs(15),
        join_timeout: Duration::from_secs(10),
        cli_timeout: Duration::from_secs(5),
    }
}

fn process_alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

#[tokio::test]
async fn starts_reports_needs_login_and_shuts_down_without_orphans() {
    let root = tempfile::tempdir().unwrap();
    let config = fake_config(root.path());
    let socket = config.socket_path.clone();
    let state_dir = config.state_dir.clone();

    let engine = MeshEngine::start(config).await.unwrap();
    assert!(socket.exists(), "daemon must have created the socket");
    let pid = engine.pid().await.unwrap();
    assert!(process_alive(pid));

    let status = engine.status().await.unwrap();
    assert_eq!(status.backend_state, BackendState::NeedsLogin);
    assert!(!status.online);
    assert!(status.peers.is_empty());
    assert_eq!(status.self_hostname.as_deref(), Some("lz-node-self"));

    engine.shutdown().await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!process_alive(pid), "daemon pid {pid} survived shutdown");
    assert!(
        !socket.exists(),
        "SIGTERM never reached the daemon (socket not cleaned up)"
    );
    assert!(state_dir.exists(), "state dir must be left intact");

    let stopped = engine.status().await.unwrap();
    assert_eq!(stopped.backend_state, BackendState::Stopped);
    assert!(stopped.peers.is_empty());
}

#[tokio::test]
async fn join_with_bad_or_empty_key_is_a_loud_typed_error() {
    let root = tempfile::tempdir().unwrap();
    let engine = MeshEngine::start(fake_config(root.path())).await.unwrap();

    let err = engine.join("", "lz-node-self").await.unwrap_err();
    assert!(matches!(err, MeshError::EmptyAuthKey), "{err}");

    let err = engine
        .join("tskey-auth-WRONG", "lz-node-self")
        .await
        .unwrap_err();
    assert!(matches!(err, MeshError::JoinFailed { .. }), "{err}");
    assert!(
        err.to_string().contains("invalid key"),
        "join error must carry tailscale's stderr: {err}"
    );

    engine.shutdown().await;
}

#[tokio::test]
async fn join_succeeds_then_logout_stops_the_daemon() {
    let root = tempfile::tempdir().unwrap();
    let engine = MeshEngine::start(fake_config(root.path())).await.unwrap();
    let pid = engine.pid().await.unwrap();

    engine
        .join("tskey-auth-good", "lz-node-self")
        .await
        .unwrap();

    let status = engine.status().await.unwrap();
    assert_eq!(status.backend_state, BackendState::Running);
    assert!(status.online);
    assert_eq!(status.self_ip.as_deref(), Some("100.64.0.1"));
    assert_eq!(status.peers.len(), 2);
    assert_eq!(status.peers[0].hostname, "lz-worker-a");

    engine.logout().await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!process_alive(pid), "logout must stop the daemon");
    assert_eq!(
        engine.status().await.unwrap().backend_state,
        BackendState::Stopped
    );
    assert!(
        engine.config().state_dir.exists(),
        "logout keeps the state dir for fast re-login"
    );
}

#[tokio::test]
async fn daemon_exit_during_startup_carries_stderr() {
    let root = tempfile::tempdir().unwrap();
    let mut config = fake_config(root.path());
    config.tailscaled_path = write_exec(
        root.path(),
        "fake-tailscaled-dying",
        "#!/usr/bin/env python3\nimport sys\nprint('unable to bind socket: operation not permitted', file=sys.stderr)\nsys.exit(3)\n",
    );

    let err = match MeshEngine::start(config).await {
        Ok(_) => panic!("start unexpectedly succeeded"),
        Err(e) => e,
    };
    assert!(matches!(err, MeshError::DaemonExited { .. }), "{err}");
    assert!(
        err.to_string().contains("unable to bind socket"),
        "stderr tail missing: {err}"
    );
}

/// R-L5 (corrected): a daemon already answering on the socket — another goosed's, or a
/// stale one — is refused BEFORE anything is spawned. Never adopted, never driven.
#[tokio::test]
async fn start_refuses_a_socket_another_daemon_already_answers() {
    let root = tempfile::tempdir().unwrap();
    let mut config = fake_config(root.path());
    let mut foreign = spawn_foreign_listener(&config.socket_path).await;
    config.tailscaled_path = write_exec(
        root.path(),
        "fake-tailscaled-marking",
        FAKE_TAILSCALED_MARKING,
    );

    let result = MeshEngine::start(config.clone()).await;
    assert!(
        !config.state_dir.join("spawned").exists(),
        "the daemon binary must not have been spawned at all"
    );
    let err = match result {
        Ok(_) => panic!("start adopted a socket it did not create"),
        Err(e) => e,
    };
    match &err {
        MeshError::AlreadyRunning {
            socket,
            listener_pid,
        } => {
            assert_eq!(*socket, config.socket_path);
            assert_eq!(
                *listener_pid,
                foreign.id(),
                "the refusal names the foreign listener's pid"
            );
        }
        other => panic!("expected AlreadyRunning, got {other}"),
    }
    assert!(
        err.to_string()
            .contains("never adopts a daemon it did not spawn"),
        "{err}"
    );
    foreign.kill().await.unwrap();
}

/// R-L5 (corrected), the structural half: even when the pre-spawn probe is fooled (one
/// transient CLI failure, the loaded-machine shape measured in this suite), readiness
/// is PROVEN by the socket's listener pid — a foreign listener is refused, and the
/// engine's own useless spawn is terminated per-pid.
#[tokio::test]
async fn start_refuses_a_foreign_listener_even_when_the_pre_spawn_probe_fails_once() {
    let root = tempfile::tempdir().unwrap();
    let mut config = fake_config(root.path());
    let mut foreign = spawn_foreign_listener(&config.socket_path).await;
    std::fs::write(config.state_dir.join("probe-fail-once"), "1").unwrap();
    config.tailscaled_path = write_exec(
        root.path(),
        "fake-tailscaled-marking",
        FAKE_TAILSCALED_MARKING,
    );
    warm_exec(&config.tailscaled_path).await;

    let err = match MeshEngine::start(config.clone()).await {
        Ok(_) => panic!("start adopted a foreign listener after a transient probe failure"),
        Err(e) => e,
    };
    match &err {
        MeshError::AlreadyRunning { listener_pid, .. } => {
            assert_eq!(*listener_pid, foreign.id(), "named the foreign listener");
        }
        other => panic!("expected AlreadyRunning, got {other}"),
    }
    let spawned = std::fs::read_to_string(config.state_dir.join("spawned"))
        .expect("the probe failure DID let the engine spawn its own daemon");
    let our_pid: u32 = spawned.trim().parse().unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        !process_alive(our_pid),
        "the engine's own spawn (pid {our_pid}) must be terminated per-pid, not leaked"
    );
    assert!(
        process_alive(foreign.id().unwrap()),
        "the foreign daemon is never touched"
    );
    foreign.kill().await.unwrap();
}

/// R-L5 (corrected), the other half: the readiness probe is answered by the socket,
/// but OUR child died meanwhile. `start` must not return an engine holding a corpse.
#[tokio::test]
async fn start_fails_loudly_when_the_daemon_dies_during_the_readiness_probe() {
    let root = tempfile::tempdir().unwrap();
    let mut config = fake_config(root.path());
    std::fs::write(config.state_dir.join("slow-probe"), "1").unwrap();
    config.tailscaled_path = write_exec(
        root.path(),
        "fake-tailscaled-dies-during-probe",
        FAKE_TAILSCALED_DIES_DURING_PROBE,
    );

    let err = match MeshEngine::start(config).await {
        Ok(_) => panic!("start returned an engine over a daemon that exited during the probe"),
        Err(e) => e,
    };
    assert!(matches!(err, MeshError::DaemonExited { .. }), "{err}");
    assert!(
        err.to_string().contains("lost the socket race"),
        "stderr tail carried: {err}"
    );
}

#[tokio::test]
async fn refuses_system_tailscale_paths_before_spawning_anything() {
    let root = tempfile::tempdir().unwrap();
    let mut config = fake_config(root.path());
    config.socket_path = PathBuf::from("/var/run/tailscaled.socket");
    let err = match MeshEngine::start(config).await {
        Ok(_) => panic!("start accepted the system socket path"),
        Err(e) => e,
    };
    assert!(matches!(err, MeshError::UnsafeConfig { .. }), "{err}");

    let mut config = fake_config(root.path());
    config.state_dir = PathBuf::from("/var/lib/tailscale");
    assert!(matches!(
        MeshEngine::start(config).await,
        Err(MeshError::UnsafeConfig { .. })
    ));
}
