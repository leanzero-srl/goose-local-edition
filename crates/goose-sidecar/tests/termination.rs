//! The SIGKILL leg's proof, measured on real processes: a wrapper that launches the engine
//! (the `uvx` → `rapid-mlx` shape) is the leader of its own group; killpg on THAT group
//! takes the engine; an orphan whose leader died fails the proof and is left alone; the
//! caller's own group is never a target.
#![cfg(unix)]

use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::time::Duration;

use goose_sidecar::{owns_process_group, sigkill_owned_group, Sidecar, SidecarConfig};

/// A wrapper (own group leader) that spawns `sleep` as a grandchild and prints its pid.
const WRAPPER_WITH_GRANDCHILD: &str = r#"
import subprocess, sys
child = subprocess.Popen(["sleep", "300"])
print(child.pid, flush=True)
child.wait()
"#;

fn alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

fn my_pgid() -> u32 {
    unsafe { libc::getpgrp() as u32 }
}

fn spawn_wrapper_in_own_group() -> (std::process::Child, u32) {
    let mut wrapper = Command::new("python3")
        .args(["-c", WRAPPER_WITH_GRANDCHILD])
        .stdout(Stdio::piped())
        .process_group(0)
        .spawn()
        .unwrap();
    let mut line = String::new();
    BufReader::new(wrapper.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let grandchild: u32 = line.trim().parse().unwrap();
    (wrapper, grandchild)
}

#[test]
fn proof_holds_for_a_live_leader_and_killpg_takes_the_grandchild() {
    let pgid_before = my_pgid();
    let (mut wrapper, grandchild) = spawn_wrapper_in_own_group();
    let leader = wrapper.id();
    assert!(alive(leader) && alive(grandchild));
    assert!(
        owns_process_group(leader),
        "spawned leader must own its group"
    );
    assert_ne!(leader, pgid_before, "the wrapper's group is not the test's");

    assert!(
        sigkill_owned_group(leader),
        "proof held, group must be signalled"
    );
    let _ = wrapper.wait();
    std::thread::sleep(Duration::from_millis(200));
    assert!(
        !alive(grandchild),
        "grandchild {grandchild} survived the group kill"
    );
    assert_eq!(my_pgid(), pgid_before, "the caller's own group was touched");
}

#[test]
fn proof_declines_an_orphan_whose_leader_died() {
    let (mut wrapper, grandchild) = spawn_wrapper_in_own_group();
    let leader = wrapper.id();
    unsafe { libc::kill(leader as libc::pid_t, libc::SIGKILL) };
    let _ = wrapper.wait();
    std::thread::sleep(Duration::from_millis(200));
    assert!(
        alive(grandchild),
        "per-pid SIGKILL of the leader must orphan the grandchild"
    );

    assert!(
        !owns_process_group(grandchild),
        "an orphan carries its dead leader's pgid, never its own"
    );
    assert!(
        !owns_process_group(leader),
        "a reaped leader answers ESRCH, the proof must fail"
    );
    assert!(!sigkill_owned_group(grandchild));
    assert!(!sigkill_owned_group(leader));
    assert!(alive(grandchild), "a declined proof must signal nothing");

    unsafe { libc::kill(grandchild as libc::pid_t, libc::SIGKILL) };
}

#[test]
fn the_callers_own_group_is_never_a_target() {
    let own = my_pgid();
    assert!(!owns_process_group(own));
    assert!(
        !sigkill_owned_group(own),
        "would have killed this test process"
    );

    let mut sibling = Command::new("sleep").arg("300").spawn().unwrap();
    let pid = sibling.id();
    assert!(
        !owns_process_group(pid),
        "a child spawned INTO the caller's group is not a leader"
    );
    assert!(!sigkill_owned_group(pid));
    assert!(alive(pid));
    sibling.kill().unwrap();
    let _ = sibling.wait();
}

/// The engine shape end to end: a wrapper that IGNORES SIGTERM (forcing the SIGKILL leg)
/// launches the HTTP engine as a grandchild. `shutdown` must leave the port free and the
/// whole group gone — before this commit, SIGKILL reached the wrapper alone and the
/// grandchild kept serving.
const TERM_IGNORING_WRAPPER: &str = r#"
import signal, subprocess, sys
signal.signal(signal.SIGTERM, signal.SIG_IGN)
engine = subprocess.Popen([sys.executable, "-c", sys.argv[1], sys.argv[2]])
engine.wait()
"#;

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

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn port_listening(port: u16) -> bool {
    std::net::TcpStream::connect_timeout(
        &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
        Duration::from_millis(200),
    )
    .is_ok()
}

fn group_members(pgid: u32) -> Vec<String> {
    let out = Command::new("pgrep")
        .args(["-g", &pgid.to_string()])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .map(str::to_string)
        .collect()
}

#[tokio::test]
async fn shutdown_takes_the_engine_launched_by_a_term_ignoring_wrapper() {
    let port = free_port();
    let mut config = SidecarConfig::new(
        "wrapped-engine",
        vec![
            "python3".to_string(),
            "-c".to_string(),
            TERM_IGNORING_WRAPPER.to_string(),
            FAKE_ENGINE.to_string(),
            port.to_string(),
        ],
        format!("http://127.0.0.1:{port}"),
        "fake",
    );
    config.startup_stall_window = Duration::from_secs(20);
    let sidecar = Sidecar::start(config).await.unwrap();
    let leader = sidecar.pid().await.unwrap();
    assert!(owns_process_group(leader));
    assert!(
        group_members(leader).len() >= 2,
        "wrapper and engine must both be in the wrapper's group"
    );

    sidecar.shutdown().await;

    assert!(
        !port_listening(port),
        "engine grandchild still serves on {port}"
    );
    assert!(
        group_members(leader).is_empty(),
        "group {leader} still has members: {:?}",
        group_members(leader)
    );
}

/// The wrapper dies on SIGTERM WITHOUT forwarding (python's default), orphaning the engine
/// with the dead leader's pgid: the proof declines a group kill, and the port is released
/// per-pid from the LISTEN entry whose pgid is the engine's own.
const TERM_DYING_WRAPPER: &str = r#"
import subprocess, sys
engine = subprocess.Popen([sys.executable, "-c", sys.argv[1], sys.argv[2]])
engine.wait()
"#;

#[tokio::test]
async fn shutdown_releases_the_port_from_residue_of_its_own_group() {
    let port = free_port();
    let mut config = SidecarConfig::new(
        "residue-engine",
        vec![
            "python3".to_string(),
            "-c".to_string(),
            TERM_DYING_WRAPPER.to_string(),
            FAKE_ENGINE.to_string(),
            port.to_string(),
        ],
        format!("http://127.0.0.1:{port}"),
        "fake",
    );
    config.startup_stall_window = Duration::from_secs(20);
    let sidecar = Sidecar::start(config).await.unwrap();
    let leader = sidecar.pid().await.unwrap();

    sidecar.shutdown().await;

    assert!(
        !port_listening(port),
        "orphaned engine still serves on {port}"
    );
    assert!(group_members(leader).is_empty());
}
