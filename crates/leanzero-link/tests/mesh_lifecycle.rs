//! Lifecycle tests against real child processes standing in for tailscaled and the
//! tailscale CLI (the goose-sidecar fake-server idiom): a fake daemon that creates the
//! unix-socket file and a fake CLI that answers canned `status --json`, proving
//! spawn -> socket-ready -> status-parse -> join -> logout -> shutdown-with-no-orphan
//! without any real tailnet or any contact with a personal Tailscale daemon.
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use leanzero_link::mesh::{BackendState, MeshConfig, MeshEngine, MeshError, DEFAULT_LOGIN_SERVER};

const RUNNING_JSON: &str = include_str!("fixtures/status_running.json");
const NEEDS_LOGIN_JSON: &str = include_str!("fixtures/status_needs_login.json");

/// Verifies the exact argv the engine promises (--tun=userspace-networking, --statedir,
/// --socket, --no-logs-no-support, --socks5-server=127.0.0.1:0) actually reaches the
/// daemon, then behaves like one: LISTENS on the unix socket after a short delay (a real
/// listener, so the engine's peer-credential proof sees THIS process as the owner), THEN
/// opens a kernel-chosen loopback TCP listener and reports it on stderr exactly as
/// tailscaled 1.98.8 does (`SOCKS5 listening on 127.0.0.1:<port>` — socket first, proxy
/// second, the real order), removes the socket on SIGTERM. It records the `HTTPS_PROXY` it
/// was given in `<statedir>/https-proxy.seen` (`<unset>` when absent), and marks
/// `<statedir>/ownership-checked` on the first connection it accepts — nothing but the
/// engine's listener-pid proof connects (the fake CLI only checks the file), and the engine
/// makes that proof only after `status` answered. Hooks in the state dir:
/// `no-proxy-report` (never report the listener), `proxy-report-wildcard` (report
/// `0.0.0.0:<port>`).
const FAKE_TAILSCALED: &str = r#"#!/usr/bin/env python3
import sys
if sys.argv[1:] == ["--warm"]: sys.exit(0)
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
if flag("--socks5-server") != "127.0.0.1:0":
    print("fake tailscaled: want --socks5-server=127.0.0.1:0: %r" % (args,), file=sys.stderr)
    sys.exit(2)
with open(os.path.join(statedir, "https-proxy.seen"), "w") as f:
    f.write(os.environ.get("HTTPS_PROXY", "<unset>"))
print("fake tailscaled starting", file=sys.stderr)
sys.stderr.flush()
time.sleep(0.3)
srv = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
srv.bind(sock)
srv.listen(8)
srv.settimeout(0.1)
time.sleep(0.2)
socks = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
socks.bind(("127.0.0.1", 0))
socks.listen(8)
if not os.path.exists(os.path.join(statedir, "no-proxy-report")):
    host = "0.0.0.0" if os.path.exists(os.path.join(statedir, "proxy-report-wildcard")) else "127.0.0.1"
    print("2026/09/23 23:03:35 SOCKS5 listening on %s:%d" % (host, socks.getsockname()[1]), file=sys.stderr)
    sys.stderr.flush()
def bye(signum, frame):
    os.unlink(sock)
    sys.exit(0)
signal.signal(signal.SIGTERM, bye)
checked = os.path.join(statedir, "ownership-checked")
while True:
    try:
        c, _ = srv.accept()
        c.close()
        if not os.path.exists(checked):
            open(checked, "w").close()
    except socket.timeout:
        pass
"#;

/// Answers like the real CLI: refuses when the socket file is absent, validates the
/// engine's `up` argv, tracks joined state via a marker file beside the socket, and
/// serves the crate's own fixtures for `status --json`.
const FAKE_TAILSCALE_CLI: &str = r#"#!/usr/bin/env python3
import sys
if sys.argv[1:] == ["--warm"]: sys.exit(0)
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
    # R-L1: the key must arrive as `file:<path>` (the CLI's documented form), the file
    # must be private (0600) and live under the state dir; the CLI reads it like the
    # real one does.
    key = flag("--auth-key")
    if not key.startswith("file:"):
        print("fake tailscale: auth key on argv (%r) — must be file:" % key, file=sys.stderr)
        sys.exit(3)
    key_path = key[len("file:"):]
    if os.path.dirname(key_path) != base:
        print("fake tailscale: key file %r not under the state dir %r" % (key_path, base), file=sys.stderr)
        sys.exit(3)
    mode = os.stat(key_path).st_mode & 0o777
    if mode != 0o600:
        print("fake tailscale: key file mode %o, want 600" % mode, file=sys.stderr)
        sys.exit(3)
    with open(key_path) as f:
        authkey = f.read().strip()
    if authkey != "tskey-auth-good":
        print("backend error: invalid key: unable to validate API key", file=sys.stderr)
        sys.exit(1)
    # Test hook: with `<base>/slow-up` present, record this CLI's pid and park — lets
    # the join future be dropped while `up` is still running. Written aside and renamed:
    # the test reads the pid the moment the file exists.
    if os.path.exists(os.path.join(base, "slow-up")):
        with open(os.path.join(base, "up-pid.tmp"), "w") as f:
            f.write(str(os.getpid()))
        os.rename(os.path.join(base, "up-pid.tmp"), os.path.join(base, "up-pid"))
        import time
        time.sleep(600)
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
    # Test hook: with `<base>/await-spawned` present, answer only once the daemon has
    # written `<base>/spawned` — the engine cannot act on this probe's answer (and
    # terminate its spawn) before the spawn has recorded its pid.
    if os.path.exists(os.path.join(base, "await-spawned")):
        import time
        while not os.path.exists(os.path.join(base, "spawned")):
            time.sleep(0.01)
    # Test hook: with `<base>/slow-after-first-status` present, the first status answers
    # at once and every later one outlasts the engine's readiness probe — a CLI starved
    # on a loaded machine after the daemon was already proven.
    if os.path.exists(os.path.join(base, "slow-after-first-status")):
        answered = os.path.join(base, "status-answered")
        if os.path.exists(answered):
            import time
            time.sleep(60)
        open(answered, "w").close()
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
import sys
if sys.argv[1:] == ["--warm"]: sys.exit(0)
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
/// python interpreter could even write the marker (measured: 6/6 misses). Where a test
/// needs the marker before the engine acts, the fake CLI's `await-spawned` hook holds
/// the probe until it exists — so the marker appears whole (written aside, renamed), or a
/// SIGTERM landing between the file's creation and its write would leave it empty.
const FAKE_TAILSCALED_MARKING: &str = r#"#!/bin/sh
[ "$1" = --warm ] && exit 0
for a in "$@"; do case "$a" in --statedir=*) statedir="${a#--statedir=}";; esac; done
echo $$ > "$statedir/spawned.tmp" && mv "$statedir/spawned.tmp" "$statedir/spawned"
exec sleep 1000
"#;

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

/// One executable file per fake script CONTENT, shared by every test and run once
/// (`--warm`, which each fake answers by exiting 0) before any test's clock starts.
///
/// Measured on macOS 26 (2026-09-26, load ~20): the first exec of a newly written
/// script is held by the system's exec-time assessment, ~140-220 ms against 4 ms for
/// the second exec, and those holds SERIALIZE machine-wide — 45 fresh scripts exec'd in
/// parallel finished in 7.0 s, the same 45 again in 46 ms. Per-test copies put every
/// test's daemon and CLI behind that queue, and behind every other build on the machine
/// emitting new binaries: 13 of 20 suite runs failed (15 s `StartupTimeout`s, "socket
/// not present yet", "warm exec never ran") while other builds ran. The files live in
/// the target's tmp dir under a content hash, so an edited script is a new file and an
/// unchanged one stays assessed across runs.
fn fake_exec(name: &str, content: &str) -> PathBuf {
    use std::collections::HashSet;
    use std::hash::{Hash, Hasher};
    use std::os::unix::fs::PermissionsExt;
    use std::sync::Mutex;

    static WARMED: Mutex<Option<HashSet<PathBuf>>> = Mutex::new(None);

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    content.hash(&mut hasher);
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("leanzero-link-fakes");
    let path = dir.join(format!("{name}-{:016x}", hasher.finish()));

    let mut warmed = WARMED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if warmed.get_or_insert_with(HashSet::new).contains(&path) {
        return path;
    }
    if !path.exists() {
        std::fs::create_dir_all(&dir).unwrap();
        // Another test process may be writing the same file: write aside, then rename.
        let staging = dir.join(format!(".{name}.{}", std::process::id()));
        std::fs::write(&staging, content).unwrap();
        std::fs::set_permissions(&staging, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::rename(&staging, &path).unwrap();
    }
    let status = loop {
        match std::process::Command::new(&path).arg("--warm").status() {
            // Linux refuses to exec a file any process holds open for writing, and a sibling
            // test that forked while the staging file was open holds it until its own exec.
            // Holders only ever go away, so once this exec succeeds the engine's cannot fail so.
            Err(err) if err.raw_os_error() == Some(libc::ETXTBSY) => {
                std::thread::sleep(Duration::from_millis(10))
            }
            other => {
                break other.unwrap_or_else(|err| {
                    panic!("warming {} failed to exec: {err}", path.display())
                })
            }
        }
    };
    assert!(
        status.success(),
        "{} --warm exited {status}",
        path.display()
    );
    warmed.as_mut().unwrap().insert(path.clone());
    path
}

fn fake_config(root: &Path) -> MeshConfig {
    let state_dir = root.join("state");
    std::fs::create_dir_all(&state_dir).unwrap();
    std::fs::write(state_dir.join("needs_login.json"), NEEDS_LOGIN_JSON).unwrap();
    std::fs::write(state_dir.join("running.json"), RUNNING_JSON).unwrap();
    MeshConfig {
        tailscaled_path: fake_exec("fake-tailscaled", FAKE_TAILSCALED),
        tailscale_cli_path: fake_exec("fake-tailscale", FAKE_TAILSCALE_CLI),
        socket_path: state_dir.join("tailscaled.sock"),
        state_dir,
        hostname: "lz-node-self".to_string(),
        login_server: DEFAULT_LOGIN_SERVER.to_string(),
        tag: None,
        startup_timeout: Duration::from_secs(15),
        join_timeout: Duration::from_secs(10),
        cli_timeout: Duration::from_secs(5),
        control_route: Default::default(),
    }
}

fn process_alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

/// Names of any `auth-key*` files left in the state dir — must be empty after `join`.
fn leftover_key_files(state_dir: &Path) -> Vec<String> {
    std::fs::read_dir(state_dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("auth-key"))
        .collect()
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

/// Q-137: a `*.ts.net` login server (Headscale behind the node's Funnel) gets the
/// control-plane proxy as the daemon's `HTTPS_PROXY`, alive for the engine's life; any
/// other login server gets no proxy, and the road says why. Nothing here dials the
/// control host, so MagicDNS is never asked.
#[tokio::test]
async fn a_tailnet_login_server_hands_the_daemon_the_control_proxy() {
    let root = tempfile::tempdir().unwrap();
    let mut config = fake_config(root.path());
    config.login_server = "https://studio.tailtest.ts.net".to_string();
    let seen = config.state_dir.join("https-proxy.seen");
    let engine = MeshEngine::start(config).await.unwrap();
    let proxy = std::fs::read_to_string(&seen).unwrap();
    let addr = proxy
        .strip_prefix("http://127.0.0.1:")
        .unwrap_or_else(|| panic!("HTTPS_PROXY must be our loopback proxy, got {proxy:?}"));
    let port: u16 = addr.parse().unwrap();
    tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .expect("the proxy listens while the engine lives");
    engine.shutdown().await;

    let root = tempfile::tempdir().unwrap();
    let config = fake_config(root.path());
    let route = config.control_route.clone();
    let seen = config.state_dir.join("https-proxy.seen");
    let engine = MeshEngine::start(config).await.unwrap();
    assert_eq!(std::fs::read_to_string(&seen).unwrap(), "<unset>");
    let report = route
        .get()
        .expect("the road is recorded even without a proxy");
    assert_eq!(report.host, "controlplane.tailscale.com");
    let leanzero_link::tailnet_route::RoutePath::Public { reason } = report.path else {
        panic!("a non-tailnet login server has only the public road");
    };
    assert!(reason.contains("not a Tailscale"), "{reason}");
    engine.shutdown().await;
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
    assert!(
        leftover_key_files(&engine.config().state_dir).is_empty(),
        "the key file is removed even when `up` fails: {:?}",
        leftover_key_files(&engine.config().state_dir)
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
    // The fake CLI exited 0 only if the key came as `file:`, 0600, under the state dir;
    // and nothing of it survives `up`.
    assert!(
        leftover_key_files(&engine.config().state_dir).is_empty(),
        "the key file is removed after a successful `up`: {:?}",
        leftover_key_files(&engine.config().state_dir)
    );

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

/// Poll until `probe` holds or a deadline passes — the deadline is a test guard, not a
/// property under test.
async fn wait_for(what: &str, mut probe: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !probe() {
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for {what}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// A pid that certainly belonged to a process which has exited and been reaped.
fn reaped_pid() -> u32 {
    let mut child = std::process::Command::new("true")
        .spawn()
        .expect("spawn `true`");
    let pid = child.id();
    child.wait().expect("reap `true`");
    pid
}

/// R-L1 residual: the join future is dropped while `tailscale up` is still running
/// (goosed exiting mid-join, a caller's abort). The key file must not outlive the join:
/// the drop guard removes it, the CLI child we spawned is killed per-pid, and the
/// daemon — which the dropped join never owned — is untouched.
#[tokio::test]
async fn join_dropped_mid_up_removes_the_key_file_and_kills_only_the_cli() {
    let root = tempfile::tempdir().unwrap();
    let engine = Arc::new(MeshEngine::start(fake_config(root.path())).await.unwrap());
    let state_dir = engine.config().state_dir.clone();
    let daemon_pid = engine.pid().await.unwrap();
    std::fs::write(state_dir.join("slow-up"), "1").unwrap();

    let joining = {
        let engine = Arc::clone(&engine);
        tokio::spawn(async move { engine.join("tskey-auth-good", "lz-node-self").await })
    };
    let up_pid_file = state_dir.join("up-pid");
    wait_for("the fake `up` to start and park", || up_pid_file.exists()).await;
    let cli_pid: u32 = std::fs::read_to_string(&up_pid_file)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert_eq!(
        leftover_key_files(&state_dir),
        vec![format!("auth-key.{}", std::process::id())],
        "while `up` runs the key file is on disk (0600, read by the CLI)"
    );

    joining.abort();
    let err = joining.await.expect_err("the aborted join never completes");
    assert!(err.is_cancelled());

    assert!(
        leftover_key_files(&state_dir).is_empty(),
        "the drop guard removed the key the moment the join future was dropped: {:?}",
        leftover_key_files(&state_dir)
    );
    wait_for(
        "the parked `tailscale up` (our own child) to be killed",
        || !process_alive(cli_pid),
    )
    .await;
    assert!(
        process_alive(daemon_pid),
        "a dropped join never touches the daemon"
    );
    assert!(
        !state_dir.join("joined").exists(),
        "`up` never completed, so nothing joined"
    );

    engine.shutdown().await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!process_alive(daemon_pid));
}

/// R-L1 residual, the other half: a key file left by a goosed that died WITHOUT
/// unwinding (no drop guard ran) is swept by the next `start`; a key whose writer is
/// still alive is left alone — it may be a join in flight.
#[tokio::test]
async fn start_sweeps_a_dead_writers_stale_key_and_keeps_a_live_writers() {
    let root = tempfile::tempdir().unwrap();
    let config = fake_config(root.path());
    let stale = config.state_dir.join(format!("auth-key.{}", reaped_pid()));
    let live = config
        .state_dir
        .join(format!("auth-key.{}", std::process::id()));
    std::fs::write(&stale, "tskey-auth-stale").unwrap();
    std::fs::write(&live, "tskey-auth-inflight").unwrap();

    let engine = MeshEngine::start(config).await.unwrap();
    assert!(!stale.exists(), "the dead writer's key was swept at start");
    assert!(
        live.exists(),
        "a live writer's key is never deleted at start"
    );
    engine.shutdown().await;
    std::fs::remove_file(&live).unwrap();
}

#[tokio::test]
async fn daemon_exit_during_startup_carries_stderr() {
    let root = tempfile::tempdir().unwrap();
    let mut config = fake_config(root.path());
    config.tailscaled_path = fake_exec(
        "fake-tailscaled-dying",
        "#!/usr/bin/env python3\nimport sys\nif sys.argv[1:] == [\"--warm\"]: sys.exit(0)\nprint('unable to bind socket: operation not permitted', file=sys.stderr)\nsys.exit(3)\n",
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
    config.tailscaled_path = fake_exec("fake-tailscaled-marking", FAKE_TAILSCALED_MARKING);

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
    std::fs::write(config.state_dir.join("await-spawned"), "1").unwrap();
    config.tailscaled_path = fake_exec("fake-tailscaled-marking", FAKE_TAILSCALED_MARKING);

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
    config.tailscaled_path = fake_exec(
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

/// FH#11: a daemon that never binds its socket times out with the LAST probe's verdict
/// in the error ("socket … not present yet"), its stderr tail, and no orphan.
#[tokio::test]
async fn startup_timeout_names_the_last_probe_and_leaves_no_orphan() {
    let root = tempfile::tempdir().unwrap();
    let mut config = fake_config(root.path());
    // The marking daemon records its pid and idles without ever binding the socket.
    config.tailscaled_path = fake_exec("fake-tailscaled-marking", FAKE_TAILSCALED_MARKING);
    // The bound is this test's own; it only has to outlast a warmed `sh` recording its pid.
    let bound = Duration::from_secs(3);
    config.startup_timeout = bound;

    let err = match MeshEngine::start(config.clone()).await {
        Ok(_) => panic!("start reported ready with no socket"),
        Err(e) => e,
    };
    match &err {
        MeshError::StartupTimeout {
            waited, last_probe, ..
        } => {
            assert_eq!(*waited, bound);
            assert!(
                last_probe.contains("not present yet")
                    && last_probe.contains(&config.socket_path.display().to_string()),
                "the timeout names what the last probe saw: {last_probe}"
            );
        }
        other => panic!("expected StartupTimeout, got {other}"),
    }
    assert!(
        err.to_string().contains("last probe: socket"),
        "the Display carries it too: {err}"
    );
    let spawned = config.state_dir.join("spawned");
    wait_for("the spawn's pid marker", || spawned.exists()).await;
    let our_pid: u32 = std::fs::read_to_string(&spawned)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        !process_alive(our_pid),
        "the never-ready daemon (pid {our_pid}) was terminated per-pid on timeout"
    );
}

/// Readiness includes the peer proxy: `start` returns only once the daemon has reported
/// its SOCKS5 listener, and `peer_proxy()` is exactly the address the daemon reported (a
/// real loopback listener the fake holds). After shutdown there is no proxy — a loud
/// error, never the stale address.
#[tokio::test]
async fn start_records_the_daemons_socks5_listener_and_forgets_it_on_shutdown() {
    let root = tempfile::tempdir().unwrap();
    let engine = MeshEngine::start(fake_config(root.path())).await.unwrap();

    let proxy = engine
        .peer_proxy()
        .await
        .expect("ready means the proxy is known");
    assert!(proxy.addr().ip().is_loopback(), "{proxy:?}");
    assert_ne!(
        proxy.addr().port(),
        0,
        "the kernel-chosen port, not the request"
    );
    std::net::TcpStream::connect(proxy.addr())
        .expect("the reported address is the daemon's live listener");

    engine.shutdown().await;
    let err = engine.peer_proxy().await.unwrap_err();
    assert!(matches!(err, MeshError::NoPeerProxy { .. }), "{err}");
}

/// Start against a fake daemon that never reports its SOCKS5 listener (plus `hooks` in its
/// state dir) until an attempt times out AFTER the engine proved the socket ours, and return
/// that attempt's `last_probe`. `Ok` fails on every attempt.
///
/// The startup bound is the test's own and decides only whether the proof happened before
/// it ran out — on a starved machine it may not have, and that attempt proves nothing either
/// way — so the bound doubles until the daemon's `ownership-checked` record says the proof
/// happened. The record is written on accept: it can only be missing after a proof (the
/// retry, the safe side), never present without one.
async fn timeout_after_ownership_proof(hooks: &[&str]) -> String {
    let mut unproven = Vec::new();
    for bound in [3, 6, 12, 24].map(Duration::from_secs) {
        let root = tempfile::tempdir().unwrap();
        let mut config = fake_config(root.path());
        for hook in ["no-proxy-report"].iter().chain(hooks) {
            std::fs::write(config.state_dir.join(hook), "1").unwrap();
        }
        config.startup_timeout = bound;
        let checked = config.state_dir.join("ownership-checked");

        let err = match MeshEngine::start(config).await {
            Ok(_) => panic!("start reported ready with no peer proxy"),
            Err(e) => e,
        };
        let MeshError::StartupTimeout { last_probe, .. } = err else {
            panic!("expected StartupTimeout, got {err}");
        };
        if checked.exists() {
            return last_probe;
        }
        unproven.push(format!("{bound:?}: {last_probe}"));
    }
    panic!("the engine never proved the socket ours within any bound: {unproven:#?}");
}

/// A daemon whose socket answers but which never reports a SOCKS5 listener is NOT
/// ready: the timeout names the missing report, and our spawn is terminated per-pid.
#[tokio::test]
async fn a_daemon_that_never_reports_its_proxy_times_out_naming_it() {
    let last_probe = timeout_after_ownership_proof(&[]).await;
    assert!(
        last_probe.contains("SOCKS5 listener") && last_probe.contains("is ours"),
        "the timeout names the missing proxy report: {last_probe}"
    );
}

/// Once the socket answered and its listener is proven ours, only the SOCKS5 report is
/// outstanding: a CLI that turns slow afterwards (a loaded machine) is not re-run each poll,
/// so it cannot replace the timeout's verdict with a transient `status` failure.
#[tokio::test]
async fn a_slow_cli_after_the_proof_cannot_hide_the_missing_proxy_report() {
    let last_probe = timeout_after_ownership_proof(&["slow-after-first-status"]).await;
    assert!(
        last_probe.contains("SOCKS5 listener") && last_probe.contains("is ours"),
        "the verdict is the missing report, not the slow CLI: {last_probe}"
    );
}

/// A reported listener that is not loopback is refused outright — peer traffic never
/// goes through a proxy other processes on the network could reach — and our daemon is
/// stopped per-pid.
#[tokio::test]
async fn a_non_loopback_proxy_report_is_refused_and_the_daemon_stopped() {
    let root = tempfile::tempdir().unwrap();
    let config = fake_config(root.path());
    std::fs::write(config.state_dir.join("proxy-report-wildcard"), "1").unwrap();
    let socket = config.socket_path.clone();

    let err = match MeshEngine::start(config).await {
        Ok(_) => panic!("start accepted a wildcard proxy listener"),
        Err(e) => e,
    };
    assert!(matches!(err, MeshError::NoPeerProxy { .. }), "{err}");
    assert!(err.to_string().contains("not loopback"), "{err}");
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        !socket.exists(),
        "SIGTERM reached our daemon (it removes its socket on exit)"
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
