//! What a node is busy with RIGHT NOW: the owner's own use of the Mac, the AI inference it
//! serves, and the jobs other machines placed on it. Served inside [`crate::wire::NodeState`]
//! as `load` on `GET /v1/swarm/nodes` (so it rides the fabric's existing 3 s poll and the
//! desktop's 5 s poll), and read by [`crate::jobs`] admission and `LinkManager::place_job`.
//!
//! The rules mirror `~/Projects/machine-sync/bin/node_fleet.py` + `heavy-run` (skill
//! `node-dispatch`), so a goose job and a `node-run` job judge a Mac the same way:
//! - memory per process is `phys_footprint` (`proc_pid_rusage` RUSAGE_INFO_V4), never RSS:
//!   RSS undercounts MLX/GPU memory (measured 31 MiB RSS vs a 4370 MB footprint);
//! - a closed lid only matters when it would actually sleep (`AppleClamshellCausesSleep`);
//! - a benchmark is detected by the launcher's live-pid marker, the `GOOSE_SWARM_BENCHMARK` env,
//!   an argv that says it IS a benchmark, then recent run files (rule in full at [`detect_benchmark`]);
//! - the heavy slot is the SAME `~/.cache/heavy-run.lock` flock heavy-run takes;
//! - limits come from the SAME `~/.leanzero/node-limits.json`, same keys, same defaults.
//!
//! Everything is read in-process (IOKit / libproc / notify FFI, spike S1 2026-10-25: 3.3 ms for
//! every read, no sudo, no shell-out). A value that cannot be read is `None`, never a made-up
//! number. The command line of a job is NEVER put on the wire: peers see id, kind and origin.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Seconds since the last keyboard/mouse input below which the owner counts as using the Mac
/// (node_fleet `user_idle_s`, 300). Tunable: `LEANZERO_LINK_JOBS_USER_IDLE_S`.
pub const DEFAULT_USER_IDLE_S: u64 = 300;

/// `com.apple.system.thermalpressurelevel` at or above which a heavy job is refused: 0 nominal,
/// 1 moderate, 2 heavy, 3 trapping, 4 sleeping. WHY 2: `IOPMCopyCPUPowerStatus` (node_fleet's
/// CPU_Speed_Limit < 80) returns 0xe00002f0 on Apple Silicon, so the pressure level is the only
/// in-process thermal signal; "heavy" is where macOS starts throttling clocks.
pub const THERMAL_REFUSE_LEVEL: u8 = 2;

/// The whole load report of one node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct NodeLoad {
    #[serde(default)]
    pub own: OwnLoad,
    #[serde(default)]
    pub inference: InferenceLoad,
    /// Jobs placed on this node (by itself or a peer), newest first, finished ones for a while.
    #[serde(default)]
    pub jobs: Vec<JobBrief>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sampled_at: Option<DateTime<Utc>>,
}

/// The Mac itself: the owner at the keyboard and the hardware's headroom.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct OwnLoad {
    /// Seconds since the last HID input (IOHIDSystem `HIDIdleTime`).
    #[serde(default)]
    pub user_idle_s: Option<f64>,
    /// `user_idle_s` below the node's user-idle threshold.
    #[serde(default)]
    pub user_active: bool,
    /// 1-minute load average divided by the logical CPU count.
    #[serde(default)]
    pub cpu_load_per_core: Option<f64>,
    #[serde(default)]
    pub ncpu: u32,
    /// Available memory (free + file cache + purgeable, Activity-Monitor style, from
    /// `goose_sidecar::memory::measure`) as % of RAM. NOTE: heavy-run reads `memory_pressure -Q`'s
    /// "free percentage", which is close but not identical; both are "what a new allocation can
    /// take without pushing anything out", and both read the same floor keys.
    #[serde(default)]
    pub mem_free_pct: Option<f64>,
    #[serde(default)]
    pub mem_total_gb: Option<f64>,
    /// IOAccelerator `PerformanceStatistics["Device Utilization %"]`.
    #[serde(default)]
    pub gpu_util_pct: Option<f64>,
    #[serde(default)]
    pub on_ac: Option<bool>,
    /// Thermal pressure level 0..4 (see [`THERMAL_REFUSE_LEVEL`]).
    #[serde(default)]
    pub thermal_limit: Option<u8>,
    /// The lid is closed AND the Mac would sleep because of it.
    #[serde(default)]
    pub lid_closed_sleeps: bool,
    /// EVERY heavy slot is held (by heavy-run, node-run or goose jobs): no heavy job fits.
    /// Kept for placers older than 3.0.131, which read it as "busy"; one held slot of several
    /// no longer sets it.
    #[serde(default)]
    pub heavy_slot_held: bool,
    /// Heavy slots held right now (`~/.cache/heavy-run.lock`, `.1`, ...).
    #[serde(default)]
    pub heavy_slots_used: u32,
    /// Heavy slots this Mac runs (`max_heavy`). 0 = an older report: read `heavy_slot_held`.
    #[serde(default)]
    pub heavy_slots_max: u32,
    /// How many more jobs of `default_need_gb` fit above the memory floor right now.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub can_take: Option<u32>,
    /// The node's own verdict WITH its numbers, e.g. `ok: 1/3 heavy, 94 GB free, load 0.28/core,
    /// can take ~11 more 8 GB jobs` or `busy: memory 12% free < 20% floor`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verdict: Option<String>,
    /// Why a benchmark is believed to be running, or `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub benchmark: Option<String>,
}

/// The AI work this node serves.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct InferenceLoad {
    /// The local engine's running + waiting requests (`/v1/status`) plus relay streams this
    /// node's control service is serving for peers.
    #[serde(default)]
    pub engine_active: u32,
    /// Swarm-router leases held in THIS goosed process ([`RouterSlot`]). Swarm CLI child
    /// processes route in their own process; their work shows up only via `engine_active`.
    #[serde(default)]
    pub router_slots_used: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobKind {
    Shell,
    Prompt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Queued,
    Running,
    Done,
    Failed,
    Killed,
    Cancelled,
    /// Cancel was asked and the group has not exited yet (TERM sent, KILL after the grace).
    Cancelling,
    /// The outcome is unknown: the job's process group was gone when goose came back (a
    /// restart, a crash), or an adopted job ended without leaving its exit status.
    Lost,
}

impl JobState {
    pub fn is_terminal(self) -> bool {
        !matches!(self, Self::Queued | Self::Running | Self::Cancelling)
    }
}

/// A job as peers see it: never the command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobBrief {
    pub job_id: String,
    pub origin_node: String,
    pub kind: JobKind,
    pub started_at: DateTime<Utc>,
    pub state: JobState,
    #[serde(default)]
    pub heavy: bool,
    /// Set on a job still waiting in its origin node's queue (not on any Mac yet).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue: Option<crate::jobs::QueueInfo>,
}

// ---------------------------------------------------------------------------------------------
// Router slots

static ROUTER_SLOTS: AtomicU32 = AtomicU32::new(0);

/// Held for the life of one swarm-router lease; the count is `router_slots_used`.
#[derive(Debug)]
pub struct RouterSlot(());

impl RouterSlot {
    pub fn acquire() -> Self {
        ROUTER_SLOTS.fetch_add(1, Ordering::SeqCst);
        Self(())
    }
}

impl Drop for RouterSlot {
    fn drop(&mut self) {
        ROUTER_SLOTS.fetch_sub(1, Ordering::SeqCst);
    }
}

pub fn router_slots_used() -> u32 {
    ROUTER_SLOTS.load(Ordering::SeqCst)
}

// ---------------------------------------------------------------------------------------------
// Limits (shared with heavy-run)

/// `~/.leanzero/node-limits.json`, keys and defaults exactly as heavy-run: timeout, max_load,
/// min_free, kill_free, max_rss_pct, wait, lock_wait, plus the measured-load keys max_heavy
/// (default `max(2, cores / 4)`), max_load_per_core (2.0, a backstop) and default_need_gb (8).
#[derive(Debug, Clone, PartialEq)]
pub struct NodeLimits {
    pub timeout_s: u64,
    pub max_load: f64,
    pub min_free: f64,
    pub kill_free: f64,
    pub max_rss_pct: f64,
    pub wait_s: u64,
    pub lock_wait_s: u64,
    /// node-run's placement strategy (`"strategy"`); goose's own setting overrides it.
    pub strategy: Option<crate::jobs::PlacementStrategy>,
    /// Concurrent heavy jobs this Mac runs (`"max_heavy"`, shared with heavy-run/node-run).
    /// `None` = [`default_max_heavy`] of the core count.
    pub max_heavy: Option<u32>,
    /// A heavy job is refused at or above this 1-minute load per core (`"max_load_per_core"`).
    /// A BACKSTOP only (owner 2026-10-25: "use resources to their fullest"): memory headroom
    /// is the real gate, CPU never blocks below 2.0 by default.
    pub max_load_per_core: f64,
    /// The memory a job without `need_mem_gb` is counted at in "can take ~N more" (`"default_need_gb"`).
    pub default_need_gb: f64,
}

/// `max(2, cores // 4)`, the shared default of heavy-run/node-run (owner 2026-10-25).
pub fn default_max_heavy(ncpu: u32) -> u32 {
    (ncpu / 4).max(2)
}

impl NodeLimits {
    /// The heavy slot count for a Mac with `ncpu` logical cores (at least 1).
    pub fn max_heavy_for(&self, ncpu: u32) -> u32 {
        self.max_heavy
            .unwrap_or_else(|| default_max_heavy(ncpu))
            .max(1)
    }
}

impl Default for NodeLimits {
    fn default() -> Self {
        Self {
            timeout_s: 1800,
            max_load: f64::INFINITY,
            min_free: 20.0,
            kill_free: 8.0,
            max_rss_pct: 50.0,
            wait_s: 900,
            lock_wait_s: 3600,
            strategy: None,
            max_heavy: None,
            max_load_per_core: 2.0,
            default_need_gb: 8.0,
        }
    }
}

pub fn limits_path() -> PathBuf {
    std::env::var_os("NODE_LIMITS_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".leanzero/node-limits.json"))
}

pub fn heavy_lock_path() -> PathBuf {
    std::env::var_os("HEAVY_RUN_LOCK")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".cache/heavy-run.lock"))
}

fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp"))
}

impl NodeLimits {
    /// Read the shared file. Absent = defaults. Unreadable/garbage = defaults plus a warning
    /// (heavy-run does the same: a broken limits file must not stop every job).
    pub fn load() -> Self {
        Self::from_json(std::fs::read_to_string(limits_path()).ok().as_deref())
    }

    pub fn from_json(text: Option<&str>) -> Self {
        let mut l = Self::default();
        let Some(text) = text else { return l };
        let v: serde_json::Value = match serde_json::from_str(text) {
            Ok(v) => v,
            Err(error) => {
                tracing::warn!(%error, "node-limits.json unparseable; using defaults");
                return l;
            }
        };
        let f = |k: &str| v.get(k).and_then(|x| x.as_f64());
        if let Some(x) = f("timeout") {
            l.timeout_s = x.max(1.0) as u64;
        }
        if let Some(x) = f("max_load") {
            l.max_load = x;
        }
        if let Some(x) = f("min_free") {
            l.min_free = x;
        }
        if let Some(x) = f("kill_free") {
            l.kill_free = x;
        }
        if let Some(x) = f("max_rss_pct") {
            l.max_rss_pct = x;
        }
        if let Some(x) = f("wait") {
            l.wait_s = x.max(0.0) as u64;
        }
        if let Some(x) = f("lock_wait") {
            l.lock_wait_s = x.max(0.0) as u64;
        }
        if let Some(x) = f("max_heavy") {
            l.max_heavy = Some(x.max(1.0) as u32);
        }
        if let Some(x) = f("max_load_per_core") {
            if x > 0.0 {
                l.max_load_per_core = x;
            }
        }
        if let Some(x) = f("default_need_gb") {
            if x >= 0.0 {
                l.default_need_gb = x;
            }
        }
        if let Some(s) = v.get("strategy").and_then(|x| x.as_str()) {
            l.strategy = crate::jobs::PlacementStrategy::parse(s);
            if l.strategy.is_none() {
                tracing::warn!(
                    strategy = s,
                    "node-limits.json strategy unknown; using local-first"
                );
            }
        }
        l
    }
}

// ---------------------------------------------------------------------------------------------
// Heavy slot (flock on the heavy-run lock)

/// The heavy slot, held while alive. Python's `fcntl.flock` is flock(2), so heavy-run and
/// node-run see this exactly as they see each other.
#[derive(Debug)]
pub struct HeavySlot {
    #[allow(dead_code)]
    file: std::fs::File,
}

impl HeavySlot {
    /// Non-blocking take. `None` = someone else holds it.
    pub fn try_take(holder: &str) -> std::io::Result<Option<Self>> {
        Self::try_take_at(&heavy_lock_path(), holder)
    }

    /// [`Self::try_take`] on an explicit lock file (the jobs service carries its path so tests
    /// never share the real `~/.cache/heavy-run.lock`).
    #[cfg(unix)]
    pub fn try_take_at(path: &std::path::Path, holder: &str) -> std::io::Result<Option<Self>> {
        use std::os::fd::AsRawFd;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path)?;
        // SAFETY: valid fd owned by `file`.
        let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if rc != 0 {
            let e = std::io::Error::last_os_error();
            if e.raw_os_error() == Some(libc::EWOULDBLOCK) {
                return Ok(None);
            }
            return Err(e);
        }
        let mut slot = Self { file };
        slot.write_holder(std::process::id() as i32, holder);
        Ok(Some(slot))
    }

    /// Rewrite the holder line in heavy-run's own format (`pid P since %F %T: label`, local
    /// time), so heavy-run, node-run and goose's refusals all read one shape. Informational:
    /// the flock is the truth. Goose calls it again with the job's pgid once it has spawned.
    pub fn write_holder(&mut self, pid: i32, label: &str) {
        use std::io::{Seek, Write};
        let _ = self.file.set_len(0);
        let _ = self.file.seek(std::io::SeekFrom::Start(0));
        let _ = writeln!(
            self.file,
            "{}",
            holder_line(pid, chrono::Local::now(), label)
        );
        let _ = self.file.flush();
    }

    #[cfg(not(unix))]
    pub fn write_holder(&mut self, _pid: i32, _label: &str) {}

    #[cfg(not(unix))]
    pub fn try_take_at(_path: &std::path::Path, _holder: &str) -> std::io::Result<Option<Self>> {
        Err(std::io::Error::other("heavy slot needs flock (unix)"))
    }

    /// Is slot 0 held right now (by anyone, this process included)? Tested with flock, not
    /// by reading the holder text.
    pub fn is_held() -> bool {
        slot_is_held(&heavy_lock_path())
    }

    /// Take ANY free slot of `max` (slot 0 = `base`, slot i = `base.i`, heavy-run/node-run's
    /// names), lowest first, non-blocking. `None` = all `max` are held.
    pub fn try_take_any(
        base: &std::path::Path,
        max: u32,
        holder: &str,
    ) -> std::io::Result<Option<(Self, u32)>> {
        let mut first_err = None;
        let mut any_held = false;
        for i in 0..max.max(1) {
            match Self::try_take_at(&heavy_slot_path(base, i), holder) {
                Ok(Some(s)) => return Ok(Some((s, i))),
                Ok(None) => any_held = true,
                Err(e) => {
                    first_err.get_or_insert(e);
                }
            }
        }
        // An error only when no slot could even be tried (an unwritable lock dir).
        match first_err {
            Some(e) if !any_held => Err(e),
            _ => Ok(None),
        }
    }
}

/// Slot `i`'s lock file: slot 0 is `base` itself (`~/.cache/heavy-run.lock`, so a goose that
/// knows one slot and heavy-run agree), slot i > 0 is `base.i`.
pub fn heavy_slot_path(base: &std::path::Path, i: u32) -> PathBuf {
    if i == 0 {
        return base.to_path_buf();
    }
    let mut s = base.as_os_str().to_os_string();
    s.push(format!(".{i}"));
    PathBuf::from(s)
}

fn slot_is_held(path: &std::path::Path) -> bool {
    match HeavySlot::try_take_at(path, "probe") {
        Ok(Some(slot)) => {
            drop(slot);
            false
        }
        Ok(None) => true,
        Err(_) => false,
    }
}

/// How many of the `max` heavy slots under `base` are held right now (flock probes).
pub fn heavy_slots_used(base: &std::path::Path, max: u32) -> u32 {
    (0..max.max(1))
        .filter(|i| slot_is_held(&heavy_slot_path(base, *i)))
        .count() as u32
}

/// heavy-run's holder line: `pid 4321 since 2026-10-25 14:57:52: cargo test -p goose`.
pub fn holder_line(pid: i32, since: chrono::DateTime<chrono::Local>, label: &str) -> String {
    format!("pid {pid} since {}: {label}", since.format("%F %T"))
}

/// The reason a heavy refusal carries: who holds the slot and for how long, read from the
/// lock file's holder line (heavy-run's format, goose's own, or the 3.0.128 goose shape
/// `pid P goose-job ID since RFC3339`). Short: no full paths, at most a few words.
pub fn describe_holder(text: &str, now: chrono::DateTime<chrono::Local>) -> String {
    let line = text.lines().map(str::trim).find(|l| !l.is_empty());
    let Some(line) = line else {
        return "heavy slot held (the holder has not written its line yet)".into();
    };
    let mut rest = line;
    let mut pid = None;
    if let Some(r) = rest.strip_prefix("pid ") {
        let (p, r) = r.split_once(' ').unwrap_or((r, ""));
        pid = p.parse::<i64>().ok();
        rest = r.trim_start();
    }
    let mut label = String::new();
    let mut since: Option<chrono::DateTime<chrono::Local>> = None;
    if let Some(r) = rest.strip_prefix("since ") {
        // heavy-run / goose >= 3.0.129: `since %F %T: label`.
        if r.len() >= 19 && r.is_char_boundary(19) {
            if let Ok(t) = chrono::NaiveDateTime::parse_from_str(&r[..19], "%F %T") {
                since = t.and_local_timezone(chrono::Local).earliest();
                label = r[19..].trim_start_matches(':').trim().to_string();
            }
        }
    } else if let Some((who, ts)) = rest.split_once(" since ") {
        // 3.0.128 goose: `goose-job ID since RFC3339`.
        since = chrono::DateTime::parse_from_rfc3339(ts.trim())
            .ok()
            .map(|t| t.with_timezone(&chrono::Local));
        label = who.trim().to_string();
    } else {
        label = rest.trim().to_string();
    }
    let who = if let Some(id) = label.strip_prefix("goose-job ") {
        let id = id.split_whitespace().next().unwrap_or("");
        format!("goose job {id}")
    } else if label.is_empty() {
        "heavy-run".into()
    } else {
        format!("heavy-run: {}", short_command(&label))
    };
    let mut out = String::from("heavy slot held by ");
    if let Some(p) = pid {
        out.push_str(&format!("pid {p} ({who})"));
    } else {
        out.push_str(&who);
    }
    if let Some(t) = since {
        let secs = (now - t).num_seconds().max(0) as u64;
        out.push_str(&format!(" for {}", human_duration(secs)));
    }
    out
}

/// `/opt/homebrew/bin/cargo test --config X -p goose --lib a::b` -> `cargo test -p goose --lib`:
/// path words lose their directories, `--config <value>` pairs go, at most 5 words / 48 chars.
fn short_command(cmd: &str) -> String {
    let mut words: Vec<String> = Vec::new();
    let mut it = cmd.split_whitespace();
    while let Some(w) = it.next() {
        if w == "--config" {
            it.next();
            continue;
        }
        if w.starts_with("--config=") {
            continue;
        }
        let w = if w.contains('/') {
            w.trim_end_matches('/').rsplit('/').next().unwrap_or(w)
        } else {
            w
        };
        words.push(w.to_string());
        if words.len() == 5 {
            break;
        }
    }
    let mut s = words.join(" ");
    if s.chars().count() > 48 {
        s = s.chars().take(47).collect::<String>() + "…";
    } else if it.next().is_some() {
        s.push_str(" …");
    }
    s
}

/// `372` -> `6m 12s`; `45` -> `45s`; `3725` -> `1h 2m`.
pub fn human_duration(secs: u64) -> String {
    match secs {
        s if s < 60 => format!("{s}s"),
        s if s < 3600 => format!("{}m {}s", s / 60, s % 60),
        s => format!("{}h {}m", s / 3600, (s % 3600) / 60),
    }
}

/// [`describe_holder`] over the lock file at `path` (unreadable = a bare "held").
/// [`heavy_holder_reason`] for every slot of `max`: `busy: 3/3 heavy slots held: <slot 0>; <slot 1>; ...`.
pub fn heavy_holders_reason(base: &std::path::Path, max: u32) -> String {
    let max = max.max(1);
    let who: Vec<String> = (0..max)
        .map(|i| heavy_holder_reason(&heavy_slot_path(base, i)))
        .collect();
    format!("busy: {max}/{max} heavy slots held: {}", who.join("; "))
}

pub fn heavy_holder_reason(path: &std::path::Path) -> String {
    match std::fs::read_to_string(path) {
        Ok(t) => describe_holder(&t, chrono::Local::now()),
        Err(_) => "heavy slot held".into(),
    }
}

// ---------------------------------------------------------------------------------------------
// Benchmark detection (node_fleet's rule)

/// True when `name` contains `benchmark` at its start or after a non-letter (`run_benchmark.py`,
/// `benchmark`), never inside a word (`microbenchmark`). Case-insensitive.
fn names_benchmark(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower
        .match_indices("benchmark")
        .any(|(at, _)| at == 0 || !lower.as_bytes()[at - 1].is_ascii_lowercase())
}

/// The goose env flag written into argv (`env GOOSE_SWARM_BENCHMARK=1 ...`).
fn argv_env_flag(args: &str) -> bool {
    args.split_whitespace().any(|t| {
        t.split_once('=').is_some_and(|(k, v)| {
            k.eq_ignore_ascii_case("GOOSE_SWARM_BENCHMARK")
                && ["1", "on", "true", "yes"]
                    .iter()
                    .any(|on| v.eq_ignore_ascii_case(on))
        })
    })
}

/// True when the ARGV says the process IS a benchmark, never when it only TALKS about one
/// (2026-10-25: the newsroom's `claude -p /leanzero-newsroom PREPARE TICK ... a benchmark result
/// ...` read as "benchmark running: prepare.py"; a flattened argv turns a quoted prompt into
/// words). Counted: the program, or a script in the first three arguments, whose basename names
/// it (`run_benchmark.py`, `benchmark`); a `benchmark`/`bench` subcommand right after the program
/// or script; a `-benchmark`/`--benchmark[=..]` flag; a path token (from `/`, `~` or `.`) with a
/// `/benchmark/` directory; a script under the app's `swarm-bench/bench/` payload in the script
/// position ([`is_swarm_bench_script`]); the goose env flag in argv. A prompt-carrying agent CLI (`claude`,
/// `codex`) is prose after its flags: only its program name counts. Same rule as node_fleet.py's
/// `argv_is_benchmark`.
pub fn argv_is_benchmark(args: &str) -> bool {
    let a: Vec<&str> = args.split_whitespace().collect();
    let Some(first) = a.first() else {
        return false;
    };
    let prog = first
        .rsplit('/')
        .next()
        .unwrap_or(first)
        .to_ascii_lowercase();
    if names_benchmark(&prog) {
        return true;
    }
    if prog == "claude" || prog == "codex" {
        return false;
    }
    if argv_env_flag(args) {
        return true;
    }
    let head = &a[1..a.len().min(4)];
    // The script position: the program itself (a shebang run) or the first non-flag argument.
    let script_pos = head.iter().find(|x| !x.starts_with('-'));
    if is_swarm_bench_script(&first.to_ascii_lowercase())
        || script_pos.is_some_and(|x| is_swarm_bench_script(&x.to_ascii_lowercase()))
    {
        return true;
    }
    const SCRIPT_EXT: [&str; 6] = [".py", ".js", ".mjs", ".sh", ".ts", ".rs"];
    for (k, x) in head.iter().enumerate() {
        let xl = x.to_ascii_lowercase();
        if xl.contains('/') || SCRIPT_EXT.iter().any(|e| xl.ends_with(e)) {
            if names_benchmark(xl.rsplit('/').next().unwrap_or(&xl)) {
                return true;
            }
        } else if xl == "benchmark" || xl == "bench" {
            let after_script = head[..k]
                .iter()
                .any(|y| y.contains('/') || y.ends_with(".py"));
            if k <= 1 + usize::from(after_script) {
                return true;
            }
        }
    }
    for x in &a[1..] {
        if x.starts_with('-') || x.starts_with('\'') || x.starts_with('"') {
            let xl = x.to_ascii_lowercase();
            let flag = xl.trim_start_matches('-');
            let dashes = xl.len() - flag.len();
            if (1..=2).contains(&dashes) && (flag == "benchmark" || flag.starts_with("benchmark="))
            {
                return true;
            }
        } else if x.contains('/')
            && (x.starts_with('/') || x.starts_with('~') || x.starts_with('.'))
            && x.len() < 400
            && x.to_ascii_lowercase().contains("/benchmark/")
        {
            return true;
        }
    }
    false
}

/// A script the app's benchmark payload runs: `<...>/swarm-bench/bench/<name>.py|.mjs|...`
/// (packaged `Contents/Resources/swarm-bench/bench/run_build.py`, dev `evals/swarm-bench/...`).
/// Its argv never says "benchmark" (D5 leftover, 3.0.130). Only checked in the program's
/// script position, so prose naming the file is not a hit.
fn is_swarm_bench_script(lower: &str) -> bool {
    const SCRIPT_EXT: [&str; 4] = [".py", ".js", ".mjs", ".sh"];
    lower
        .rsplit_once("/swarm-bench/bench/")
        .is_some_and(|(_, file)| {
            !file.contains('/') && SCRIPT_EXT.iter().any(|e| file.ends_with(e))
        })
}

/// Processes that look at benchmarks without being one (node_fleet's exclusion).
fn is_observer(args: &str) -> bool {
    let first = args.split_whitespace().next().unwrap_or("");
    let base = first.rsplit('/').next().unwrap_or(first);
    matches!(
        base,
        "grep" | "rg" | "find" | "ps" | "tail" | "less" | "cat"
    ) || ["node_fleet", "node-probe", "node-run", "node-jobs"]
        .iter()
        .any(|n| args.contains(n))
}

fn benchmark_dir() -> PathBuf {
    home().join("Library/Application Support/Goose/benchmark/runs/build")
}

/// Any file modified in the last `window` under `dir`, up to depth 3 (node_fleet `-maxdepth 3 -mmin -2`).
pub fn recent_file_under(dir: &std::path::Path, window: Duration) -> bool {
    fn walk(dir: &std::path::Path, depth: u32, cutoff: SystemTime, budget: &mut u32) -> bool {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return false;
        };
        for entry in rd.flatten() {
            if *budget == 0 {
                return false;
            }
            *budget -= 1;
            let Ok(md) = entry.metadata() else { continue };
            if md.modified().map(|m| m >= cutoff).unwrap_or(false) {
                return true;
            }
            if md.is_dir() && depth < 3 && walk(&entry.path(), depth + 1, cutoff, budget) {
                return true;
            }
        }
        false
    }
    let cutoff = SystemTime::now() - window;
    let mut budget = 20_000;
    walk(dir, 1, cutoff, &mut budget)
}

/// Where launchers mark a running benchmark: `~/.leanzero/benchmarks/<pid>.json`.
pub fn benchmark_marker_dir() -> PathBuf {
    home().join(".leanzero/benchmarks")
}

/// How much later than the marker's `started_at` its pid may have started and still be the runner
/// (the launcher writes the marker right AFTER spawning, so the real runner is never younger).
const MARKER_PID_SLACK_S: i64 = 120;

#[derive(Deserialize)]
struct BenchMarker {
    pid: u32,
    label: String,
    started_at: String,
}

/// The sysinfo refresh benchmark detection needs: argv, environment and start time.
fn bench_refresh_kind() -> sysinfo::ProcessRefreshKind {
    use sysinfo::{ProcessRefreshKind, UpdateKind};
    ProcessRefreshKind::nothing()
        .with_cmd(UpdateKind::Always)
        .with_environ(UpdateKind::Always)
}

/// The first LIVE marker in `dir`, as "<label> <started_at, UTC minute>Z (pid N)". A marker whose
/// pid is gone, or whose pid now belongs to a process that started more than
/// [`MARKER_PID_SLACK_S`] after `started_at` (a reused pid), is stale and ignored, so a crashed
/// launcher never leaves "benchmark running" behind. Unparseable markers are ignored.
fn marker_reason(dir: &std::path::Path, sys: &sysinfo::System) -> Option<String> {
    let mut rd: Vec<_> = std::fs::read_dir(dir).ok()?.flatten().collect();
    rd.sort_by_key(|e| e.file_name());
    for entry in rd {
        let Ok(text) = std::fs::read_to_string(entry.path()) else {
            continue;
        };
        let Ok(m) = serde_json::from_str::<BenchMarker>(&text) else {
            continue;
        };
        let Ok(started) = DateTime::parse_from_rfc3339(&m.started_at) else {
            continue;
        };
        let started = started.with_timezone(&Utc);
        let Some(p) = sys.process(sysinfo::Pid::from_u32(m.pid)) else {
            continue;
        };
        // 0 = start time unknown: the live pid alone decides (node_fleet does the same).
        let proc_start = p.start_time() as i64;
        if proc_start != 0 && proc_start > started.timestamp() + MARKER_PID_SLACK_S {
            continue;
        }
        return Some(format!(
            "{} {} (pid {})",
            m.label,
            started.format("%Y-%m-%dT%H:%MZ"),
            m.pid
        ));
    }
    None
}

/// `GOOSE_SWARM_BENCHMARK` set to an ON value (`1`/`on`/`true`/`yes`, the engine's own reading).
fn env_flag_on(entry: &str) -> bool {
    entry
        .strip_prefix("GOOSE_SWARM_BENCHMARK=")
        .map(|v| {
            matches!(
                v.trim().to_ascii_lowercase().as_str(),
                "1" | "on" | "true" | "yes"
            )
        })
        .unwrap_or(false)
}

/// Live processes (other than this one) whose environment carries the benchmark flag, lowest pid
/// first (the launcher's runner before the engines it spawned). Only same-user environments are
/// readable on macOS, which is the user a benchmark runs as.
fn env_benchmark_pids(sys: &sysinfo::System) -> Vec<u32> {
    let me = std::process::id();
    let mut pids: Vec<u32> = sys
        .processes()
        .iter()
        .filter(|(pid, p)| {
            pid.as_u32() != me
                && p.environ()
                    .iter()
                    .any(|e| env_flag_on(&e.to_string_lossy()))
        })
        .map(|(pid, _)| pid.as_u32())
        .collect();
    pids.sort_unstable();
    pids
}

/// A process named for a person: its script's basename (`run_build.py`), else its program's
/// basename. Never an interpreter path.
fn run_name<S: AsRef<str>>(cmd: &[S]) -> String {
    let base = |s: &str| s.rsplit('/').next().unwrap_or(s).to_string();
    let script = cmd.iter().skip(1).map(AsRef::as_ref).find(|a| {
        !a.starts_with('-')
            && [".py", ".js", ".mjs", ".sh", ".ts"]
                .iter()
                .any(|x| a.ends_with(x))
    });
    match (script, cmd.first()) {
        (Some(s), _) => base(s),
        (None, Some(p)) => base(p.as_ref()),
        (None, None) => "unknown".into(),
    }
}

/// THE RULE, identical in `~/Projects/machine-sync/bin/node_fleet.py` (PROBE) and the launcher
/// `ui/desktop/src/benchMarker.ts`; change all three together. First hit wins:
/// 1. a live marker `~/.leanzero/benchmarks/<pid>.json` {pid, label, started_at} (see
///    [`marker_reason`]); the desktop writes it for every build run and rescore it spawns;
/// 2. a live process whose ENV has `GOOSE_SWARM_BENCHMARK` on (the desktop sets it on the runner,
///    run_build passes it to every engine), named by its script;
/// 3. a process whose ARGV says it is one ([`argv_is_benchmark`]), not one that mentions the word;
/// 4. a run file under `.../Goose/benchmark/runs/build` modified in the last 2 min.
///
/// Why: the packaged app runs `.../Resources/swarm-bench/bench/run_build.py`, whose argv never says
/// "benchmark", so 3 alone missed it and only 4 caught it (and not through a quiet phase > 2 min).
fn detect_benchmark() -> Option<String> {
    use sysinfo::{ProcessesToUpdate, System};
    let mut sys = System::new();
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, bench_refresh_kind());
    if let Some(r) = marker_reason(&benchmark_marker_dir(), &sys) {
        return Some(r);
    }
    let cmd_of = |pid: u32| -> Vec<String> {
        sys.process(sysinfo::Pid::from_u32(pid))
            .map(|p| {
                p.cmd()
                    .iter()
                    .map(|s| s.to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default()
    };
    if let Some(&pid) = env_benchmark_pids(&sys).first() {
        return Some(format!("{} (pid {pid})", run_name(&cmd_of(pid))));
    }
    let me = std::process::id();
    let mut argv_hits: Vec<u32> = sys
        .processes()
        .keys()
        .map(|p| p.as_u32())
        .filter(|&pid| pid != me)
        .filter(|&pid| {
            let args = cmd_of(pid).join(" ");
            argv_is_benchmark(&args) && !is_observer(&args)
        })
        .collect();
    argv_hits.sort_unstable();
    if let Some(&pid) = argv_hits.first() {
        return Some(format!("{} (pid {pid})", run_name(&cmd_of(pid))));
    }
    if recent_file_under(&benchmark_dir(), Duration::from_secs(120)) {
        return Some("benchmark run files written in the last 2 min".into());
    }
    None
}

static BENCH_CACHE: Mutex<Option<(Instant, Option<String>)>> = Mutex::new(None);

/// [`detect_benchmark`], cached ~3 s (a full process scan per `/nodes` call is waste).
pub fn benchmark_running() -> Option<String> {
    if let Ok(v) = std::env::var("LEANZERO_LINK_FAKE_BENCHMARK") {
        // Test seam: lets the control-service tests prove "benchmark refuses jobs" without a
        // real benchmark run.
        return (!v.is_empty()).then_some(v);
    }
    let mut cache = BENCH_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((at, v)) = cache.as_ref() {
        if at.elapsed() < Duration::from_secs(3) {
            return v.clone();
        }
    }
    let v = detect_benchmark();
    *cache = Some((Instant::now(), v.clone()));
    v
}

// ---------------------------------------------------------------------------------------------
// The OWN sampler

/// One synchronous read of everything in [`OwnLoad`] except `benchmark` and
/// `heavy_slot_held`, which [`sample_own`] adds. A few ms; call from `spawn_blocking`.
pub fn sample_hardware(user_idle_threshold_s: u64) -> OwnLoad {
    let mut own = OwnLoad::default();
    let ncpu = std::thread::available_parallelism()
        .map(|n| n.get() as u32)
        .unwrap_or(1);
    own.ncpu = ncpu;
    #[cfg(unix)]
    {
        let mut la = [0f64; 3];
        // SAFETY: writes up to 3 doubles into `la`.
        if unsafe { libc::getloadavg(la.as_mut_ptr(), 3) } >= 1 {
            own.cpu_load_per_core = Some(round2(la[0] / ncpu.max(1) as f64));
        }
    }
    if let Ok(m) = goose_sidecar::measure() {
        if m.total_bytes > 0 {
            own.mem_free_pct = Some(round1(
                m.available_bytes as f64 * 100.0 / m.total_bytes as f64,
            ));
            own.mem_total_gb = Some(round1(m.total_bytes as f64 / (1u64 << 30) as f64));
        }
    }
    #[cfg(target_os = "macos")]
    {
        own.user_idle_s = mac::hid_idle_ns().map(|ns| round1(ns as f64 / 1e9));
        own.gpu_util_pct = mac::gpu_util_pct();
        own.on_ac = mac::on_ac_power();
        own.thermal_limit = mac::thermal_pressure();
        let closed = mac::root_domain_bool("AppleClamshellState").unwrap_or(false);
        // Unknown "would it sleep" with the lid closed counts as sleeping (node_fleet:
        // `clamshell_sleeps is not False`).
        let sleeps = mac::root_domain_bool("AppleClamshellCausesSleep").unwrap_or(true);
        own.lid_closed_sleeps = closed && sleeps;
    }
    own.user_active = own
        .user_idle_s
        .map(|s| s < user_idle_threshold_s as f64)
        .unwrap_or(false);
    own
}

/// The full OWN reading (hardware + benchmark + heavy slots).
pub fn sample_own(user_idle_threshold_s: u64) -> OwnLoad {
    let mut own = sample_hardware(user_idle_threshold_s);
    let max = NodeLimits::load().max_heavy_for(own.ncpu);
    own.heavy_slots_max = max;
    own.heavy_slots_used = heavy_slots_used(&heavy_lock_path(), max);
    own.heavy_slot_held = own.heavy_slots_used >= max;
    own.benchmark = benchmark_running();
    own
}

fn round1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}
fn round2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

// ---------------------------------------------------------------------------------------------
// Process-group memory (the watchdog's reading)

/// `max(resident_size, phys_footprint)` of one pid, bytes. `None` = gone / unreadable.
#[cfg(target_os = "macos")]
pub fn pid_memory_bytes(pid: i32) -> Option<u64> {
    mac::rusage_mem(pid)
}

#[cfg(not(target_os = "macos"))]
pub fn pid_memory_bytes(pid: i32) -> Option<u64> {
    // Linux: VmRSS from /proc (there is no footprint; GPU memory is not counted there).
    let s = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    let line = s.lines().find(|l| l.starts_with("VmRSS:"))?;
    let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb * 1024)
}

/// Sum of [`pid_memory_bytes`] over every pid in process group `pgid`. `None` when no member
/// could be read at all (the caller counts consecutive `None`s and kills at 3, like heavy-run).
#[cfg(unix)]
pub fn group_memory_bytes(pgid: i32) -> Option<u64> {
    let pids = all_pids();
    let mut total = 0u64;
    let mut read_any = false;
    for pid in pids {
        // SAFETY: getpgid has no memory effects.
        if unsafe { libc::getpgid(pid) } != pgid {
            continue;
        }
        if let Some(b) = pid_memory_bytes(pid) {
            total += b;
            read_any = true;
        }
    }
    read_any.then_some(total)
}

#[cfg(target_os = "macos")]
fn all_pids() -> Vec<i32> {
    mac::list_pids()
}

#[cfg(all(unix, not(target_os = "macos")))]
fn all_pids() -> Vec<i32> {
    std::fs::read_dir("/proc")
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| e.file_name().to_str()?.parse().ok())
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(target_os = "macos")]
mod mac {
    //! Raw FFI copied from spike S1 (2026-10-25, Gabee macOS 27; values matched `ioreg`,
    //! `footprint -p` and `pmset`).
    use core_foundation_sys::base::{
        kCFAllocatorDefault, CFAllocatorRef, CFGetTypeID, CFRelease, CFTypeRef,
    };
    use core_foundation_sys::dictionary::{
        CFDictionaryGetTypeID, CFDictionaryGetValue, CFDictionaryRef, CFMutableDictionaryRef,
    };
    use core_foundation_sys::number::{
        kCFBooleanTrue, kCFNumberSInt64Type, CFBooleanGetTypeID, CFNumberGetTypeID,
        CFNumberGetValue, CFNumberRef,
    };
    use core_foundation_sys::string::{
        kCFStringEncodingUTF8, CFStringCreateWithCString, CFStringGetCString, CFStringRef,
    };
    use std::ffi::{c_char, c_void, CString};

    type IoObj = u32;

    #[link(name = "IOKit", kind = "framework")]
    extern "C" {
        fn IOServiceMatching(name: *const c_char) -> CFMutableDictionaryRef;
        fn IOServiceGetMatchingServices(port: u32, m: CFDictionaryRef, it: *mut IoObj) -> i32;
        fn IOIteratorNext(it: IoObj) -> IoObj;
        fn IOObjectRelease(o: IoObj) -> i32;
        fn IORegistryEntryCreateCFProperty(
            e: IoObj,
            k: CFStringRef,
            a: CFAllocatorRef,
            o: u32,
        ) -> CFTypeRef;
        fn IOPSCopyPowerSourcesInfo() -> CFTypeRef;
        fn IOPSGetProvidingPowerSourceType(snap: CFTypeRef) -> CFStringRef;
    }
    extern "C" {
        fn proc_pid_rusage(pid: i32, flavor: i32, buf: *mut c_void) -> i32;
        fn proc_listallpids(buf: *mut c_void, size: i32) -> i32;
        fn notify_register_check(name: *const c_char, tok: *mut i32) -> u32;
        fn notify_get_state(tok: i32, st: *mut u64) -> u32;
        fn notify_cancel(tok: i32) -> u32;
    }

    struct Cf(CFTypeRef);
    impl Drop for Cf {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: we own one retain from a Create/Copy call.
                unsafe { CFRelease(self.0) }
            }
        }
    }

    fn cfs(s: &str) -> Cf {
        let c = CString::new(s).expect("no NUL");
        // SAFETY: valid C string.
        Cf(unsafe {
            CFStringCreateWithCString(kCFAllocatorDefault, c.as_ptr(), kCFStringEncodingUTF8)
                as CFTypeRef
        })
    }

    fn num(v: CFTypeRef) -> Option<i64> {
        // SAFETY: type-checked before the cast.
        unsafe {
            if v.is_null() || CFGetTypeID(v) != CFNumberGetTypeID() {
                return None;
            }
            let mut x: i64 = 0;
            CFNumberGetValue(
                v as CFNumberRef,
                kCFNumberSInt64Type,
                &mut x as *mut i64 as *mut c_void,
            )
            .then_some(x)
        }
    }

    /// The property `key` of every service of IOKit class `class`.
    fn props(class: &str, key: &str) -> Vec<Cf> {
        let mut out = Vec::new();
        let c = CString::new(class).expect("no NUL");
        // SAFETY: IOServiceGetMatchingServices consumes the matching dict; each object is released.
        unsafe {
            let m = IOServiceMatching(c.as_ptr());
            if m.is_null() {
                return out;
            }
            let mut it: IoObj = 0;
            if IOServiceGetMatchingServices(0, m as CFDictionaryRef, &mut it) != 0 {
                return out;
            }
            let k = cfs(key);
            loop {
                let e = IOIteratorNext(it);
                if e == 0 {
                    break;
                }
                let v =
                    IORegistryEntryCreateCFProperty(e, k.0 as CFStringRef, kCFAllocatorDefault, 0);
                if !v.is_null() {
                    out.push(Cf(v));
                }
                IOObjectRelease(e);
            }
            IOObjectRelease(it);
        }
        out
    }

    pub fn gpu_util_pct() -> Option<f64> {
        let k = cfs("Device Utilization %");
        props("IOAccelerator", "PerformanceStatistics")
            .iter()
            .filter_map(|v| {
                // SAFETY: type-checked dictionary lookup; the value is borrowed (Get rule).
                unsafe {
                    if CFGetTypeID(v.0) != CFDictionaryGetTypeID() {
                        return None;
                    }
                    num(CFDictionaryGetValue(v.0 as CFDictionaryRef, k.0))
                }
            })
            .max()
            .map(|x| x as f64)
    }

    pub fn hid_idle_ns() -> Option<i64> {
        props("IOHIDSystem", "HIDIdleTime")
            .iter()
            .filter_map(|v| num(v.0))
            .min()
    }

    pub fn root_domain_bool(key: &str) -> Option<bool> {
        let v = props("IOPMrootDomain", key);
        let v = v.first()?;
        // SAFETY: type check then pointer compare with the singleton.
        unsafe {
            if CFGetTypeID(v.0) != CFBooleanGetTypeID() {
                return None;
            }
            Some(v.0 == kCFBooleanTrue as CFTypeRef)
        }
    }

    pub fn on_ac_power() -> Option<bool> {
        // SAFETY: the snapshot is released by Cf; the type string is borrowed from it.
        unsafe {
            let snap = Cf(IOPSCopyPowerSourcesInfo());
            if snap.0.is_null() {
                return None;
            }
            let t = IOPSGetProvidingPowerSourceType(snap.0);
            if t.is_null() {
                return None;
            }
            let mut buf = [0 as c_char; 64];
            if CFStringGetCString(t, buf.as_mut_ptr(), 64, kCFStringEncodingUTF8) == 0 {
                return None;
            }
            let s = std::ffi::CStr::from_ptr(buf.as_ptr()).to_string_lossy();
            Some(s == "AC Power")
        }
    }

    pub fn thermal_pressure() -> Option<u8> {
        let n = CString::new("com.apple.system.thermalpressurelevel").expect("no NUL");
        let mut tok = 0;
        let mut st = 0u64;
        // SAFETY: plain out-params; the token is cancelled.
        unsafe {
            if notify_register_check(n.as_ptr(), &mut tok) != 0 {
                return None;
            }
            let rc = notify_get_state(tok, &mut st);
            notify_cancel(tok);
            (rc == 0).then_some(st.min(255) as u8)
        }
    }

    /// RUSAGE_INFO_V4: resident_size at [64..72], phys_footprint at [72..80] (spike S1).
    pub fn rusage_mem(pid: i32) -> Option<u64> {
        let mut buf = [0u8; 512];
        // SAFETY: 512 bytes > sizeof(rusage_info_v4) (296).
        let rc = unsafe { proc_pid_rusage(pid, 4, buf.as_mut_ptr() as *mut c_void) };
        if rc != 0 {
            return None;
        }
        let resident = u64::from_le_bytes(buf[64..72].try_into().ok()?);
        let footprint = u64::from_le_bytes(buf[72..80].try_into().ok()?);
        Some(resident.max(footprint))
    }

    pub fn list_pids() -> Vec<i32> {
        let mut v = vec![0i32; 8192];
        // SAFETY: size in bytes of `v`.
        let n = unsafe { proc_listallpids(v.as_mut_ptr() as *mut c_void, (v.len() * 4) as i32) };
        if n <= 0 {
            return Vec::new();
        }
        v.truncate(n as usize);
        v.retain(|&p| p > 0);
        v
    }
}

// ---------------------------------------------------------------------------------------------
// The snapshot `/v1/swarm/nodes` attaches to `self`

/// Env key of the user-idle threshold in seconds (default [`DEFAULT_USER_IDLE_S`]).
pub const USER_IDLE_ENV: &str = "LEANZERO_LINK_JOBS_USER_IDLE_S";

/// The user-idle threshold this node reports `user_active` against: `LEANZERO_LINK_JOBS_USER_IDLE_S`
/// when set to a positive integer, else [`DEFAULT_USER_IDLE_S`].
pub fn user_idle_threshold_s() -> u64 {
    std::env::var(USER_IDLE_ENV)
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(DEFAULT_USER_IDLE_S)
}

/// `num_running + num_waiting` from an engine `/v1/status` body; `None` when neither is a number.
pub fn engine_active_from_status(body: &str) -> Option<u32> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    let running = v.get("num_running").and_then(|n| n.as_u64());
    let waiting = v.get("num_waiting").and_then(|n| n.as_u64());
    if running.is_none() && waiting.is_none() {
        return None;
    }
    Some((running.unwrap_or(0) + waiting.unwrap_or(0)).min(u32::MAX as u64) as u32)
}

/// The engine at `base` (loopback) asked for its `/v1/status`, 500 ms total (node_fleet reads
/// 127.0.0.1:8090/v1/status the same way). An engine that does not answer serves nothing: 0.
pub async fn engine_active(client: &reqwest::Client, base: &str) -> u32 {
    let url = format!("{}/v1/status", base.trim_end_matches('/'));
    let Ok(Ok(resp)) =
        tokio::time::timeout(Duration::from_millis(500), client.get(&url).send()).await
    else {
        return 0;
    };
    match tokio::time::timeout(Duration::from_millis(500), resp.text()).await {
        Ok(Ok(body)) => engine_active_from_status(&body).unwrap_or(0),
        _ => 0,
    }
}

/// One full load snapshot: OWN sampled on a blocking thread (IOKit + proc reads, ~3 ms),
/// `engine_active` as the caller measured it, this process's router slots, and `jobs`.
pub async fn snapshot(engine_active: u32, jobs: Vec<JobBrief>) -> NodeLoad {
    let threshold = user_idle_threshold_s();
    snapshot_with(
        engine_active,
        jobs,
        std::sync::Arc::new(move || sample_own(threshold)),
    )
    .await
}

/// [`snapshot`] with the OWN sampler supplied by the caller (the jobs registry's).
pub async fn snapshot_with(
    engine_active: u32,
    jobs: Vec<JobBrief>,
    sampler: std::sync::Arc<dyn Fn() -> OwnLoad + Send + Sync>,
) -> NodeLoad {
    let own = tokio::task::spawn_blocking(move || sampler())
        .await
        .unwrap_or_default();
    NodeLoad {
        own,
        inference: InferenceLoad {
            engine_active,
            router_slots_used: router_slots_used(),
        },
        jobs,
        sampled_at: Some(Utc::now()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sys_now() -> sysinfo::System {
        let mut sys = sysinfo::System::new();
        sys.refresh_processes_specifics(
            sysinfo::ProcessesToUpdate::All,
            true,
            bench_refresh_kind(),
        );
        sys
    }

    fn write_marker(dir: &std::path::Path, pid: u32, label: &str, started_at: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(
            dir.join(format!("{pid}.json")),
            format!(r#"{{"pid":{pid},"label":"{label}","started_at":"{started_at}"}}"#),
        )
        .unwrap();
    }

    #[test]
    fn a_marker_with_a_live_pid_is_a_running_benchmark_named_by_its_label() {
        let dir = tempfile::tempdir().unwrap();
        let me = std::process::id();
        let now = Utc::now();
        write_marker(dir.path(), me, "Gauntlet build run", &now.to_rfc3339());
        let reason = marker_reason(dir.path(), &sys_now());
        assert_eq!(
            reason,
            Some(format!(
                "Gauntlet build run {} (pid {me})",
                now.format("%Y-%m-%dT%H:%MZ")
            ))
        );
    }

    #[test]
    fn a_marker_whose_pid_died_or_was_reused_is_stale() {
        let dir = tempfile::tempdir().unwrap();
        let mut child = std::process::Command::new("true").spawn().unwrap();
        let dead = child.id();
        child.wait().unwrap();
        write_marker(
            dir.path(),
            dead,
            "Forge build run",
            &Utc::now().to_rfc3339(),
        );
        // a live pid that started long AFTER the marker's run: a reused pid, not the runner
        write_marker(
            dir.path(),
            std::process::id(),
            "Old run",
            "2000-01-01T00:00:00Z",
        );
        std::fs::write(dir.path().join("junk.json"), "{not json").unwrap();
        assert_eq!(marker_reason(dir.path(), &sys_now()), None);
    }

    /// The env test's child: this test binary itself, parked. A platform binary (`/bin/sleep`)
    /// hides its environment on macOS; a benchmark runner (python, goose) does not.
    #[test]
    #[ignore]
    fn env_probe_child() {
        if std::env::var_os("LZ_ENV_PROBE_CHILD").is_some() {
            std::thread::sleep(Duration::from_secs(30));
        }
    }

    #[test]
    fn a_live_process_with_the_benchmark_env_is_a_running_benchmark() {
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "load::tests::env_probe_child", "--ignored"])
            .env("GOOSE_SWARM_BENCHMARK", "1")
            .env("LZ_ENV_PROBE_CHILD", "1")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let pid = child.id();
        std::thread::sleep(Duration::from_millis(300));
        let found = env_benchmark_pids(&sys_now());
        let _ = child.kill();
        let _ = child.wait();
        assert!(found.contains(&pid), "pid {pid} not in {found:?}");
        assert!(env_flag_on("GOOSE_SWARM_BENCHMARK=on"));
        assert!(!env_flag_on("GOOSE_SWARM_BENCHMARK=0"));
        assert!(!env_flag_on("GOOSE_SWARM_BENCHMARKX=1"));
    }

    #[test]
    fn a_run_is_named_by_its_script_never_the_interpreter_path() {
        let py = [
            "/Library/Frameworks/Python.framework/Versions/3.12/Resources/Python.app/Contents/MacOS/Python",
            "/Applications/Goose.app/Contents/Resources/swarm-bench/bench/run_build.py",
            "--entrant",
            "swarm-3node",
        ];
        assert_eq!(run_name(&py), "run_build.py");
        assert_eq!(run_name(&["/usr/local/bin/goose", "run"]), "goose");
        assert_eq!(run_name(&[] as &[&str]), "unknown");
    }

    #[test]
    fn argv_says_it_is_a_benchmark() {
        assert!(argv_is_benchmark("goose run benchmark --x"));
        assert!(argv_is_benchmark("goose bench run --suite gauntlet"));
        assert!(argv_is_benchmark("goose --benchmark"));
        assert!(argv_is_benchmark("cli -benchmark=1"));
        assert!(argv_is_benchmark("env GOOSE_SWARM_BENCHMARK=1 goose"));
        assert!(argv_is_benchmark(
            "/usr/bin/env GOOSE_SWARM_BENCHMARK=1 python3 x.py"
        ));
        assert!(argv_is_benchmark("BENCHMARK"));
        assert!(argv_is_benchmark("python3 /x/run_benchmark.py"));
        assert!(argv_is_benchmark("node /x/benchmark/run.mjs"));
        assert!(argv_is_benchmark("python3 run.py --benchmark"));
        assert!(argv_is_benchmark(
            "python3 /x/y/run_build.py /h/Goose/benchmark/runs/build/a"
        ));
        assert!(!argv_is_benchmark("python3 -c \"print('benchmark')\""));
        assert!(!argv_is_benchmark("run benchmarks"));
        assert!(!argv_is_benchmark("microbenchmark"));
        assert!(!argv_is_benchmark("goose --benchmarks"));
        assert!(!argv_is_benchmark(
            "python3 /x/prepare.py write about the benchmark board"
        ));
    }

    /// D5 leftover (3.0.130): the app's own runner, `.../Resources/swarm-bench/bench/run_build.py`
    /// (and the dev tree's `evals/swarm-bench/bench/...`), never says "benchmark" in its argv.
    #[test]
    fn the_apps_swarm_bench_runner_is_a_benchmark() {
        let py = "/Library/Frameworks/Python.framework/Versions/3.12/Resources/Python.app/Contents/MacOS/Python";
        assert!(argv_is_benchmark(&format!(
            "{py} /Applications/Goose.app/Contents/Resources/swarm-bench/bench/run_build.py \
             --entrant swarm-3node --only-rep 0 --timeout 0 --out /tmp/o"
        )));
        assert!(argv_is_benchmark(
            "python3 /Users/x/Projects/goose/evals/swarm-bench/bench/bench_rescore.py /tmp/run"
        ));
        assert!(argv_is_benchmark(
            "/usr/bin/python3 -u /A/Goose.app/Contents/Resources/swarm-bench/bench/run_build.py"
        ));
        // Mentioning the runner in prose, or a non-script file there, is not running it.
        assert!(!argv_is_benchmark(
            "claude -p fix swarm-bench/bench/run_build.py /x/swarm-bench/bench/run_build.py"
        ));
        assert!(!argv_is_benchmark(
            "python3 /x/newsroom/prepare.py see /a/swarm-bench/bench/run_build.py benchmark"
        ));
        assert!(!argv_is_benchmark("cat /a/swarm-bench/bench/notes.txt"));
        assert!(!argv_is_benchmark(
            "python3 /x/swarm-bench-notes/bench/run_build.py"
        ));
    }

    /// 2026-10-25 live false positive: node-probe marked the workhorse BUSY "benchmark running:
    /// prepare.py (pid 37048)" for a newsroom session whose prompt mentions benchmarks.
    #[test]
    fn a_prompt_that_mentions_benchmarks_is_not_a_benchmark() {
        let prompt = "/leanzero-newsroom PREPARE TICK STEP 1: any article whose subject is a \
                      benchmark result follows references/benchmark-narratives.md; the benchmark \
                      board --benchmark /Goose/benchmark/runs/build --quiet";
        assert!(!argv_is_benchmark(&format!("claude -p {prompt}")));
        assert!(!argv_is_benchmark(&format!(
            "/opt/homebrew/bin/claude --dangerously-skip-permissions -p {prompt}"
        )));
        assert!(!argv_is_benchmark(
            "python3 /x/newsroom/prepare.py STEP 1: any article whose subject is a benchmark \
             result follows references/benchmark-narratives.md; the benchmark board --quiet"
        ));
        assert!(!argv_is_benchmark(
            "python3 /x/newsroom/prepare.py -p /leanzero-newsroom STEP 1: a benchmark result \
             the benchmark board --quiet"
        ));
        assert!(!argv_is_benchmark(
            "codex exec run the benchmark --benchmark"
        ));
    }

    #[test]
    fn observers_are_not_benchmarks() {
        assert!(is_observer("grep benchmark"));
        assert!(is_observer("/usr/bin/grep -r benchmark"));
        assert!(is_observer("python3 node_fleet.py benchmark"));
        assert!(!is_observer("goose benchmark"));
    }

    #[test]
    fn limits_strategy_is_node_runs_key() {
        assert_eq!(NodeLimits::from_json(None).strategy, None);
        assert_eq!(
            NodeLimits::from_json(Some(r#"{"strategy": "selfish"}"#)).strategy,
            Some(crate::jobs::PlacementStrategy::Selfish)
        );
        assert_eq!(
            NodeLimits::from_json(Some(r#"{"strategy": "local-first", "timeout": 60}"#)).strategy,
            Some(crate::jobs::PlacementStrategy::LocalFirst)
        );
        // An unknown name is not a strategy (the caller falls back to local-first).
        assert_eq!(
            NodeLimits::from_json(Some(r#"{"strategy": "greedy"}"#)).strategy,
            None
        );
    }

    #[test]
    fn limits_defaults_and_overrides() {
        let d = NodeLimits::from_json(None);
        assert_eq!(d, NodeLimits::default());
        assert_eq!(d.timeout_s, 1800);
        assert!(d.max_load.is_infinite());
        let l = NodeLimits::from_json(Some(r#"{"timeout": 60, "min_free": 30, "kill_free": 5}"#));
        assert_eq!(l.timeout_s, 60);
        assert_eq!(l.min_free, 30.0);
        assert_eq!(l.kill_free, 5.0);
        assert_eq!(l.max_rss_pct, 50.0);
        assert_eq!(
            NodeLimits::from_json(Some("not json")),
            NodeLimits::default()
        );
    }

    #[test]
    fn router_slot_counts_and_releases() {
        let before = router_slots_used();
        let a = RouterSlot::acquire();
        let b = RouterSlot::acquire();
        assert_eq!(router_slots_used(), before + 2);
        drop(a);
        drop(b);
        assert_eq!(router_slots_used(), before);
    }

    #[test]
    fn old_peer_load_json_parses() {
        // A peer that sends only part of the report (a newer field missing) still parses.
        let l: NodeLoad = serde_json::from_str(r#"{"own":{"user_active":true}}"#).unwrap();
        assert!(l.own.user_active);
        assert_eq!(l.inference.engine_active, 0);
        assert!(l.jobs.is_empty());
    }

    #[test]
    fn job_brief_never_has_a_command_field() {
        let b = JobBrief {
            job_id: "j1".into(),
            origin_node: "studio".into(),
            kind: JobKind::Shell,
            started_at: Utc::now(),
            state: JobState::Running,
            heavy: true,
            queue: None,
        };
        let s = serde_json::to_string(&b).unwrap();
        assert!(!s.contains("command"), "{s}");
        assert!(s.contains("\"kind\":\"shell\""));
    }

    #[test]
    fn recent_file_detection() {
        let d = tempfile::tempdir().unwrap();
        assert!(!recent_file_under(d.path(), Duration::from_secs(120)));
        std::fs::create_dir_all(d.path().join("a/b")).unwrap();
        std::fs::write(d.path().join("a/b/x"), "1").unwrap();
        assert!(recent_file_under(d.path(), Duration::from_secs(120)));
        assert!(!recent_file_under(
            &d.path().join("missing"),
            Duration::from_secs(120)
        ));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn mac_hardware_sampler_reads_real_values() {
        let own = sample_hardware(DEFAULT_USER_IDLE_S);
        assert!(own.ncpu >= 1);
        assert!(own.cpu_load_per_core.is_some(), "{own:?}");
        let free = own.mem_free_pct.expect("memory readable");
        assert!((0.0..=100.0).contains(&free), "{free}");
        assert!(own.mem_total_gb.unwrap() > 1.0);
        assert!(own.user_idle_s.is_some(), "HIDIdleTime readable: {own:?}");
        let g = own.gpu_util_pct.expect("IOAccelerator readable");
        assert!((0.0..=100.0).contains(&g));
        assert!(own.on_ac.is_some());
        assert!(own.thermal_limit.is_some());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn own_process_footprint_and_group() {
        let me = std::process::id() as i32;
        let b = pid_memory_bytes(me).expect("own footprint");
        assert!(b > 1 << 20, "{b}");
        // SAFETY: getpgrp has no memory effects.
        let pg = unsafe { libc::getpgrp() };
        let g = group_memory_bytes(pg).expect("group readable");
        assert!(g >= b);
        assert_eq!(group_memory_bytes(i32::MAX - 7), None);
    }

    #[test]
    fn engine_status_body_counts_running_plus_waiting() {
        assert_eq!(
            engine_active_from_status(r#"{"status":"generating","num_running":1,"num_waiting":2}"#),
            Some(3)
        );
        assert_eq!(engine_active_from_status(r#"{"num_running":0}"#), Some(0));
        assert_eq!(engine_active_from_status(r#"{"status":"ok"}"#), None);
        assert_eq!(engine_active_from_status("not json"), None);
    }

    #[tokio::test]
    async fn an_engine_that_does_not_answer_counts_zero_within_the_cap() {
        let started = Instant::now();
        // Port 9 (discard) on loopback: refused or silent, never a status body.
        let n = engine_active(&reqwest::Client::new(), "http://127.0.0.1:9").await;
        assert_eq!(n, 0);
        assert!(started.elapsed() < Duration::from_millis(1500));
    }

    #[tokio::test]
    async fn snapshot_carries_router_slots_and_a_sample_time() {
        let _slot = RouterSlot::acquire();
        let load = snapshot(4, Vec::new()).await;
        assert_eq!(load.inference.engine_active, 4);
        assert!(load.inference.router_slots_used >= 1);
        assert!(load.sampled_at.is_some());
    }

    #[test]
    fn holder_description_names_who_and_how_long() {
        let now = chrono::Local::now();
        let at = |secs: i64| {
            (now - chrono::Duration::seconds(secs))
                .format("%F %T")
                .to_string()
        };
        let hr = format!(
            "pid 66379 since {}: cargo test --config profile.dev.package.\"*\".strip=false -p goose --lib providers::swarm_router\n",
            at(372)
        );
        assert_eq!(
            describe_holder(&hr, now),
            "heavy slot held by pid 66379 (heavy-run: cargo test -p goose --lib …) for 6m 12s"
        );
        let py = format!(
            "pid 7 since {}: /usr/bin/python3 /Users/x/Projects/a/run.py\n",
            at(45)
        );
        assert_eq!(
            describe_holder(&py, now),
            "heavy slot held by pid 7 (heavy-run: python3 run.py) for 45s"
        );
        let goose = holder_line(
            812,
            now - chrono::Duration::seconds(3725),
            "goose-job j1-abc from laptop",
        );
        assert_eq!(
            describe_holder(&goose, now),
            "heavy slot held by pid 812 (goose job j1-abc) for 1h 2m"
        );
        let old = format!(
            "pid 9 goose-job j2-def since {}",
            (now - chrono::Duration::seconds(5))
                .with_timezone(&Utc)
                .to_rfc3339()
        );
        assert_eq!(
            describe_holder(&old, now),
            "heavy slot held by pid 9 (goose job j2-def) for 5s"
        );
        assert!(describe_holder("", now).contains("has not written"));
    }
}
