//! App-under-test PROCESS GROUPS — invariant 5's mechanism, moved verbatim from swarm.rs under the
//! incremental-split law (development_gates::swarm_rs_line_count_only_decreases), paying for the
//! 2026-09-05 error-arm/loudness wiring in the root (A1-A4 sink+ledger truth, B1-B2 shadow copy and
//! question file, C1-C5 REPAIR tail truth and app spawns). Every spawn of the produced app or of a
//! smoke command runs as the leader of its OWN process group (`spawn_grouped_capturing`), is torn
//! down whole (`kill_app_tree`) and releases its pipe readers on the GROUP's liveness, never on EOF
//! (`kill_app_tree_and_drain` — the r0 park); `ShellGroupReaper` sweeps the groups a worker
//! attempt's shell tool left behind. `smoke_output` (C5) is the smoke commands' door into the same
//! mechanism — `Command::output()` + `kill_on_drop` reached ONE pid.

use std::path::Path;
use std::sync::{Arc, Mutex};

use console::style;

/// Run a smoke subcommand with a HARD TIMEOUT + null stdin, so a produced server/REPL/daemon that ignores
/// `--help` (or a build that waits on input) can never hang the whole run at the finish line. Returns None on
/// spawn error OR timeout (inconclusive — never a finding). The timeout is on a smoke command, not a model.
///
/// C5 (invariant 5): the command runs as the leader of its OWN process group via `spawn_grouped_capturing`
/// and is torn down with `kill_app_tree` — `Command::output()` + `kill_on_drop` reached ONE pid, so a
/// `go run`/`npm test`/`cargo run`/`pytest` whose child server outlived the timeout kept its port and its
/// pipe write-ends (the r0 park). The output is captured WHOLE (never the 16 KiB tail the boot probes
/// keep), so every parser over stdout/stderr sees exactly what `output()` gave it; a process that exits
/// on its own is reaped by `wait()` and the group kill that follows is a no-op on a dead group.
pub(super) async fn smoke_output(mut cmd: tokio::process::Command, secs: u64) -> Option<std::process::Output> {
    let mut app = spawn_grouped_capturing(&mut cmd, PipeCapture::Whole).ok()?;
    let status = tokio::time::timeout(std::time::Duration::from_secs(secs), app.child.wait())
        .await
        .ok()
        .and_then(|s| s.ok());
    let (stdout, stderr) = app.kill_tree_split().await;
    Some(std::process::Output {
        status: status?,
        stdout,
        stderr,
    })
}

/// Run ONE repro command in `cwd` with a hard timeout, capturing combined stdout+stderr and success. A
/// timeout or spawn error yields `("", false)` — NOT a crash (empty output has no traceback), so a hang is
/// never mistaken for a reproduced defect. The repro runs as its own process group and the whole group is
/// killed with it, so a repro that spawns children cannot outlive us — the old `timeout(output())` dropped
/// the `Child` on expiry, which kills ONE pid and leaked the rest. The timeout is on the repro command,
/// which is not a model.
pub(super) async fn run_repro_once(argv: &[String], cwd: &Path) -> (String, bool) {
    let mut c = tokio::process::Command::new(&argv[0]);
    c.args(&argv[1..]).current_dir(cwd);
    let Ok(mut app) = spawn_grouped(&mut c) else {
        return (String::new(), false);
    };
    let status = tokio::time::timeout(std::time::Duration::from_secs(30), app.child.wait())
        .await
        .ok()
        .and_then(|s| s.ok());
    let out = app.kill_tree().await;
    match status {
        Some(st) => (out, st.success()),
        None => (String::new(), false),
    }
}

/// Spawn an app entry as the LEADER of its own process group, so everything it forks — r0's
/// wrapper `Popen`'d ledgerd and notifierd with inherited stdio — can be killed as one unit.
pub(super) fn own_process_group(cmd: &mut tokio::process::Command) {
    #[cfg(unix)]
    cmd.process_group(0);
    #[cfg(not(unix))]
    let _ = cmd;
}

/// Whether anything in the group still exists (signal 0 is the existence check). No group id, or
/// a platform without process groups, counts as gone: there is nothing left to wait for.
fn process_group_alive(pgid: Option<i32>) -> bool {
    #[cfg(unix)]
    {
        pgid.is_some_and(|g| unsafe { libc::kill(-g, 0) } == 0)
    }
    #[cfg(not(unix))]
    {
        let _ = pgid;
        false
    }
}

/// SIGKILL the child's whole process group (pgid == pid under `own_process_group`), then reap the
/// child. tokio's `Child::kill` signals ONE pid: the wrapper dies and the services it `Popen`'d
/// with inherited stdio survive it, each holding our pipe write-ends open and its port bound —
/// which is how one probe's leak poisoned the next probe's "port already bound" refusal.
pub(super) async fn kill_app_tree(child: &mut tokio::process::Child, pgid: Option<i32>) {
    #[cfg(unix)]
    if let Some(g) = pgid {
        unsafe { libc::kill(-g, libc::SIGKILL) };
    }
    #[cfg(not(unix))]
    let _ = pgid;
    let _ = child.kill().await;
    let _ = child.wait().await;
}

/// How much of a child's pipe a `GroupedChild` keeps: the boot/repro probes want only the TAIL
/// (the traceback is at the end; a chatty server must not grow memory for the whole poll), the
/// smoke commands (C5) want the WHOLE output — `pytest --collect-only`'s item list and the
/// parsers over `npm test` read more than the last 16 KiB, and `Command::output()` kept it all.
#[derive(Clone, Copy)]
enum PipeCapture {
    Tail,
    Whole,
}

/// Read one of a child's pipes on its own task into `buf` — the last 8-16 KiB for `Tail`, every
/// byte for `Whole`. The buffer is shared rather than returned so a reader aborted mid-stream
/// still yields what it captured.
fn spawn_pipe_tail(
    stream: Option<impl tokio::io::AsyncRead + Unpin + Send + 'static>,
    buf: Arc<Mutex<Vec<u8>>>,
    capture: PipeCapture,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        use tokio::io::AsyncReadExt;
        let Some(mut s) = stream else {
            return;
        };
        let mut chunk = [0u8; 4096];
        while let Ok(n) = s.read(&mut chunk).await {
            if n == 0 {
                break;
            }
            let mut b = buf.lock().unwrap_or_else(|e| e.into_inner());
            b.extend_from_slice(&chunk[..n]);
            if matches!(capture, PipeCapture::Tail) && b.len() > 16384 {
                let cut = b.len() - 8192;
                b.drain(..cut);
            }
        }
    })
}

/// Kill the child's group, reap it, then release its pipe readers on the GROUP's liveness.
///
/// DRAIN AFTER THE KILL — this is not a model cap: the process is already dead. Its whole group
/// is SIGKILLed and the child reaped before the loop starts; all that is bounded is how long we
/// keep reading pipes nothing of ours can still write to. Pipe EOF arrives only when the LAST
/// write-end closes, and every write-end we can reach closes with the group — so the wait is on
/// group liveness, never on EOF: a member that left the group (`setsid`, a double-forked daemon)
/// can hold the write-end forever and no signal we send will close it. An EOF-only wait is
/// exactly what parked r0's run for good after its verdict was in. The 50ms cadence is the one
/// the bind poll already uses; no new time constant.
async fn kill_app_tree_and_drain(
    child: &mut tokio::process::Child,
    pgid: Option<i32>,
    readers: [tokio::task::JoinHandle<()>; 2],
) {
    kill_app_tree(child, pgid).await;
    let mut ticks_after_group_gone = 0u8;
    while !readers.iter().all(|r| r.is_finished()) {
        if !process_group_alive(pgid) {
            // One more tick lets the readers take what the kernel already buffered (a pipe holds
            // at most 64 KiB; one wake drains it). After that, whoever still holds the write-end
            // is not ours to wait for.
            ticks_after_group_gone += 1;
            if ticks_after_group_gone > 1 {
                for r in &readers {
                    r.abort();
                }
                break;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    for r in readers {
        let _ = r.await;
    }
}

/// Sweeps the process groups a worker attempt's OWN shell calls spawned, at the attempt's end.
///
/// Invariant 5 covers the ENGINE's spawns via `spawn_grouped`/`kill_app_tree`; this closes the
/// other half: the MODEL boots app servers through its developer shell tool, and on r2 the sink's
/// dead attempt 0 left 3 of them as PPID-1 orphans in the ENGINE's process group — the task_retry
/// path killed nothing, and the operator's `killpg` reap took the engine down at INTEGRATE minute
/// 139. With `process_groups::enable()`, every shell spawn leads its own group registered under
/// the attempt's session; this guard kills whatever of them survives the attempt.
///
/// It is a DROP guard on purpose: the attempt has many exits (completion, the stream-decode
/// Transient at the #121 branch, ContentRetry, judge termination, an `?` error, cancellation of
/// the whole dispatch future) and each of them unwinds through here — patching the branches one by
/// one is how the retry path got missed. `reap_now()` at the normal exit reports what was killed;
/// the Drop arm is the net for every other path and never signals the engine's own group (the
/// registry's reaper guards that even against a pathological registration).
pub(super) struct ShellGroupReaper {
    session_id: String,
    armed: bool,
}

impl ShellGroupReaper {
    pub(super) fn armed(session_id: String) -> Self {
        Self {
            session_id,
            armed: true,
        }
    }

    pub(super) fn reap_now(&mut self) -> Vec<i32> {
        self.armed = false;
        goose::agents::platform_extensions::developer::process_groups::reap_session(
            &self.session_id,
        )
    }
}

impl Drop for ShellGroupReaper {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let killed = goose::agents::platform_extensions::developer::process_groups::reap_session(
            &self.session_id,
        );
        if !killed.is_empty() {
            eprintln!(
                "  {} reaped {} leaked shell process group(s) on an early attempt exit: {:?}",
                style("⚠").yellow().bold(),
                killed.len(),
                killed
            );
        }
    }
}

/// A child running as its own group leader with both pipes tailed into shared buffers.
struct GroupedChild {
    child: tokio::process::Child,
    pgid: Option<i32>,
    out: Arc<Mutex<Vec<u8>>>,
    err: Arc<Mutex<Vec<u8>>>,
    readers: [tokio::task::JoinHandle<()>; 2],
}

impl GroupedChild {
    /// Kill the group, reap the child, release the readers on group liveness, and hand back the
    /// combined stdout+stderr tail — whatever was captured, even if a reader had to be released.
    async fn kill_tree(self) -> String {
        let (out, err) = self.kill_tree_split().await;
        format!(
            "{}{}",
            String::from_utf8_lossy(&out),
            String::from_utf8_lossy(&err)
        )
    }

    /// The same, with stdout and stderr kept apart (the `std::process::Output` shape the smoke
    /// callers were written against).
    async fn kill_tree_split(self) -> (Vec<u8>, Vec<u8>) {
        let GroupedChild {
            mut child,
            pgid,
            out,
            err,
            readers,
        } = self;
        kill_app_tree_and_drain(&mut child, pgid, readers).await;
        let take = |b: &Mutex<Vec<u8>>| b.lock().unwrap_or_else(|e| e.into_inner()).clone();
        (take(&out), take(&err))
    }
}

/// Spawn `cmd` with null stdin and piped, tailed stdout/stderr as the leader of its own process
/// group. The one way to start anything that may fork: `Command::output()` reads to EOF, and
/// EOF never comes while a grandchild holds the inherited write-end.
fn spawn_grouped(cmd: &mut tokio::process::Command) -> std::io::Result<GroupedChild> {
    spawn_grouped_capturing(cmd, PipeCapture::Tail)
}

fn spawn_grouped_capturing(
    cmd: &mut tokio::process::Command,
    capture: PipeCapture,
) -> std::io::Result<GroupedChild> {
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    own_process_group(cmd);
    let mut child = cmd.spawn()?;
    let pgid = child.id().map(|p| p as i32);
    let out = Arc::new(Mutex::new(Vec::new()));
    let err = Arc::new(Mutex::new(Vec::new()));
    let readers = [
        spawn_pipe_tail(child.stdout.take(), Arc::clone(&out), capture),
        spawn_pipe_tail(child.stderr.take(), Arc::clone(&err), capture),
    ];
    Ok(GroupedChild {
        child,
        pgid,
        out,
        err,
        readers,
    })
}

/// Spawn ONE `python3 -m pkg argv` and decide bind-or-die: `None` = some probe port bound
/// (alive), `Some(tail)` = it never bound, with the normalized output tail as evidence. The
/// shared core of the boot floor AND the gate's every-advertised-invocation check (F910
/// defect 2). Pipes are drained during the poll (an undrained pipe wedges a chatty child at
/// ~64KB and misreads as dead) and released on the PROCESS GROUP's liveness after the kill,
/// never on pipe EOF — `kill_app_tree_and_drain` carries the r0 hang that rule comes from.
/// Per-probe randomness (ports, scratch paths) is normalized OUT of the tail so the repair
/// loop's identical-traceback stop can actually stop.
pub(super) async fn boot_invocation(
    root: &Path,
    pkg: &str,
    argv: &[String],
    probe_ports: &[u16],
    scratch: &Path,
) -> Option<String> {
    let mut cmd = tokio::process::Command::new("python3");
    cmd.args(["-m", pkg])
        .args(argv)
        .current_dir(root)
        .env("PYTHONPATH", "src");
    let app = match spawn_grouped(&mut cmd) {
        Ok(a) => a,
        Err(e) => return Some(format!("spawn failed: {e}")),
    };
    let mut up = false;
    'poll: for _ in 0..80 {
        for p in probe_ports {
            if tokio::net::TcpStream::connect(("127.0.0.1", *p))
                .await
                .is_ok()
            {
                up = true;
                break 'poll;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    let combined = app.kill_tree().await;
    if up {
        return None;
    }
    let tail: String = combined
        .chars()
        .rev()
        .take(1200)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    let mut tail = tail.replace(&*scratch.to_string_lossy(), "SCRATCH");
    for p in probe_ports {
        tail = tail.replace(&p.to_string(), "PORT");
    }
    if tail.trim().is_empty() {
        Some("no output captured".to_string())
    } else {
        Some(tail)
    }
}

#[cfg(test)]
mod boot_invocation_tests {
    use std::path::PathBuf;

    use super::*;

    /// A fake app with EXACTLY r0's wrapper shape: it `Popen`s a grandchild with no `stdout=`/
    /// `stderr=` kwargs, so the grandchild inherits the pipe write-ends `boot_invocation` is
    /// reading, prints the grandchild's pid so the test can check it died, and never binds a port.
    /// `popen_kwargs` lets one test move the grandchild out of the process group. The sleeps are
    /// a test-fixture bound on a fake app, not model work.
    fn fake_app(name: &str, popen_kwargs: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("goose_bootinv_{}_{name}", std::process::id()));
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
}
