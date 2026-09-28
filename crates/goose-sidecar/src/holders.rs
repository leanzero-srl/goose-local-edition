//! Who holds this Mac's MLX engine across processes, and the swap claim (design:
//! DESIGN-NODES-AND-STRATEGIES.md §6.4 "Holders", §13 item 2).
//!
//! Every desktop window runs its own goosed and a `goose swarm run` child runs its own engine
//! manager, so what one process holds is invisible to another's memory. The record is files, under
//! the same Mac-wide state directory as the load lock (`machine::load_lock_path`'s directory —
//! it ignores `GOOSE_PATH_ROOT`: the resource is the Mac):
//!
//! - `<pid>-<start>.json` — one per registered process: its kind (a goosed with its open agent
//!   replies, or a swarm run with the way its engine serves). Rewritten whole through a rename, so
//!   a reader never sees half a record.
//! - `<pid>-<start>.lock` — held under an exclusive `flock` for the registration's life. The kernel
//!   drops a flock with its holder's last open description, so a process that dies frees itself.
//!   A record whose lock is free is displaced only on PROOF its pid is gone (`machine::prove`, the
//!   reaping gate's rule); a live pid keeps its record, whatever the lock says.
//! - `<pid>-<start>-r<n>.lock` — held under an exclusive `flock` while reply `n` is open. A loader
//!   in another process that must wait for that reply blocks on a shared `flock` of this file: the
//!   kernel wakes it the instant the reply ends or its process dies. No clock decides the wait.
//! - `swap.claim` — the one loader that swaps this Mac's goose to another way holds it (the
//!   `machine` lock discipline, reused: record, proof-of-gone, a blocking wait on the flock).
//!
//! A record that cannot be read is named (`HolderEntry::Unreadable`), never taken for "no holder".

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex as StdMutex;

use crate::machine::{self, Liveness, LoadClaim, LoadLock, LoadLockAttempt};
use crate::placement::store::PlacementKey;

/// The directory beside the Mac's load lock.
pub const HOLDERS_DIR: &str = "mlx-holders";
/// The swap claim, inside [`HOLDERS_DIR`].
pub const SWAP_CLAIM: &str = "swap.claim";

/// This Mac's holder directory: beside the load lock, under the account's home.
pub fn holders_dir() -> Result<PathBuf> {
    let lock = machine::load_lock_path()?;
    let state = lock
        .parent()
        .with_context(|| format!("the load lock {} has no directory", lock.display()))?;
    Ok(state.join(HOLDERS_DIR))
}

/// Whose reply holds the way: a person's, or a loop's tick (goose's `on_prompt` opens a tick's
/// reply on the same guard as `Tick`). Another window's loader reads it: a tick never stops a way
/// under a person's reply. A record written before the field existed is a person's.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ReplyKind {
    #[default]
    User,
    Tick,
}

/// One open agent reply of a goosed: the session it answers, the session at the root of its
/// delegate chain, and the way its last model call used (`None` before its first lease).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplyHold {
    /// The reply's number in its process: its lock is `<pid>-<start>-r<n>.lock`.
    pub reply: u64,
    pub session: String,
    pub root_session: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub way: Option<PlacementKey>,
    /// The reply is waiting in its own loader for a way: no model call of it is in flight, so it
    /// holds nothing a switch could cut, and its lock is released meanwhile (two loaders each
    /// waiting on the other's reply would otherwise wait forever).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub waiting: bool,
    #[serde(default)]
    pub kind: ReplyKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum HolderKind {
    Goosed {
        replies: Vec<ReplyHold>,
    },
    /// A `goose swarm run` holding this Mac's engine for the life of the run (S8 registers it).
    SwarmRun {
        way: PlacementKey,
        /// The model its engine serves, by id.
        model: String,
        /// In words: "swarm build <run id> on Qwen3.8-Flash".
        what: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HolderRecord {
    pub pid: u32,
    /// The process's start (unix seconds): with the pid, what proves the record is still its.
    pub started_at: u64,
    pub since: u64,
    #[serde(flatten)]
    pub kind: HolderKind,
}

impl HolderRecord {
    fn stem(&self) -> String {
        stem(self.pid, self.started_at)
    }
}

fn stem(pid: u32, started_at: u64) -> String {
    format!("{pid}-{started_at}")
}

pub fn reply_lock_path(dir: &Path, pid: u32, started_at: u64, reply: u64) -> PathBuf {
    dir.join(format!("{}-r{reply}.lock", stem(pid, started_at)))
}

/// One record as read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HolderEntry {
    /// Its process holds its lock, or its pid is proven alive.
    Live(HolderRecord),
    /// Its lock is free and its pid is proven gone: displaced (its files removed).
    Stale { record: HolderRecord, proof: String },
    /// Not a holder's record: which process holds what is unknown. Never read as "no holder".
    Unreadable { path: PathBuf, error: String },
}

/// Every record in `dir`. A missing directory is no holder (nothing ever registered).
pub fn read_all(dir: &Path) -> Result<Vec<HolderEntry>> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e).with_context(|| format!("reading {}", dir.display())),
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    paths.sort();
    let mut out = Vec::new();
    for path in paths {
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            // Withdrawn between the listing and the read: its holder deregistered.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                out.push(HolderEntry::Unreadable {
                    path,
                    error: e.to_string(),
                });
                continue;
            }
        };
        let record: HolderRecord = match serde_json::from_str(&text) {
            Ok(record) => record,
            Err(e) => {
                out.push(HolderEntry::Unreadable {
                    path,
                    error: e.to_string(),
                });
                continue;
            }
        };
        let lock = dir.join(format!("{}.lock", record.stem()));
        if lock_is_held(&lock)? {
            out.push(HolderEntry::Live(record));
            continue;
        }
        match machine::prove(record.pid, record.started_at) {
            Liveness::Alive => out.push(HolderEntry::Live(record)),
            Liveness::Gone(proof) => {
                remove_files_of(dir, &record);
                tracing::warn!(
                    path = %path.display(),
                    %proof,
                    "displaced a stale MLX holder record: its lock was free and its process is gone"
                );
                out.push(HolderEntry::Stale { record, proof });
            }
        }
    }
    Ok(out)
}

fn remove_files_of(dir: &Path, record: &HolderRecord) {
    let stem = record.stem();
    let _ = std::fs::remove_file(dir.join(format!("{stem}.json")));
    let _ = std::fs::remove_file(dir.join(format!("{stem}.lock")));
    if let HolderKind::Goosed { replies } = &record.kind {
        for reply in replies {
            let _ = std::fs::remove_file(reply_lock_path(
                dir,
                record.pid,
                record.started_at,
                reply.reply,
            ));
        }
    }
}

/// Whether some process holds `lock`'s flock. A missing lock file is not held.
#[cfg(unix)]
fn lock_is_held(lock: &Path) -> Result<bool> {
    use std::os::unix::io::AsRawFd;
    let file = match std::fs::File::open(lock) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e).with_context(|| format!("opening {}", lock.display())),
    };
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_SH | libc::LOCK_NB) } == 0 {
        unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_UN) };
        return Ok(false);
    }
    let error = std::io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::EWOULDBLOCK) {
        return Ok(true);
    }
    bail!("flock({}) failed: {error}", lock.display())
}

#[cfg(not(unix))]
fn lock_is_held(lock: &Path) -> Result<bool> {
    bail!("holder locks are flocks: unix only ({})", lock.display())
}

/// Block until the reply whose lock is `path` ends (its holder releases the flock, or dies). An
/// absent lock file is a reply that already ended. Blocking: run it off the async runtime.
#[cfg(unix)]
pub fn wait_for_reply_end(path: &Path) -> Result<()> {
    use std::os::unix::io::AsRawFd;
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e).with_context(|| format!("opening {}", path.display())),
    };
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_SH) } != 0 {
        bail!(
            "flock({}) failed: {}",
            path.display(),
            std::io::Error::last_os_error()
        );
    }
    Ok(())
}

#[cfg(not(unix))]
pub fn wait_for_reply_end(path: &Path) -> Result<()> {
    bail!("holder locks are flocks: unix only ({})", path.display())
}

#[cfg(unix)]
fn open_locked(path: &Path) -> Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    use std::os::unix::io::AsRawFd;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o644)
        .open(path)
        .with_context(|| format!("opening {}", path.display()))?;
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } != 0 {
        bail!(
            "flock({}) failed: {}",
            path.display(),
            std::io::Error::last_os_error()
        );
    }
    Ok(file)
}

#[cfg(not(unix))]
fn open_locked(path: &Path) -> Result<std::fs::File> {
    bail!("holder locks are flocks: unix only ({})", path.display())
}

fn write_atomically(path: &Path, text: &str) -> Result<()> {
    use std::io::Write;
    let tmp = path.with_extension("json.tmp");
    let mut file =
        std::fs::File::create(&tmp).with_context(|| format!("creating {}", tmp.display()))?;
    file.write_all(text.as_bytes())?;
    file.sync_data()?;
    std::fs::rename(&tmp, path)
        .with_context(|| format!("renaming {} to {}", tmp.display(), path.display()))
}

/// This process's registration. Dropping it withdraws the record, then its reply locks, then its
/// own lock — in that order, so a reader never sees a record whose lock is already gone.
pub struct Registration {
    dir: PathBuf,
    record: StdMutex<HolderRecord>,
    /// The open replies' locks, by reply number.
    reply_locks: StdMutex<Vec<(u64, std::fs::File)>>,
    lock: Option<std::fs::File>,
    /// Held from reading the record to renaming its file: writers publish one at a time, and each
    /// publishes the record as it is when its turn comes (no older state lands over a newer one).
    publishing: StdMutex<()>,
}

impl std::fmt::Debug for Registration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Registration")
            .field("dir", &self.dir)
            .field("record", &self.record.lock().unwrap())
            .finish()
    }
}

impl Registration {
    /// Register this process in `dir` as `kind`.
    pub fn register(dir: &Path, kind: HolderKind) -> Result<Self> {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        let pid = std::process::id();
        let (started_at, _) = machine::process_start(pid)
            .with_context(|| format!("reading this process's own start time (pid {pid})"))?;
        let record = HolderRecord {
            pid,
            started_at,
            since: machine::now_unix(),
            kind,
        };
        let lock = open_locked(&dir.join(format!("{}.lock", record.stem())))?;
        let registration = Registration {
            dir: dir.to_path_buf(),
            record: StdMutex::new(record),
            reply_locks: StdMutex::new(Vec::new()),
            lock: Some(lock),
            publishing: StdMutex::new(()),
        };
        registration.publish()?;
        Ok(registration)
    }

    pub fn record(&self) -> HolderRecord {
        self.record.lock().unwrap().clone()
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn json_path(&self, record: &HolderRecord) -> PathBuf {
        self.dir.join(format!("{}.json", record.stem()))
    }

    fn publish(&self) -> Result<()> {
        let _turn = self.publishing.lock().unwrap();
        let record = self.record.lock().unwrap().clone();
        write_atomically(&self.json_path(&record), &serde_json::to_string(&record)?)
    }

    /// Open reply `reply`: its lock is taken BEFORE the record lists it, so a reader that finds it
    /// listed always finds its lock held (or already released: the reply ended).
    pub fn open_reply(&self, hold: ReplyHold) -> Result<()> {
        let (pid, started_at) = {
            let record = self.record.lock().unwrap();
            (record.pid, record.started_at)
        };
        let lock = open_locked(&reply_lock_path(&self.dir, pid, started_at, hold.reply))?;
        self.reply_locks.lock().unwrap().push((hold.reply, lock));
        if let HolderKind::Goosed { replies } = &mut self.record.lock().unwrap().kind {
            replies.push(hold);
        }
        self.publish()
    }

    /// The way reply `reply`'s last lease used.
    pub fn set_reply_way(&self, reply: u64, way: PlacementKey) -> Result<()> {
        {
            let mut record = self.record.lock().unwrap();
            let HolderKind::Goosed { replies } = &mut record.kind else {
                return Ok(());
            };
            match replies.iter_mut().find(|r| r.reply == reply) {
                Some(hold) if hold.way.as_ref() == Some(&way) => return Ok(()),
                Some(hold) => hold.way = Some(way),
                None => return Ok(()),
            }
        }
        self.publish()
    }

    /// Reply `reply` waits in its loader: the record says so first, then its lock is released —
    /// a loader in another process waiting on it wakes and reads that it holds nothing.
    pub fn pause_reply(&self, reply: u64) -> Result<()> {
        if !self.set_waiting(reply, true) {
            return Ok(());
        }
        let published = self.publish();
        self.reply_locks
            .lock()
            .unwrap()
            .retain(|(n, _)| *n != reply);
        published
    }

    /// Reply `reply` runs again: its lock is taken first, then the record says it holds its way.
    pub fn resume_reply(&self, reply: u64) -> Result<()> {
        let (pid, started_at) = {
            let record = self.record.lock().unwrap();
            let listed = matches!(&record.kind, HolderKind::Goosed { replies }
                if replies.iter().any(|r| r.reply == reply && r.waiting));
            if !listed {
                return Ok(());
            }
            (record.pid, record.started_at)
        };
        let lock = open_locked(&reply_lock_path(&self.dir, pid, started_at, reply))?;
        self.reply_locks.lock().unwrap().push((reply, lock));
        self.set_waiting(reply, false);
        self.publish()
    }

    fn set_waiting(&self, reply: u64, waiting: bool) -> bool {
        let mut record = self.record.lock().unwrap();
        let HolderKind::Goosed { replies } = &mut record.kind else {
            return false;
        };
        match replies.iter_mut().find(|r| r.reply == reply) {
            Some(hold) if hold.waiting != waiting => {
                hold.waiting = waiting;
                true
            }
            _ => false,
        }
    }

    /// Close reply `reply`: the record stops listing it first, then its lock is released (which
    /// wakes a loader in another process waiting on it).
    pub fn close_reply(&self, reply: u64) -> Result<()> {
        let (pid, started_at) = {
            let mut record = self.record.lock().unwrap();
            if let HolderKind::Goosed { replies } = &mut record.kind {
                replies.retain(|r| r.reply != reply);
            }
            (record.pid, record.started_at)
        };
        let published = self.publish();
        let path = reply_lock_path(&self.dir, pid, started_at, reply);
        let _ = std::fs::remove_file(&path);
        self.reply_locks
            .lock()
            .unwrap()
            .retain(|(n, _)| *n != reply);
        published
    }
}

impl Drop for Registration {
    fn drop(&mut self) {
        let record = self.record.lock().unwrap().clone();
        let _ = std::fs::remove_file(self.json_path(&record));
        for (reply, _) in self.reply_locks.lock().unwrap().drain(..) {
            let _ = std::fs::remove_file(reply_lock_path(
                &self.dir,
                record.pid,
                record.started_at,
                reply,
            ));
        }
        let _ = std::fs::remove_file(self.dir.join(format!("{}.lock", record.stem())));
        drop(self.lock.take());
    }
}

/// Take the swap claim if no loader holds it. The `machine` lock discipline: a holder that died
/// freed it with its flock; a recorded holder is displaced only on proof it is gone.
pub fn try_claim_swap(dir: &Path, what: &str, model: &str) -> Result<LoadLockAttempt> {
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    machine::try_acquire(&dir.join(SWAP_CLAIM), &swap_claim(what, model))
}

/// Take the swap claim, waiting for the loader that holds it (blocking: run it off the async
/// runtime). No clock bounds the wait: a swap ends when its load ends.
pub fn claim_swap_waiting(dir: &Path, what: &str, model: &str) -> Result<LoadLock> {
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    machine::acquire_waiting(&dir.join(SWAP_CLAIM), &swap_claim(what, model))
}

fn swap_claim(what: &str, model: &str) -> LoadClaim {
    LoadClaim {
        what: what.to_string(),
        port: None,
        group: None,
        model: Some(model.to_string()),
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::io::{BufRead, Write};

    fn goosed() -> HolderKind {
        HolderKind::Goosed {
            replies: Vec::new(),
        }
    }

    fn hold(reply: u64, session: &str) -> ReplyHold {
        ReplyHold {
            reply,
            session: session.to_string(),
            root_session: session.to_string(),
            way: None,
            waiting: false,
            kind: ReplyKind::User,
        }
    }

    /// A reply that waits in its own loader releases its lock (a loader waiting on it wakes) and
    /// says it is waiting; resumed, it holds its lock and its way again.
    #[test]
    fn a_paused_reply_frees_its_waiters_and_says_it_waits() {
        let dir = tempfile::tempdir().unwrap();
        let reg = Registration::register(dir.path(), goosed()).unwrap();
        reg.open_reply(hold(4, "chat-b")).unwrap();
        let record = reg.record();
        let lock = reply_lock_path(dir.path(), record.pid, record.started_at, 4);
        let waiting = lock.clone();
        let waiter = std::thread::spawn(move || wait_for_reply_end(&waiting).unwrap());
        std::thread::sleep(crate::GRACE_TICK);
        assert!(!waiter.is_finished());
        reg.pause_reply(4).unwrap();
        waiter.join().unwrap();
        let read = read_all(dir.path()).unwrap();
        let HolderKind::Goosed { replies } = &live(&read)[0].kind else {
            panic!()
        };
        assert!(replies[0].waiting);
        assert!(!lock_is_held(&lock).unwrap());
        reg.resume_reply(4).unwrap();
        assert!(lock_is_held(&lock).unwrap());
        let read = read_all(dir.path()).unwrap();
        let HolderKind::Goosed { replies } = &live(&read)[0].kind else {
            panic!()
        };
        assert!(!replies[0].waiting);
    }

    fn live(entries: &[HolderEntry]) -> Vec<&HolderRecord> {
        entries
            .iter()
            .filter_map(|e| match e {
                HolderEntry::Live(r) => Some(r),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_registration_publishes_its_replies_and_withdraws_on_drop() {
        let dir = tempfile::tempdir().unwrap();
        let reg = Registration::register(dir.path(), goosed()).unwrap();
        reg.open_reply(hold(1, "chat-a")).unwrap();
        reg.set_reply_way(1, PlacementKey::single("local")).unwrap();
        let read = read_all(dir.path()).unwrap();
        let records = live(&read);
        assert_eq!(records.len(), 1);
        let HolderKind::Goosed { replies } = &records[0].kind else {
            panic!()
        };
        assert_eq!(replies[0].way, Some(PlacementKey::single("local")));
        let lock = reply_lock_path(dir.path(), records[0].pid, records[0].started_at, 1);
        assert!(lock_is_held(&lock).unwrap(), "an open reply holds its lock");

        reg.close_reply(1).unwrap();
        assert!(!lock.exists(), "a closed reply's lock is gone");
        let after = read_all(dir.path()).unwrap();
        let HolderKind::Goosed { replies } = &live(&after)[0].kind else {
            panic!()
        };
        assert!(replies.is_empty());
        drop(reg);
        assert!(read_all(dir.path()).unwrap().is_empty());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    /// A process that died without deregistering: its flock went with it; its record is displaced
    /// only once its pid is proven gone.
    #[test]
    fn a_dead_holder_is_stale_and_ignored_only_on_proof() {
        let dir = tempfile::tempdir().unwrap();
        let mut child = std::process::Command::new("/bin/sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let pid = child.id();
        let (started_at, _) = machine::process_start(pid).unwrap();
        let record = HolderRecord {
            pid,
            started_at,
            since: 0,
            kind: HolderKind::Goosed {
                replies: vec![hold(3, "gone-chat")],
            },
        };
        std::fs::write(
            dir.path().join(format!("{pid}-{started_at}.json")),
            serde_json::to_string(&record).unwrap(),
        )
        .unwrap();
        // Its lock is free (no flock), but the pid is alive: it stays a holder.
        assert_eq!(
            read_all(dir.path()).unwrap(),
            vec![HolderEntry::Live(record.clone())]
        );
        child.kill().unwrap();
        child.wait().unwrap();
        let read = read_all(dir.path()).unwrap();
        assert!(
            matches!(&read[..], [HolderEntry::Stale { record: r, .. }] if *r == record),
            "{read:?}"
        );
        assert!(
            read_all(dir.path()).unwrap().is_empty(),
            "the stale record was displaced"
        );
    }

    #[test]
    fn a_reply_says_whose_it_is_and_an_older_record_is_a_persons() {
        let old: ReplyHold =
            serde_json::from_str(r#"{"reply":1,"session":"s","rootSession":"s"}"#).unwrap();
        assert_eq!(old.kind, ReplyKind::User);
        let tick = ReplyHold {
            kind: ReplyKind::Tick,
            ..hold(2, "loop")
        };
        let text = serde_json::to_string(&tick).unwrap();
        assert!(text.contains(r#""kind":"tick""#), "{text}");
        assert_eq!(serde_json::from_str::<ReplyHold>(&text).unwrap(), tick);
    }

    /// Replies opening and closing on many threads at once: the published record always ends as
    /// the registration holds it, and every read of it along the way parses.
    #[test]
    fn concurrent_changes_publish_whole_records_and_lose_none() {
        let dir = tempfile::tempdir().unwrap();
        let reg = std::sync::Arc::new(Registration::register(dir.path(), goosed()).unwrap());
        let reader_dir = dir.path().to_path_buf();
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let reading = std::sync::Arc::clone(&stop);
        let reader = std::thread::spawn(move || {
            while !reading.load(std::sync::atomic::Ordering::SeqCst) {
                for entry in read_all(&reader_dir).unwrap() {
                    assert!(
                        !matches!(entry, HolderEntry::Unreadable { .. }),
                        "{entry:?}"
                    );
                }
            }
        });
        let writers: Vec<_> = (0..8u64)
            .map(|t| {
                let reg = std::sync::Arc::clone(&reg);
                std::thread::spawn(move || {
                    for i in 0..25u64 {
                        let n = t * 100 + i;
                        reg.open_reply(hold(n, &format!("chat-{n}"))).unwrap();
                        if i % 2 == 0 {
                            reg.close_reply(n).unwrap();
                        }
                    }
                })
            })
            .collect();
        for w in writers {
            w.join().unwrap();
        }
        stop.store(true, std::sync::atomic::Ordering::SeqCst);
        reader.join().unwrap();
        let read = read_all(dir.path()).unwrap();
        let HolderKind::Goosed { replies } = &live(&read)[0].kind else {
            panic!()
        };
        let HolderKind::Goosed { replies: held } = reg.record().kind else {
            panic!()
        };
        assert_eq!(replies.len(), 8 * 12, "every open reply is published");
        assert_eq!(*replies, held);
    }

    #[test]
    fn a_torn_record_is_unreadable_never_no_holder() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("123-456.json"), "{\"pid\": 123, \"star").unwrap();
        let read = read_all(dir.path()).unwrap();
        assert!(
            matches!(&read[..], [HolderEntry::Unreadable { .. }]),
            "{read:?}"
        );
    }

    /// Another process holding a reply (a python stand-in taking the same flock): a waiter here is
    /// woken the instant it lets go, with no clock involved.
    #[test]
    fn a_waiter_is_woken_when_another_process_ends_its_reply() {
        let dir = tempfile::tempdir().unwrap();
        let lock = dir.path().join("999-1-r7.lock");
        let script =
            "import fcntl, sys\nf = open(sys.argv[1], 'a+')\nfcntl.flock(f, fcntl.LOCK_EX)\n\
                      print('HELD', flush=True)\nsys.stdin.readline()\n";
        let mut other = std::process::Command::new("/usr/bin/python3")
            .args(["-c", script, lock.to_str().unwrap()])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let mut out = std::io::BufReader::new(other.stdout.take().unwrap());
        let mut line = String::new();
        out.read_line(&mut line).unwrap();
        assert_eq!(line.trim(), "HELD");
        assert!(lock_is_held(&lock).unwrap());

        let waiting = lock.clone();
        let waiter = std::thread::spawn(move || wait_for_reply_end(&waiting).unwrap());
        std::thread::sleep(crate::GRACE_TICK);
        assert!(!waiter.is_finished(), "the reply is open: the waiter waits");
        other.stdin.take().unwrap().write_all(b"\n").unwrap();
        other.wait().unwrap();
        waiter.join().unwrap();
        assert!(!lock_is_held(&lock).unwrap());
    }

    #[test]
    fn a_reply_that_already_ended_is_not_waited_for() {
        let dir = tempfile::tempdir().unwrap();
        wait_for_reply_end(&dir.path().join("1-1-r1.lock")).unwrap();
    }

    /// Two loaders (two open descriptions of the claim — the kernel treats them as two processes'
    /// flocks): one holds the swap claim, the other is refused, then waits and takes it on release.
    #[test]
    fn one_loader_holds_the_swap_claim_the_other_waits_for_it() {
        let dir = tempfile::tempdir().unwrap();
        let LoadLockAttempt::Acquired(first) =
            try_claim_swap(dir.path(), "switching to Flash", "flash").unwrap()
        else {
            panic!("an unheld claim is taken")
        };
        let LoadLockAttempt::Held(held) =
            try_claim_swap(dir.path(), "switching to 27B", "27b").unwrap()
        else {
            panic!("a held claim refuses a second loader")
        };
        assert_eq!(held.holder.unwrap().what, "switching to Flash");
        let path = dir.path().to_path_buf();
        let waiter = std::thread::spawn(move || {
            claim_swap_waiting(&path, "switching to 27B", "27b").unwrap()
        });
        std::thread::sleep(crate::GRACE_TICK);
        assert!(
            !waiter.is_finished(),
            "the second loader waits on the claim"
        );
        drop(first);
        let second = waiter.join().unwrap();
        assert_eq!(second.holder().what, "switching to 27B");
    }

    /// A loader in ANOTHER process (python taking the same flock and record format) holds the claim:
    /// this process's loader is refused while it holds, and takes it once it lets go.
    #[test]
    fn a_swap_claim_held_by_another_process_is_waited_for() {
        let dir = tempfile::tempdir().unwrap();
        let claim = dir.path().join(SWAP_CLAIM);
        // The stand-in writes start time 0: once it exits, its pid is proven gone either way.
        let script = "import fcntl, os, sys\nf = open(sys.argv[1], 'a+')\nfcntl.flock(f, fcntl.LOCK_EX)\n\
                      f.truncate(0); f.write('pid=%d\\nstarted=0\\nsince=0\\nwhat=the other window switches\\n' % os.getpid()); f.flush()\n\
                      print('HELD', flush=True)\nsys.stdin.readline()\n";
        let mut other = std::process::Command::new("/usr/bin/python3")
            .args(["-c", script, claim.to_str().unwrap()])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let mut out = std::io::BufReader::new(other.stdout.take().unwrap());
        let mut line = String::new();
        out.read_line(&mut line).unwrap();
        assert_eq!(line.trim(), "HELD");
        let LoadLockAttempt::Held(held) = try_claim_swap(dir.path(), "mine", "m").unwrap() else {
            panic!("the other process's claim refuses this loader")
        };
        assert_eq!(held.holder.unwrap().what, "the other window switches");
        let path = dir.path().to_path_buf();
        let waiter = std::thread::spawn(move || claim_swap_waiting(&path, "mine", "m"));
        std::thread::sleep(crate::GRACE_TICK);
        assert!(!waiter.is_finished());
        other.stdin.take().unwrap().write_all(b"\n").unwrap();
        other.wait().unwrap();
        // The other process exited holding a record whose start time is not its own: the waiter's
        // flock is free, and the record's pid is proven gone, so the claim is taken.
        let taken = waiter.join().unwrap().unwrap();
        assert_eq!(taken.holder().what, "mine");
    }
}
