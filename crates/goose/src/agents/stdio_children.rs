//! Every stdio extension child goose spawns dies with what owns it (Q-138).
//!
//! Measured 2026-09-26 on the owner's MacBook: eight bundled `leanzero-web-search` processes were
//! orphaned (PPID 1), six at 100% CPU for up to 3.4 days. Two things let that happen here:
//!
//! 1. `goose serve` ends with `std::process::exit` after its signal teardown, so no destructor runs
//!    — rmcp's `ChildWithCleanup` (the only thing that killed a stdio child) never fired. On any
//!    other exit its drop `tokio::spawn`s the kill, which a shutting-down runtime never polls.
//! 2. `configure_subprocess` puts every stdio child in a process group of its own, so the desktop's
//!    group signal to goosed never reaches it, and macOS has no parent-death signal.
//!
//! So: each stdio child is registered here at spawn with its identity (parent + start time). Its
//! guard lives inside the `McpClient`; dropping the client (session close, extension removal,
//! reload) sends SIGTERM to the child's PROVEN own group and escalates to SIGKILL only on proof it
//! ignored the TERM. `teardown_all` does the same for every live child on goosed's way out. A crash
//! can still orphan children, so `spawn_startup_reaper` runs at startup and stops, per pid,
//! orphans that provably run one of goose's own bundled-MCP entries.
//!
//! Gate 4: a group is signalled only when `goose_sidecar::owns_process_group` proves it (the pid is
//! the live leader of its own group, and that group is not ours) AND the pid still carries the
//! identity recorded at spawn — the group is one this goose created. Orphans from an earlier goose
//! are signalled per pid, never by group. The one grace window is goose-sidecar's
//! (`GRACE_TICKS` × `GRACE_TICK`); it bounds process teardown, never model work.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use goose_sidecar::{GRACE_TICK, GRACE_TICKS};
use serde::Deserialize;
use sysinfo::{Pid, ProcessRefreshKind, ProcessStatus, ProcessesToUpdate, System, UpdateKind};
use tracing::{info, warn};

/// What a pid was when goose registered it. A later pid with a different parent or start time is a
/// different process, and is never signalled on this record's authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Identity {
    parent: Option<u32>,
    start_time: u64,
}

/// `None` when no live process holds `pid`; a zombie counts as gone (it has exited, and signalling
/// its pid reaches nothing).
fn identity(pid: u32) -> Option<Identity> {
    let mut sys = System::new();
    let wanted = [Pid::from_u32(pid)];
    sys.refresh_processes_specifics(
        ProcessesToUpdate::Some(&wanted),
        true,
        ProcessRefreshKind::nothing(),
    );
    let process = sys.process(wanted[0])?;
    if process.status() == ProcessStatus::Zombie {
        return None;
    }
    Some(Identity {
        parent: process.parent().map(|p| p.as_u32()),
        start_time: process.start_time(),
    })
}

#[derive(Debug, Clone)]
struct Registered {
    pid: u32,
    name: String,
    identity: Identity,
}

impl Registered {
    fn alive(&self) -> bool {
        identity(self.pid) == Some(self.identity)
    }
}

fn registry() -> &'static Mutex<HashMap<u32, Registered>> {
    static LIVE: OnceLock<Mutex<HashMap<u32, Registered>>> = OnceLock::new();
    LIVE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn take(pid: u32) -> Option<Registered> {
    registry()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .remove(&pid)
}

fn take_all() -> Vec<Registered> {
    registry()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .drain()
        .map(|(_, child)| child)
        .collect()
}

/// The pids of every stdio extension child goose currently holds.
pub fn registered_pids() -> Vec<u32> {
    registry()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .keys()
        .copied()
        .collect()
}

/// Held by the `McpClient` of a stdio extension; dropping it tears the child down.
#[derive(Debug)]
pub struct StdioChildGuard {
    pid: u32,
}

impl StdioChildGuard {
    pub fn pid(&self) -> u32 {
        self.pid
    }
}

/// Register a stdio child goose just spawned. `None` when the child is already gone (nothing is
/// left to guard; the caller's connect reports why).
pub fn register(pid: u32, name: &str) -> Option<StdioChildGuard> {
    let identity = identity(pid)?;
    registry()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(
            pid,
            Registered {
                pid,
                name: name.to_string(),
                identity,
            },
        );
    Some(StdioChildGuard { pid })
}

/// One process of a child's group, as it was when the group was proven ours and signalled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Member {
    pid: u32,
    start_time: u64,
}

/// Every live process whose group is `pgid`, read from the process table.
#[cfg(unix)]
fn group_members(pgid: u32) -> Vec<Member> {
    let mut sys = System::new();
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing());
    sys.processes()
        .iter()
        .filter(|(_, process)| process.status() != ProcessStatus::Zombie)
        .filter(|(pid, _)| unsafe { libc::getpgid(pid.as_u32() as libc::pid_t) } == pgid as i32)
        .map(|(pid, process)| Member {
            pid: pid.as_u32(),
            start_time: process.start_time(),
        })
        .collect()
}

/// A child that has been sent SIGTERM, with the members of its group recorded at that moment.
///
/// The group is signalled as a group only while its leader is proven (`owns_process_group` on a
/// pid that still carries the identity goose recorded at spawn). Once the leader is gone — an MCP
/// that died while a grandchild it launched (a browser) ignored the TERM, measured in this module's
/// test: rmcp SIGKILLs the leader pid alone — the group can no longer be proven by its leader, so
/// the escalation reaches each RECORDED member per pid, and only while that pid still carries the
/// start time it had AND still sits in the child's group.
struct Stopping {
    child: Registered,
    members: Vec<Member>,
}

impl Stopping {
    #[cfg(unix)]
    fn start(child: Registered) -> Option<Self> {
        if !child.alive() {
            return None;
        }
        let members = if goose_sidecar::owns_process_group(child.pid) {
            let members = group_members(child.pid);
            unsafe { libc::killpg(child.pid as libc::pid_t, libc::SIGTERM) };
            members
        } else {
            unsafe { libc::kill(child.pid as libc::pid_t, libc::SIGTERM) };
            vec![Member {
                pid: child.pid,
                start_time: child.identity.start_time,
            }]
        };
        Some(Self { child, members })
    }

    #[cfg(not(unix))]
    fn start(_child: Registered) -> Option<Self> {
        None
    }

    #[cfg(unix)]
    fn still_member(&self, member: &Member) -> bool {
        identity(member.pid).map(|id| id.start_time) == Some(member.start_time)
            && unsafe { libc::getpgid(member.pid as libc::pid_t) } == self.child.pid as libc::pid_t
    }

    #[cfg(not(unix))]
    fn still_member(&self, _member: &Member) -> bool {
        false
    }

    fn remaining(&self) -> Vec<Member> {
        self.members
            .iter()
            .filter(|member| self.still_member(member))
            .copied()
            .collect()
    }

    /// SIGKILL whatever is left: the whole group while its leader is proven, else each recorded
    /// member per pid. Returns the pids that were still running.
    #[cfg(unix)]
    fn kill(&self) -> Vec<u32> {
        let remaining = self.remaining();
        if self.child.alive() && goose_sidecar::owns_process_group(self.child.pid) {
            unsafe { libc::killpg(self.child.pid as libc::pid_t, libc::SIGKILL) };
        } else {
            for member in &remaining {
                unsafe { libc::kill(member.pid as libc::pid_t, libc::SIGKILL) };
            }
        }
        remaining.iter().map(|member| member.pid).collect()
    }

    #[cfg(not(unix))]
    fn kill(&self) -> Vec<u32> {
        Vec::new()
    }

    fn report_ignored(&self, survivors: &[u32]) {
        warn!(
            event = "stdio_extension_ignored_sigterm",
            pid = self.child.pid,
            extension = %self.child.name,
            ?survivors,
            "stdio extension (or a process in its group) ignored SIGTERM through the grace window; SIGKILL sent"
        );
    }
}

fn escalate_blocking(stopping: Stopping) {
    for _ in 0..GRACE_TICKS {
        if stopping.remaining().is_empty() {
            info!(pid = stopping.child.pid, extension = %stopping.child.name, "stdio extension exited on SIGTERM");
            return;
        }
        std::thread::sleep(GRACE_TICK);
    }
    let survivors = stopping.kill();
    if !survivors.is_empty() {
        stopping.report_ignored(&survivors);
    }
}

impl Drop for StdioChildGuard {
    fn drop(&mut self) {
        let Some(child) = take(self.pid) else {
            return;
        };
        let Some(stopping) = Stopping::start(child) else {
            return;
        };
        // A plain thread, not a tokio task: this drop can run while the runtime shuts down, when a
        // spawned task would never be polled — the exact way rmcp's own cleanup was lost.
        std::thread::spawn(move || escalate_blocking(stopping));
    }
}

/// Stop every stdio extension child goose holds: SIGTERM each proven group, one shared grace
/// window, SIGKILL whatever provably ignored it. One line for goosed's teardown report.
pub async fn teardown_all() -> String {
    let children = take_all();
    if children.is_empty() {
        return "no stdio extension children were running".to_string();
    }
    let total = children.len();
    let stopping: Vec<Stopping> = children.into_iter().filter_map(Stopping::start).collect();
    let already_gone = total - stopping.len();
    for _ in 0..GRACE_TICKS {
        if stopping.iter().all(|s| s.remaining().is_empty()) {
            break;
        }
        tokio::time::sleep(GRACE_TICK).await;
    }
    let killed: Vec<String> = stopping
        .iter()
        .filter_map(|s| {
            let survivors = s.kill();
            if survivors.is_empty() {
                return None;
            }
            s.report_ignored(&survivors);
            Some(format!(
                "{} (pid {}, group survivors {:?})",
                s.child.name, s.child.pid, survivors
            ))
        })
        .collect();
    let exited = total - already_gone - killed.len();
    let mut outcome = format!(
        "{total} stdio extension child(ren): {exited} exited on SIGTERM, {already_gone} already gone"
    );
    if !killed.is_empty() {
        outcome.push_str(&format!(
            ", {} SIGKILLed after ignoring SIGTERM: {}",
            killed.len(),
            killed.join(", ")
        ));
    }
    outcome
}

// ---- startup reaper -------------------------------------------------------------------------

/// The desktop's bundled-MCP pins, compiled in from the one file the bundle script reads.
const BUNDLED_MCPS_JSON: &str = include_str!("../../../../ui/desktop/scripts/bundled-mcps.json");

#[derive(Deserialize)]
struct BundledPin {
    id: String,
    entry: String,
}

/// `bundled-mcps/<id>/<entry>` for every MCP the desktop bundles — the tail every install and dev
/// tree runs them from (`<Resources or ui/desktop>/bundled-mcps/<id>/<entry>`).
pub fn bundled_mcp_scripts() -> anyhow::Result<Vec<PathBuf>> {
    let pins: Vec<BundledPin> = serde_json::from_str(BUNDLED_MCPS_JSON)?;
    Ok(pins
        .into_iter()
        .map(|pin| Path::new("bundled-mcps").join(pin.id).join(pin.entry))
        .collect())
}

/// One process as the reaper sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessRow {
    pub pid: u32,
    pub parent: Option<u32>,
    pub uid: Option<u32>,
    pub start_time: u64,
    pub argv: Vec<String>,
}

/// An orphan that provably runs one of goose's bundled-MCP entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GooseOrphan {
    pub row: ProcessRow,
    pub script: PathBuf,
}

/// The proof, as a pure function: reparented to init/launchd (PPID 1), owned by `uid`, and an
/// argument after the interpreter is a path ending in one of `scripts` (component-wise).
pub fn select_goose_orphans(
    rows: &[ProcessRow],
    scripts: &[PathBuf],
    uid: u32,
) -> Vec<GooseOrphan> {
    rows.iter()
        .filter(|row| row.parent == Some(1) && row.uid == Some(uid))
        .filter_map(|row| {
            let script = row.argv.iter().skip(1).find_map(|arg| {
                let arg = Path::new(arg);
                scripts
                    .iter()
                    .find(|script| arg.is_absolute() && arg.ends_with(script))
                    .map(|_| arg.to_path_buf())
            })?;
            Some(GooseOrphan {
                row: row.clone(),
                script,
            })
        })
        .collect()
}

fn process_table() -> Vec<ProcessRow> {
    let mut sys = System::new();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing()
            .with_cmd(UpdateKind::Always)
            .with_user(UpdateKind::Always),
    );
    sys.processes()
        .iter()
        .filter(|(_, process)| process.status() != ProcessStatus::Zombie)
        .map(|(pid, process)| ProcessRow {
            pid: pid.as_u32(),
            parent: process.parent().map(|p| p.as_u32()),
            uid: process.user_id().map(|uid| **uid),
            start_time: process.start_time(),
            argv: process
                .cmd()
                .iter()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect(),
        })
        .collect()
}

/// The orphan still is what the scan saw: same pid, still PPID 1, same start time.
fn still_the_orphan(orphan: &GooseOrphan) -> bool {
    identity(orphan.row.pid)
        == Some(Identity {
            parent: Some(1),
            start_time: orphan.row.start_time,
        })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reaped {
    pub pid: u32,
    pub script: PathBuf,
    /// "SIGTERM" when it exited on the TERM, "SIGKILL" when it ignored the TERM through the grace.
    pub signal: &'static str,
}

/// Stop, per pid, every orphan that provably runs one of `scripts`: SIGTERM, one shared grace
/// window, SIGKILL only for those that provably ignored it. One `orphan_mcp_reaped` event per reap.
#[cfg(unix)]
pub async fn reap_orphans_running(scripts: &[PathBuf]) -> Vec<Reaped> {
    let uid = unsafe { libc::getuid() };
    let orphans = select_goose_orphans(&process_table(), scripts, uid);
    let mut termed = Vec::new();
    for orphan in orphans {
        if still_the_orphan(&orphan) {
            unsafe { libc::kill(orphan.row.pid as libc::pid_t, libc::SIGTERM) };
            termed.push(orphan);
        }
    }
    let mut pending: Vec<&GooseOrphan> = termed.iter().collect();
    for _ in 0..GRACE_TICKS {
        pending.retain(|orphan| still_the_orphan(orphan));
        if pending.is_empty() {
            break;
        }
        tokio::time::sleep(GRACE_TICK).await;
    }
    pending.retain(|orphan| still_the_orphan(orphan));
    let ignored: Vec<u32> = pending.iter().map(|orphan| orphan.row.pid).collect();
    for pid in &ignored {
        unsafe { libc::kill(*pid as libc::pid_t, libc::SIGKILL) };
    }
    termed
        .into_iter()
        .map(|orphan| {
            let signal = if ignored.contains(&orphan.row.pid) {
                "SIGKILL"
            } else {
                "SIGTERM"
            };
            warn!(
                event = "orphan_mcp_reaped",
                pid = orphan.row.pid,
                script = %orphan.script.display(),
                started_at_unix = orphan.row.start_time,
                signal,
                "stopped an orphaned bundled MCP left behind by an earlier goose"
            );
            Reaped {
                pid: orphan.row.pid,
                script: orphan.script,
                signal,
            }
        })
        .collect()
}

/// The startup reaper over goose's own bundled-MCP entries, run in the background so it never
/// delays boot. Every outcome is logged, including "none found" and an unreadable catalog.
#[cfg(unix)]
pub fn spawn_startup_reaper() {
    tokio::spawn(async {
        let scripts = match bundled_mcp_scripts() {
            Ok(scripts) => scripts,
            Err(error) => {
                warn!(
                    event = "orphan_mcp_reaper_unarmed",
                    %error,
                    "the bundled-MCP catalog did not parse; orphaned MCPs were not looked for"
                );
                return;
            }
        };
        let reaped = reap_orphans_running(&scripts).await;
        info!(
            reaped = reaped.len(),
            "orphaned bundled-MCP scan at startup complete"
        );
    });
}

#[cfg(not(unix))]
pub fn spawn_startup_reaper() {}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(pid: u32, parent: u32, uid: u32, argv: &[&str]) -> ProcessRow {
        ProcessRow {
            pid,
            parent: Some(parent),
            uid: Some(uid),
            start_time: 1,
            argv: argv.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn the_catalog_is_the_desktops_bundled_mcps() {
        let scripts = bundled_mcp_scripts().expect("bundled-mcps.json parses");
        assert!(scripts.contains(&PathBuf::from(
            "bundled-mcps/leanzero-web-search/dist/index.js"
        )));
        assert!(scripts.contains(&PathBuf::from(
            "bundled-mcps/leanzero-documents/src/index.js"
        )));
    }

    #[test]
    fn only_provable_goose_orphans_are_selected() {
        let scripts = bundled_mcp_scripts().unwrap();
        let installed = "/Applications/Goose Swarm.app/Contents/Resources/bundled-mcps/leanzero-web-search/dist/index.js";
        let dev =
            "/Users/me/Projects/goose/ui/desktop/bundled-mcps/leanzero-documents/src/index.js";
        let rows = vec![
            // measured shapes: hermit node on the installed app, Electron on the dev tree
            row(10, 1, 501, &["/x/node-24.21.0/bin/node", installed]),
            row(
                11,
                1,
                501,
                &["/x/Electron.app/Contents/MacOS/Electron", dev],
            ),
            // a live goose's child — not an orphan
            row(12, 4242, 501, &["node", installed]),
            // another user's orphan
            row(13, 1, 502, &["node", installed]),
            // the Studio's own web-search service, not a bundled entry
            row(
                14,
                1,
                501,
                &["node", "/Users/me/Projects/mcp-web-search/dist/index.js"],
            ),
            // the path only as argv[0] (not a script argument)
            row(15, 1, 501, &[installed]),
            // a relative path proves nothing about which tree it is
            row(
                16,
                1,
                501,
                &["node", "bundled-mcps/leanzero-web-search/dist/index.js"],
            ),
            // same id, a file that is not the entry
            row(
                17,
                1,
                501,
                &["node", "/a/bundled-mcps/leanzero-web-search/dist/other.js"],
            ),
        ];
        let picked: Vec<u32> = select_goose_orphans(&rows, &scripts, 501)
            .into_iter()
            .map(|o| o.row.pid)
            .collect();
        assert_eq!(picked, vec![10, 11]);
    }
}
