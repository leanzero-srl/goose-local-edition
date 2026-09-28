//! Who holds a sidecar's port before it starts, and whether this goose may stop it (Q-240).
//!
//! A start that finds its port taken reads every LISTEN pid on it — its command line, its
//! environment, its launcher chain — and stops the holder only on PROOF that it is an engine a
//! goose sidecar started for this very name and port and that nobody alive supervises: the
//! launcher chain (the holder and its ancestors in its own process group — the `uv` → python pair)
//! ends at init (its goosed is gone) or at this process. Anything else is a named refusal: nothing
//! is signalled and no other port is tried.
//!
//! The proof's middle leg is a marker this crate stamps on every spawn (`GOOSE_SIDECAR=<name>@<base
//! url>`), inherited down the launcher chain (measured 2026-09-28 with `ps -E`: the Rapid-MLX
//! python under `uv tool uvx` carries goosed's environment and the spawn's additions). An engine
//! started by a goose older than the marker carries none and is refused, named, with the pid to stop.
//!
//! Stopping is per pid — the listener first, then each launcher above it — every signal preceded
//! by a re-read of that pid's identity (its start time and command line); SIGTERM, the crate's
//! grace window, then SIGKILL for what outlived it. Never a group: the holder's group leader may be
//! long dead, and its pgid proves nothing about who else carries it.

use std::fmt;

/// The environment variable every sidecar spawn carries: `<name>@<base_url>`.
pub const SIDECAR_MARKER_ENV: &str = "GOOSE_SIDECAR";

/// The value of [`SIDECAR_MARKER_ENV`] a sidecar named `name` serving at `base_url` stamps.
pub fn sidecar_marker(name: &str, base_url: &str) -> String {
    format!("{name}@{base_url}")
}

/// One process as the ownership proof reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HolderProcess {
    pub pid: u32,
    pub parent: Option<u32>,
    /// Its process group (`getpgid`).
    pub group: Option<u32>,
    pub uid: Option<u32>,
    /// Unix seconds; with the pid and the command line, what a signal must still match.
    pub started_at: u64,
    pub argv: Vec<String>,
    /// Its [`SIDECAR_MARKER_ENV`] value; `None` when it carries none.
    pub marker: Option<String>,
}

/// Which of [`ownership_proof`]'s rules a holder failed — what a surface says in its own words
/// (Q-249); `NotOurs::reason` carries the same finding with its pids and values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotOursRule {
    /// The listener, its process group, or a launcher's parent could not be read.
    Unreadable,
    /// The listener is init or this goosed itself.
    InitOrSelf,
    /// It runs as another user.
    OtherUser,
    /// A goose sidecar started it, for another engine or port.
    OtherEngine,
    /// It carries no marker: not started by a goose sidecar, or by a goose older than the marker.
    NoMarker,
    /// The process that started it is alive: another goose on this Mac, or a shell, runs it.
    LiveStarter,
}

impl NotOursRule {
    pub fn as_str(self) -> &'static str {
        match self {
            NotOursRule::Unreadable => "unreadable",
            NotOursRule::InitOrSelf => "initOrSelf",
            NotOursRule::OtherUser => "otherUser",
            NotOursRule::OtherEngine => "otherEngine",
            NotOursRule::NoMarker => "noMarker",
            NotOursRule::LiveStarter => "liveStarter",
        }
    }
}

/// The live process that started a holder, as read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Starter {
    pub pid: u32,
    pub argv: Vec<String>,
}

/// Why a holder is not an engine this goose may stop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotOurs {
    pub rule: NotOursRule,
    pub reason: String,
    /// The live process that started it, when THAT names the step: under
    /// [`NotOursRule::LiveStarter`] another goose (or a shell) runs this marked engine, and the
    /// step is to quit that one; under [`NotOursRule::NoMarker`] a live goose older than the
    /// marker runs it (Q-251: its program is this process's own), and the step is to restart that
    /// goose. An unmarked holder whose starter is anything else carries none.
    pub live_starter: Option<Starter>,
}

impl NotOurs {
    fn because(rule: NotOursRule, reason: String) -> Self {
        Self {
            rule,
            reason,
            live_starter: None,
        }
    }
}

/// One LISTEN pid on the port and the verdict on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortHolder {
    pub pid: u32,
    /// Its command line; empty when it could not be read.
    pub argv: Vec<String>,
    /// `Ok` carries the pids a stop signals — the listener first, then its launchers — as read.
    pub verdict: Result<Vec<HolderProcess>, NotOurs>,
}

impl fmt::Display for PortHolder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let argv = if self.argv.is_empty() {
            "command line unreadable".to_string()
        } else {
            self.argv.join(" ")
        };
        write!(f, "pid {} (`{argv}`) — ", self.pid)?;
        match &self.verdict {
            Ok(chain) => write!(
                f,
                "ours: an engine a goose sidecar started whose goosed is gone (pids {})",
                chain
                    .iter()
                    .map(|p| p.pid.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Err(not_ours) => write!(f, "not this goose's: {}", not_ours.reason),
        }
    }
}

/// The one next step for a port's holders — what a refused start (Q-240), the swarm's events
/// (Q-248/Q-250), a refused Unmount (Q-252) and the Engine panel (Q-249, as the status's
/// `stray_listener_step`) all say. Derived here and nowhere else (Q-251).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NextStep {
    /// Every holder is this goose's own leftover: a start (or an Unmount) stops it first.
    Start,
    /// A goose engine for this port that a live process runs: quit that process.
    QuitStarter { pid: u32 },
    /// An engine a goose older than the marker started, and that goose still runs it: restarted,
    /// it mounts its engine again carrying the marker.
    RestartGoose { pid: u32 },
    /// This process itself listens on the engine port.
    OtherPort,
    /// Nothing proves whose it is and no live goose runs it: stop these pids.
    Kill { pids: Vec<u32> },
}

impl NextStep {
    /// The wire name of the step's kind.
    pub fn kind(&self) -> &'static str {
        match self {
            NextStep::Start => "start",
            NextStep::QuitStarter { .. } => "quitStarter",
            NextStep::RestartGoose { .. } => "restartGoose",
            NextStep::OtherPort => "otherPort",
            NextStep::Kill { .. } => "kill",
        }
    }

    /// The process the step names: the starter to quit or restart.
    pub fn pid(&self) -> Option<u32> {
        match self {
            NextStep::QuitStarter { pid } | NextStep::RestartGoose { pid } => Some(*pid),
            _ => None,
        }
    }

    /// The pids a person stops, for [`NextStep::Kill`].
    pub fn kill_pids(&self) -> &[u32] {
        match self {
            NextStep::Kill { pids } => pids,
            _ => &[],
        }
    }
}

impl fmt::Display for NextStep {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NextStep::Start => {
                f.write_str("start the engine again — the start stops this leftover first")
            }
            NextStep::QuitStarter { pid } => {
                write!(f, "quit what started it (pid {pid}), then start again")
            }
            NextStep::RestartGoose { pid } => write!(
                f,
                "restart the goose that started it (pid {pid}) — it is older than the goose engine \
                 mark, and restarted it mounts its engine again carrying it"
            ),
            NextStep::OtherPort => f.write_str("give the engine another port in its settings"),
            NextStep::Kill { pids } => write!(
                f,
                "stop it per pid (`kill {}`), then start again",
                pids.iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(" ")
            ),
        }
    }
}

/// The step for `holders`, in the order the rules name it: all ours → a start stops them; a live
/// starter → quit it (marked) or restart it (an older goose); this process itself → another port;
/// otherwise the pids no goose runs, to stop. `None` when no holder was named.
pub fn next_step(holders: &[PortHolder]) -> Option<NextStep> {
    if holders.is_empty() {
        return None;
    }
    let not_ours: Vec<&NotOurs> = holders
        .iter()
        .filter_map(|h| h.verdict.as_ref().err())
        .collect();
    if not_ours.is_empty() {
        return Some(NextStep::Start);
    }
    if let Some((rule, starter)) = not_ours
        .iter()
        .find_map(|n| n.live_starter.as_ref().map(|s| (n.rule, s.pid)))
    {
        return Some(match rule {
            NotOursRule::NoMarker => NextStep::RestartGoose { pid: starter },
            _ => NextStep::QuitStarter { pid: starter },
        });
    }
    if not_ours.iter().any(|n| n.rule == NotOursRule::InitOrSelf) {
        return Some(NextStep::OtherPort);
    }
    Some(NextStep::Kill {
        pids: holders
            .iter()
            .filter(|h| h.verdict.is_err())
            .map(|h| h.pid)
            .collect(),
    })
}

/// The holders, each named with its verdict, joined for a sentence.
pub fn named_holders(holders: &[PortHolder]) -> String {
    holders
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ")
}

/// The port is held by something this start may not stop, or that outlived the stop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortHeld {
    pub port: u16,
    pub holders: Vec<PortHolder>,
    /// Set when the holders were ours and were signalled, yet the port still answers.
    pub survived_the_stop: bool,
}

impl fmt::Display for PortHeld {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let named = named_holders(&self.holders);
        let Some(step) = next_step(&self.holders) else {
            return write!(
                f,
                "port {} answers but lsof names no LISTEN pid on it, so who holds it is unknown — \
                 nothing was signalled; find it with `lsof -nP -iTCP:{} -sTCP:LISTEN`, stop it, \
                 then start again",
                self.port, self.port
            );
        };
        if self.survived_the_stop {
            return write!(
                f,
                "port {} is still held after this goose stopped its own leftover engine: {named} — \
                 the process is likely stuck in the kernel (an uninterruptible GPU wait survives \
                 every signal)",
                self.port,
            );
        }
        write!(
            f,
            "port {} is held by {named} — nothing was signalled and no other port is tried; {step}",
            self.port,
        )
    }
}

impl std::error::Error for PortHeld {}

/// What a start that finds its engine's id ALREADY served may do instead of starting one (Q-248):
/// [`ownership_proof`]'s verdicts on the port's holders, read for reuse rather than for a stop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reuse {
    /// Its launcher chain ends at this process: this process's own engine.
    Own,
    /// A goose sidecar's engine for this very name and port that a live process (`starter`)
    /// supervises — a goose window's. Reused, never stopped.
    Supervised { holder: PortHolder, starter: u32 },
    /// Proven a goose sidecar's engine for this port whose starter is gone: nobody supervises it,
    /// so it is never reused — a start stops it ([`claim_port`]).
    Leftover { holders: Vec<PortHolder> },
}

/// The reuse verdict over every LISTEN pid on `port`. No holder named, a holder not proven a goose
/// sidecar's engine for this name and port, or holders that disagree (two engines on one port) is
/// [`PortHeld`]: named, never reused, nothing signalled.
pub fn reuse_verdict(holders: Vec<PortHolder>, port: u16, own_pid: u32) -> Result<Reuse, PortHeld> {
    let Some(first) = holders.first() else {
        return Err(PortHeld {
            port,
            holders,
            survived_the_stop: false,
        });
    };
    if holders.iter().all(|h| h.verdict.is_ok()) {
        let started_here = |h: &PortHolder| {
            h.verdict
                .as_ref()
                .is_ok_and(|chain| chain.last().and_then(|top| top.parent) == Some(own_pid))
        };
        return Ok(if holders.iter().all(started_here) {
            Reuse::Own
        } else {
            Reuse::Leftover { holders }
        });
    }
    // Only a MARKED engine's live starter supervises an engine for this port; an unmarked one's
    // (Q-251, an older goose) is named for its step and never reused.
    let live_starter = |h: &PortHolder| {
        h.verdict
            .as_ref()
            .err()
            .filter(|n| n.rule == NotOursRule::LiveStarter)
            .and_then(|n| n.live_starter.as_ref().map(|s| s.pid))
    };
    match live_starter(first) {
        Some(starter) if holders.iter().all(|h| live_starter(h) == Some(starter)) => {
            Ok(Reuse::Supervised {
                holder: first.clone(),
                starter,
            })
        }
        _ => Err(PortHeld {
            port,
            holders,
            survived_the_stop: false,
        }),
    }
}

/// The proof, as a pure function over what was read. `lineage` is the listener followed by its
/// ancestors, read upward until the first one outside the listener's process group (included) or
/// until the parent is init or `own_pid` (not included). `Ok` is the launcher chain a stop
/// signals: the listener and its same-group ancestors. `Err` names the first rule it fails:
/// 1. it is neither init nor this process;
/// 2. it runs as this user;
/// 3. it carries `GOOSE_SIDECAR=<marker>` — a goose sidecar started it for this name and port;
/// 4. every launcher above it in its group carries the same user and marker, and the chain's top
///    was started by init (its goosed is gone) or by this process. A live starter outside the
///    group is another goose on this Mac, or a person's shell, and is never touched.
///
/// An unmarked holder whose live starter runs `own_program` — this process's own program, so a
/// goose older than the marker (Q-251) — carries that starter, so its step is to restart that
/// goose rather than to kill its engine.
pub fn ownership_proof(
    lineage: &[HolderProcess],
    marker: &str,
    own_uid: u32,
    own_pid: u32,
    own_program: &str,
) -> Result<Vec<HolderProcess>, NotOurs> {
    let Some(listener) = lineage.first() else {
        return Err(NotOurs::because(
            NotOursRule::Unreadable,
            "it could not be read".to_string(),
        ));
    };
    if listener.pid <= 1 || listener.pid == own_pid {
        return Err(NotOurs::because(
            NotOursRule::InitOrSelf,
            format!("pid {} is init or this goosed itself", listener.pid),
        ));
    }
    let marked = |process: &HolderProcess| -> Result<(), NotOurs> {
        if process.uid != Some(own_uid) {
            return Err(NotOurs::because(
                NotOursRule::OtherUser,
                format!(
                    "pid {} runs as uid {}, not this user's {own_uid}",
                    process.pid,
                    process
                        .uid
                        .map_or("unknown".to_string(), |uid| uid.to_string())
                ),
            ));
        }
        match process.marker.as_deref() {
            Some(found) if found == marker => Ok(()),
            Some(found) => Err(NotOurs::because(
                NotOursRule::OtherEngine,
                format!(
                    "pid {} carries {SIDECAR_MARKER_ENV}={found}, a goose sidecar's for another \
                     engine or port, not {marker}",
                    process.pid
                ),
            )),
            None => Err(NotOurs::because(
                NotOursRule::NoMarker,
                format!(
                    "pid {} carries no {SIDECAR_MARKER_ENV} in its environment — every engine a \
                     goose sidecar starts carries {SIDECAR_MARKER_ENV}={marker} (a goose older \
                     than this check stamped none)",
                    process.pid
                ),
            )),
        }
    };
    let older_goose = |mut not_ours: NotOurs| {
        if not_ours.rule == NotOursRule::NoMarker {
            not_ours.live_starter = goose_starter(lineage, own_program);
        }
        not_ours
    };
    marked(listener).map_err(older_goose)?;
    let Some(group) = listener.group else {
        return Err(NotOurs::because(
            NotOursRule::Unreadable,
            format!(
                "the process group of pid {} could not be read",
                listener.pid
            ),
        ));
    };
    let mut chain = Vec::new();
    for process in lineage.iter().take_while(|p| p.group == Some(group)) {
        marked(process).map_err(older_goose)?;
        chain.push(process.clone());
    }
    let top = chain.last().unwrap_or(listener);
    match top.parent {
        Some(1) => Ok(chain),
        Some(parent) if parent == own_pid => Ok(chain),
        Some(parent) => match lineage.iter().find(|p| p.pid == parent) {
            Some(starter) => Err(NotOurs {
                rule: NotOursRule::LiveStarter,
                reason: format!(
                    "the process that started it, pid {parent} (`{}`), is alive — another goose \
                     on this Mac, or a shell, runs it",
                    starter.argv.join(" ")
                ),
                live_starter: Some(Starter {
                    pid: parent,
                    argv: starter.argv.clone(),
                }),
            }),
            None => Err(NotOurs::because(
                NotOursRule::Unreadable,
                format!(
                    "pid {}'s parent, pid {parent}, could not be read (it may have just exited)",
                    top.pid
                ),
            )),
        },
        None => Err(NotOurs::because(
            NotOursRule::Unreadable,
            format!("the parent of pid {} could not be read", top.pid),
        )),
    }
}

/// The program a command line runs: its first word's last path segment.
pub fn program_name(argv: &[String]) -> Option<&str> {
    std::path::Path::new(argv.first()?).file_name()?.to_str()
}

/// The live process that started `lineage`'s listener — the first ancestor outside its process
/// group, read alive by the walk — when it runs `own_program`: a goose, since this goose's own
/// program is the only name a goose is known by here. An ancestor chain that ends at init or at
/// this process names no such starter.
fn goose_starter(lineage: &[HolderProcess], own_program: &str) -> Option<Starter> {
    let group = lineage.first()?.group?;
    let starter = lineage.iter().find(|p| p.group != Some(group))?;
    (program_name(&starter.argv) == Some(own_program)).then(|| Starter {
        pid: starter.pid,
        argv: starter.argv.clone(),
    })
}

/// This process's program name — what [`ownership_proof`] recognises a goose starter by.
pub fn own_program() -> anyhow::Result<String> {
    let exe = std::env::current_exe()
        .map_err(|e| anyhow::anyhow!("reading this process's own program: {e}"))?;
    exe.file_name()
        .and_then(|name| name.to_str())
        .map(str::to_string)
        .ok_or_else(|| anyhow::anyhow!("this process's program {} has no name", exe.display()))
}

#[cfg(unix)]
mod read {
    use super::*;
    use anyhow::{bail, Result};
    use sysinfo::{Pid, ProcessRefreshKind, ProcessStatus, ProcessesToUpdate, System, UpdateKind};

    /// `None` when no live process holds `pid`; a zombie has exited and counts as gone.
    pub fn read_process(pid: u32) -> Option<HolderProcess> {
        let mut sys = System::new();
        let wanted = [Pid::from_u32(pid)];
        sys.refresh_processes_specifics(
            ProcessesToUpdate::Some(&wanted),
            true,
            ProcessRefreshKind::nothing()
                .with_cmd(UpdateKind::Always)
                .with_environ(UpdateKind::Always)
                .with_user(UpdateKind::Always),
        );
        let process = sys.process(wanted[0])?;
        if process.status() == ProcessStatus::Zombie {
            return None;
        }
        let prefix = format!("{SIDECAR_MARKER_ENV}=");
        let group = unsafe { libc::getpgid(pid as libc::pid_t) };
        Some(HolderProcess {
            pid,
            parent: process.parent().map(|parent| parent.as_u32()),
            group: (group > 0).then_some(group as u32),
            uid: process.user_id().map(|uid| **uid),
            started_at: process.start_time(),
            argv: process
                .cmd()
                .iter()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect(),
            marker: process.environ().iter().find_map(|entry| {
                entry
                    .to_string_lossy()
                    .strip_prefix(&prefix)
                    .map(str::to_string)
            }),
        })
    }

    /// The listener, then its ancestors up to the first one outside its group (included), stopping
    /// below init and below `own_pid`.
    pub fn lineage(listener: HolderProcess, own_pid: u32) -> Vec<HolderProcess> {
        let group = listener.group;
        let mut lineage = vec![listener];
        loop {
            let last = lineage.last().expect("the listener is first");
            if last.group != group {
                break;
            }
            let Some(parent) = last.parent.filter(|p| *p > 1 && *p != own_pid) else {
                break;
            };
            if lineage.iter().any(|p| p.pid == parent) {
                break;
            }
            let Some(process) = read_process(parent) else {
                break;
            };
            lineage.push(process);
        }
        lineage
    }

    /// A holder and the processes its verdict was read from — the listener and its ancestors.
    struct Judged {
        holder: PortHolder,
        lineage: Vec<HolderProcess>,
    }

    /// Every LISTEN pid on `port`, read and judged. An unreadable `lsof` is an error, never an
    /// empty port.
    async fn judge_port(port: u16, marker: &str) -> Result<Vec<Judged>> {
        let own_uid = unsafe { libc::getuid() };
        let own_pid = std::process::id();
        let own_program = own_program()?;
        let pids = crate::listening_pids(port).await?;
        Ok(pids
            .into_iter()
            .map(|pid| match read_process(pid) {
                None => Judged {
                    holder: PortHolder {
                        pid,
                        argv: Vec::new(),
                        verdict: Err(NotOurs::because(
                            NotOursRule::Unreadable,
                            format!("pid {pid} could not be read (it may have just exited)"),
                        )),
                    },
                    lineage: Vec::new(),
                },
                Some(listener) => {
                    let argv = listener.argv.clone();
                    let lineage = lineage(listener, own_pid);
                    Judged {
                        holder: PortHolder {
                            pid,
                            argv,
                            verdict: ownership_proof(
                                &lineage,
                                marker,
                                own_uid,
                                own_pid,
                                &own_program,
                            ),
                        },
                        lineage,
                    }
                }
            })
            .collect())
    }

    /// Every LISTEN pid on `port`, read and judged. An unreadable `lsof` is an error, never an
    /// empty port.
    pub async fn inspect_port(port: u16, marker: &str) -> Result<Vec<PortHolder>> {
        Ok(judge_port(port, marker)
            .await?
            .into_iter()
            .map(|judged| judged.holder)
            .collect())
    }

    /// Header constants from `<sys/proc_info.h>` (macOS SDK): what the listener read asks libproc
    /// for, and where in a `struct socket_fdinfo` its answer sits. The offsets are that header's
    /// layout on 64-bit macOS; `listener_pids_agree_with_lsof` checks them against lsof on a real
    /// listener, with a client connection to the same port as the negative control.
    #[cfg(target_os = "macos")]
    mod libproc_abi {
        pub const PROC_UID_ONLY: u32 = 4;
        pub const PROC_PIDFDSOCKETINFO: libc::c_int = 3;
        pub const SOCKET_FDINFO_SIZE: usize = 792;
        /// `psi.soi_kind`: `proc_fileinfo` (24 bytes) + `socket_info.soi_kind` (232).
        pub const SOI_KIND: usize = 256;
        pub const SOCKINFO_TCP: i32 = 2;
        /// `psi.soi_proto.pri_tcp.tcpsi_ini.insi_lport` (network byte order in its low 16 bits).
        pub const INSI_LPORT: usize = 268;
        /// `psi.soi_proto.pri_tcp.tcpsi_state`.
        pub const TCPSI_STATE: usize = 344;
        pub const TSI_S_LISTEN: i32 = 1;
    }

    /// The pids of this user's processes holding a TCP LISTEN socket on `port`, read in-process
    /// through libproc — what `lsof -ti TCP:<port> -sTCP:LISTEN` answers, at the cost of a scan
    /// of this user's file tables instead of a process spawn. Measured 2026-09-28 on this Mac
    /// (1,000 processes, 683 of this user's): 2.9 ms median against lsof's 60 ms, the same 20
    /// (pid, port) LISTEN pairs Mac-wide, a client connection to the port not listed. lsof, run
    /// as this user, sees no other user's sockets either, so the two answer the same set.
    #[cfg(target_os = "macos")]
    pub fn listener_pids(port: u16) -> Result<Vec<u32>> {
        use libproc_abi::*;
        let uid = unsafe { libc::getuid() };
        let wanted = unsafe { libc::proc_listpids(PROC_UID_ONLY, uid, std::ptr::null_mut(), 0) };
        if wanted <= 0 {
            bail!(
                "proc_listpids could not size this user's process list: {}",
                std::io::Error::last_os_error()
            );
        }
        // Room for processes started between the sizing call and the read.
        let mut pids =
            vec![0 as libc::c_int; wanted as usize / std::mem::size_of::<libc::c_int>() * 2];
        let got = unsafe {
            libc::proc_listpids(
                PROC_UID_ONLY,
                uid,
                pids.as_mut_ptr().cast(),
                (pids.len() * std::mem::size_of::<libc::c_int>()) as libc::c_int,
            )
        };
        if got <= 0 {
            bail!(
                "proc_listpids could not list this user's processes: {}",
                std::io::Error::last_os_error()
            );
        }
        pids.truncate(got as usize / std::mem::size_of::<libc::c_int>());
        let fd_size = std::mem::size_of::<libc::proc_fdinfo>();
        let mut socket = vec![0u8; SOCKET_FDINFO_SIZE];
        let mut found = Vec::new();
        for pid in pids.into_iter().filter(|pid| *pid > 0) {
            // A process that exits mid-scan, or one whose table this user may not read, answers
            // nothing: it holds no socket this read can name, exactly as lsof skips it.
            let size = unsafe {
                libc::proc_pidinfo(pid, libc::PROC_PIDLISTFDS, 0, std::ptr::null_mut(), 0)
            };
            if size <= 0 {
                continue;
            }
            let mut fds = vec![
                libc::proc_fdinfo {
                    proc_fd: 0,
                    proc_fdtype: 0
                };
                size as usize / fd_size * 2
            ];
            let size = unsafe {
                libc::proc_pidinfo(
                    pid,
                    libc::PROC_PIDLISTFDS,
                    0,
                    fds.as_mut_ptr().cast(),
                    (fds.len() * fd_size) as libc::c_int,
                )
            };
            if size <= 0 {
                continue;
            }
            fds.truncate(size as usize / fd_size);
            for fd in fds
                .iter()
                .filter(|fd| fd.proc_fdtype == libc::PROX_FDTYPE_SOCKET as u32)
            {
                let read = unsafe {
                    libc::proc_pidfdinfo(
                        pid,
                        fd.proc_fd,
                        PROC_PIDFDSOCKETINFO,
                        socket.as_mut_ptr().cast(),
                        SOCKET_FDINFO_SIZE as libc::c_int,
                    )
                };
                if read as usize != SOCKET_FDINFO_SIZE {
                    continue;
                }
                let int_at = |offset: usize| {
                    i32::from_ne_bytes(socket[offset..offset + 4].try_into().expect("4 bytes"))
                };
                let local_port = u16::from_be((int_at(INSI_LPORT) as u32 & 0xffff) as u16);
                if int_at(SOI_KIND) == SOCKINFO_TCP
                    && int_at(TCPSI_STATE) == TSI_S_LISTEN
                    && local_port == port
                {
                    found.push(pid as u32);
                }
            }
        }
        found.sort_unstable();
        found.dedup();
        Ok(found)
    }

    /// Off macOS the listener set is lsof's own answer — no cheaper read exists here.
    #[cfg(not(target_os = "macos"))]
    pub fn listener_pids(port: u16) -> Result<Vec<u32>> {
        let lsof = crate::resolve_lsof()?;
        let output = std::process::Command::new(&lsof)
            .args(["-ti", &format!("TCP:{port}"), "-sTCP:LISTEN"])
            .output()?;
        let mut pids = crate::listening_pids_of(&lsof, &output)?;
        pids.sort_unstable();
        pids.dedup();
        Ok(pids)
    }

    /// What a cached verdict was read from, per process: a verdict holds while every one of them
    /// is still the same process (its start time) under the same parent.
    #[derive(Debug, Clone, PartialEq, Eq)]
    struct Mark {
        pid: u32,
        started_at: u64,
        parent: Option<u32>,
    }

    fn mark_now(pid: u32) -> Option<Mark> {
        let mut sys = System::new();
        let wanted = [Pid::from_u32(pid)];
        sys.refresh_processes_specifics(
            ProcessesToUpdate::Some(&wanted),
            true,
            ProcessRefreshKind::nothing(),
        );
        let process = sys.process(wanted[0])?;
        (process.status() != ProcessStatus::Zombie).then(|| Mark {
            pid,
            started_at: process.start_time(),
            parent: process.parent().map(|parent| parent.as_u32()),
        })
    }

    struct Cached {
        port: u16,
        marker: String,
        listeners: Vec<u32>,
        marks: Vec<Mark>,
        holders: Vec<PortHolder>,
    }

    /// A port's judged holders, read again only when what they were judged from changes (Q-253):
    /// an Engine panel polls its status every few seconds, and while a leftover holds the port each
    /// poll ran lsof and read every holder's lineage (~0.15 s measured). Between reads only the
    /// port's LISTEN pid set ([`listener_pids`]) and each judged process's start time and parent
    /// are read; any difference — a listener gone or added, a launcher or starter exited, a
    /// process re-parented to init — is a full read. A verdict naming a process that could not be
    /// read is never kept, and neither is a read whose lsof set and libproc set disagree.
    #[derive(Default)]
    pub struct HoldersCache {
        entry: std::sync::Mutex<Option<Cached>>,
        full_reads: std::sync::atomic::AtomicU64,
    }

    impl HoldersCache {
        pub fn new() -> Self {
            Self::default()
        }

        /// How many reads judged the port from scratch.
        pub fn full_reads(&self) -> u64 {
            self.full_reads.load(std::sync::atomic::Ordering::SeqCst)
        }

        /// [`inspect_port`]'s answer, from the cache while nothing it was judged from has changed.
        pub async fn read(&self, port: u16, marker: &str) -> Result<Vec<PortHolder>> {
            let listeners = match listener_pids(port) {
                Ok(listeners) => Some(listeners),
                Err(e) => {
                    tracing::warn!(
                        event = "sidecar_listener_read_failed",
                        port,
                        error = %format!("{e:#}"),
                        "the port's LISTEN pids could not be read in-process; its holders are read \
                         with lsof on every poll until they can"
                    );
                    None
                }
            };
            if let Some(listeners) = &listeners {
                if let Some(holders) = self.still(port, marker, listeners) {
                    return Ok(holders);
                }
            }
            self.full_reads
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let judged = judge_port(port, marker).await;
            let mut entry = self.entry.lock().unwrap_or_else(|e| e.into_inner());
            *entry = None;
            let judged = judged?;
            let holders: Vec<PortHolder> = judged.iter().map(|j| j.holder.clone()).collect();
            let mut judged_pids: Vec<u32> = holders.iter().map(|h| h.pid).collect();
            judged_pids.sort_unstable();
            judged_pids.dedup();
            let readable = judged.iter().all(|j| {
                !j.lineage.is_empty()
                    && j.holder
                        .verdict
                        .as_ref()
                        .err()
                        .is_none_or(|n| n.rule != NotOursRule::Unreadable)
            });
            if readable && listeners.as_ref() == Some(&judged_pids) {
                *entry = Some(Cached {
                    port,
                    marker: marker.to_string(),
                    listeners: judged_pids,
                    marks: judged
                        .iter()
                        .flat_map(|j| &j.lineage)
                        .map(|p| Mark {
                            pid: p.pid,
                            started_at: p.started_at,
                            parent: p.parent,
                        })
                        .collect(),
                    holders: holders.clone(),
                });
            }
            Ok(holders)
        }

        fn still(&self, port: u16, marker: &str, listeners: &[u32]) -> Option<Vec<PortHolder>> {
            let entry = self.entry.lock().unwrap_or_else(|e| e.into_inner());
            let cached = entry.as_ref()?;
            let same = cached.port == port
                && cached.marker == marker
                && cached.listeners == listeners
                && cached
                    .marks
                    .iter()
                    .all(|mark| mark_now(mark.pid).as_ref() == Some(mark));
            same.then(|| cached.holders.clone())
        }
    }

    /// Who serves on `port` for a start that would REUSE the engine there instead of starting its
    /// own (Q-248) — [`reuse_verdict`] over [`inspect_port`]. Nothing is signalled on any arm; a
    /// [`Reuse::Leftover`] is stopped only by a start ([`claim_port`]).
    pub async fn reuse_port(port: u16, marker: &str) -> Result<Reuse> {
        let holders = inspect_port(port, marker).await.map_err(|e| {
            e.context(format!(
                "port {port} answers and who holds it could not be read; nothing was signalled"
            ))
        })?;
        Ok(reuse_verdict(holders, port, std::process::id())?)
    }

    /// Whether `pid` is still exactly the process that was judged.
    fn still(process: &HolderProcess) -> bool {
        read_process(process.pid).is_some_and(|now| {
            now.started_at == process.started_at
                && now.argv == process.argv
                && now.marker == process.marker
        })
    }

    /// What a stop signalled.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Reaped {
        pub pid: u32,
        /// "SIGTERM" when it exited on the TERM, "SIGKILL" when it outlived the grace.
        pub signal: &'static str,
        pub argv: Vec<String>,
    }

    /// Free `port` before a spawn. Nothing accepting → nothing to do. Every holder proven ours
    /// (see [`ownership_proof`]) → each is stopped per pid, its identity re-read before every
    /// signal, and the port waited for. Anything else → [`PortHeld`], nothing signalled.
    pub async fn claim_port(port: u16, marker: &str) -> Result<Vec<Reaped>> {
        if !crate::port_has_listener(port) {
            return Ok(Vec::new());
        }
        let holders = inspect_port(port, marker).await.map_err(|e| {
            e.context(format!(
                "port {port} answers and who holds it could not be read; nothing was signalled"
            ))
        })?;
        if holders.is_empty() {
            if crate::wait_port_clear(port).await {
                return Ok(Vec::new());
            }
            bail!("port {port} answers but lsof names no LISTEN pid on it; nothing was signalled");
        }
        if holders.iter().any(|h| h.verdict.is_err()) {
            return Err(PortHeld {
                port,
                holders,
                survived_the_stop: false,
            }
            .into());
        }
        let mut targets: Vec<HolderProcess> = Vec::new();
        for process in holders
            .iter()
            .filter_map(|h| h.verdict.as_ref().ok())
            .flatten()
        {
            // Two listeners of one launcher share its chain; each pid is signalled once.
            if !targets.iter().any(|t| t.pid == process.pid) {
                targets.push(process.clone());
            }
        }
        let mut reaped = Vec::new();
        let mut left = targets;
        for (signal, name) in [(libc::SIGTERM, "SIGTERM"), (libc::SIGKILL, "SIGKILL")] {
            for process in &left {
                if still(process) {
                    unsafe { libc::kill(process.pid as libc::pid_t, signal) };
                }
            }
            for _ in 0..crate::GRACE_TICKS {
                if left.iter().all(|p| !still(p)) {
                    break;
                }
                tokio::time::sleep(crate::GRACE_TICK).await;
            }
            let (gone, alive): (Vec<_>, Vec<_>) = left.into_iter().partition(|p| !still(p));
            for process in gone {
                tracing::warn!(
                    event = "sidecar_port_orphan_reaped",
                    port,
                    pid = process.pid,
                    signal = name,
                    started_at_unix = process.started_at,
                    argv = %process.argv.join(" "),
                    "stopped an engine a goose sidecar started on this port whose goosed is gone"
                );
                reaped.push(Reaped {
                    pid: process.pid,
                    signal: name,
                    argv: process.argv,
                });
            }
            left = alive;
            if left.is_empty() {
                break;
            }
        }
        if left.is_empty() && crate::wait_port_clear(port).await {
            return Ok(reaped);
        }
        Err(PortHeld {
            port,
            holders,
            survived_the_stop: true,
        }
        .into())
    }
}

#[cfg(unix)]
pub use read::{
    claim_port, inspect_port, listener_pids, read_process, reuse_port, HoldersCache, Reaped,
};

#[cfg(test)]
mod tests {
    use super::*;

    const UID: u32 = 501;
    const GOOSED: u32 = 73403;
    const MARKER: &str = "mlx-engine@http://127.0.0.1:8090";
    /// This process's program, as the packaged goosed runs it (`…/Resources/bin/goose serve`).
    const PROGRAM: &str = "goose";

    /// The live engine measured 2026-09-28: `uv tool uvx …` (35242, its own group, parent goosed
    /// 73403) and the Rapid-MLX python under it (35319, the same group) listening on 8090.
    fn engine() -> HolderProcess {
        HolderProcess {
            pid: 35319,
            parent: Some(35242),
            group: Some(35242),
            uid: Some(UID),
            started_at: 1_790_558_354,
            argv: vec![
                "/Users/me/.cache/uv/archive-v0/U_t/bin/python".to_string(),
                "/Users/me/.cache/uv/archive-v0/U_t/bin/rapid-mlx".to_string(),
                "serve".to_string(),
                "--port".to_string(),
                "8090".to_string(),
            ],
            marker: Some(MARKER.to_string()),
        }
    }

    fn uv(parent: u32) -> HolderProcess {
        HolderProcess {
            pid: 35242,
            parent: Some(parent),
            group: Some(35242),
            uid: Some(UID),
            started_at: 1_790_558_352,
            argv: vec!["/opt/homebrew/bin/uv".to_string(), "tool".to_string()],
            marker: Some(MARKER.to_string()),
        }
    }

    fn goosed(pid: u32) -> HolderProcess {
        HolderProcess {
            pid,
            parent: Some(73384),
            group: Some(pid),
            uid: Some(UID),
            started_at: 1_790_550_000,
            argv: vec!["goose".to_string(), "serve".to_string()],
            marker: None,
        }
    }

    fn pids(chain: &[HolderProcess]) -> Vec<u32> {
        chain.iter().map(|p| p.pid).collect()
    }

    #[test]
    fn a_leftover_pair_whose_goosed_is_gone_is_ours_launcher_and_all() {
        let chain = ownership_proof(&[engine(), uv(1)], MARKER, UID, 999, PROGRAM).unwrap();
        assert_eq!(pids(&chain), [35319, 35242], "the listener first, then uv");

        let orphan = HolderProcess {
            parent: Some(1),
            ..engine()
        };
        let chain = ownership_proof(&[orphan], MARKER, UID, 999, PROGRAM).unwrap();
        assert_eq!(
            pids(&chain),
            [35319],
            "uv died too: the engine alone, its dead leader's pgid still its group"
        );

        let chain = ownership_proof(&[engine(), uv(999)], MARKER, UID, 999, PROGRAM).unwrap();
        assert_eq!(
            pids(&chain),
            [35319, 35242],
            "started by this very goosed and supervised by nothing"
        );
    }

    #[test]
    fn a_live_goosed_above_it_makes_it_another_gooses() {
        let kept = ownership_proof(
            &[engine(), uv(GOOSED), goosed(GOOSED)],
            MARKER,
            UID,
            999,
            PROGRAM,
        )
        .unwrap_err();
        assert_eq!(kept.rule, NotOursRule::LiveStarter);
        assert_eq!(
            kept.live_starter,
            Some(Starter {
                pid: GOOSED,
                argv: vec!["goose".to_string(), "serve".to_string()],
            }),
            "the starter carries its command line, so a surface can say what to quit"
        );
        assert!(
            kept.reason.contains("pid 73403 (`goose serve`)"),
            "{}",
            kept.reason
        );
        assert!(kept.reason.contains("is alive"), "{}", kept.reason);
    }

    #[test]
    fn every_rule_names_itself() {
        let unmarked = HolderProcess {
            marker: None,
            ..engine()
        };
        let kept = ownership_proof(&[unmarked, uv(1)], MARKER, UID, 999, PROGRAM).unwrap_err();
        assert!(
            kept.reason.contains("carries no GOOSE_SIDECAR"),
            "{}",
            kept.reason
        );
        assert_eq!(kept.rule, NotOursRule::NoMarker);

        let elsewhere = HolderProcess {
            marker: Some("mlx-engine@http://127.0.0.1:8091".to_string()),
            ..engine()
        };
        let kept = ownership_proof(&[elsewhere, uv(1)], MARKER, UID, 999, PROGRAM).unwrap_err();
        assert!(kept.reason.contains("127.0.0.1:8091"), "{}", kept.reason);
        assert_eq!(kept.rule, NotOursRule::OtherEngine);

        let unmarked_launcher = HolderProcess {
            marker: None,
            ..uv(1)
        };
        let kept =
            ownership_proof(&[engine(), unmarked_launcher], MARKER, UID, 999, PROGRAM).unwrap_err();
        assert!(
            kept.reason.contains("pid 35242 carries no"),
            "{}",
            kept.reason
        );
        assert_eq!(kept.rule, NotOursRule::NoMarker);

        let other_user = HolderProcess {
            uid: Some(0),
            ..engine()
        };
        let kept = ownership_proof(&[other_user, uv(1)], MARKER, UID, 999, PROGRAM).unwrap_err();
        assert!(kept.reason.contains("uid 0"), "{}", kept.reason);
        assert_eq!(kept.rule, NotOursRule::OtherUser);

        let itself = HolderProcess {
            pid: 999,
            ..engine()
        };
        let kept = ownership_proof(&[itself], MARKER, UID, 999, PROGRAM).unwrap_err();
        assert_eq!(kept.rule, NotOursRule::InitOrSelf);
        let kept = ownership_proof(&[], MARKER, UID, 999, PROGRAM).unwrap_err();
        assert_eq!(kept.rule, NotOursRule::Unreadable);

        let unknown_parent = HolderProcess {
            parent: None,
            ..uv(1)
        };
        let kept =
            ownership_proof(&[engine(), unknown_parent], MARKER, UID, 999, PROGRAM).unwrap_err();
        assert!(kept.reason.contains("could not be read"), "{}", kept.reason);
        assert_eq!(kept.rule, NotOursRule::Unreadable);

        let kept = ownership_proof(&[engine()], MARKER, UID, 999, PROGRAM).unwrap_err();
        assert!(
            kept.reason.contains("pid 35242, could not be read"),
            "a parent that could not be read is never taken for a live starter: {}",
            kept.reason
        );
        assert_eq!(kept.live_starter, None);
        assert_eq!(kept.rule, NotOursRule::Unreadable);

        let groupless = HolderProcess {
            group: None,
            parent: Some(1),
            ..engine()
        };
        let kept = ownership_proof(&[groupless], MARKER, UID, 999, PROGRAM).unwrap_err();
        assert!(kept.reason.contains("process group"), "{}", kept.reason);
        assert_eq!(kept.rule, NotOursRule::Unreadable);
    }

    #[test]
    fn a_refusal_names_every_holder_and_the_one_step_that_fits() {
        let held = PortHeld {
            port: 8090,
            holders: vec![PortHolder {
                pid: 35319,
                argv: engine().argv,
                verdict: ownership_proof(
                    &[engine(), uv(GOOSED), goosed(GOOSED)],
                    MARKER,
                    UID,
                    1,
                    PROGRAM,
                ),
            }],
            survived_the_stop: false,
        };
        let text = held.to_string();
        assert!(
            text.starts_with("port 8090 is held by pid 35319 (`/Users/me"),
            "{text}"
        );
        assert!(
            text.contains("rapid-mlx serve --port 8090`) — not this goose's"),
            "{text}"
        );
        assert!(text.contains("nothing was signalled"), "{text}");
        assert!(text.contains("quit what started it (pid 73403)"), "{text}");
        assert!(
            !text.contains("kill"),
            "a live starter's engine is never the thing to kill: {text}"
        );

        let unmarked = PortHeld {
            port: 8090,
            holders: vec![PortHolder {
                pid: 35319,
                argv: Vec::new(),
                verdict: Err(NotOurs::because(
                    NotOursRule::Unreadable,
                    "pid 35319 could not be read".to_string(),
                )),
            }],
            survived_the_stop: false,
        };
        let text = unmarked.to_string();
        assert!(text.contains("command line unreadable"), "{text}");
        assert!(text.contains("`kill 35319`"), "{text}");
    }

    fn holder(lineage: &[HolderProcess], own_pid: u32) -> PortHolder {
        PortHolder {
            pid: lineage[0].pid,
            argv: lineage[0].argv.clone(),
            verdict: ownership_proof(lineage, MARKER, UID, own_pid, PROGRAM),
        }
    }

    #[test]
    fn a_reuse_takes_only_this_processs_engine_or_one_a_live_goose_supervises() {
        let own = reuse_verdict(vec![holder(&[engine(), uv(999)], 999)], 8090, 999).unwrap();
        assert_eq!(own, Reuse::Own);

        let window = holder(&[engine(), uv(GOOSED), goosed(GOOSED)], 999);
        let Reuse::Supervised { holder: h, starter } =
            reuse_verdict(vec![window.clone(), window], 8090, 999).unwrap()
        else {
            panic!("a live goose's engine is shared");
        };
        assert_eq!((h.pid, starter), (35319, GOOSED));

        let Reuse::Leftover { holders } =
            reuse_verdict(vec![holder(&[engine(), uv(1)], 999)], 8090, 999).unwrap()
        else {
            panic!("an engine whose goose is gone is never reused");
        };
        assert_eq!(holders[0].pid, 35319);
    }

    #[test]
    fn a_reuse_refuses_by_name_what_is_unproven_or_disagrees() {
        let unmarked = HolderProcess {
            marker: None,
            ..engine()
        };
        let held = reuse_verdict(vec![holder(&[unmarked, uv(1)], 999)], 8090, 999).unwrap_err();
        let text = held.to_string();
        assert!(text.contains("carries no GOOSE_SIDECAR"), "{text}");
        assert!(text.contains("`kill 35319`"), "{text}");

        let window = holder(&[engine(), uv(GOOSED), goosed(GOOSED)], 999);
        let leftover = holder(&[engine(), uv(1)], 999);
        assert!(
            reuse_verdict(vec![window, leftover], 8090, 999).is_err(),
            "two engines on one port are no engine to reuse"
        );

        let nobody = reuse_verdict(Vec::new(), 8090, 999)
            .unwrap_err()
            .to_string();
        assert!(
            nobody.contains("lsof names no LISTEN pid") && nobody.contains("nothing was signalled"),
            "{nobody}"
        );
    }

    fn unmarked(process: HolderProcess) -> HolderProcess {
        HolderProcess {
            marker: None,
            ..process
        }
    }

    /// Q-251: an engine a goose older than the marker mounted — no marker on it or on its `uv`,
    /// that goose still running above them — is told to restart that goose, which mounts it again
    /// marked; never `kill <pid>`, and never reused by a swarm.
    #[test]
    fn an_unmarked_engine_a_live_goose_runs_is_told_to_restart_that_goose() {
        let lineage = [unmarked(engine()), unmarked(uv(GOOSED)), goosed(GOOSED)];
        let kept = ownership_proof(&lineage, MARKER, UID, 999, PROGRAM).unwrap_err();
        assert_eq!(kept.rule, NotOursRule::NoMarker, "the rule it failed stays");
        assert_eq!(kept.live_starter.as_ref().map(|s| s.pid), Some(GOOSED));
        let held = PortHeld {
            port: 8090,
            holders: vec![holder(&lineage, 999)],
            survived_the_stop: false,
        };
        assert_eq!(
            next_step(&held.holders),
            Some(NextStep::RestartGoose { pid: GOOSED })
        );
        let text = held.to_string();
        assert!(
            text.contains("restart the goose that started it (pid 73403)"),
            "{text}"
        );
        assert!(!text.contains("kill"), "{text}");
        assert!(
            reuse_verdict(held.holders.clone(), 8090, 999).is_err(),
            "an unmarked engine is never shared, whoever runs it"
        );
    }

    /// The same unmarked engine under a live process that is NOT a goose (a person's shell), or
    /// orphaned to init, keeps the kill step: nothing alive that a restart would help runs it.
    #[test]
    fn an_unmarked_engine_nothing_gooseish_runs_keeps_the_kill_step() {
        let shell = HolderProcess {
            argv: vec!["-zsh".to_string()],
            ..goosed(GOOSED)
        };
        for lineage in [
            vec![unmarked(engine()), unmarked(uv(GOOSED)), shell],
            vec![unmarked(engine()), unmarked(uv(1))],
        ] {
            let held = vec![holder(&lineage, 999)];
            assert_eq!(held[0].verdict.as_ref().unwrap_err().live_starter, None);
            assert_eq!(next_step(&held), Some(NextStep::Kill { pids: vec![35319] }));
        }
        let goose_program_elsewhere = ownership_proof(
            &[unmarked(engine()), unmarked(uv(GOOSED)), goosed(GOOSED)],
            MARKER,
            UID,
            999,
            "goose-cli",
        )
        .unwrap_err();
        assert_eq!(
            goose_program_elsewhere.live_starter, None,
            "a starter is a goose only when it runs this process's own program"
        );
    }

    /// The step for every shape, from one derivation.
    #[test]
    fn the_step_follows_the_rules_in_order() {
        assert_eq!(next_step(&[]), None);
        assert_eq!(
            next_step(&[holder(&[engine(), uv(1)], 999)]),
            Some(NextStep::Start)
        );
        assert_eq!(
            next_step(&[holder(&[engine(), uv(GOOSED), goosed(GOOSED)], 999)]),
            Some(NextStep::QuitStarter { pid: GOOSED })
        );
        let itself = holder(
            &[HolderProcess {
                pid: 999,
                ..engine()
            }],
            999,
        );
        assert_eq!(next_step(&[itself]), Some(NextStep::OtherPort));
        let two = [
            holder(&[engine(), uv(1)], 999),
            holder(
                &[
                    HolderProcess {
                        pid: 35320,
                        ..unmarked(engine())
                    },
                    uv(1),
                ],
                999,
            ),
        ];
        assert_eq!(
            next_step(&two),
            Some(NextStep::Kill { pids: vec![35320] }),
            "what a start would stop itself is left to it"
        );
    }
}
