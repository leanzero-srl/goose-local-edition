//! Which goose process runs a loop's clock, and the proof that a recorded owner is gone (design
//! §5.1). The owner is `{goosed_pid, goosed_started_at, app_pid}`: `app_pid` is goosed's parent —
//! the app — at the moment of the claim. The recorded owner is GONE when any of:
//!
//! 1. its pid is not alive (or is a zombie);
//! 2. its pid is alive with another start time (the pid was reused);
//! 3. its pid is alive but its parent is no longer `app_pid`: the app quit and left it orphaned
//!    (Q-223 measured this on every quit and update — reparented to launchd, ppid 1), so the
//!    renderer that fired its ticks is gone with the app.
//!
//! Anything this process cannot read is `Unproven`, never "gone" and never "live".

use goose_sdk_types::custom_requests::LoopOwner;

use super::rules::OwnerProof;

/// What the proof reads about another process.
pub trait ProcessTable: Send + Sync {
    /// `Err(why)` = proven gone (no such pid, a zombie, or another process under that pid).
    fn alive(&self, pid: u32, started_at: u64) -> Result<(), String>;
    /// The process's parent now; `None` when this process cannot read it.
    fn parent(&self, pid: u32) -> Option<u32>;
}

/// The real process table: `goose_sidecar::machine::prove` (signal 0, zombie and start time) and
/// the kernel's parent pid.
pub struct SystemProcesses;

impl ProcessTable for SystemProcesses {
    fn alive(&self, pid: u32, started_at: u64) -> Result<(), String> {
        match goose_sidecar::machine::prove(pid, started_at) {
            goose_sidecar::machine::Liveness::Alive => Ok(()),
            goose_sidecar::machine::Liveness::Gone(why) => Err(why),
        }
    }

    fn parent(&self, pid: u32) -> Option<u32> {
        use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
        let mut sys = System::new();
        let wanted = [Pid::from_u32(pid)];
        sys.refresh_processes_specifics(
            ProcessesToUpdate::Some(&wanted),
            true,
            ProcessRefreshKind::nothing(),
        );
        sys.process(wanted[0])
            .and_then(|p| p.parent())
            .map(|p| p.as_u32())
    }
}

/// This process as a loop owner: its pid, its start, and its parent now (the app that spawned it).
pub fn this_process(processes: &dyn ProcessTable) -> Result<LoopOwner, String> {
    let pid = std::process::id();
    let (started_at, _) = goose_sidecar::machine::process_start(pid)
        .ok_or_else(|| format!("the kernel names no start time for this goose (pid {pid})"))?;
    let app_pid = processes
        .parent(pid)
        .ok_or_else(|| format!("the kernel names no parent for this goose (pid {pid})"))?;
    Ok(LoopOwner {
        goosed_pid: pid,
        goosed_started_at: started_at,
        app_pid,
    })
}

/// Whether `owner` is this process (`me`), another live one still under its app, or proven gone.
pub fn prove(owner: &LoopOwner, me: &LoopOwner, processes: &dyn ProcessTable) -> OwnerProof {
    if owner == me {
        return OwnerProof::ThisProcess;
    }
    let pid = owner.goosed_pid;
    if let Err(why) = processes.alive(pid, owner.goosed_started_at) {
        return OwnerProof::Gone { why };
    }
    match processes.parent(pid) {
        Some(parent) if parent == owner.app_pid => OwnerProof::Live,
        Some(parent) => OwnerProof::Gone {
            why: format!(
                "goose (pid {pid}) outlived its app (pid {}): its parent is now pid {parent}",
                owner.app_pid
            ),
        },
        None => OwnerProof::Unproven {
            why: format!("the parent of goose (pid {pid}) cannot be read"),
        },
    }
}
