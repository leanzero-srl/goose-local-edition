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

/// Why a holder is not an engine this goose may stop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotOurs {
    pub reason: String,
    /// The live process that started it, when THAT is the reason: another goose (or a shell)
    /// runs it, and the step is to stop that one.
    pub live_starter: Option<u32>,
}

impl NotOurs {
    fn because(reason: String) -> Self {
        Self {
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
        let named: Vec<String> = self.holders.iter().map(ToString::to_string).collect();
        if self.survived_the_stop {
            return write!(
                f,
                "port {} is still held after this goose stopped its own leftover engine: {} — \
                 the process is likely stuck in the kernel (an uninterruptible GPU wait survives \
                 every signal)",
                self.port,
                named.join("; ")
            );
        }
        let step = match self
            .holders
            .iter()
            .find_map(|h| h.verdict.as_ref().err().and_then(|n| n.live_starter))
        {
            Some(starter) => format!("quit what started it (pid {starter}), then start again"),
            None => format!(
                "stop it per pid (`kill {}`), then start again",
                self.holders
                    .iter()
                    .map(|h| h.pid.to_string())
                    .collect::<Vec<_>>()
                    .join(" ")
            ),
        };
        write!(
            f,
            "port {} is held by {} — nothing was signalled and no other port is tried; {step}",
            self.port,
            named.join("; ")
        )
    }
}

impl std::error::Error for PortHeld {}

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
pub fn ownership_proof(
    lineage: &[HolderProcess],
    marker: &str,
    own_uid: u32,
    own_pid: u32,
) -> Result<Vec<HolderProcess>, NotOurs> {
    let Some(listener) = lineage.first() else {
        return Err(NotOurs::because("it could not be read".to_string()));
    };
    if listener.pid <= 1 || listener.pid == own_pid {
        return Err(NotOurs::because(format!(
            "pid {} is init or this goosed itself",
            listener.pid
        )));
    }
    let marked = |process: &HolderProcess| -> Result<(), NotOurs> {
        if process.uid != Some(own_uid) {
            return Err(NotOurs::because(format!(
                "pid {} runs as uid {}, not this user's {own_uid}",
                process.pid,
                process
                    .uid
                    .map_or("unknown".to_string(), |uid| uid.to_string())
            )));
        }
        match process.marker.as_deref() {
            Some(found) if found == marker => Ok(()),
            Some(found) => Err(NotOurs::because(format!(
                "pid {} carries {SIDECAR_MARKER_ENV}={found}, a goose sidecar's for another \
                 engine or port, not {marker}",
                process.pid
            ))),
            None => Err(NotOurs::because(format!(
                "pid {} carries no {SIDECAR_MARKER_ENV} in its environment — every engine a goose \
                 sidecar starts carries {SIDECAR_MARKER_ENV}={marker} (a goose older than this \
                 check stamped none)",
                process.pid
            ))),
        }
    };
    marked(listener)?;
    let Some(group) = listener.group else {
        return Err(NotOurs::because(format!(
            "the process group of pid {} could not be read",
            listener.pid
        )));
    };
    let mut chain = Vec::new();
    for process in lineage.iter().take_while(|p| p.group == Some(group)) {
        marked(process)?;
        chain.push(process.clone());
    }
    let top = chain.last().unwrap_or(listener);
    match top.parent {
        Some(1) => Ok(chain),
        Some(parent) if parent == own_pid => Ok(chain),
        Some(parent) => match lineage.iter().find(|p| p.pid == parent) {
            Some(starter) => Err(NotOurs {
                reason: format!(
                    "the process that started it, pid {parent} (`{}`), is alive — another goose \
                     on this Mac, or a shell, runs it",
                    starter.argv.join(" ")
                ),
                live_starter: Some(parent),
            }),
            None => Err(NotOurs::because(format!(
                "pid {}'s parent, pid {parent}, could not be read (it may have just exited)",
                top.pid
            ))),
        },
        None => Err(NotOurs::because(format!(
            "the parent of pid {} could not be read",
            top.pid
        ))),
    }
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

    /// Every LISTEN pid on `port`, read and judged. An unreadable `lsof` is an error, never an
    /// empty port.
    pub async fn inspect_port(port: u16, marker: &str) -> Result<Vec<PortHolder>> {
        let own_uid = unsafe { libc::getuid() };
        let own_pid = std::process::id();
        let pids = crate::listening_pids(port).await?;
        Ok(pids
            .into_iter()
            .map(|pid| match read_process(pid) {
                None => PortHolder {
                    pid,
                    argv: Vec::new(),
                    verdict: Err(NotOurs::because(format!(
                        "pid {pid} could not be read (it may have just exited)"
                    ))),
                },
                Some(listener) => {
                    let argv = listener.argv.clone();
                    let lineage = lineage(listener, own_pid);
                    PortHolder {
                        pid,
                        argv,
                        verdict: ownership_proof(&lineage, marker, own_uid, own_pid),
                    }
                }
            })
            .collect())
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
pub use read::{claim_port, inspect_port, read_process, Reaped};

#[cfg(test)]
mod tests {
    use super::*;

    const UID: u32 = 501;
    const GOOSED: u32 = 73403;
    const MARKER: &str = "mlx-engine@http://127.0.0.1:8090";

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
        let chain = ownership_proof(&[engine(), uv(1)], MARKER, UID, 999).unwrap();
        assert_eq!(pids(&chain), [35319, 35242], "the listener first, then uv");

        let orphan = HolderProcess {
            parent: Some(1),
            ..engine()
        };
        let chain = ownership_proof(&[orphan], MARKER, UID, 999).unwrap();
        assert_eq!(
            pids(&chain),
            [35319],
            "uv died too: the engine alone, its dead leader's pgid still its group"
        );

        let chain = ownership_proof(&[engine(), uv(999)], MARKER, UID, 999).unwrap();
        assert_eq!(
            pids(&chain),
            [35319, 35242],
            "started by this very goosed and supervised by nothing"
        );
    }

    #[test]
    fn a_live_goosed_above_it_makes_it_another_gooses() {
        let kept =
            ownership_proof(&[engine(), uv(GOOSED), goosed(GOOSED)], MARKER, UID, 999).unwrap_err();
        assert_eq!(kept.live_starter, Some(GOOSED));
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
        let kept = ownership_proof(&[unmarked, uv(1)], MARKER, UID, 999).unwrap_err();
        assert!(
            kept.reason.contains("carries no GOOSE_SIDECAR"),
            "{}",
            kept.reason
        );

        let elsewhere = HolderProcess {
            marker: Some("mlx-engine@http://127.0.0.1:8091".to_string()),
            ..engine()
        };
        let kept = ownership_proof(&[elsewhere, uv(1)], MARKER, UID, 999).unwrap_err();
        assert!(kept.reason.contains("127.0.0.1:8091"), "{}", kept.reason);

        let unmarked_launcher = HolderProcess {
            marker: None,
            ..uv(1)
        };
        let kept = ownership_proof(&[engine(), unmarked_launcher], MARKER, UID, 999).unwrap_err();
        assert!(
            kept.reason.contains("pid 35242 carries no"),
            "{}",
            kept.reason
        );

        let other_user = HolderProcess {
            uid: Some(0),
            ..engine()
        };
        let kept = ownership_proof(&[other_user, uv(1)], MARKER, UID, 999).unwrap_err();
        assert!(kept.reason.contains("uid 0"), "{}", kept.reason);

        let itself = HolderProcess {
            pid: 999,
            ..engine()
        };
        assert!(ownership_proof(&[itself], MARKER, UID, 999).is_err());
        assert!(ownership_proof(&[], MARKER, UID, 999).is_err());

        let unknown_parent = HolderProcess {
            parent: None,
            ..uv(1)
        };
        let kept = ownership_proof(&[engine(), unknown_parent], MARKER, UID, 999).unwrap_err();
        assert!(kept.reason.contains("could not be read"), "{}", kept.reason);

        let kept = ownership_proof(&[engine()], MARKER, UID, 999).unwrap_err();
        assert!(
            kept.reason.contains("pid 35242, could not be read"),
            "a parent that could not be read is never taken for a live starter: {}",
            kept.reason
        );
        assert_eq!(kept.live_starter, None);

        let groupless = HolderProcess {
            group: None,
            parent: Some(1),
            ..engine()
        };
        let kept = ownership_proof(&[groupless], MARKER, UID, 999).unwrap_err();
        assert!(kept.reason.contains("process group"), "{}", kept.reason);
    }

    #[test]
    fn a_refusal_names_every_holder_and_the_one_step_that_fits() {
        let held = PortHeld {
            port: 8090,
            holders: vec![PortHolder {
                pid: 35319,
                argv: engine().argv,
                verdict: ownership_proof(&[engine(), uv(GOOSED), goosed(GOOSED)], MARKER, UID, 1),
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
                verdict: Err(NotOurs::because("pid 35319 could not be read".to_string())),
            }],
            survived_the_stop: false,
        };
        let text = unmarked.to_string();
        assert!(text.contains("command line unreadable"), "{text}");
        assert!(text.contains("`kill 35319`"), "{text}");
    }
}
