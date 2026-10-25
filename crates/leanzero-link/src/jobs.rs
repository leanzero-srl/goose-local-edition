//! Remote jobs: a same-account peer (or this node's own desktop) asks THIS node to run a shell
//! command or a goose prompt, and the node decides from its own resources whether it can.
//!
//! The rules mirror `~/Projects/machine-sync/bin/heavy-run` and `node_fleet.py` (machine-sync
//! a789c6c, skill `node-dispatch`) so goose jobs and `heavy-run`/`node-run` jobs behave as one
//! system on a Mac:
//! - ONE heavy slot per machine: the SAME `~/.cache/heavy-run.lock` flock, held for the job's
//!   whole life (python's `fcntl.flock` is flock(2), so each side sees the other).
//! - Admission is resources, not just "Idle": benchmark running = refuse; memory headroom above
//!   the `min_free` floor must cover `need_mem_gb`; a heavy job also needs the slot, AC power,
//!   no thermal pressure; a lid that would sleep refuses everything; the owner at the keyboard
//!   is the LAST resort (policy below). Every refusal carries a machine-readable
//!   [`RefusalCode`].
//! - Shell jobs run `/bin/zsh -lc <command>` in their own process group under a 1 s watchdog:
//!   timeout -> 124, group memory (sum of `max(resident, phys_footprint)` over the group, since
//!   RSS undercounts MLX/GPU memory 100x) over `max_rss_pct` of RAM or system free under
//!   `kill_free` or memory unreadable 3x -> 137, cancel -> 143; TERM the group, KILL after 10 s.
//! - Prompt jobs wrap the node's [`RemoteExecutor`] (what `/v1/swarm/execute` runs).
//! - Every job is recorded under `~/.leanzero/jobs/<id>/` (`job.json` + `log`), with its origin.
//! - The command line NEVER leaves the node: HTTP answers and `NodeLoad.jobs` carry id, kind,
//!   origin, state and timings only (owner decision: any shell command is allowed, so peers
//!   must not see them). The ledger on disk keeps it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::Notify;

use crate::load::{
    heavy_holder_reason, sample_own, user_idle_threshold_s, HeavySlot, JobBrief, JobKind, JobState,
    NodeLimits, NodeLoad, OwnLoad, THERMAL_REFUSE_LEVEL,
};
use crate::state::{ExecuteRequest, RemoteExecutor, SwarmStateSource};

/// Config key of the switch (goose reads it; this crate only sees [`JobsSwitch`]).
pub const ALLOW_REMOTE_JOBS_KEY: &str = "LEANZERO_LINK_ALLOW_REMOTE_JOBS";
/// Overrides `node-limits.json`'s `min_free` for job admission (percent).
pub const MIN_FREE_ENV: &str = "LEANZERO_LINK_JOBS_MIN_FREE";
/// `allow` | `last_resort` (default) | `refuse`.
pub const USER_ACTIVE_POLICY_ENV: &str = "LEANZERO_LINK_JOBS_USER_ACTIVE";
/// The ledger root (default `~/.leanzero/jobs`).
pub const JOBS_DIR_ENV: &str = "LEANZERO_LINK_JOBS_DIR";
/// Exit codes, as heavy-run.
pub const EXIT_TIMEOUT: i32 = 124;
pub const EXIT_MEMORY: i32 = 137;
pub const EXIT_CANCEL: i32 = 143;
/// Consecutive unreadable memory samples before the watchdog kills (heavy-run MEM_FAIL_KILL).
const MEM_FAIL_KILL: u32 = 3;
/// Finished jobs stay in `NodeLoad.jobs` this long.
const BRIEF_KEEP: Duration = Duration::from_secs(600);
const BRIEF_MAX: usize = 20;
/// One `/log` read returns at most this many bytes.
pub const LOG_CHUNK_MAX: u64 = 256 * 1024;
/// Cancel waits for the group this long past the TERM->KILL grace before answering
/// `cancelling` (the KILL itself and the reap are near instant).
const CANCEL_SLACK: Duration = Duration::from_secs(3);
/// A ledger job that ended this long ago is not loaded back into the list after a restart.
const LEDGER_KEEP_HOURS: i64 = 24;
/// The shell's exit status, written by the wrapper (`<job>/exit`), so a job that outlived the
/// goose that started it still leaves its outcome for the goose that adopts it.
const EXIT_FILE: &str = "exit";

// ---------------------------------------------------------------------------------------------
// Wire

/// Body of `POST /v1/swarm/jobs`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JobRequest {
    pub kind: JobKind,
    /// The shell command line (`kind=shell`) or the goose prompt (`kind=prompt`).
    pub command: String,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub need_mem_gb: Option<f64>,
    #[serde(default)]
    pub heavy: bool,
    /// `None` = `node-limits.json` `timeout` (1800 s).
    #[serde(default)]
    pub timeout_s: Option<u64>,
    /// The node that placed the job (shown to every peer). `None` = "unknown".
    #[serde(default)]
    pub origin_node: Option<String>,
    /// The placer found no idle node and chose this one knowing its owner is at the keyboard
    /// (node_fleet's USER-ACTIVE last resort).
    #[serde(default)]
    pub allow_user_active: bool,
}

/// `202` body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobAccepted {
    pub job_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefusalCode {
    JobsDisabled,
    BadRequest,
    BenchmarkRunning,
    MemoryUnreadable,
    MemoryHeadroom,
    HeavySlotHeld,
    OnBattery,
    Thermal,
    LidSleeps,
    UserActive,
    ExecutorMissing,
    NotFound,
    CancelUnsupported,
    AlreadyFinished,
    Internal,
}

impl RefusalCode {
    /// The HTTP status each code answers with. Disabled = 403 (as `/execute`); the node's
    /// state right now (benchmark, memory unreadable) = 503; a resource it lacks = 409.
    pub fn http_status(self) -> u16 {
        match self {
            Self::JobsDisabled => 403,
            Self::BadRequest => 400,
            Self::NotFound => 404,
            Self::ExecutorMissing | Self::CancelUnsupported => 501,
            Self::BenchmarkRunning | Self::MemoryUnreadable => 503,
            Self::Internal => 500,
            Self::MemoryHeadroom
            | Self::HeavySlotHeld
            | Self::OnBattery
            | Self::Thermal
            | Self::LidSleeps
            | Self::UserActive
            | Self::AlreadyFinished => 409,
        }
    }
}

/// Every non-2xx answer of the jobs routes: `{code, reason}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Refusal {
    pub code: RefusalCode,
    pub reason: String,
}

impl Refusal {
    pub fn new(code: RefusalCode, reason: impl Into<String>) -> Self {
        Self {
            code,
            reason: reason.into(),
        }
    }
}

/// `GET /v1/swarm/jobs/{id}` (and each row of the list): the brief plus the outcome. No command.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JobView {
    #[serde(flatten)]
    pub brief: JobBrief,
    #[serde(default)]
    pub ended_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub exit_code: Option<i32>,
    #[serde(default)]
    pub reason: Option<String>,
    /// The goose session a prompt job drives.
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub log_bytes: u64,
}

/// `GET /v1/swarm/jobs/{id}/log?from=N`: bytes `[from, next)` of the combined stdout+stderr.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobLogChunk {
    pub from: u64,
    pub next: u64,
    pub data: String,
    /// The job has ended and `next` is the end of the log.
    pub done: bool,
}

/// The ledger's `job.json` (the only place the command is kept).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JobRecord {
    pub job_id: String,
    pub origin_node: String,
    pub kind: JobKind,
    pub command: String,
    pub cwd: Option<String>,
    pub heavy: bool,
    pub need_mem_gb: Option<f64>,
    pub timeout_s: u64,
    pub state: JobState,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub exit_code: Option<i32>,
    pub reason: Option<String>,
    pub session_id: Option<String>,
    pub pgid: Option<i32>,
}

// ---------------------------------------------------------------------------------------------
// Admission (pure)

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UserActivePolicy {
    /// Take jobs while the owner is typing.
    Allow,
    /// Take them only when the placer says no idle node was left (`allow_user_active`).
    #[default]
    LastResort,
    /// Never while the owner is typing.
    Refuse,
}

impl UserActivePolicy {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().replace('-', "_").as_str() {
            "allow" => Some(Self::Allow),
            "last_resort" => Some(Self::LastResort),
            "refuse" => Some(Self::Refuse),
            _ => None,
        }
    }

    pub fn from_env() -> Self {
        std::env::var(USER_ACTIVE_POLICY_ENV)
            .ok()
            .and_then(|v| Self::parse(&v))
            .unwrap_or_default()
    }
}

/// node_fleet's verdict classes: OK, USER-ACTIVE (last resort), or BUSY with reasons.
#[derive(Debug, Clone, PartialEq)]
pub struct Verdict {
    /// The first blocking reason (BUSY), or `None`.
    pub busy: Option<Refusal>,
    pub user_active: bool,
    /// node_fleet `score`: GB above the floor minus need, minus 10 x load per core, minus 20
    /// off AC. Higher is better.
    pub score: f64,
    pub avail_gb: Option<f64>,
}

/// GB of memory above the `min_free` floor (node_fleet `avail`).
pub fn avail_gb(own: &OwnLoad, min_free: f64) -> Option<f64> {
    let free = own.mem_free_pct?;
    let total = own.mem_total_gb?;
    Some(((total * (free - min_free) / 100.0) * 10.0).round() / 10.0)
}

/// node_fleet `verdict()` over one node's OWN load, for a job needing `need_gb` (heavy or not).
pub fn verdict(own: &OwnLoad, need_gb: f64, heavy: bool, min_free: f64) -> Verdict {
    let mut busy: Option<Refusal> = None;
    let mut block = |code, why: String| {
        if busy.is_none() {
            busy = Some(Refusal::new(code, why));
        }
    };
    if let Some(b) = &own.benchmark {
        block(
            RefusalCode::BenchmarkRunning,
            format!("benchmark running: {b}"),
        );
    }
    let avail = avail_gb(own, min_free);
    match (own.mem_free_pct, avail) {
        (Some(free), Some(av)) => {
            if av < need_gb || free < min_free {
                block(
                    RefusalCode::MemoryHeadroom,
                    format!(
                        "memory: {free}% free, {av} GB above the {min_free}% floor < need {need_gb} GB"
                    ),
                );
            }
        }
        _ => block(RefusalCode::MemoryUnreadable, "memory unreadable".into()),
    }
    if heavy {
        if own.heavy_slot_held {
            block(RefusalCode::HeavySlotHeld, "heavy slot held".into());
        }
        if own.on_ac == Some(false) {
            block(RefusalCode::OnBattery, "on battery".into());
        }
        if let Some(t) = own.thermal_limit {
            if t >= THERMAL_REFUSE_LEVEL {
                block(
                    RefusalCode::Thermal,
                    format!("thermal pressure level {t} (>= {THERMAL_REFUSE_LEVEL})"),
                );
            }
        }
    }
    if own.lid_closed_sleeps {
        block(RefusalCode::LidSleeps, "lid closed (it would sleep)".into());
    }
    let lpc = own.cpu_load_per_core.unwrap_or(0.0);
    let mut score = avail.unwrap_or(0.0) - need_gb - 10.0 * lpc;
    if own.on_ac != Some(true) {
        score -= 20.0;
    }
    Verdict {
        busy,
        user_active: own.user_active,
        score: (score * 10.0).round() / 10.0,
        avail_gb: avail,
    }
}

/// Admission on the RECEIVING node: the verdict plus the user-active policy.
pub fn admit(
    own: &OwnLoad,
    req: &JobRequest,
    min_free: f64,
    policy: UserActivePolicy,
) -> Result<(), Refusal> {
    let v = verdict(own, req.need_mem_gb.unwrap_or(0.0), req.heavy, min_free);
    if let Some(r) = v.busy {
        return Err(r);
    }
    if v.user_active {
        let ok = match policy {
            UserActivePolicy::Allow => true,
            UserActivePolicy::LastResort => req.allow_user_active,
            UserActivePolicy::Refuse => false,
        };
        if !ok {
            let idle = own
                .user_idle_s
                .map(|s| format!("{s:.0}s since last input"))
                .unwrap_or_else(|| "recent input".into());
            return Err(Refusal::new(
                RefusalCode::UserActive,
                format!("user active ({idle}); policy {policy:?}"),
            ));
        }
    }
    Ok(())
}

/// One candidate of [`rank_placements`].
#[derive(Debug, Clone, PartialEq)]
pub struct Placement {
    pub node_id: String,
    pub user_active: bool,
    pub score: f64,
}

/// node_fleet `order_candidates`: nodes that may take the job (they allow jobs and have a
/// load report), OK ones first, then USER-ACTIVE as the last resort, each by score; BUSY nodes
/// are not candidates. `min_free` is the PLACER's floor (each receiver re-checks with its own).
pub fn rank_placements<'a>(
    nodes: impl IntoIterator<Item = (&'a str, bool, Option<&'a NodeLoad>)>,
    need_gb: f64,
    heavy: bool,
    min_free: f64,
) -> Vec<Placement> {
    let mut out: Vec<Placement> = nodes
        .into_iter()
        .filter_map(|(id, allows_jobs, load)| {
            let load = load?;
            if !allows_jobs {
                return None;
            }
            let v = verdict(&load.own, need_gb, heavy, min_free);
            v.busy.is_none().then(|| Placement {
                node_id: id.to_string(),
                user_active: v.user_active,
                score: v.score,
            })
        })
        .collect();
    out.sort_by(|a, b| {
        a.user_active.cmp(&b.user_active).then(
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal),
        )
    });
    out
}

// ---------------------------------------------------------------------------------------------
// Config

/// The jobs switch, read on every request (a settings toggle applies without a reconnect).
#[derive(Clone)]
pub struct JobsSwitch(Arc<dyn Fn() -> bool + Send + Sync>);

impl JobsSwitch {
    pub fn fixed(on: bool) -> Self {
        Self(Arc::new(move || on))
    }
    pub fn from_fn(f: impl Fn() -> bool + Send + Sync + 'static) -> Self {
        Self(Arc::new(f))
    }
    pub fn is_on(&self) -> bool {
        (self.0)()
    }
}

impl std::fmt::Debug for JobsSwitch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "JobsSwitch({})", self.is_on())
    }
}

/// The OWN sampler admission reads (a seam so tests can present any machine state).
pub type OwnSampler = Arc<dyn Fn() -> OwnLoad + Send + Sync>;

#[derive(Clone)]
pub struct JobsConfig {
    /// OFF by default: no job runs here unless the owner opts the node in.
    pub switch: JobsSwitch,
    /// Ledger root (`~/.leanzero/jobs`, env [`JOBS_DIR_ENV`]).
    pub root: PathBuf,
    /// The heavy-run lock file (`~/.cache/heavy-run.lock`, env `HEAVY_RUN_LOCK`).
    pub heavy_lock: PathBuf,
    /// `None` = re-read `~/.leanzero/node-limits.json` at every submit (heavy-run's file).
    pub limits: Option<NodeLimits>,
    /// `None` = `limits.min_free`, or env [`MIN_FREE_ENV`].
    pub min_free: Option<f64>,
    pub user_active: UserActivePolicy,
    pub sampler: OwnSampler,
    /// Watchdog tick (1 s) and the TERM->KILL grace (10 s).
    pub tick: Duration,
    pub kill_grace: Duration,
}

impl std::fmt::Debug for JobsConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JobsConfig")
            .field("switch", &self.switch)
            .field("root", &self.root)
            .field("heavy_lock", &self.heavy_lock)
            .field("user_active", &self.user_active)
            .finish_non_exhaustive()
    }
}

impl Default for JobsConfig {
    fn default() -> Self {
        let root = std::env::var_os(JOBS_DIR_ENV)
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                dirs::home_dir()
                    .unwrap_or_else(|| PathBuf::from("/tmp"))
                    .join(".leanzero/jobs")
            });
        Self {
            switch: JobsSwitch::fixed(false),
            root,
            heavy_lock: crate::load::heavy_lock_path(),
            limits: None,
            min_free: std::env::var(MIN_FREE_ENV)
                .ok()
                .and_then(|v| v.trim().parse().ok()),
            user_active: UserActivePolicy::from_env(),
            sampler: Arc::new(|| sample_own(user_idle_threshold_s())),
            tick: Duration::from_secs(1),
            kill_grace: Duration::from_secs(10),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Registry

struct JobEntry {
    record: Mutex<JobRecord>,
    dir: PathBuf,
    cancel: AtomicBool,
    cancel_notify: Notify,
    pgid: AtomicI32,
}

impl JobEntry {
    fn snapshot(&self) -> JobRecord {
        self.record.lock().unwrap().clone()
    }

    fn update(&self, f: impl FnOnce(&mut JobRecord)) {
        let rec = {
            let mut r = self.record.lock().unwrap();
            f(&mut r);
            r.clone()
        };
        write_record(&self.dir, &rec);
    }

    fn finish(&self, state: JobState, code: Option<i32>, reason: Option<String>) {
        self.update(|r| {
            if r.state.is_terminal() {
                return;
            }
            r.state = state;
            r.exit_code = code;
            r.reason = reason;
            r.ended_at = Some(Utc::now());
        });
    }

    fn view(&self) -> JobView {
        let r = self.snapshot();
        JobView {
            brief: brief_of(&r),
            ended_at: r.ended_at,
            exit_code: r.exit_code,
            reason: r.reason.clone(),
            session_id: r.session_id.clone(),
            log_bytes: std::fs::metadata(self.dir.join("log"))
                .map(|m| m.len())
                .unwrap_or(0),
        }
    }
}

fn brief_of(r: &JobRecord) -> JobBrief {
    JobBrief {
        job_id: r.job_id.clone(),
        origin_node: r.origin_node.clone(),
        kind: r.kind,
        started_at: r.started_at,
        state: r.state,
        heavy: r.heavy,
    }
}

fn write_record(dir: &Path, rec: &JobRecord) {
    let tmp = dir.join("job.json.tmp");
    let ok = serde_json::to_vec_pretty(rec)
        .ok()
        .and_then(|b| std::fs::write(&tmp, b).ok())
        .and_then(|_| std::fs::rename(&tmp, dir.join("job.json")).ok());
    if ok.is_none() {
        tracing::warn!(job = %rec.job_id, dir = %dir.display(), "jobs: ledger write failed");
    }
}

fn new_job_id() -> String {
    format!(
        "j{}-{:06x}",
        Utc::now().format("%Y%m%d%H%M%S"),
        rand::random::<u32>() & 0xff_ffff
    )
}

/// The node's jobs: admission, spawn, watchdog, ledger, views.
///
/// One per ledger root for the whole process ([`Self::shared`]): the control service is
/// rebuilt on every Link reconnect, and a registry that lived inside it lost every running job
/// with it (3.0.128 live test: the job vanished from the list, cancel said not_found, the
/// process kept running and holding the heavy slot). On first use with the switch on, the
/// registry also rebuilds itself from the ledger, so a goose restart adopts what still runs.
pub struct JobRegistry {
    cfg: RwLock<JobsConfig>,
    jobs: Mutex<HashMap<String, Arc<JobEntry>>>,
    rebuilt: AtomicBool,
}

static SHARED: OnceLock<Mutex<HashMap<PathBuf, Arc<JobRegistry>>>> = OnceLock::new();

impl std::fmt::Debug for JobRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JobRegistry")
            .field("cfg", &self.cfg)
            .finish_non_exhaustive()
    }
}

impl JobRegistry {
    /// A private registry (tests). Production uses [`Self::shared`].
    pub fn new(cfg: JobsConfig) -> Self {
        Self {
            cfg: RwLock::new(cfg),
            jobs: Mutex::new(HashMap::new()),
            rebuilt: AtomicBool::new(false),
        }
    }

    /// The process-wide registry for `cfg.root`, created on first use and handed to every
    /// control service started over that root afterwards. The newest config wins (the switch
    /// or the limits may have changed between starts); the jobs and their watchdogs stay.
    pub fn shared(cfg: JobsConfig) -> Arc<Self> {
        let map = SHARED.get_or_init(Default::default);
        let reg = map
            .lock()
            .unwrap()
            .entry(cfg.root.clone())
            .or_insert_with(|| Arc::new(Self::new(cfg.clone())))
            .clone();
        *reg.cfg.write().unwrap() = cfg;
        reg.ensure_rebuilt();
        reg
    }

    fn cfg(&self) -> JobsConfig {
        self.cfg.read().unwrap().clone()
    }

    pub fn enabled(&self) -> bool {
        self.cfg().switch.is_on()
    }

    /// The memory floor this node admits against (config, else node-limits.json `min_free`);
    /// the placer uses its own floor to pre-screen peers (each receiver re-checks with its own).
    pub fn min_free(&self) -> f64 {
        let cfg = self.cfg();
        cfg.min_free
            .unwrap_or_else(|| cfg.limits.unwrap_or_else(NodeLimits::load).min_free)
    }

    fn get(&self, id: &str) -> Option<Arc<JobEntry>> {
        self.ensure_rebuilt();
        self.jobs.lock().unwrap().get(id).cloned()
    }

    /// Rebuild from the ledger once, the first time the switch is on inside a tokio runtime.
    /// Gated on the switch so a node that never ran jobs (and every test that leaves the
    /// switch off over the real `~/.leanzero/jobs`) never touches the ledger.
    fn ensure_rebuilt(&self) {
        if self.rebuilt.load(Ordering::SeqCst) {
            return;
        }
        let cfg = self.cfg();
        let Ok(rt) = tokio::runtime::Handle::try_current() else {
            return;
        };
        if !cfg.switch.is_on() || self.rebuilt.swap(true, Ordering::SeqCst) {
            return;
        }
        self.rebuild(&cfg, &rt);
    }

    /// Each `<root>/<id>/job.json`: a `running`/`cancelling` shell job whose process group is
    /// still the one it started is ADOPTED (listed, watched, cancellable by group kill, its
    /// heavy slot taken again); one whose group is gone gets its exit file's outcome, else
    /// `lost`. Never left "running". Jobs that ended in the last day are listed again.
    fn rebuild(&self, cfg: &JobsConfig, rt: &tokio::runtime::Handle) {
        let Ok(rd) = std::fs::read_dir(&cfg.root) else {
            return;
        };
        let limits = cfg.limits.clone().unwrap_or_else(NodeLimits::load);
        let keep_since = Utc::now() - chrono::Duration::hours(LEDGER_KEEP_HOURS);
        for d in rd.flatten() {
            let dir = d.path();
            let Ok(raw) = std::fs::read_to_string(dir.join("job.json")) else {
                continue;
            };
            let Ok(mut rec) = serde_json::from_str::<JobRecord>(&raw) else {
                tracing::warn!(dir = %dir.display(), "jobs: unreadable ledger record skipped");
                continue;
            };
            if self.jobs.lock().unwrap().contains_key(&rec.job_id) {
                continue;
            }
            let mut adopt = None;
            if !rec.state.is_terminal() {
                match (rec.kind, rec.pgid) {
                    (JobKind::Shell, Some(p)) if ledger_group_alive(p, rec.started_at) => {
                        adopt = Some(p)
                    }
                    (kind, _) => {
                        let (state, code, reason) = match (kind, read_exit(&dir)) {
                            (JobKind::Shell, Some(0)) => (JobState::Done, Some(0), None),
                            (JobKind::Shell, Some(c)) => (JobState::Failed, Some(c), None),
                            (JobKind::Shell, None) => (
                                JobState::Lost,
                                None,
                                Some("its process group was gone when goose came back; exit status unknown"),
                            ),
                            (JobKind::Prompt, _) => (
                                JobState::Lost,
                                None,
                                Some("goose restarted while its session ran; the end was not observed"),
                            ),
                        };
                        rec.state = state;
                        rec.exit_code = code;
                        rec.reason = Some(reason.map_or_else(
                            || "ended while goose was not running".to_string(),
                            str::to_string,
                        ));
                        rec.ended_at = Some(Utc::now());
                        write_record(&dir, &rec);
                        tracing::info!(job = %rec.job_id, state = ?rec.state, "jobs: ledger job closed on rebuild");
                    }
                }
            } else if rec.ended_at.is_none_or(|t| t < keep_since) {
                continue;
            }
            let cancelling = rec.state == JobState::Cancelling;
            let id = rec.job_id.clone();
            let entry = Arc::new(JobEntry {
                record: Mutex::new(rec),
                dir,
                cancel: AtomicBool::new(cancelling),
                cancel_notify: Notify::new(),
                pgid: AtomicI32::new(adopt.unwrap_or(0)),
            });
            self.jobs.lock().unwrap().insert(id.clone(), entry.clone());
            if let Some(pgid) = adopt {
                tracing::info!(job = %id, pgid, "jobs: adopted a running ledger job");
                rt.spawn(watch_adopted(entry, pgid, limits.clone(), cfg.clone()));
            }
        }
    }

    /// Admit and start one job. `executor`/`source` serve prompt jobs.
    pub async fn submit(
        &self,
        req: JobRequest,
        executor: Option<Arc<dyn RemoteExecutor>>,
        source: Arc<dyn SwarmStateSource>,
    ) -> Result<JobAccepted, Refusal> {
        let cfg = self.cfg();
        if !cfg.switch.is_on() {
            return Err(Refusal::new(
                RefusalCode::JobsDisabled,
                "remote jobs are disabled on this node",
            ));
        }
        if req.command.trim().is_empty() {
            return Err(Refusal::new(RefusalCode::BadRequest, "empty command"));
        }
        if req.kind == JobKind::Prompt && executor.is_none() {
            return Err(Refusal::new(
                RefusalCode::ExecutorMissing,
                "prompt jobs need goose's executor; not wired on this node",
            ));
        }
        if let Some(cwd) = &req.cwd {
            if !Path::new(cwd).is_dir() {
                return Err(Refusal::new(
                    RefusalCode::BadRequest,
                    format!("cwd is not a directory: {cwd}"),
                ));
            }
        }
        self.ensure_rebuilt();
        let limits = cfg.limits.clone().unwrap_or_else(NodeLimits::load);
        let min_free = cfg.min_free.unwrap_or(limits.min_free);
        let sampler = cfg.sampler.clone();
        let mut own = tokio::task::spawn_blocking(move || sampler())
            .await
            .unwrap_or_default();
        let job_id = new_job_id();
        // The slot is the truth, not the sampled flag: take it now (non-blocking) and hold it
        // for the job's life. Taken BEFORE the verdict so a refusal names the right cause.
        let mut slot = if req.heavy {
            match HeavySlot::try_take_at(&cfg.heavy_lock, &job_id) {
                Ok(Some(s)) => {
                    own.heavy_slot_held = false;
                    Some(s)
                }
                Ok(None) => {
                    own.heavy_slot_held = true;
                    None
                }
                Err(e) => {
                    return Err(Refusal::new(
                        RefusalCode::Internal,
                        format!("heavy-run lock {}: {e}", cfg.heavy_lock.display()),
                    ))
                }
            }
        } else {
            None
        };
        admit(&own, &req, min_free, cfg.user_active).map_err(|mut r| {
            if r.code == RefusalCode::HeavySlotHeld {
                // Who holds it and for how long (the lock's holder line), not a bare "held".
                r.reason = heavy_holder_reason(&cfg.heavy_lock);
            }
            r
        })?;

        let dir = cfg.root.join(&job_id);
        std::fs::create_dir_all(&dir).map_err(|e| {
            Refusal::new(
                RefusalCode::Internal,
                format!("ledger {}: {e}", dir.display()),
            )
        })?;
        let rec = JobRecord {
            job_id: job_id.clone(),
            origin_node: req
                .origin_node
                .clone()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| "unknown".into()),
            kind: req.kind,
            command: req.command.clone(),
            cwd: req.cwd.clone(),
            heavy: req.heavy,
            need_mem_gb: req.need_mem_gb,
            timeout_s: req.timeout_s.unwrap_or(limits.timeout_s).max(1),
            state: JobState::Running,
            started_at: Utc::now(),
            ended_at: None,
            exit_code: None,
            reason: None,
            session_id: None,
            pgid: None,
        };
        write_record(&dir, &rec);
        let entry = Arc::new(JobEntry {
            record: Mutex::new(rec),
            dir,
            cancel: AtomicBool::new(false),
            cancel_notify: Notify::new(),
            pgid: AtomicI32::new(0),
        });
        self.jobs
            .lock()
            .unwrap()
            .insert(job_id.clone(), entry.clone());
        tracing::info!(job = %job_id, kind = ?req.kind, heavy = req.heavy, "jobs: accepted");

        match req.kind {
            JobKind::Shell => {
                let child = match spawn_shell(&req, &entry.dir) {
                    Ok(c) => c,
                    Err(e) => {
                        entry.finish(JobState::Failed, None, Some(format!("spawn: {e}")));
                        return Err(Refusal::new(RefusalCode::Internal, format!("spawn: {e}")));
                    }
                };
                let pgid = child.id().map(|p| p as i32).unwrap_or(0);
                entry.pgid.store(pgid, Ordering::SeqCst);
                entry.update(|r| r.pgid = Some(pgid));
                if let Some(s) = slot.as_mut() {
                    s.write_holder(pgid, &holder_label(&entry.snapshot()));
                }
                tokio::spawn(watch_shell(entry, child, pgid, limits, cfg, slot));
            }
            JobKind::Prompt => {
                let exec = executor.expect("checked above");
                let er = ExecuteRequest {
                    prompt: req.command.clone(),
                    working_dir: req.cwd.clone(),
                    session_id: None,
                };
                match exec.execute(er).await {
                    Ok(acc) => {
                        entry.update(|r| r.session_id = Some(acc.session_id.clone()));
                        tokio::spawn(watch_prompt(entry, acc.session_id, source, cfg.tick, slot));
                    }
                    Err(e) => {
                        entry.finish(JobState::Failed, None, Some(e.to_string()));
                        let code = match e {
                            crate::state::ExecuteError::Busy => RefusalCode::MemoryHeadroom,
                            crate::state::ExecuteError::BadRequest(_) => RefusalCode::BadRequest,
                            _ => RefusalCode::Internal,
                        };
                        return Err(Refusal::new(code, e.to_string()));
                    }
                }
            }
        }
        Ok(JobAccepted { job_id })
    }

    /// Newest first.
    pub fn list(&self) -> Vec<JobView> {
        self.ensure_rebuilt();
        let mut v: Vec<JobView> = self
            .jobs
            .lock()
            .unwrap()
            .values()
            .map(|e| e.view())
            .collect();
        v.sort_by(|a, b| b.brief.started_at.cmp(&a.brief.started_at));
        v
    }

    pub fn view(&self, id: &str) -> Option<JobView> {
        self.get(id).map(|e| e.view())
    }

    /// What `NodeLoad.jobs` carries: running jobs plus those that ended < 10 min ago.
    pub fn briefs(&self) -> Vec<JobBrief> {
        self.ensure_rebuilt();
        let cutoff = Utc::now() - chrono::Duration::from_std(BRIEF_KEEP).unwrap_or_default();
        let mut v: Vec<JobRecord> = self
            .jobs
            .lock()
            .unwrap()
            .values()
            .map(|e| e.snapshot())
            .filter(|r| !r.state.is_terminal() || r.ended_at.is_some_and(|t| t > cutoff))
            .collect();
        v.sort_by(|a, b| b.started_at.cmp(&a.started_at));
        v.iter().take(BRIEF_MAX).map(brief_of).collect()
    }

    pub fn log(&self, id: &str, from: u64) -> Option<JobLogChunk> {
        let e = self.get(id)?;
        let done = e.snapshot().state.is_terminal();
        let path = e.dir.join("log");
        let data = std::fs::read(&path).unwrap_or_default();
        let len = data.len() as u64;
        let from = from.min(len);
        let next = (from + LOG_CHUNK_MAX).min(len);
        Some(JobLogChunk {
            from,
            next,
            data: String::from_utf8_lossy(&data[from as usize..next as usize]).into_owned(),
            done: done && next == len,
        })
    }

    /// Cancel: the watchdog TERMs the group (KILL after the grace) and records 143. Waits for
    /// that (the grace plus [`CANCEL_SLACK`]) and answers the FINAL view; a group still alive
    /// then answers `cancelling` (the record says so too), never the pre-cancel `running`.
    pub async fn cancel(&self, id: &str) -> Result<JobView, Refusal> {
        let e = self
            .get(id)
            .ok_or_else(|| Refusal::new(RefusalCode::NotFound, format!("no job {id}")))?;
        let r = e.snapshot();
        if r.state.is_terminal() {
            return Err(Refusal::new(
                RefusalCode::AlreadyFinished,
                format!("job {id} already {:?}", r.state),
            ));
        }
        if r.kind == JobKind::Prompt {
            return Err(Refusal::new(
                RefusalCode::CancelUnsupported,
                "a prompt job cannot be cancelled from the mesh (the executor has no cancel seam); stop its session in goose",
            ));
        }
        e.update(|r| {
            if !r.state.is_terminal() {
                r.state = JobState::Cancelling;
            }
        });
        e.cancel.store(true, Ordering::SeqCst);
        e.cancel_notify.notify_one();
        let deadline = Instant::now() + self.cfg().kill_grace + CANCEL_SLACK;
        while !e.snapshot().state.is_terminal() && Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        Ok(e.view())
    }
}

/// What the control service's `/nodes` attaches: a load snapshot carrying this registry's briefs.
/// With a registry, OWN comes from the registry's own sampler, so what peers are shown is
/// exactly what admission reads here (same user-idle threshold, same test seam).
pub async fn load_with_jobs(engine_active: u32, jobs: Option<&JobRegistry>) -> NodeLoad {
    let Some(reg) = jobs else {
        return crate::load::snapshot(engine_active, Vec::new()).await;
    };
    crate::load::snapshot_with(engine_active, reg.briefs(), reg.cfg().sampler).await
}

// ---------------------------------------------------------------------------------------------
// Shell: spawn + watchdog

fn spawn_shell(req: &JobRequest, dir: &Path) -> std::io::Result<tokio::process::Child> {
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("log"))?;
    let err = log.try_clone()?;
    // `/bin/zsh -lc <command>` as before, under a non-login `zsh -f` that records its exit
    // status in `<job>/exit`: a goose that adopts this job after a restart is not its parent
    // and cannot wait() for it, so the file is how the outcome survives. A signalled group
    // kills the wrapper too (no file), which the watchdog records itself.
    let mut cmd = tokio::process::Command::new("/bin/zsh");
    cmd.arg("-f")
        .arg("-c")
        .arg(r#"/bin/zsh -lc "$1"; rc=$?; print -r -- $rc >| "$2"; exit $rc"#)
        .arg("goose-job")
        .arg(&req.command)
        .arg(dir.join(EXIT_FILE))
        .stdin(std::process::Stdio::null())
        .stdout(log)
        .stderr(err)
        .kill_on_drop(false);
    if let Some(cwd) = &req.cwd {
        cmd.current_dir(cwd);
    } else if let Some(h) = dirs::home_dir() {
        cmd.current_dir(h);
    }
    #[cfg(unix)]
    cmd.process_group(0);
    cmd.spawn()
}

fn system_free_pct() -> Option<f64> {
    let m = goose_sidecar::measure().ok()?;
    (m.total_bytes > 0).then(|| m.available_bytes as f64 * 100.0 / m.total_bytes as f64)
}

fn ram_bytes() -> Option<u64> {
    goose_sidecar::measure()
        .ok()
        .map(|m| m.total_bytes)
        .filter(|b| *b > 0)
}

#[cfg(unix)]
fn group_alive(pgid: i32) -> bool {
    // SAFETY: signal 0 only checks existence/permission.
    pgid > 0 && unsafe { libc::killpg(pgid, 0) } == 0
}

/// The ledger's pgid is still the group this job started: the group exists, and if its
/// leader is still alive the leader started with the job (pids are reused after a reboot;
/// the kernel never reuses a pid while it is still some group's id, so a group whose leader
/// is gone is the original one).
#[cfg(unix)]
fn ledger_group_alive(pgid: i32, started_at: DateTime<Utc>) -> bool {
    if !group_alive(pgid) {
        return false;
    }
    // SAFETY: getpgid has no memory effects.
    let leader_pgid = unsafe { libc::getpgid(pgid) };
    if leader_pgid < 0 {
        return true; // the leader is gone, its group lives on
    }
    if leader_pgid != pgid {
        return false; // that pid is now some other group's member
    }
    match process_start_secs(pgid) {
        Some(t) => {
            let s = started_at.timestamp();
            (s - 5..=s + 300).contains(&t)
        }
        None => true,
    }
}

#[cfg(not(unix))]
fn ledger_group_alive(_pgid: i32, _started_at: DateTime<Utc>) -> bool {
    false
}

#[cfg(target_os = "macos")]
fn process_start_secs(pid: i32) -> Option<i64> {
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as i32;
    // SAFETY: proc_pidinfo writes at most `size` bytes into `info`.
    let n = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            &mut info as *mut libc::proc_bsdinfo as *mut libc::c_void,
            size,
        )
    };
    (n == size).then_some(info.pbi_start_tvsec as i64)
}

#[cfg(not(target_os = "macos"))]
fn process_start_secs(_pid: i32) -> Option<i64> {
    None
}

/// The wrapper's `<job>/exit` (see [`spawn_shell`]).
fn read_exit(dir: &Path) -> Option<i32> {
    std::fs::read_to_string(dir.join(EXIT_FILE))
        .ok()?
        .trim()
        .parse()
        .ok()
}

/// What goose writes after `pid P since T: ` in the heavy-run lock (never the command).
fn holder_label(r: &JobRecord) -> String {
    format!("goose-job {} from {}", r.job_id, r.origin_node)
}

/// Only a group this registry created (`process_group(0)` makes the shell's pid its pgid) and
/// never the caller's own group (the r2 killpg shape, refused by construction — reaping gate 4).
#[cfg(unix)]
fn signal_group(pgid: i32, sig: i32) {
    // SAFETY: getpgrp has no memory effects.
    if pgid > 1 && pgid != unsafe { libc::getpgrp() } {
        // SAFETY: plain syscall on our own child's group.
        unsafe {
            libc::killpg(pgid, sig);
        }
    }
}

/// TERM the whole group, KILL after `grace` if any member is left (heavy-run `kill_group`).
#[cfg(unix)]
async fn kill_group(child: &mut tokio::process::Child, pgid: i32, grace: Duration) {
    signal_group(pgid, libc::SIGTERM);
    let deadline = Instant::now() + grace;
    loop {
        let exited = matches!(child.try_wait(), Ok(Some(_)));
        if exited && !group_alive(pgid) {
            return;
        }
        if Instant::now() >= deadline {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    signal_group(pgid, libc::SIGKILL);
    let _ = tokio::time::timeout(Duration::from_secs(5), child.wait()).await;
}

async fn watch_shell(
    entry: Arc<JobEntry>,
    mut child: tokio::process::Child,
    pgid: i32,
    limits: NodeLimits,
    cfg: JobsConfig,
    slot: Option<HeavySlot>,
) {
    let start = Instant::now();
    let timeout = Duration::from_secs(entry.snapshot().timeout_s);
    let ram = ram_bytes();
    let mut fails = 0u32;
    let outcome: (JobState, Option<i32>, Option<String>) = loop {
        tokio::select! {
            res = child.wait() => {
                break match res {
                    Ok(st) => {
                        let code = st.code().or_else(|| {
                            #[cfg(unix)]
                            { std::os::unix::process::ExitStatusExt::signal(&st).map(|s| 128 + s) }
                            #[cfg(not(unix))]
                            { None }
                        });
                        if code == Some(0) {
                            (JobState::Done, code, None)
                        } else {
                            (JobState::Failed, code, None)
                        }
                    }
                    Err(e) => (JobState::Failed, None, Some(format!("wait: {e}"))),
                };
            }
            _ = entry.cancel_notify.notified() => {}
            _ = tokio::time::sleep(cfg.tick) => {}
        }
        let why: Option<(JobState, i32, String)> = if entry.cancel.load(Ordering::SeqCst) {
            Some((JobState::Cancelled, EXIT_CANCEL, "cancelled".into()))
        } else if start.elapsed() > timeout {
            Some((
                JobState::Killed,
                EXIT_TIMEOUT,
                format!("timeout after {}s", timeout.as_secs()),
            ))
        } else {
            memory_verdict(pgid, ram, &limits, &mut fails).await
        };
        if let Some((state, code, reason)) = why {
            // A job that already exited between ticks is not killed again.
            if let Ok(Some(_)) = child.try_wait() {
                if !group_alive(pgid) && state != JobState::Cancelled {
                    continue;
                }
            }
            tracing::warn!(job = %entry.snapshot().job_id, %reason, "jobs: killing job group");
            #[cfg(unix)]
            kill_group(&mut child, pgid, cfg.kill_grace).await;
            break (state, Some(code), Some(reason));
        }
    };
    // Leftover members of a finished job's group get the same treatment (heavy-run kills the
    // group on its own exit only when signalled; a background child would hold the slot).
    #[cfg(unix)]
    if group_alive(pgid) {
        signal_group(pgid, libc::SIGTERM);
    }
    let _ = append_line(
        &entry.dir,
        &format!(
            "[goose-job] end {:?} exit {:?} {}",
            outcome.0,
            outcome.1,
            outcome.2.clone().unwrap_or_default()
        ),
    );
    entry.finish(outcome.0, outcome.1, outcome.2);
    drop(slot);
}

/// The watchdog's memory rule (heavy-run's): group footprint over `max_rss_pct` of RAM, system
/// free under `kill_free`, or memory unreadable [`MEM_FAIL_KILL`] times in a row -> 137.
async fn memory_verdict(
    pgid: i32,
    ram: Option<u64>,
    limits: &NodeLimits,
    fails: &mut u32,
) -> Option<(JobState, i32, String)> {
    let used = tokio::task::spawn_blocking(move || crate::load::group_memory_bytes(pgid))
        .await
        .ok()
        .flatten();
    let free = tokio::task::spawn_blocking(system_free_pct)
        .await
        .ok()
        .flatten();
    if used.is_none() || free.is_none() {
        *fails += 1;
    } else {
        *fails = 0;
    }
    let cap = ram.map(|r| r as f64 * limits.max_rss_pct / 100.0);
    match (used, cap, free) {
        (Some(u), Some(c), _) if u as f64 > c => Some((
            JobState::Killed,
            EXIT_MEMORY,
            format!(
                "job memory passed {}% of RAM ({} MB)",
                limits.max_rss_pct,
                u / (1 << 20)
            ),
        )),
        (_, _, Some(f)) if f < limits.kill_free => Some((
            JobState::Killed,
            EXIT_MEMORY,
            format!("memory free fell under {}% ({f:.1}%)", limits.kill_free),
        )),
        _ if *fails >= MEM_FAIL_KILL => Some((
            JobState::Killed,
            EXIT_MEMORY,
            format!("memory could not be read {fails} times in a row"),
        )),
        _ => None,
    }
}

/// TERM the group, KILL after `grace` if any member is left, for a group that is not our child
/// (an adopted job): only `killpg(0)` tells us when it is gone.
#[cfg(unix)]
async fn kill_adopted(pgid: i32, grace: Duration) {
    signal_group(pgid, libc::SIGTERM);
    let deadline = Instant::now() + grace;
    while group_alive(pgid) && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    if group_alive(pgid) {
        signal_group(pgid, libc::SIGKILL);
        let deadline = Instant::now() + Duration::from_secs(5);
        while group_alive(pgid) && Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}

/// The watchdog for a job adopted from the ledger after a goose restart: same timeout (from the
/// recorded start), memory and cancel rules as [`watch_shell`], by group signals. The flock of
/// the goose that started it died with that goose (Rust opens files `O_CLOEXEC`, so the job
/// never inherited the fd), so a heavy job takes the slot again here and holds it until its
/// group is gone; if someone else took the slot in between, the log says so.
async fn watch_adopted(entry: Arc<JobEntry>, pgid: i32, limits: NodeLimits, cfg: JobsConfig) {
    let rec = entry.snapshot();
    let mut slot = None;
    if rec.heavy {
        match HeavySlot::try_take_at(&cfg.heavy_lock, &rec.job_id) {
            Ok(Some(mut s)) => {
                s.write_holder(pgid, &holder_label(&rec));
                slot = Some(s);
            }
            Ok(None) => {
                let who = heavy_holder_reason(&cfg.heavy_lock);
                tracing::warn!(job = %rec.job_id, %who, "jobs: adopted heavy job could not retake the slot");
                let _ = append_line(
                    &entry.dir,
                    &format!("[goose-job] adopted after a goose restart; the slot was taken meanwhile: {who}"),
                );
            }
            Err(e) => {
                tracing::warn!(job = %rec.job_id, error = %e, "jobs: heavy lock unreadable on adoption")
            }
        }
    }
    let _ = append_line(
        &entry.dir,
        &format!("[goose-job] adopted after a goose restart (pgid {pgid})"),
    );
    let timeout = Duration::from_secs(rec.timeout_s);
    let ram = ram_bytes();
    let mut fails = 0u32;
    let outcome: (JobState, Option<i32>, Option<String>) = loop {
        tokio::select! {
            _ = entry.cancel_notify.notified() => {}
            _ = tokio::time::sleep(cfg.tick) => {}
        }
        if !group_alive(pgid) {
            break match read_exit(&entry.dir) {
                Some(0) => (JobState::Done, Some(0), None),
                Some(c) => (JobState::Failed, Some(c), None),
                None => (
                    JobState::Lost,
                    None,
                    Some("ended after goose adopted it and left no exit status".into()),
                ),
            };
        }
        let elapsed = (Utc::now() - rec.started_at).to_std().unwrap_or_default();
        let why = if entry.cancel.load(Ordering::SeqCst) {
            Some((JobState::Cancelled, EXIT_CANCEL, "cancelled".to_string()))
        } else if elapsed > timeout {
            Some((
                JobState::Killed,
                EXIT_TIMEOUT,
                format!("timeout after {}s", timeout.as_secs()),
            ))
        } else {
            memory_verdict(pgid, ram, &limits, &mut fails).await
        };
        if let Some((state, code, reason)) = why {
            tracing::warn!(job = %rec.job_id, %reason, "jobs: killing adopted job group");
            #[cfg(unix)]
            kill_adopted(pgid, cfg.kill_grace).await;
            break (state, Some(code), Some(reason));
        }
    };
    let _ = append_line(
        &entry.dir,
        &format!(
            "[goose-job] end {:?} exit {:?} {}",
            outcome.0,
            outcome.1,
            outcome.2.clone().unwrap_or_default()
        ),
    );
    entry.finish(outcome.0, outcome.1, outcome.2);
    drop(slot);
}

fn append_line(dir: &Path, line: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("log"))?;
    writeln!(f, "{line}")
}

/// A prompt job is Running while its session is `live` on this node; then Done. It holds the
/// heavy slot (when asked for) the same way.
async fn watch_prompt(
    entry: Arc<JobEntry>,
    session_id: String,
    source: Arc<dyn SwarmStateSource>,
    tick: Duration,
    slot: Option<HeavySlot>,
) {
    let timeout = Duration::from_secs(entry.snapshot().timeout_s);
    let start = Instant::now();
    let mut seen_live = false;
    let mut misses = 0u32;
    loop {
        tokio::time::sleep(tick.max(Duration::from_millis(200))).await;
        let live = match source.local_sessions().await {
            Ok(s) => s.iter().any(|x| x.session_id == session_id && x.live),
            Err(_) => true, // index unreadable: unknown, keep waiting
        };
        if live {
            seen_live = true;
            misses = 0;
        } else {
            misses += 1;
        }
        // Not live after having been live (or never live for 30 ticks) = finished.
        if (seen_live && !live) || misses >= 30 {
            entry.finish(JobState::Done, Some(0), None);
            break;
        }
        if start.elapsed() > timeout {
            entry.finish(
                JobState::Failed,
                Some(EXIT_TIMEOUT),
                Some(format!(
                    "session still live after {}s (not stopped: prompt jobs have no cancel seam)",
                    timeout.as_secs()
                )),
            );
            break;
        }
    }
    drop(slot);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn idle_mac() -> OwnLoad {
        OwnLoad {
            user_idle_s: Some(900.0),
            user_active: false,
            cpu_load_per_core: Some(0.1),
            ncpu: 16,
            mem_free_pct: Some(60.0),
            mem_total_gb: Some(128.0),
            gpu_util_pct: Some(0.0),
            on_ac: Some(true),
            thermal_limit: Some(0),
            ..Default::default()
        }
    }

    fn req(heavy: bool, need: f64) -> JobRequest {
        JobRequest {
            kind: JobKind::Shell,
            command: "true".into(),
            cwd: None,
            need_mem_gb: Some(need),
            heavy,
            timeout_s: None,
            origin_node: None,
            allow_user_active: false,
        }
    }

    fn code(own: &OwnLoad, r: &JobRequest, p: UserActivePolicy) -> Option<RefusalCode> {
        admit(own, r, 20.0, p).err().map(|e| e.code)
    }

    #[test]
    fn the_benchmark_refusal_names_the_run() {
        let mut m = idle_mac();
        m.benchmark = Some("Gauntlet build run 2026-10-25T15:40Z (pid 1234)".into());
        let e = admit(&m, &req(false, 0.0), 20.0, UserActivePolicy::LastResort).unwrap_err();
        assert_eq!(
            e.reason,
            "benchmark running: Gauntlet build run 2026-10-25T15:40Z (pid 1234)"
        );
    }

    #[test]
    fn admission_codes() {
        let lr = UserActivePolicy::LastResort;
        assert_eq!(code(&idle_mac(), &req(true, 10.0), lr), None);
        let mut m = idle_mac();
        m.benchmark = Some("pid 1 goose benchmark".into());
        assert_eq!(
            code(&m, &req(false, 0.0), lr),
            Some(RefusalCode::BenchmarkRunning)
        );
        let mut m = idle_mac();
        m.mem_free_pct = None;
        assert_eq!(
            code(&m, &req(false, 0.0), lr),
            Some(RefusalCode::MemoryUnreadable)
        );
        // 60% free of 128 GB, floor 20% -> 51.2 GB above the floor.
        assert_eq!(
            code(&idle_mac(), &req(false, 60.0), lr),
            Some(RefusalCode::MemoryHeadroom)
        );
        let mut m = idle_mac();
        m.mem_free_pct = Some(15.0);
        assert_eq!(
            code(&m, &req(false, 0.0), lr),
            Some(RefusalCode::MemoryHeadroom)
        );
        let mut m = idle_mac();
        m.heavy_slot_held = true;
        assert_eq!(
            code(&m, &req(true, 0.0), lr),
            Some(RefusalCode::HeavySlotHeld)
        );
        assert_eq!(
            code(&m, &req(false, 0.0), lr),
            None,
            "light jobs ignore the slot"
        );
        let mut m = idle_mac();
        m.on_ac = Some(false);
        assert_eq!(code(&m, &req(true, 0.0), lr), Some(RefusalCode::OnBattery));
        let mut m = idle_mac();
        m.thermal_limit = Some(2);
        assert_eq!(code(&m, &req(true, 0.0), lr), Some(RefusalCode::Thermal));
        let mut m = idle_mac();
        m.lid_closed_sleeps = true;
        assert_eq!(code(&m, &req(false, 0.0), lr), Some(RefusalCode::LidSleeps));
    }

    #[test]
    fn user_active_policy() {
        let mut m = idle_mac();
        m.user_active = true;
        m.user_idle_s = Some(4.0);
        let mut r = req(false, 0.0);
        assert_eq!(
            code(&m, &r, UserActivePolicy::LastResort),
            Some(RefusalCode::UserActive)
        );
        assert_eq!(code(&m, &r, UserActivePolicy::Allow), None);
        r.allow_user_active = true;
        assert_eq!(code(&m, &r, UserActivePolicy::LastResort), None);
        assert_eq!(
            code(&m, &r, UserActivePolicy::Refuse),
            Some(RefusalCode::UserActive)
        );
        assert_eq!(
            UserActivePolicy::parse("last-resort"),
            Some(UserActivePolicy::LastResort)
        );
        assert_eq!(UserActivePolicy::parse("nope"), None);
    }

    #[test]
    fn placement_ok_first_then_user_active_by_score() {
        let load = |own: OwnLoad| NodeLoad {
            own,
            ..Default::default()
        };
        let a = load(idle_mac());
        let mut b_own = idle_mac();
        b_own.mem_free_pct = Some(80.0);
        b_own.user_active = true;
        let b = load(b_own);
        let mut c_own = idle_mac();
        c_own.mem_free_pct = Some(40.0);
        let c = load(c_own);
        let mut d_own = idle_mac();
        d_own.benchmark = Some("x".into());
        let d = load(d_own);
        let e = load(idle_mac());
        let nodes = vec![
            ("a", true, Some(&a)),
            ("b", true, Some(&b)),
            ("c", true, Some(&c)),
            ("d", true, Some(&d)),
            ("e", false, Some(&e)),
            ("f", true, None),
        ];
        let p = rank_placements(nodes, 4.0, true, 20.0);
        let ids: Vec<&str> = p.iter().map(|x| x.node_id.as_str()).collect();
        assert_eq!(ids, vec!["a", "c", "b"]);
        // node_fleet score: 128*(60-20)/100 = 51.2 - 4 - 10*0.1 = 46.2
        assert_eq!(p[0].score, 46.2);
        let mut batt = idle_mac();
        batt.on_ac = Some(false);
        assert_eq!(verdict(&batt, 4.0, false, 20.0).score, 26.2);
    }

    #[test]
    fn refusal_wire_shape_and_status() {
        let r = Refusal::new(RefusalCode::HeavySlotHeld, "x");
        assert_eq!(
            serde_json::to_value(&r).unwrap(),
            serde_json::json!({"code":"heavy_slot_held","reason":"x"})
        );
        assert_eq!(RefusalCode::JobsDisabled.http_status(), 403);
        assert_eq!(RefusalCode::BenchmarkRunning.http_status(), 503);
        assert_eq!(RefusalCode::UserActive.http_status(), 409);
    }

    #[test]
    fn job_view_has_no_command() {
        let v = JobView {
            brief: JobBrief {
                job_id: "j".into(),
                origin_node: "o".into(),
                kind: JobKind::Shell,
                started_at: Utc::now(),
                state: JobState::Running,
                heavy: false,
            },
            ended_at: None,
            exit_code: None,
            reason: None,
            session_id: None,
            log_bytes: 0,
        };
        let s = serde_json::to_string(&v).unwrap();
        assert!(!s.contains("command"), "{s}");
        assert!(s.contains("\"job_id\":\"j\""), "{s}");
    }
}
