//! One model load at a time on this Mac, and the memory the OTHER engines on it already hold.
//!
//! Q-106 (2026-09-26): several dev goosed processes loaded 27B engines on the MacBook while another
//! engine was loading, with ~8 GiB free, and the GPU wedged — a one-line `mx.ones` hung
//! uninterruptibly and only a reboot recovered it. Every goosed runs its own mount gate, and each
//! gate read the memory of a Mac on which none of the concurrent loads had allocated yet. Two
//! machine-wide rules close that:
//!
//! - ONE LOAD AT A TIME. A load holds an exclusive `flock` on [`LOAD_LOCK_RELATIVE`] under the
//!   account's home from before its gate judges until its weights are in. The kernel drops a flock
//!   with its holder's last open description, so a holder that dies frees the Mac by itself. The
//!   file carries the holder's record (pid, the pid's start time, what is loading); a recorded
//!   holder is overwritten only when PROVEN gone — no process has the pid, the pid has exited, or
//!   the pid now belongs to a process that started later (the reaping gate's rule: a pid is
//!   signalled or displaced only on proof). The path ignores `GOOSE_PATH_ROOT`, `XDG_STATE_HOME`
//!   and `HOME`: the resource is the Mac, and the wedge's dev builds ran under roots of their own.
//!   The split's ranks take the same lock from Python (`distributed/rank_load_lock.py`), so a
//!   rank's load on any Mac waits its turn beside that Mac's single engine.
//! - THE CEILING IS THE MAC'S. Metal's recommended working-set ceiling bounds everything on the GPU
//!   together, and each engine wires up to all of it (Q-11). A load's budget charges the resident
//!   bytes the other MLX engines on this Mac hold — their real footprint, read from the kernel —
//!   against the ceiling (`fit::NodeMemoryFacts::other_engines_bytes`); the RAM side already sees
//!   them, since memory they hold is not available.

use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::fit::gb;

/// Where the Mac's load lock lives, relative to the account's home: goose's default state dir
/// (etcetera's XDG layout, which the installed app writes `mlx-serving-intent.json` beside).
/// `rank_load_lock.py` builds the same path; a test pins the two together.
pub const LOAD_LOCK_RELATIVE: &str = ".local/state/goose/mlx-load.lock";

/// The Mac's load lock: [`LOAD_LOCK_RELATIVE`] under the home directory the account database
/// names for this uid — never `$HOME`, which a test harness or a dev launcher may point elsewhere.
pub fn load_lock_path() -> Result<PathBuf> {
    Ok(account_home()?.join(LOAD_LOCK_RELATIVE))
}

#[cfg(unix)]
fn account_home() -> Result<PathBuf> {
    use std::os::unix::ffi::OsStrExt;
    let uid = unsafe { libc::getuid() };
    let mut buf = vec![0u8; 1024];
    loop {
        let mut entry: libc::passwd = unsafe { std::mem::zeroed() };
        let mut found: *mut libc::passwd = std::ptr::null_mut();
        let rc = unsafe {
            libc::getpwuid_r(
                uid,
                &mut entry,
                buf.as_mut_ptr() as *mut libc::c_char,
                buf.len(),
                &mut found,
            )
        };
        if rc == libc::ERANGE {
            let doubled = buf.len() * 2;
            buf.resize(doubled, 0);
            continue;
        }
        ensure!(
            rc == 0,
            "getpwuid_r({uid}) failed: {}",
            std::io::Error::from_raw_os_error(rc)
        );
        ensure!(
            !found.is_null(),
            "the account database has no entry for uid {uid}"
        );
        let dir = unsafe { std::ffi::CStr::from_ptr(entry.pw_dir) };
        ensure!(
            !dir.to_bytes().is_empty(),
            "the account entry for uid {uid} names no home directory"
        );
        return Ok(PathBuf::from(std::ffi::OsStr::from_bytes(dir.to_bytes())));
    }
}

#[cfg(not(unix))]
fn account_home() -> Result<PathBuf> {
    bail!("the Mac's load lock lives under the account's home, read from the unix account database")
}

/// Who holds the Mac's load lock, as its record says.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadHolder {
    pub pid: u32,
    /// The holder process's start, unix seconds (the kernel's `pbi_start_tvsec`): with the pid,
    /// what proves the recorded holder is still the process that took the lock — a reused pid
    /// starts later.
    pub started_at: u64,
    /// When the lock was taken, unix seconds.
    pub since: u64,
    /// What is loading, in words.
    pub what: String,
    /// The engine's port, when a single engine is loading.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// A split's ranks on one Mac are one load: their common launch identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
}

impl LoadHolder {
    /// The record's text: `key=value` lines, so the shell (preflight) and Python (the ranks) read
    /// it without a JSON parser.
    pub fn to_record(&self) -> String {
        let mut text = format!(
            "pid={}\nstarted={}\nsince={}\nwhat={}\n",
            self.pid,
            self.started_at,
            self.since,
            one_line(&self.what)
        );
        if let Some(port) = self.port {
            text.push_str(&format!("port={port}\n"));
        }
        if let Some(group) = &self.group {
            text.push_str(&format!("group={}\n", one_line(group)));
        }
        text
    }

    /// `None` for an empty record (no holder, or one that released it); an error for text that is
    /// not a holder's record.
    pub fn parse_record(text: &str) -> Result<Option<Self>> {
        if text.trim().is_empty() {
            return Ok(None);
        }
        let field = |key: &str| {
            text.lines()
                .find_map(|line| line.strip_prefix(key)?.strip_prefix('='))
                .map(str::trim)
        };
        let number = |key: &str| -> Result<u64> {
            field(key)
                .with_context(|| format!("the load lock record has no `{key}`: {text:?}"))?
                .parse()
                .with_context(|| {
                    format!("the load lock record's `{key}` is not a number: {text:?}")
                })
        };
        Ok(Some(LoadHolder {
            pid: u32::try_from(number("pid")?).context("the load lock record's pid")?,
            started_at: number("started")?,
            since: number("since")?,
            what: field("what").unwrap_or_default().to_string(),
            port: field("port").and_then(|p| p.parse().ok()),
            group: field("group").map(str::to_string),
        }))
    }

    /// "pid 4321 — Qwen3.8-27B (a single engine on port 8124) — loading for 2m 10s".
    pub fn describe(&self, now: u64) -> String {
        format!(
            "pid {} — {} — loading for {}",
            self.pid,
            self.what,
            elapsed_words(now.saturating_sub(self.since))
        )
    }
}

fn one_line(text: &str) -> String {
    text.replace(['\n', '\r'], " ")
}

fn elapsed_words(seconds: u64) -> String {
    match seconds {
        s if s < 60 => format!("{s}s"),
        s if s < 3600 => format!("{}m {}s", s / 60, s % 60),
        s => format!("{}h {}m", s / 3600, (s % 3600) / 60),
    }
}

pub fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// What a load claims the Mac for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadClaim {
    pub what: String,
    pub port: Option<u16>,
    pub group: Option<String>,
}

impl LoadClaim {
    pub fn single_engine(model_id: &str, port: u16, weights_bytes: u64) -> Self {
        LoadClaim {
            what: format!(
                "goose (pid {}) is loading {model_id} ({} of weights) as a single engine on port \
                 {port}",
                std::process::id(),
                gb(weights_bytes)
            ),
            port: Some(port),
            group: None,
        }
    }
}

/// Whether a recorded holder is still the process that took the lock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Liveness {
    Alive,
    /// Proven gone, with the proof in words.
    Gone(String),
}

/// The proof: the pid exists (signal 0 is delivered or refused for permission, never "no such
/// process"), it is not a zombie, and it started when the record says. The kernel answers no
/// process info for a zombie (measured 2026-09-26: `proc_pidinfo(PROC_PIDTBSDINFO)` returns 0 for
/// a `ps` stat `Z` child), so a pid this account may signal but not read has exited; another
/// account's pid, which it may not signal, is never taken for gone.
#[cfg(unix)]
pub fn prove(pid: u32, started_at: u64) -> Liveness {
    let rc = unsafe { libc::kill(pid as libc::pid_t, 0) };
    let signal_refused = rc != 0;
    if signal_refused && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
        return Liveness::Gone(format!("no process has pid {pid}"));
    }
    let Some((started, zombie)) = process_start(pid) else {
        if signal_refused {
            return Liveness::Alive;
        }
        return Liveness::Gone(format!(
            "pid {pid} has exited (the kernel answers no process info for it: a zombie)"
        ));
    };
    if zombie {
        return Liveness::Gone(format!("pid {pid} has exited (a zombie holds no files)"));
    }
    if started != started_at {
        return Liveness::Gone(format!(
            "pid {pid} is now another process (it started at {started}, the holder at {started_at})"
        ));
    }
    Liveness::Alive
}

#[cfg(not(unix))]
pub fn prove(pid: u32, _started_at: u64) -> Liveness {
    Liveness::Gone(format!(
        "pid {pid} cannot be proven alive: signals are unix-only"
    ))
}

/// A process's start (unix seconds) and whether it is a zombie; `None` when the kernel names no
/// such process to this account.
pub fn process_start(pid: u32) -> Option<(u64, bool)> {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessStatus, ProcessesToUpdate, System};
    let mut sys = System::new();
    let wanted = [Pid::from_u32(pid)];
    sys.refresh_processes_specifics(
        ProcessesToUpdate::Some(&wanted),
        true,
        ProcessRefreshKind::nothing(),
    );
    sys.process(wanted[0])
        .map(|p| (p.start_time(), p.status() == ProcessStatus::Zombie))
}

fn own_holder(claim: &LoadClaim) -> Result<LoadHolder> {
    let pid = std::process::id();
    let (started_at, _) = process_start(pid)
        .with_context(|| format!("reading this process's own start time (pid {pid})"))?;
    Ok(LoadHolder {
        pid,
        started_at,
        since: now_unix(),
        what: claim.what.clone(),
        port: claim.port,
        group: claim.group.clone(),
    })
}

/// The lock is held by someone else.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadLockHeld {
    pub path: PathBuf,
    /// The record, when it names a holder; `record_error` when it is not a record.
    pub holder: Option<LoadHolder>,
    pub record_error: Option<String>,
    pub liveness: Liveness,
}

impl LoadLockHeld {
    /// The refusal's words: who holds the Mac, and what the owner can do.
    pub fn message(&self) -> String {
        let path = self.path.display();
        match (&self.holder, &self.liveness) {
            (Some(holder), Liveness::Alive) => format!(
                "another model is loading on this Mac: {}. One model loads at a time per Mac — two \
                 loads at once can wedge its GPU. Wait for it to finish, or stop it first",
                holder.describe(now_unix())
            ),
            (Some(holder), Liveness::Gone(proof)) => format!(
                "this Mac's load lock ({path}) is held, but its recorded holder ({}) is gone \
                 ({proof}): a process that inherited the lock still holds it — `lsof {path}` names it",
                holder.what
            ),
            (None, _) => format!(
                "this Mac's load lock ({path}) is held by a process whose record cannot be read ({}) \
                 — `lsof {path}` names it",
                self.record_error.as_deref().unwrap_or("the record is empty")
            ),
        }
    }
}

/// A held lock. Dropping it empties the record, then releases the flock — in that order, so a
/// released lock never shows a holder.
#[derive(Debug)]
pub struct LoadLock {
    file: std::fs::File,
    path: PathBuf,
    holder: LoadHolder,
}

impl LoadLock {
    pub fn holder(&self) -> &LoadHolder {
        &self.holder
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for LoadLock {
    fn drop(&mut self) {
        if let Err(e) = self.file.set_len(0) {
            tracing::warn!(path = %self.path.display(), error = %e, "the load lock's record could not be emptied on release");
        }
        #[cfg(unix)]
        {
            use std::os::unix::io::AsRawFd;
            unsafe { libc::flock(self.file.as_raw_fd(), libc::LOCK_UN) };
        }
    }
}

#[derive(Debug)]
pub enum LoadLockAttempt {
    Acquired(LoadLock),
    Held(LoadLockHeld),
}

#[cfg(unix)]
fn open_lock(path: &Path) -> Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o644)
        .open(path)
        .with_context(|| format!("opening the load lock {}", path.display()))
}

fn read_record(file: &std::fs::File) -> Result<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = file;
    file.seek(SeekFrom::Start(0))?;
    let mut text = String::new();
    file.read_to_string(&mut text)?;
    Ok(text)
}

/// Take the Mac's load lock if nobody holds it. A holder that has taken the flock but not yet
/// written its record (the instant between the two, or the instant a release empties the record
/// before unlocking) is waited out one grace tick at a time; a record is always there otherwise.
#[cfg(unix)]
pub fn try_acquire(path: &Path, claim: &LoadClaim) -> Result<LoadLockAttempt> {
    use std::os::unix::io::AsRawFd;
    let file = open_lock(path)?;
    loop {
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
            return take(file, path, claim);
        }
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::EWOULDBLOCK) {
            bail!("flock({}) failed: {error}", path.display());
        }
        let text = read_record(&file)?;
        let held = match LoadHolder::parse_record(&text) {
            Ok(None) => {
                std::thread::sleep(crate::GRACE_TICK);
                continue;
            }
            Ok(Some(holder)) => LoadLockHeld {
                path: path.to_path_buf(),
                liveness: prove(holder.pid, holder.started_at),
                holder: Some(holder),
                record_error: None,
            },
            Err(e) => LoadLockHeld {
                path: path.to_path_buf(),
                holder: None,
                record_error: Some(format!("{e:#}")),
                liveness: Liveness::Alive,
            },
        };
        return Ok(LoadLockAttempt::Held(held));
    }
}

/// Take the Mac's load lock, waiting for whoever holds it (blocking: run it off the async runtime).
/// No clock bounds the wait — a load ends when it ends.
#[cfg(unix)]
pub fn acquire_waiting(path: &Path, claim: &LoadClaim) -> Result<LoadLock> {
    use std::os::unix::io::AsRawFd;
    let file = open_lock(path)?;
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } != 0 {
        bail!(
            "flock({}) failed: {}",
            path.display(),
            std::io::Error::last_os_error()
        );
    }
    match take(file, path, claim)? {
        LoadLockAttempt::Acquired(lock) => Ok(lock),
        LoadLockAttempt::Held(held) => bail!(held.message()),
    }
}

/// The flock is ours: prove the file is still the one at `path`, displace a recorded holder only
/// when it is proven gone, then write our record.
#[cfg(unix)]
fn take(file: std::fs::File, path: &Path, claim: &LoadClaim) -> Result<LoadLockAttempt> {
    use std::io::Write;
    use std::os::unix::fs::MetadataExt;
    let opened = file.metadata()?;
    let named = std::fs::metadata(path)
        .with_context(|| format!("the load lock {} vanished while taken", path.display()))?;
    ensure!(
        (opened.dev(), opened.ino()) == (named.dev(), named.ino()),
        "the load lock {} was replaced while it was being taken",
        path.display()
    );
    let own = own_holder(claim)?;
    match LoadHolder::parse_record(&read_record(&file)?) {
        Ok(Some(previous)) if previous.pid != own.pid => {
            match prove(previous.pid, previous.started_at) {
                Liveness::Alive => {
                    return Ok(LoadLockAttempt::Held(LoadLockHeld {
                        path: path.to_path_buf(),
                        holder: Some(previous),
                        record_error: None,
                        liveness: Liveness::Alive,
                    }));
                }
                Liveness::Gone(proof) => tracing::warn!(
                    path = %path.display(),
                    previous = %previous.to_record(),
                    %proof,
                    "reclaimed the load lock: the kernel held no lock for it and its recorded holder is gone"
                ),
            }
        }
        Ok(_) => {}
        Err(e) => tracing::warn!(
            path = %path.display(),
            error = %format!("{e:#}"),
            "reclaimed the load lock over an unreadable record: the kernel held no lock for it"
        ),
    }
    let mut writer = &file;
    file.set_len(0)?;
    use std::io::{Seek, SeekFrom};
    writer.seek(SeekFrom::Start(0))?;
    writer.write_all(own.to_record().as_bytes())?;
    file.sync_data()?;
    Ok(LoadLockAttempt::Acquired(LoadLock {
        file,
        path: path.to_path_buf(),
        holder: own,
    }))
}

#[cfg(not(unix))]
pub fn try_acquire(path: &Path, _claim: &LoadClaim) -> Result<LoadLockAttempt> {
    bail!(
        "the Mac's load lock ({}) is an flock: unix only",
        path.display()
    )
}

#[cfg(not(unix))]
pub fn acquire_waiting(path: &Path, _claim: &LoadClaim) -> Result<LoadLock> {
    bail!(
        "the Mac's load lock ({}) is an flock: unix only",
        path.display()
    )
}

/// Who is loading on this Mac right now, besides this process — read without touching the lock:
/// the record, and the proof its holder is alive. A released lock has an empty record; a holder
/// that died leaves one its proof refutes.
pub fn current_holder(path: &Path) -> Result<Option<LoadHolder>> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    Ok(LoadHolder::parse_record(&text)?.filter(|holder| {
        holder.pid != std::process::id() && prove(holder.pid, holder.started_at) == Liveness::Alive
    }))
}

/// An MLX engine on this Mac that is not this goose's own.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OtherEngine {
    pub pid: u32,
    /// Unix seconds; with the pid, what a stop must match.
    pub started_at: u64,
    /// "singleServer" (one model behind one port) | "distributed" (a split's rank).
    pub kind: String,
    /// Its command line, cut for display.
    pub command: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// Its resident bytes — the weights and caches it holds on the GPU, measured.
    pub resident_bytes: u64,
}

impl OtherEngine {
    pub fn describe(&self) -> String {
        let what = match (self.kind.as_str(), self.port) {
            ("singleServer", Some(port)) => format!("an MLX server on port {port}"),
            ("singleServer", None) => "an MLX server".to_string(),
            _ => "a distributed MLX rank".to_string(),
        };
        format!(
            "pid {} — {what}, holding {} (`{}`)",
            self.pid,
            gb(self.resident_bytes),
            self.command
        )
    }
}

pub fn other_engines_bytes(engines: &[OtherEngine]) -> u64 {
    engines.iter().map(|e| e.resident_bytes).sum()
}

/// The sentence a fit carries when other engines hold part of the ceiling.
pub fn other_engines_note(engines: &[OtherEngine]) -> Option<String> {
    (!engines.is_empty()).then(|| {
        let each: Vec<String> = engines.iter().map(OtherEngine::describe).collect();
        format!(
            "other MLX engines on this Mac hold {} of the GPU's working set: {} — stop one to make \
             room",
            gb(other_engines_bytes(engines)),
            each.join("; ")
        )
    })
}

/// `--port N` on a command line.
pub fn port_arg(command: &str) -> Option<u16> {
    let mut args = command.split_whitespace();
    while let Some(arg) = args.next() {
        if arg == "--port" {
            return args.next()?.parse().ok();
        }
        if let Some(port) = arg.strip_prefix("--port=") {
            return port.parse().ok();
        }
    }
    None
}

/// Every MLX engine on this Mac, by the preflight's own classifier (`probe::classify_foreign_engines`:
/// a python interpreter serving `mlx_lm.server` / `rapid-mlx serve`, or a distributed process),
/// with its resident bytes and start time — except a single server on `own_port` (this goose's
/// own engine, which a mount replaces and counts as available).
#[cfg(unix)]
pub fn other_engines(own_port: Option<u16>) -> Vec<OtherEngine> {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
    let mut sys = System::new();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing()
            .with_memory()
            .with_cmd(UpdateKind::Always),
    );
    let mut rows = String::new();
    let mut full = std::collections::HashMap::new();
    for (pid, process) in sys.processes() {
        let command: Vec<String> = process
            .cmd()
            .iter()
            .map(|a| one_line(&a.to_string_lossy()))
            .collect();
        if !command.is_empty() {
            let command = command.join(" ");
            rows.push_str(&format!("{} {command}\n", pid.as_u32()));
            full.insert(pid.as_u32(), command);
        }
    }
    let own = [std::process::id()];
    crate::distributed::probe::classify_foreign_engines(&rows, &own)
        .into_iter()
        .filter_map(|(pid, command, kind)| {
            let process = sys.process(sysinfo::Pid::from_u32(pid))?;
            let (kind, port) = match kind {
                // The classifier's command is cut for display; the port is read from the whole line
                // (a Rapid-MLX argv puts `--port` after the model's full path).
                crate::distributed::probe::ForeignKind::SingleServer => {
                    ("singleServer", full.get(&pid).and_then(|c| port_arg(c)))
                }
                crate::distributed::probe::ForeignKind::Distributed => ("distributed", None),
            };
            if kind == "singleServer" && port.is_some() && port == own_port {
                return None;
            }
            Some(OtherEngine {
                pid,
                started_at: process.start_time(),
                kind: kind.to_string(),
                command,
                port,
                resident_bytes: process.memory(),
            })
        })
        .collect()
}

#[cfg(not(unix))]
pub fn other_engines(_own_port: Option<u16>) -> Vec<OtherEngine> {
    Vec::new()
}

/// What stopping another engine did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StopReport {
    pub pid: u32,
    /// "SIGTERM" (it left in the grace window) | "SIGKILL".
    pub signal: String,
    /// What it held when it was stopped.
    pub resident_bytes: u64,
    pub message: String,
}

/// Stop ANOTHER MLX engine on this Mac — per-pid, and only the exact process the owner was shown:
/// it must still be an MLX engine by the census's classifier, and still the process that started
/// at `started_at` (a reused pid is refused, nothing signalled). This goose's own engine is not
/// stopped here — Unmount is its stop. SIGTERM, the crate's grace window, then SIGKILL; never a
/// group (the engine's group is its launcher's, not provably its own).
#[cfg(unix)]
pub async fn stop_other_engine(pid: u32, started_at: u64, own_port: u16) -> Result<StopReport> {
    let engines = tokio::task::spawn_blocking(move || other_engines(Some(own_port)))
        .await
        .context("the engine census task panicked")?;
    let Some(engine) = engines.into_iter().find(|e| e.pid == pid) else {
        let own = other_engines(None)
            .into_iter()
            .any(|e| e.pid == pid && e.port == Some(own_port));
        if own {
            bail!("pid {pid} is this goose's own engine (port {own_port}): Unmount stops it; nothing was signalled");
        }
        bail!("pid {pid} is not an MLX engine on this Mac: nothing was signalled");
    };
    ensure!(
        engine.started_at == started_at,
        "pid {pid} is no longer the engine you were shown (it started at {}, that one at \
         {started_at}): nothing was signalled",
        engine.started_at
    );
    let gone = |pid: u32| prove(pid, started_at) != Liveness::Alive;
    for signal in [libc::SIGTERM, libc::SIGKILL] {
        if unsafe { libc::kill(pid as libc::pid_t, signal) } != 0 {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() != Some(libc::ESRCH) {
                bail!("signalling pid {pid} failed: {error}");
            }
        }
        for _ in 0..crate::GRACE_TICKS {
            if gone(pid) {
                let name = if signal == libc::SIGTERM {
                    "SIGTERM"
                } else {
                    "SIGKILL"
                };
                return Ok(StopReport {
                    pid,
                    signal: name.to_string(),
                    resident_bytes: engine.resident_bytes,
                    message: format!(
                        "stopped {} ({name}); its {} are the Mac's again",
                        engine.describe(),
                        gb(engine.resident_bytes)
                    ),
                });
            }
            tokio::time::sleep(crate::GRACE_TICK).await;
        }
    }
    bail!(
        "pid {pid} is still running after SIGTERM and SIGKILL — it is likely stuck in the kernel \
         (an uninterruptible GPU wait survives every signal; only a reboot frees it)"
    )
}

#[cfg(not(unix))]
pub async fn stop_other_engine(pid: u32, _started_at: u64, _own_port: u16) -> Result<StopReport> {
    bail!("stopping pid {pid} needs unix signals")
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn claim(what: &str) -> LoadClaim {
        LoadClaim {
            what: what.to_string(),
            port: Some(8124),
            group: None,
        }
    }

    #[test]
    fn a_record_round_trips_and_an_empty_one_is_no_holder() {
        let holder = LoadHolder {
            pid: 4321,
            started_at: 1_790_375_237,
            since: 1_790_375_300,
            what: "goose (pid 4321) is loading Qwen3.8-27B\nsecond line".to_string(),
            port: Some(8124),
            group: Some("split:8090:m".to_string()),
        };
        let text = holder.to_record();
        assert_eq!(text.lines().count(), 6, "{text}");
        let back = LoadHolder::parse_record(&text).unwrap().unwrap();
        assert_eq!(
            back.what,
            "goose (pid 4321) is loading Qwen3.8-27B second line"
        );
        assert_eq!((back.pid, back.port), (4321, Some(8124)));
        assert_eq!(back.group.as_deref(), Some("split:8090:m"));
        assert_eq!(LoadHolder::parse_record("").unwrap(), None);
        assert_eq!(LoadHolder::parse_record("\n").unwrap(), None);
        assert!(LoadHolder::parse_record("garbage").is_err());
    }

    #[test]
    fn the_lock_lives_under_the_accounts_home_whatever_the_environment_says() {
        let path = load_lock_path().unwrap();
        assert!(path.ends_with(LOAD_LOCK_RELATIVE), "{}", path.display());
        let python = include_str!("distributed/rank_load_lock.py");
        assert!(
            python.contains(&format!("\"{LOAD_LOCK_RELATIVE}\"")),
            "rank_load_lock.py must build the same path"
        );
    }

    #[test]
    fn one_load_at_a_time_and_the_refusal_names_the_holder() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state/mlx-load.lock");
        let LoadLockAttempt::Acquired(first) =
            try_acquire(&path, &claim("the first load")).unwrap()
        else {
            panic!("an unheld lock is taken")
        };
        assert_eq!(first.holder().pid, std::process::id());
        let LoadLockAttempt::Held(held) = try_acquire(&path, &claim("the second load")).unwrap()
        else {
            panic!("a second load on the same Mac is refused while the first holds it")
        };
        assert_eq!(held.liveness, Liveness::Alive);
        let message = held.message();
        assert!(message.contains("the first load"), "{message}");
        assert!(
            message.contains("Wait for it to finish, or stop it"),
            "{message}"
        );
        assert_eq!(
            current_holder(&path).unwrap(),
            None,
            "a holder is never reported to itself"
        );
        drop(first);
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "",
            "release empties the record"
        );
        assert!(matches!(
            try_acquire(&path, &claim("the second load")).unwrap(),
            LoadLockAttempt::Acquired(_)
        ));
    }

    /// A holder that died leaves its record behind (it never ran its release); the kernel dropped
    /// its flock with it. The next load proves the recorded pid gone and takes the Mac.
    #[test]
    fn a_dead_holders_record_is_reclaimed_only_on_proof() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mlx-load.lock");
        let mut child = std::process::Command::new("/bin/sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let pid = child.id();
        let (started_at, _) = process_start(pid).unwrap();
        let record = LoadHolder {
            pid,
            started_at,
            since: now_unix(),
            what: "a load whose goose crashed".to_string(),
            port: None,
            group: None,
        };
        std::fs::write(&path, record.to_record()).unwrap();

        // Alive (and the same process): refused even though the kernel holds no flock for it.
        let LoadLockAttempt::Held(held) = try_acquire(&path, &claim("mine")).unwrap() else {
            panic!("a live recorded holder is never displaced")
        };
        assert_eq!(held.liveness, Liveness::Alive);
        assert_eq!(current_holder(&path).unwrap(), Some(record.clone()));

        // The same pid number with another start time is another process: gone.
        let reused = LoadHolder {
            started_at: started_at - 1,
            ..record.clone()
        };
        assert!(matches!(prove(pid, reused.started_at), Liveness::Gone(_)));

        child.kill().unwrap();
        child.wait().unwrap();
        assert!(matches!(prove(pid, started_at), Liveness::Gone(_)));
        assert_eq!(current_holder(&path).unwrap(), None);
        let LoadLockAttempt::Acquired(lock) = try_acquire(&path, &claim("mine")).unwrap() else {
            panic!("a proven-dead holder's record is reclaimed")
        };
        assert_eq!(lock.holder().what, "mine");
    }

    #[test]
    fn a_waiting_load_takes_the_mac_when_the_holder_releases() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mlx-load.lock");
        let LoadLockAttempt::Acquired(first) = try_acquire(&path, &claim("first")).unwrap() else {
            panic!()
        };
        let waiter_path = path.clone();
        let waiter =
            std::thread::spawn(move || acquire_waiting(&waiter_path, &claim("waiter")).unwrap());
        std::thread::sleep(crate::GRACE_TICK);
        assert!(
            !waiter.is_finished(),
            "the waiter waits while the first load holds the Mac"
        );
        drop(first);
        let lock = waiter.join().unwrap();
        assert_eq!(lock.holder().what, "waiter");
    }

    #[test]
    fn a_port_is_read_from_either_spelling() {
        assert_eq!(
            port_arg("python rapid-mlx serve /m --port 8124 --x"),
            Some(8124)
        );
        assert_eq!(port_arg("python -m mlx_lm.server --port=8090"), Some(8090));
        assert_eq!(port_arg("python -m mlx_lm.server"), None);
    }

    /// The census reads the kernel, and sees what the preflight's classifier sees: a python
    /// interpreter whose command line serves `rapid-mlx serve` on a port.
    #[tokio::test]
    async fn the_census_names_an_engine_and_a_stop_needs_its_exact_identity() {
        let mut stand_in = std::process::Command::new("/usr/bin/python3")
            .args([
                "-c",
                "import time; time.sleep(60)",
                "rapid-mlx",
                "serve",
                "/tmp/q106-stand-in-model",
                "--port",
                "59391",
            ])
            .spawn()
            .unwrap();
        let pid = stand_in.id();
        let seen = (0..crate::GRACE_TICKS).find_map(|_| {
            let found = other_engines(Some(8090)).into_iter().find(|e| e.pid == pid);
            if found.is_none() {
                std::thread::sleep(crate::GRACE_TICK);
            }
            found
        });
        let engine = seen.expect("the census names the stand-in engine");
        assert_eq!(engine.kind, "singleServer");
        assert_eq!(engine.port, Some(59391));
        assert!(engine.resident_bytes > 0);
        assert!(
            other_engines(Some(59391)).iter().all(|e| e.pid != pid),
            "a single server on this goose's own port is its own engine, not another"
        );

        let wrong = stop_other_engine(pid, engine.started_at + 1, 8090)
            .await
            .unwrap_err()
            .to_string();
        assert!(wrong.contains("nothing was signalled"), "{wrong}");
        assert!(
            stand_in.try_wait().unwrap().is_none(),
            "a mismatched stop signals nothing"
        );
        let own = stop_other_engine(pid, engine.started_at, 59391)
            .await
            .unwrap_err()
            .to_string();
        assert!(own.contains("Unmount stops it"), "{own}");

        let report = stop_other_engine(pid, engine.started_at, 8090)
            .await
            .unwrap();
        assert_eq!(report.signal, "SIGTERM");
        stand_in.wait().unwrap();
    }

    /// The Python ranks write and read the same record, and `pbi_start_tvsec` through ctypes is
    /// the start time sysinfo reads.
    #[test]
    fn a_rank_and_goose_share_one_lock() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mlx-load.lock");
        let LoadLockAttempt::Acquired(ours) = try_acquire(&path, &claim("goose's load")).unwrap()
        else {
            panic!()
        };
        let driver = format!(
            "{}\nimport sys\nfd = take_load_lock(sys.argv[1], 'split:8090:m', 'rank 1 of 2')\n",
            include_str!("distributed/rank_load_lock.py")
        );
        let refused = std::process::Command::new("/usr/bin/python3")
            .args(["-c", &driver, path.to_str().unwrap()])
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&refused.stderr);
        assert_eq!(refused.status.code(), Some(1), "{stderr}");
        assert!(stderr.contains("goose's load"), "{stderr}");
        assert!(
            stderr.contains(&format!("pid {}", std::process::id())),
            "{stderr}"
        );
        let stdout = String::from_utf8_lossy(&refused.stdout);
        assert!(stdout.contains("GOOSE_RANK_LOAD_REFUSED "), "{stdout}");
        drop(ours);

        let holding = format!(
            "{driver}print('HELD', flush=True)\nsys.stdin.readline()\nrelease_load_lock()\n\
             print('RELEASED', flush=True)\nsys.stdin.readline()\n"
        );
        let mut rank = std::process::Command::new("/usr/bin/python3")
            .args(["-c", &holding, path.to_str().unwrap()])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let mut out = std::io::BufReader::new(rank.stdout.take().unwrap());
        let mut line = String::new();
        use std::io::{BufRead, Write};
        out.read_line(&mut line).unwrap();
        assert_eq!(line.trim(), "HELD");
        let LoadLockAttempt::Held(held) = try_acquire(&path, &claim("goose")).unwrap() else {
            panic!("goose waits its turn behind a rank's load")
        };
        let holder = held.holder.clone().unwrap();
        assert_eq!(holder.pid, rank.id());
        assert_eq!(
            held.liveness,
            Liveness::Alive,
            "ctypes' start time is sysinfo's"
        );
        assert_eq!(holder.group.as_deref(), Some("split:8090:m"));
        assert!(held.message().contains("rank 1 of 2"), "{}", held.message());

        let mut stdin = rank.stdin.take().unwrap();
        stdin.write_all(b"\n").unwrap();
        line.clear();
        out.read_line(&mut line).unwrap();
        assert_eq!(line.trim(), "RELEASED");
        assert!(matches!(
            try_acquire(&path, &claim("goose")).unwrap(),
            LoadLockAttempt::Acquired(_)
        ));
        stdin.write_all(b"\n").unwrap();
        rank.wait().unwrap();
    }

    /// Two ranks of ONE split on one Mac are one load: the second joins the first's hold instead of
    /// refusing it (a refusal there would deadlock the group's own collectives).
    #[test]
    fn a_splits_sibling_rank_joins_its_hold() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mlx-load.lock");
        let driver = format!(
            "{}\nimport sys\nfd = take_load_lock(sys.argv[1], 'split:8090:m', 'rank 0 of 2')\n\
             print('HELD', flush=True)\nsys.stdin.readline()\n",
            include_str!("distributed/rank_load_lock.py")
        );
        let mut first = std::process::Command::new("/usr/bin/python3")
            .args(["-c", &driver, path.to_str().unwrap()])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let mut out = std::io::BufReader::new(first.stdout.take().unwrap());
        let mut line = String::new();
        use std::io::{BufRead, Write};
        out.read_line(&mut line).unwrap();
        assert_eq!(line.trim(), "HELD");
        let sibling = format!(
            "{}\nimport sys\nfd = take_load_lock(sys.argv[1], sys.argv[2], 'rank 1 of 2')\n\
             print('JOINED' if fd is None else 'OWN', flush=True)\n",
            include_str!("distributed/rank_load_lock.py")
        );
        let joined = std::process::Command::new("/usr/bin/python3")
            .args(["-c", &sibling, path.to_str().unwrap(), "split:8090:m"])
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&joined.stdout).trim(), "JOINED");
        let other_split = std::process::Command::new("/usr/bin/python3")
            .args(["-c", &sibling, path.to_str().unwrap(), "split:9090:other"])
            .output()
            .unwrap();
        assert_eq!(
            other_split.status.code(),
            Some(1),
            "another split's rank waits its turn"
        );
        first.stdin.take().unwrap().write_all(b"\n").unwrap();
        first.wait().unwrap();
    }
}
