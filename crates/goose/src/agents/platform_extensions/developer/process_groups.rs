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
//! OFF unless `enable()` is called: an interactive goose session keeps today's behaviour, where a
//! terminal Ctrl+C reaches shell children through the shared foreground group. The swarm engine
//! enables it because its workers are exactly the callers whose leftovers must be reapable per
//! attempt. Registration is keyed by session id — each swarm attempt runs in its own session — so a
//! reap of one attempt can never touch a concurrent sibling's processes.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

static ENABLED: AtomicBool = AtomicBool::new(false);

fn registry() -> &'static Mutex<Vec<(String, i32)>> {
    static REGISTRY: OnceLock<Mutex<Vec<(String, i32)>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(Vec::new()))
}

/// Turn own-group spawning ON for this process. One-way and idempotent: the swarm engine calls it
/// on every worker dispatch, and there is no path back because a half-enabled process would mix
/// reapable and unreapable children under the same sessions.
pub fn enable() {
    ENABLED.store(true, Ordering::Relaxed);
}

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
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
}
