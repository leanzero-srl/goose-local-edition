//! Which goosed runs this Mac's DISTRIBUTED MLX engine, for every other goosed on the Mac.
//!
//! The single engine is found across processes by its port: every goosed probes
//! `mlx_engine.port`. The distributed engine's supervisor lives inside ONE goosed (the window that
//! started it) and each desktop window runs its own goosed, so the others need a published fact:
//! the owning goosed writes this record (its pid, the engine's base URL and the id it serves) when
//! its start is accepted and withdraws it on stop, on a status read that finds its run over, and
//! at exit. Readers never trust the record blindly: a dead pid is `Stale` (named, ignored for
//! routing), an unparseable file is `Unreadable` (named, routing refuses to guess), and whether
//! the engine is up is decided by probing its `/v1/models`, exactly like the single engine's port.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::config::paths::Paths;

const RECORD_FILE: &str = "mlx-distributed-owner.json";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PublishedEngine {
    /// The goosed that supervises the ranks — the only process that may stop or restart them.
    pub pid: u32,
    pub base_url: String,
    /// `engine::served_model_id` over the owner's saved settings — the id a swarm node names.
    pub served_model_id: String,
    pub model_id: String,
    pub backend: String,
    pub node_names: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum OwnerRecord {
    Absent,
    /// Published by THIS process.
    Mine(PublishedEngine),
    /// Published by another live goosed (another window).
    Other(PublishedEngine),
    /// The publishing goosed is gone; its record outlived it (killed before it could withdraw).
    Stale(PublishedEngine),
    Unreadable {
        path: PathBuf,
        error: String,
    },
}

pub fn record_path() -> PathBuf {
    Paths::in_state_dir(RECORD_FILE)
}

pub fn publish(engine: &PublishedEngine) -> Result<()> {
    publish_at(&record_path(), engine)
}

/// Removes the record only when THIS process published it — a window never withdraws another's.
pub fn withdraw_if_mine() -> Result<bool> {
    withdraw_at(&record_path(), std::process::id())
}

pub fn read() -> OwnerRecord {
    read_at(&record_path(), std::process::id(), pid_alive)
}

pub(crate) fn publish_at(path: &Path, engine: &PublishedEngine) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    let tmp = path.with_extension(format!("json.{}", std::process::id()));
    std::fs::write(&tmp, serde_json::to_vec_pretty(engine)?)
        .with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("renaming onto {}", path.display()))
}

pub(crate) fn withdraw_at(path: &Path, self_pid: u32) -> Result<bool> {
    match read_at(path, self_pid, |_| true) {
        OwnerRecord::Mine(_) => {
            std::fs::remove_file(path).with_context(|| format!("removing {}", path.display()))?;
            Ok(true)
        }
        _ => Ok(false),
    }
}

pub(crate) fn read_at(path: &Path, self_pid: u32, alive: impl Fn(u32) -> bool) -> OwnerRecord {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return OwnerRecord::Absent,
        Err(e) => {
            return OwnerRecord::Unreadable {
                path: path.to_path_buf(),
                error: e.to_string(),
            }
        }
    };
    match serde_json::from_slice::<PublishedEngine>(&bytes) {
        Ok(engine) if engine.pid == self_pid => OwnerRecord::Mine(engine),
        Ok(engine) if alive(engine.pid) => OwnerRecord::Other(engine),
        Ok(engine) => OwnerRecord::Stale(engine),
        Err(e) => OwnerRecord::Unreadable {
            path: path.to_path_buf(),
            error: e.to_string(),
        },
    }
}

/// The base URL of the distributed engine THIS goosed supervises, when it serves.
#[cfg(unix)]
pub fn own_active_base_url() -> Option<String> {
    goose_sidecar::distributed::global_manager().active_base_url()
}

/// `goose_sidecar::distributed` compiles only on Unix, so no goosed on this platform can
/// supervise a distributed engine: there is no URL to report.
#[cfg(not(unix))]
pub fn own_active_base_url() -> Option<String> {
    None
}

#[cfg(unix)]
fn pid_alive(pid: u32) -> bool {
    let Ok(pid) = libc::pid_t::try_from(pid) else {
        return false;
    };
    // Signal 0 checks existence only; EPERM means it exists under another user.
    let delivered = unsafe { libc::kill(pid, 0) } == 0;
    delivered || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(not(unix))]
fn pid_alive(_pid: u32) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine(pid: u32) -> PublishedEngine {
        PublishedEngine {
            pid,
            base_url: "http://127.0.0.1:8191".to_string(),
            served_model_id: "mihai-qwen3.8-27b-atlassian-q8-mlx".to_string(),
            model_id: "Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx".to_string(),
            backend: "jaccl".to_string(),
            node_names: vec!["Mihai Macbook".to_string(), "Work’s Mac Studio".to_string()],
        }
    }

    #[test]
    fn the_record_names_its_owner_and_only_the_owner_withdraws_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state").join(RECORD_FILE);
        assert_eq!(read_at(&path, 10, |_| true), OwnerRecord::Absent);

        publish_at(&path, &engine(10)).unwrap();
        assert_eq!(read_at(&path, 10, |_| true), OwnerRecord::Mine(engine(10)));
        assert_eq!(read_at(&path, 20, |_| true), OwnerRecord::Other(engine(10)));
        assert_eq!(
            read_at(&path, 20, |_| false),
            OwnerRecord::Stale(engine(10))
        );

        assert!(
            !withdraw_at(&path, 20).unwrap(),
            "another window never withdraws it"
        );
        assert!(path.exists());
        assert!(withdraw_at(&path, 10).unwrap());
        assert_eq!(read_at(&path, 10, |_| true), OwnerRecord::Absent);
    }

    #[test]
    fn a_torn_record_is_unreadable_never_absent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(RECORD_FILE);
        std::fs::write(&path, b"{\"pid\": 1, \"base_u").unwrap();
        assert!(matches!(
            read_at(&path, 2, |_| true),
            OwnerRecord::Unreadable { .. }
        ));
    }

    #[test]
    fn this_process_is_alive_and_a_reaped_child_is_not() {
        assert!(pid_alive(std::process::id()));
        let mut child = std::process::Command::new("true").spawn().unwrap();
        let pid = child.id();
        child.wait().unwrap();
        assert!(!pid_alive(pid));
    }
}
