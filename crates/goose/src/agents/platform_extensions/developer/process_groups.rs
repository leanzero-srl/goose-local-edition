//! Own-process-group hygiene for shell-tool spawns, and the registry an attempt-scoped reaper sweeps.
//!
//! WHY THIS EXISTS (r2, 2026-08-30): the swarm's integrate sink booted its app servers through the
//! developer shell tool, whose spawn carried NO process group of its own — so when the attempt died
//! to a mid-stream body drop, the daemonized servers survived as PPID-1 orphans INSIDE THE ENGINE'S
//! process group. The only way to reap them from outside was `killpg`, and that killpg took the
//! engine down with them at INTEGRATE minute 139. Two invariants fall out, both enforced here:
//!
//! 1. A shell-tool child may NEVER share the engine's pgid — each spawn leads its own group, so its
//!    whole subtree (backgrounded servers included) is addressable as one unit that is not ours.
//! 2. The engine's own process group is NEVER signalled by the reaper, whatever the registry holds.
//!
//! THE REGISTRY is OFF unless `enable()` is called; the swarm engine enables it because its
//! workers are exactly the callers whose leftovers must be reapable per attempt. Registration is
//! keyed by session id — each swarm attempt runs in its own session — so a reap of one attempt can
//! never touch a concurrent sibling's processes.
//!
//! OWN-GROUP SPAWNING is wider (Q-406, 2026-09-28): every shell command leads its own group unless
//! a terminal Ctrl+C can reach this process (`host_holds_terminal_foreground`). Before Q-406 it
//! followed `enable()`, so goose serve and the desktop spawned into goosed's group, and a timeout
//! killed bash ALONE: a `find ~ … | head` whose find sat in a macOS privacy (TCC) `open()` outlived
//! the turn forever as a PPID-1 orphan, head held the output pipe, and the model saw a bare
//! "(no output)". The terminal keeps the shared foreground group because that is what lets a
//! Ctrl+C — and a `sudo`/`ssh` prompt on /dev/tty — reach the command; a background group would
//! stop on SIGTTIN instead. `CommandProcesses` is what signals a command's processes on timeout,
//! cancel and goose serve's teardown, through the same proof the sidecar's group kill requires.
//! A goose serve that dies WITHOUT its teardown (a crash, a SIGKILL) hands the same groups to its
//! watchdog, which ends them when goose's end of a pipe closes (Q-407, `shell_watchdog`).

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

static ENABLED: AtomicBool = AtomicBool::new(false);

/// How long a command's processes get to exit (and its pipes to close) before the next step: the
/// shell tool's post-exit output drain, and the SIGTERM → SIGKILL window of a terminated command.
/// One window for both because they wait on the same thing — the last holder of the pipe going.
pub const EXIT_GRACE: Duration = Duration::from_millis(500);

/// Whether a terminal Ctrl+C reaches this process: it has a controlling terminal and its group is
/// that terminal's foreground group (goose in a terminal). Opening /dev/tty fails with ENXIO when
/// there is no controlling terminal — goose serve under the desktop, a daemon — and that failure
/// IS the answer "no terminal", so it reads as false.
pub fn host_holds_terminal_foreground() -> bool {
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        use std::os::unix::fs::OpenOptionsExt;
        let Ok(tty) = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOCTTY)
            .open("/dev/tty")
        else {
            return false;
        };
        unsafe { libc::tcgetpgrp(tty.as_raw_fd()) == libc::getpgrp() }
    }
    #[cfg(not(unix))]
    {
        false
    }
}

/// Whether the next shell command leads its own process group (see the module doc).
pub fn spawn_in_own_group() -> bool {
    enabled() || !host_holds_terminal_foreground()
}

fn registry() -> &'static Mutex<Vec<(String, i32)>> {
    static REGISTRY: OnceLock<Mutex<Vec<(String, i32)>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(Vec::new()))
}

/// Turn the session registry ON for this process (and own-group spawning with it, terminal or
/// not). One-way and idempotent: the swarm engine calls it on every worker dispatch, and there is
/// no path back because a half-enabled process would mix reapable and unreapable children under
/// the same sessions.
pub fn enable() {
    ENABLED.store(true, Ordering::Relaxed);
}

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Leaders of own-group shell commands whose `run_command` is still in flight — what goose serve's
/// teardown stops on its way out. Before Q-406 those commands sat in goosed's own group, which the
/// desktop signals on quit; in groups of their own that signal no longer reaches them.
fn live() -> &'static Mutex<HashSet<i32>> {
    static LIVE: OnceLock<Mutex<HashSet<i32>>> = OnceLock::new();
    LIVE.get_or_init(|| Mutex::new(HashSet::new()))
}

/// An own-group command that ENDED while its group still had members — a server the model started
/// with `&`. Before Q-406 such a server sat in goosed's group and died with the desktop's quit
/// signal; in a group of its own only goose serve's teardown reaches it. The leader is gone by then
/// and a group id is reusable once the group empties, so each member is stamped with its start
/// time: the teardown signals the group only while a stamped member is provably the same process.
#[cfg(unix)]
pub struct Lingering {
    leader: i32,
    stamps: Vec<(i32, String)>,
}

#[cfg(unix)]
impl Lingering {
    pub(super) fn new(leader: i32, stamps: Vec<(i32, String)>) -> Self {
        Self { leader, stamps }
    }

    pub(super) fn leader(&self) -> i32 {
        self.leader
    }

    pub(super) fn stamps(&self) -> &[(i32, String)] {
        &self.stamps
    }
}

#[cfg(unix)]
fn lingering() -> &'static Mutex<Vec<Lingering>> {
    static LINGERING: OnceLock<Mutex<Vec<Lingering>>> = OnceLock::new();
    LINGERING.get_or_init(|| Mutex::new(Vec::new()))
}

#[cfg(unix)]
pub(super) fn live_leaders() -> Vec<i32> {
    live()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .copied()
        .collect()
}

#[cfg(unix)]
pub(super) fn with_lingering(read: impl FnOnce(&[Lingering])) {
    read(&lingering().lock().unwrap_or_else(|e| e.into_inner()));
}

/// The process's start time, as an opaque stamp with no whitespace; a pid reused by another
/// process starts later. Read from the kernel where it can be (Q-407: every own-group command is
/// stamped as it spawns, and a `ps` per command measured 3.5 ms; the kernel's stamp costs a
/// syscall and is finer than `ps`'s one-second `lstart`), else from `ps`.
#[cfg(target_os = "macos")]
pub(super) fn start_time(pid: i32) -> Option<String> {
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
    let read = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            (&mut info as *mut libc::proc_bsdinfo).cast(),
            size,
        )
    };
    (read == size).then(|| format!("{}.{:06}", info.pbi_start_tvsec, info.pbi_start_tvusec))
}

/// `/proc/<pid>/stat` field 22, the start in clock ticks since boot. The command name (field 2)
/// may hold spaces and parentheses, so the fields are counted from its closing parenthesis.
#[cfg(target_os = "linux")]
pub(super) fn start_time(pid: i32) -> Option<String> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let after_name = &stat[stat.rfind(')')? + 1..];
    after_name.split_whitespace().nth(19).map(str::to_string)
}

#[cfg(all(unix, not(any(target_os = "macos", target_os = "linux"))))]
pub(super) fn start_time(pid: i32) -> Option<String> {
    let out = std::process::Command::new("ps")
        .args(["-o", "lstart=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    let stamp = String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("_");
    (!stamp.is_empty()).then_some(stamp)
}

#[cfg(unix)]
impl Lingering {
    /// A stamped member still alive, still in the group, and still the same process — proof the
    /// group id has not been handed to anyone else (it cannot be while a member lives).
    pub(super) fn still_the_same_group(&self) -> bool {
        self.stamps.iter().any(|(pid, stamp)| {
            pgid_of(*pid) == Some(self.leader) && start_time(*pid).as_deref() == Some(stamp)
        })
    }
}

/// `killpg(leader, signal)` only when the sidecar's proof holds on this exact pid: it is the LIVE
/// leader of its own group (`getpgid(leader) == leader`) and that group is not ours
/// (`leader != getpgrp()`) — the gate-4-sanctioned shape. Returns whether the group was signalled;
/// `false` means nothing was.
#[cfg(unix)]
pub fn signal_owned_group(leader: i32, signal: i32) -> bool {
    if leader <= 1 || !goose_sidecar::owns_process_group(leader as u32) {
        return false;
    }
    unsafe { libc::killpg(leader, signal) == 0 }
}

/// pids `pgrep` matches for `args`. Exit 1 is pgrep's "matched nothing" — an honest empty; any
/// other failure is an error the caller must state, never an empty list.
#[cfg(unix)]
fn pgrep(args: &[&str]) -> std::io::Result<Vec<i32>> {
    let out = std::process::Command::new("pgrep").args(args).output()?;
    match out.status.code() {
        Some(0) => Ok(String::from_utf8_lossy(&out.stdout)
            .split_whitespace()
            .filter_map(|p| p.parse().ok())
            .collect()),
        Some(1) => Ok(Vec::new()),
        _ => Err(std::io::Error::other(format!(
            "pgrep {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ))),
    }
}

#[cfg(unix)]
fn pgid_of(pid: i32) -> Option<i32> {
    let pgid = unsafe { libc::getpgid(pid) };
    (pgid > 0).then_some(pgid)
}

/// The processes of one shell command, from spawn until it is finished, detached, or terminated.
/// Own-group commands are addressed as their group (members: `pgrep -g leader`, and an orphan
/// keeps the pgid); a command in the terminal's shared group is addressed as the shell plus its
/// descendants, per pid. Every pid ever enumerated is remembered, because an orphan of a shared-
/// group command is no longer anyone's descendant by the time the SIGKILL leg looks.
///
/// Dropped while still ARMED — the tool call was cancelled and its future dropped — it signals the
/// command itself: SIGTERM now, SIGKILL for whatever is left after `EXIT_GRACE`.
#[cfg(unix)]
pub struct CommandProcesses {
    leader: i32,
    own_group: bool,
    group: i32,
    seen: Vec<i32>,
    armed: bool,
}

#[cfg(unix)]
impl CommandProcesses {
    pub fn spawned(leader: i32, own_group: bool) -> Self {
        if own_group {
            live()
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(leader);
            super::shell_watchdog::note_live(leader);
        }
        Self {
            leader,
            own_group,
            group: if own_group {
                leader
            } else {
                unsafe { libc::getpgrp() }
            },
            seen: vec![leader],
            armed: true,
        }
    }

    /// The command ended on its own or was detached on purpose: nothing of it is signalled from
    /// here on, and goose serve's teardown no longer counts it as live.
    pub fn finished(&mut self) {
        self.armed = false;
        let was_live = live()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.leader);
        if was_live {
            super::shell_watchdog::note_gone(self.leader);
        }
    }

    /// The command ended on its own (or was terminated and something survived): like `finished`,
    /// and if its own group still has members they are recorded, stamped, for goose serve's
    /// teardown — the model may have started a server on purpose, and it may run until goose goes.
    pub fn ended(&mut self) {
        self.finished();
        if !self.own_group || !group_alive(self.leader) {
            return;
        }
        let Ok(members) = self.members() else {
            return;
        };
        let stamps: Vec<(i32, String)> = members
            .into_iter()
            .filter_map(|pid| start_time(pid).map(|t| (pid, t)))
            .collect();
        if stamps.is_empty() {
            return;
        }
        let recorded = Lingering {
            leader: self.leader,
            stamps,
        };
        super::shell_watchdog::note_ended(&recorded);
        let emptied: Vec<i32> = {
            let mut lingering = lingering().lock().unwrap_or_else(|e| e.into_inner());
            let (kept, emptied): (Vec<Lingering>, Vec<Lingering>) = lingering
                .drain(..)
                .filter(|l| l.leader != self.leader)
                .partition(|l| group_alive(l.leader));
            *lingering = kept;
            lingering.push(recorded);
            emptied.iter().map(|l| l.leader).collect()
        };
        for leader in emptied {
            super::shell_watchdog::note_gone(leader);
        }
    }

    /// An own group this process did not just spawn (the teardown's view): never armed, never
    /// counted as live.
    fn adopt(leader: i32) -> Self {
        Self {
            leader,
            own_group: true,
            group: leader,
            seen: Vec::new(),
            armed: false,
        }
    }

    /// Every process of the command right now (the leader included while it lives).
    pub fn members(&mut self) -> std::io::Result<Vec<i32>> {
        let found = if self.own_group {
            pgrep(&["-g", &self.leader.to_string()])?
        } else {
            let mut found = vec![self.leader];
            let mut frontier = vec![self.leader];
            while let Some(parent) = frontier.pop() {
                for child in pgrep(&["-P", &parent.to_string()])? {
                    if !found.contains(&child) {
                        found.push(child);
                        frontier.push(child);
                    }
                }
            }
            found
        };
        for pid in &found {
            if !self.seen.contains(pid) {
                self.seen.push(*pid);
            }
        }
        Ok(found)
    }

    /// A pid this command may still signal: alive, never us, and still in the group it was seen
    /// in (a group id cannot be reused while any member lives, so a match is the same process
    /// group; for the shared group the pid was confirmed a descendant when it was seen).
    fn still_ours(&self, pid: i32) -> bool {
        let own_pid = std::process::id() as i32;
        let own_group = unsafe { libc::getpgrp() };
        if pid <= 1 || pid == own_pid || (self.own_group && self.group == own_group) {
            return false;
        }
        pgid_of(pid) == Some(self.group)
    }

    /// SIGTERM the command: its whole group under the proof, else each confirmed member per pid
    /// (the leader already gone, or the terminal's shared group — never a killpg there).
    pub fn sigterm(&mut self) {
        if self.own_group && signal_owned_group(self.leader, libc::SIGTERM) {
            return;
        }
        let _ = self.members();
        for pid in self.seen.clone() {
            if self.still_ours(pid) {
                unsafe { libc::kill(pid, libc::SIGTERM) };
            }
        }
    }

    /// SIGKILL, per pid, every process of the command still in its group; returns those pids.
    pub fn sigkill_survivors(&mut self) -> Vec<i32> {
        let _ = self.members();
        let mut killed = Vec::new();
        for pid in self.seen.clone() {
            if self.still_ours(pid) && unsafe { libc::kill(pid, libc::SIGKILL) } == 0 {
                killed.push(pid);
            }
        }
        killed
    }

    /// Processes of the command that are still in its group — after a SIGKILL, the ones the kernel
    /// has not let go of (a process in uninterruptible sleep dies only when the call returns).
    pub fn survivors(&mut self) -> Vec<i32> {
        let _ = self.members();
        self.seen
            .iter()
            .copied()
            .filter(|p| self.still_ours(*p))
            .collect()
    }
}

#[cfg(unix)]
impl Drop for CommandProcesses {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        self.sigterm();
        self.finished();
        let mut rest = CommandProcesses {
            leader: self.leader,
            own_group: self.own_group,
            group: self.group,
            seen: std::mem::take(&mut self.seen),
            armed: false,
        };
        std::thread::spawn(move || {
            std::thread::sleep(EXIT_GRACE);
            rest.sigkill_survivors();
        });
    }
}

/// goose serve's teardown step: SIGTERM every in-flight own-group shell command under the group
/// proof, then SIGKILL per pid whatever is left after `EXIT_GRACE`. One line describes the outcome.
pub async fn terminate_live_commands() -> String {
    let leaders: Vec<i32> = live()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .drain()
        .collect();
    #[cfg(unix)]
    {
        let ended: Vec<Lingering> = lingering()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .drain(..)
            .collect();
        // The watchdog keeps its records until this step has signalled them: a goose killed in
        // the middle of it (the desktop's SIGKILL fallback) leaves the rest to the watchdog.
        let handled: Vec<i32> = leaders
            .iter()
            .copied()
            .chain(ended.iter().map(|l| l.leader))
            .collect();
        let outcome = terminate_command_groups(leaders, ended).await;
        for leader in handled {
            super::shell_watchdog::note_gone(leader);
        }
        outcome
    }
    #[cfg(not(unix))]
    {
        if leaders.is_empty() {
            "no shell command was running".to_string()
        } else {
            format!(
                "{} shell command(s) left running: no process groups on this platform",
                leaders.len()
            )
        }
    }
}

/// Test door: the recorded ended group containing `pid`, taken out of the process-wide registry so
/// a test acts on its own group only (the teardown itself drains everything, including groups of
/// tests running beside it).
#[cfg(all(test, unix))]
pub(crate) fn take_lingering_with(pid: i32) -> Option<Lingering> {
    let mut lingering = lingering().lock().unwrap_or_else(|e| e.into_inner());
    let at = lingering
        .iter()
        .position(|l| l.stamps.iter().any(|(p, _)| *p == pid))?;
    Some(lingering.remove(at))
}

#[cfg(all(test, unix))]
pub(crate) async fn terminate_ended_for_test(ended: Lingering) -> String {
    terminate_command_groups(Vec::new(), vec![ended]).await
}

/// In-flight groups by their live leader (the proof-gated killpg), ended groups only after their
/// stamps prove the group is still the one recorded (then per pid — their leader is gone).
#[cfg(unix)]
pub(super) async fn terminate_command_groups(leaders: Vec<i32>, ended: Vec<Lingering>) -> String {
    let (still_ours, gone): (Vec<Lingering>, Vec<Lingering>) =
        ended.into_iter().partition(|l| l.still_the_same_group());
    if leaders.is_empty() && still_ours.is_empty() {
        return format!(
            "no shell command was running and none left processes behind ({} ended group(s) already gone)",
            gone.len()
        );
    }
    let ended_leaders: Vec<i32> = still_ours.iter().map(|l| l.leader).collect();
    let mut commands: Vec<CommandProcesses> = leaders
        .iter()
        .chain(ended_leaders.iter())
        .map(|leader| {
            let mut command = CommandProcesses::adopt(*leader);
            let _ = command.members();
            command.sigterm();
            command
        })
        .collect();
    tokio::time::sleep(EXIT_GRACE).await;
    let killed: Vec<i32> = commands
        .iter_mut()
        .flat_map(|c| c.sigkill_survivors())
        .collect();
    let survivors: Vec<i32> = commands.iter_mut().flat_map(|c| c.survivors()).collect();
    format!(
        "SIGTERM to {} running shell command group(s) {leaders:?} and {} ended group(s) with processes left behind {ended_leaders:?}; SIGKILL to {killed:?}; still present: {survivors:?}",
        leaders.len(),
        ended_leaders.len()
    )
}

/// Record a spawned group under the session that spawned it. pgid <= 1 is refused at the door —
/// killpg(-1) is "everything I may signal" and must be unrepresentable in this registry.
pub fn register(session_id: &str, pgid: i32) {
    if pgid <= 1 {
        return;
    }
    registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push((session_id.to_string(), pgid));
}

/// Whether anything in the group still exists. Signal 0 probes without sending; EPERM still means
/// "exists". Non-unix has no process groups: nothing to reap, always gone.
pub fn group_alive(pgid: i32) -> bool {
    #[cfg(unix)]
    {
        if pgid <= 1 {
            return false;
        }
        if unsafe { libc::kill(-pgid, 0) } == 0 {
            return true;
        }
        std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }
    #[cfg(not(unix))]
    {
        let _ = pgid;
        false
    }
}

/// Called when a shell command finishes and its group left no survivors: drop the entry so the
/// registry only ever holds groups that still have members — i.e. leaks-in-waiting. A group whose
/// direct child exited but whose daemonized grandchildren live on is exactly what must stay.
pub fn prune_finished(pgid: i32) {
    if group_alive(pgid) {
        return;
    }
    registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .retain(|(_, g)| *g != pgid);
}

/// SIGKILL every group this session registered and still has members, and return the pgids killed.
/// The one caller-facing sweep: the swarm runs it at every attempt's terminal transition
/// (completion, transient retry, content retry, judge kill, cancellation) so no attempt's
/// app-under-test outlives the attempt. Guards, in order: never pgid <= 1, never the engine's own
/// process group, never a group led by the engine's own pid — whatever a buggy registration put in.
pub fn reap_session(session_id: &str) -> Vec<i32> {
    let mine: Vec<i32> = {
        let mut reg = registry().lock().unwrap_or_else(|e| e.into_inner());
        let mut taken = Vec::new();
        reg.retain(|(sid, pgid)| {
            if sid == session_id {
                taken.push(*pgid);
                false
            } else {
                true
            }
        });
        taken
    };
    #[cfg_attr(not(unix), allow(unused_mut))]
    let mut killed = Vec::new();
    #[cfg(unix)]
    {
        let own_group = unsafe { libc::getpgrp() };
        let own_pid = std::process::id() as i32;
        for pgid in mine {
            if pgid <= 1 || pgid == own_group || pgid == own_pid {
                continue;
            }
            if group_alive(pgid) {
                unsafe { libc::kill(-pgid, libc::SIGKILL) };
                killed.push(pgid);
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = mine;
    }
    killed
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn spawn_own_group_daemonizer() -> i32 {
        use std::os::unix::process::CommandExt;
        // The r2 leak shape: the direct child backgrounds a long sleeper inside a subshell and
        // exits, so the sleeper reparents to PPID 1 — but because the direct child led its own
        // group, the sleeper is still addressable through that pgid.
        let mut child = std::process::Command::new("sh")
            .args(["-c", "( sleep 300 & )"])
            .process_group(0)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("spawn daemonizer");
        let pgid = child.id() as i32;
        // Reap the direct sh (the shell tool always waits its child) — an unreaped zombie would
        // keep the group "alive" and the fixture would be testing the test's own leak instead.
        let _ = child.wait();
        pgid
    }

    fn wait_for<F: Fn() -> bool>(cond: F) -> bool {
        for _ in 0..100 {
            if cond() {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        false
    }

    #[test]
    fn reap_kills_daemonized_survivor_and_clears_entry() {
        let pgid = spawn_own_group_daemonizer();
        register("attempt-session-a", pgid);
        // The direct sh exits fast; the backgrounded sleep keeps the GROUP alive — the exact state
        // prune_finished must keep registered.
        assert!(wait_for(|| group_alive(pgid)));
        prune_finished(pgid);
        let killed = reap_session("attempt-session-a");
        assert_eq!(killed, vec![pgid]);
        assert!(wait_for(|| !group_alive(pgid)), "group must die on reap");
        // Idempotent: the entry is gone, a second sweep signals nothing.
        assert!(reap_session("attempt-session-a").is_empty());
    }

    #[test]
    fn reap_never_signals_the_engines_own_group() {
        let own_group = unsafe { libc::getpgrp() };
        // A pathological registration of the engine's own pgid — the r2 killpg death shape. The
        // guard must refuse to signal it (this test process surviving IS the assertion; a killpg
        // here would take the whole test runner down).
        registry()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(("attempt-session-b".to_string(), own_group));
        let killed = reap_session("attempt-session-b");
        assert!(killed.is_empty());
        assert!(group_alive(own_group), "we must still be alive");
    }

    #[test]
    fn reap_only_touches_its_own_session() {
        let pgid = spawn_own_group_daemonizer();
        register("attempt-session-c", pgid);
        assert!(wait_for(|| group_alive(pgid)));
        // A sibling attempt's sweep must not kill session c's group.
        assert!(reap_session("attempt-session-other").is_empty());
        assert!(group_alive(pgid));
        assert_eq!(reap_session("attempt-session-c"), vec![pgid]);
    }

    #[test]
    fn register_refuses_unrepresentable_groups() {
        register("attempt-session-d", 1);
        register("attempt-session-d", 0);
        register("attempt-session-d", -1);
        assert!(reap_session("attempt-session-d").is_empty());
    }

    fn alive(pid: i32) -> bool {
        unsafe { libc::kill(pid, 0) == 0 }
    }

    /// Gate 4's proof, on every shape it must refuse: our own group (the r2 death), a child that
    /// shares our group (not a leader), and a group whose leader already exited (an orphan keeps
    /// the dead leader's pgid). Refusing means NOTHING is signalled — the survivors prove it.
    #[test]
    fn the_group_kill_refuses_when_the_proof_fails() {
        let own_group = unsafe { libc::getpgrp() };
        assert!(!signal_owned_group(own_group, 0), "never our own group");
        assert!(!signal_owned_group(std::process::id() as i32, 0));
        assert!(!signal_owned_group(1, 0));

        let mut shared = std::process::Command::new("sleep")
            .arg("406011")
            .spawn()
            .expect("spawn shared-group sleep");
        let shared_pid = shared.id() as i32;
        assert!(
            !signal_owned_group(shared_pid, libc::SIGTERM),
            "a child in our group is not the leader of its own"
        );
        assert!(shared.try_wait().unwrap().is_none(), "it must still run");
        let _ = shared.kill();
        let _ = shared.wait();

        // Leader exits at once and is reaped; its backgrounded sleep lives on in the group.
        let leader = spawn_own_group_daemonizer();
        // The daemonizing subshell exits a beat after the leader; wait for the lone sleeper.
        assert!(wait_for(|| pgrep(&["-g", &leader.to_string()])
            .map(|m| m.len() == 1)
            .unwrap_or(false)));
        let orphans = pgrep(&["-g", &leader.to_string()]).unwrap();
        assert_eq!(orphans.len(), 1, "{orphans:?}");
        assert!(
            !signal_owned_group(leader, libc::SIGTERM),
            "a reaped leader proves nothing"
        );
        assert!(alive(orphans[0]), "the refused signal must not have landed");
        // The per-pid leg still reaches it: confirmed in the group, killed by pid.
        let mut command = CommandProcesses::spawned(leader, true);
        command.finished();
        assert_eq!(command.sigkill_survivors(), orphans);
        assert!(wait_for(|| !group_alive(leader)));

        use std::os::unix::process::CommandExt;
        let mut owned = std::process::Command::new("sleep")
            .arg("406012")
            .process_group(0)
            .spawn()
            .expect("spawn own-group sleep");
        assert!(signal_owned_group(owned.id() as i32, libc::SIGTERM));
        let status = owned.wait().unwrap();
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(status.signal(), Some(libc::SIGTERM));
    }

    /// An ended group whose stamps no longer prove it is the same group is never signalled — the
    /// shape a reused group id would take.
    #[tokio::test]
    async fn an_ended_group_that_cannot_be_proven_the_same_is_left_alone() {
        let leader = spawn_own_group_daemonizer();
        assert!(wait_for(|| pgrep(&["-g", &leader.to_string()])
            .map(|m| m.len() == 1)
            .unwrap_or(false)));
        let member = pgrep(&["-g", &leader.to_string()]).unwrap()[0];
        let forged = Lingering {
            leader,
            stamps: vec![(member, "Thu Jan  1 00:00:00 1970".to_string())],
        };
        assert!(!forged.still_the_same_group());
        let outcome = terminate_command_groups(Vec::new(), vec![forged]).await;
        assert!(outcome.contains("already gone"), "{outcome}");
        assert!(alive(member), "an unproven group must not be signalled");
        let genuine = Lingering {
            leader,
            stamps: vec![(member, start_time(member).unwrap())],
        };
        assert!(genuine.still_the_same_group());
        terminate_command_groups(Vec::new(), vec![genuine]).await;
        assert!(wait_for(|| !group_alive(leader)));
    }

    /// goose serve's teardown step on a group this test made: a pipeline leader plus its members
    /// all go, and the step says what it did.
    #[tokio::test]
    async fn teardown_terminates_an_in_flight_command_group() {
        use std::os::unix::process::CommandExt;
        let mut child = std::process::Command::new("sh")
            .args(["-c", "sleep 406013 | cat"])
            .process_group(0)
            .stdout(std::process::Stdio::null())
            .spawn()
            .expect("spawn pipeline");
        let leader = child.id() as i32;
        assert!(wait_for(|| pgrep(&["-g", &leader.to_string()])
            .map(|m| m.len() >= 3)
            .unwrap_or(false)));
        let outcome = terminate_command_groups(vec![leader], Vec::new()).await;
        assert!(outcome.contains("SIGTERM"), "{outcome}");
        let _ = child.wait();
        assert!(
            wait_for(|| pgrep(&["-f", "sleep 406013"]).unwrap().is_empty()),
            "{outcome}"
        );
    }
}
