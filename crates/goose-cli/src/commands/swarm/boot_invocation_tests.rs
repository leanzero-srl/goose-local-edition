//! `boot_invocation`'s process-group tests (invariant 5: every app-under-test spawn is a group,
//! `kill_app_tree` reaps it) — moved verbatim from swarm.rs under the incremental-split law,
//! paying for the VA-124 settled-list wiring and the VA-128 research hand in the worker loop.

use super::*;

/// A fake app with EXACTLY r0's wrapper shape: it `Popen`s a grandchild with no `stdout=`/
/// `stderr=` kwargs, so the grandchild inherits the pipe write-ends `boot_invocation` is
/// reading, prints the grandchild's pid so the test can check it died, and never binds a port.
/// `popen_kwargs` lets one test move the grandchild out of the process group. The sleeps are
/// a test-fixture bound on a fake app, not model work.
fn fake_app(name: &str, popen_kwargs: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("goose_bootinv_{}_{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("fakeapp")).unwrap();
    std::fs::write(root.join("fakeapp/__init__.py"), "").unwrap();
    std::fs::write(
        root.join("fakeapp/__main__.py"),
        format!(
            "import subprocess, sys, time\n\
             p = subprocess.Popen([sys.executable, \"-c\", \"import time; time.sleep(60)\"]{popen_kwargs})\n\
             print(f\"grandchild={{p.pid}}\", flush=True)\n\
             time.sleep(60)\n"
        ),
    )
    .unwrap();
    root
}

fn have_python3() -> bool {
    std::process::Command::new("python3")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// A port nobody will bind during the test: taken from the kernel and released at once.
fn never_bound_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn pid_alive(pid: i32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn grandchild_pid(tail: &str) -> i32 {
    tail.split("grandchild=")
        .nth(1)
        .and_then(|s| s.split_whitespace().next())
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| panic!("no grandchild pid in the captured tail: {tail:?}"))
}

/// The probe against the fake app, under a TEST-HARNESS bound: the fake app never binds, so
/// the 80x50ms bind poll ends in ~4s and anything past 30s is the hang itself. Not a model cap.
async fn probe(root: &Path, port: u16) -> Result<Option<String>, tokio::time::error::Elapsed> {
    let scratch = root.join("scratch");
    tokio::time::timeout(
        std::time::Duration::from_secs(30),
        boot_invocation(root, "fakeapp", &[], &[port], &scratch),
    )
    .await
}

/// THE r0 EXIT HANG. The wrapper was SIGKILLed after the bind poll, but the services it
/// `Popen`'d with inherited stdio outlived tokio's one-pid kill and held the pipe write-ends
/// open, so the readers never saw EOF and `stdout_task.await` parked the whole run — the
/// heartbeat ticked, CPU sat at 0%, and no result was ever emitted. The probe must return once
/// its child is dead, and the grandchild must die with it.
#[tokio::test]
async fn boot_invocation_returns_when_a_grandchild_holds_the_pipe() {
    if !have_python3() {
        return;
    }
    let root = fake_app("inherits", "");
    let res = probe(&root, never_bound_port()).await;
    let tail = res
        .expect("boot_invocation must return once the child is killed even though a grandchild inherited the pipe")
        .expect("the fake app never binds, so this must be Some(tail)");
    assert!(
        tail.contains("grandchild="),
        "the wrapper's own output must survive the group kill: {tail}"
    );
    let pid = grandchild_pid(&tail);
    // `kill -0` still succeeds on a zombie until launchd reaps it, so give it a moment.
    for _ in 0..40 {
        if !pid_alive(pid) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    let leaked = pid_alive(pid);
    let _ = std::process::Command::new("kill")
        .args(["-9", &pid.to_string()])
        .status();
    let _ = std::fs::remove_dir_all(&root);
    assert!(
        !leaked,
        "grandchild {pid} outlived the probe — the process group was not killed"
    );
}

/// The other half of the guarantee, independent of the group kill: a grandchild that left the
/// group (`setsid`, a double-forked daemon) cannot be reached by any signal we send and holds
/// the write-end for as long as it likes. The probe must STILL return, because the wait is on
/// the group's liveness and never on pipe EOF.
#[tokio::test]
async fn boot_invocation_returns_when_the_grandchild_escaped_the_group() {
    if !have_python3() {
        return;
    }
    let root = fake_app("escapes", ", start_new_session=True");
    let res = probe(&root, never_bound_port()).await;
    let tail = res
        .expect("boot_invocation must return once its process group is gone even though an escaped grandchild still holds the pipe")
        .expect("the fake app never binds, so this must be Some(tail)");
    let pid = grandchild_pid(&tail);
    let _ = std::process::Command::new("kill")
        .args(["-9", &pid.to_string()])
        .status();
    let _ = std::fs::remove_dir_all(&root);
}
