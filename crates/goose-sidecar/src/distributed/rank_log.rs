//! Every rank's output on disk (Q-114). goosed reads each rank it spawns — this Mac's, an ssh
//! peer's through its session; a Link peer's goosed reads the rank it hosts — and kept only the
//! last lines in memory ([`super::launch::RankLive::tail`]), with the memory and state reports
//! left out of even that. On 2026-09-26 a split stalled mid-generation, goosed stopped it after
//! 20 s of silence, and nothing anywhere said where rank 1's loop had stopped. Every line a rank
//! prints now also lands in a file under goose's state dir (`logs/distributed/`), stamped with
//! the instant goosed read it and the stream it came on, written before the in-memory tail sees
//! it — so the last `GOOSE_RANK_STATE` a hung rank printed, and the thread stacks it dumps on
//! SIGTERM, outlive the process and goosed's memory.
//!
//! Bounded by a measurement, not a size: the directory keeps at most
//! [`super::RANK_LOG_SHARE_OF_FREE_SPACE`] of the free space its volume has when a launch opens
//! its log. Half of that is history — opening a log removes the oldest rank logs (never one this
//! goosed still writes) until the rest fit — and half is the launch itself, in two generations: a
//! log that reaches a quarter moves to `<name>.1` (replacing the previous one) and starts again,
//! so a long-lived launch keeps its most recent output.

use std::collections::HashSet;
use std::ffi::OsString;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};

/// The logs this process writes now: pruning never removes one of them (two ranks of one launch
/// can log on this Mac — rank 0 and an ssh peer's session).
fn open_logs() -> &'static Mutex<HashSet<PathBuf>> {
    static OPEN: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();
    OPEN.get_or_init(|| Mutex::new(HashSet::new()))
}

/// Where goose's own logs live (`Paths::in_state_dir("logs")` in the goose crate, which this
/// crate cannot depend on): `GOOSE_PATH_ROOT/state` when set, else etcetera's XDG state dir for
/// the app — `$XDG_STATE_HOME/goose` when that is absolute, `~/.local/state/goose` otherwise.
pub fn goose_state_dir(
    path_root: Option<OsString>,
    xdg_state_home: Option<OsString>,
    home: Option<PathBuf>,
) -> Result<PathBuf> {
    if let Some(root) = path_root.filter(|r| !r.is_empty()) {
        return Ok(PathBuf::from(root).join("state"));
    }
    if let Some(xdg) = xdg_state_home
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
    {
        return Ok(xdg.join("goose"));
    }
    let home = home.context("no home directory to hold goose's state dir")?;
    Ok(home.join(".local/state/goose"))
}

/// The directory rank logs are written to on this Mac.
#[cfg(not(test))]
pub fn rank_log_dir() -> Result<PathBuf> {
    Ok(goose_state_dir(
        std::env::var_os("GOOSE_PATH_ROOT"),
        std::env::var_os("XDG_STATE_HOME"),
        dirs::home_dir(),
    )?
    .join("logs/distributed"))
}

/// Test builds never write into the owner's state dir: a stand-in rank's log goes to this test
/// process's own temp dir.
#[cfg(test)]
pub fn rank_log_dir() -> Result<PathBuf> {
    Ok(std::env::temp_dir()
        .join("goose-sidecar-test-rank-logs")
        .join(std::process::id().to_string()))
}

/// The bytes available to an unprivileged writer on the volume holding `dir`.
#[cfg(unix)]
pub fn free_bytes(dir: &Path) -> Result<u64> {
    use std::os::unix::ffi::OsStrExt;
    let c_path = std::ffi::CString::new(dir.as_os_str().as_bytes())
        .with_context(|| format!("{} holds a NUL byte", dir.display()))?;
    let mut stats: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: c_path is NUL-terminated and `stats` is a valid out-pointer for the call.
    if unsafe { libc::statvfs(c_path.as_ptr(), &mut stats) } != 0 {
        bail!(
            "statvfs {}: {}",
            dir.display(),
            std::io::Error::last_os_error()
        );
    }
    // The widths are the platform's (fsblkcnt_t is 32-bit on macOS, c_ulong 64-bit): widen both.
    #[allow(clippy::unnecessary_cast)]
    let free = stats.f_bavail as u64 * stats.f_frsize as u64;
    Ok(free)
}

#[cfg(not(unix))]
pub fn free_bytes(dir: &Path) -> Result<u64> {
    bail!(
        "no free-space measurement for {} on this platform",
        dir.display()
    )
}

/// "2026-09-26T07:07:37.123Z" — the instant goosed read a line, in UTC.
pub fn utc_stamp(at: SystemTime) -> String {
    let since = at.duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = since.as_secs();
    let (days, rest) = ((secs / 86_400) as i64, secs % 86_400);
    // Howard Hinnant's civil_from_days: days since 1970-01-01 to a proleptic Gregorian date.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        rest / 3_600,
        rest % 3_600 / 60,
        rest % 60,
        since.subsec_millis()
    )
}

fn slug(node: &str) -> String {
    let slug: String = node
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    slug.split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

fn is_rank_log(name: &str) -> bool {
    name.starts_with("rank") && (name.ends_with(".log") || name.ends_with(".log.1"))
}

/// Removes this directory's oldest rank logs until the rest hold at most `keep` bytes, skipping
/// every log this process is writing now. Returns what it removed.
fn prune(dir: &Path, keep: u64) -> Result<Vec<PathBuf>> {
    let open = open_logs().lock().unwrap().clone();
    let mut logs = Vec::new();
    for entry in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        let current = open.contains(&path)
            || name
                .strip_suffix(".1")
                .is_some_and(|base| open.contains(&dir.join(base)));
        if !is_rank_log(&name) || current {
            continue;
        }
        let meta = entry.metadata()?;
        logs.push((meta.modified()?, meta.len(), path));
    }
    logs.sort();
    let mut total: u64 = logs.iter().map(|(_, len, _)| len).sum();
    let mut removed = Vec::new();
    for (_, len, path) in logs {
        if total <= keep {
            break;
        }
        std::fs::remove_file(&path).with_context(|| format!("removing {}", path.display()))?;
        total -= len;
        removed.push(path);
    }
    Ok(removed)
}

/// One rank's durable log.
pub struct RankLog {
    path: PathBuf,
    file: File,
    written: u64,
    generation_bytes: u64,
}

impl RankLog {
    /// Opens a new log for `rank` on `node` in `dir`, bounded by the free space measured now.
    pub fn open(dir: &Path, rank: usize, node: &str) -> Result<Self> {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        let free = free_bytes(dir)?;
        let budget = (free as f64 * super::RANK_LOG_SHARE_OF_FREE_SPACE) as u64;
        Self::open_within(dir, rank, node, budget)
    }

    /// [`RankLog::open`] with the directory's budget given.
    pub fn open_within(dir: &Path, rank: usize, node: &str, budget: u64) -> Result<Self> {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        prune(dir, budget / 2)?;
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let path = dir.join(format!("rank{rank}-{}-{millis}.log", slug(node)));
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("opening {}", path.display()))?;
        open_logs().lock().unwrap().insert(path.clone());
        Ok(RankLog {
            path,
            file,
            written: 0,
            generation_bytes: budget / 4,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Appends one line the rank printed on `stream` (`out` / `err`), stamped now.
    pub fn append(&mut self, stream: &str, line: &str) -> Result<()> {
        let record = format!("{} {stream} {line}\n", utc_stamp(SystemTime::now()));
        if self.written > 0 && self.written + record.len() as u64 > self.generation_bytes {
            self.rotate()?;
        }
        self.file
            .write_all(record.as_bytes())
            .with_context(|| format!("writing {}", self.path.display()))?;
        self.written += record.len() as u64;
        Ok(())
    }

    fn previous_generation(&self) -> PathBuf {
        let mut name = self.path.as_os_str().to_owned();
        name.push(".1");
        PathBuf::from(name)
    }

    fn rotate(&mut self) -> Result<()> {
        let previous = self.previous_generation();
        std::fs::rename(&self.path, &previous)
            .with_context(|| format!("moving {} to {}", self.path.display(), previous.display()))?;
        self.file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .with_context(|| format!("reopening {}", self.path.display()))?;
        self.written = 0;
        Ok(())
    }
}

impl Drop for RankLog {
    fn drop(&mut self) {
        open_logs().lock().unwrap().remove(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_log_dir_is_goose_state_dir_as_goose_resolves_it() {
        let home = Some(PathBuf::from("/Users/me"));
        assert_eq!(
            goose_state_dir(None, None, home.clone()).unwrap(),
            PathBuf::from("/Users/me/.local/state/goose"),
            "etcetera's XDG default, where goosed writes logs/cli today"
        );
        assert_eq!(
            goose_state_dir(Some("/tmp/root".into()), Some("/x".into()), home.clone()).unwrap(),
            PathBuf::from("/tmp/root/state")
        );
        assert_eq!(
            goose_state_dir(None, Some("/var/state".into()), home.clone()).unwrap(),
            PathBuf::from("/var/state/goose")
        );
        assert_eq!(
            goose_state_dir(Some("".into()), Some("relative".into()), home).unwrap(),
            PathBuf::from("/Users/me/.local/state/goose"),
            "an empty root and a relative XDG_STATE_HOME are ignored, as etcetera ignores them"
        );
        assert!(goose_state_dir(None, None, None).is_err());
    }

    #[test]
    fn stamps_are_utc_calendar_instants() {
        // 2026-09-26T07:07:57.811Z — the hang line's own timestamp in the goosed log.
        let at = UNIX_EPOCH + std::time::Duration::from_millis(1_790_406_477_811);
        assert_eq!(utc_stamp(at), "2026-09-26T07:07:57.811Z");
        assert_eq!(utc_stamp(UNIX_EPOCH), "1970-01-01T00:00:00.000Z");
        let leap = UNIX_EPOCH + std::time::Duration::from_secs(951_782_400);
        assert_eq!(utc_stamp(leap), "2000-02-29T00:00:00.000Z");
    }

    #[test]
    fn a_log_keeps_every_line_stamped_and_rotates_at_a_quarter_of_its_budget() {
        let dir = tempfile::tempdir().unwrap();
        let mut log = RankLog::open_within(dir.path(), 1, "Work’s Mac Studio", 4 * 200).unwrap();
        let name = log
            .path()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert!(name.starts_with("rank1-work-s-mac-studio-"), "{name}");
        log.append("out", "GOOSE_RANK_STATE {\"steps\": 3041}")
            .unwrap();
        log.append("err", "Thread 0x1 (most recent call first):")
            .unwrap();
        let text = std::fs::read_to_string(log.path()).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].ends_with(" out GOOSE_RANK_STATE {\"steps\": 3041}"));
        assert!(lines[1].contains("Z err Thread 0x1"));
        for n in 0..20 {
            log.append("out", &format!("GOOSE_RANK_MEM {{\"n\": {n}}}"))
                .unwrap();
        }
        let current = std::fs::read_to_string(log.path()).unwrap();
        let previous = std::fs::read_to_string(log.previous_generation()).unwrap();
        assert!(current.len() as u64 <= 200 && previous.len() as u64 <= 200);
        assert!(
            current.contains("\"n\": 19"),
            "the newest line is in the current generation: {current}"
        );
    }

    #[test]
    fn opening_a_log_prunes_the_oldest_history_but_never_a_log_being_written() {
        let dir = tempfile::tempdir().unwrap();
        let budget = 4_000;
        let writing = RankLog::open_within(dir.path(), 0, "Mihai Macbook", budget).unwrap();
        std::fs::write(writing.path(), vec![b'x'; 1_500]).unwrap();
        let old = dir.path().join("rank1-studio-1.log");
        let newer = dir.path().join("rank1-studio-2.log");
        let unrelated = dir.path().join("notes.txt");
        // History 2,500 bytes against the half-budget's 2,000: the oldest goes, the rest fits.
        std::fs::write(&old, vec![b'x'; 1_500]).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(&newer, vec![b'x'; 1_000]).unwrap();
        std::fs::write(&unrelated, vec![b'x'; 5_000]).unwrap();

        let next = RankLog::open_within(dir.path(), 1, "studio", budget).unwrap();
        assert!(
            writing.path().exists(),
            "a log this goosed writes is never pruned"
        );
        assert!(!old.exists(), "the oldest history goes first");
        assert!(newer.exists(), "history that fits the half-budget stays");
        assert!(unrelated.exists(), "only rank logs are goose's to prune");
        assert!(next.path().exists());
    }
}
