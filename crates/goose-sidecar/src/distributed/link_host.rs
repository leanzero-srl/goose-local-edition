//! LeanZero Link: THIS Mac as a node of another Mac's distributed engine (the peer's side of
//! [`super::link_control`]). A same-account requester's goosed asks over the mesh; this goosed runs
//! goose's own node operations on itself, builds its own managed envs, and spawns, watches and
//! stops its rank as its OWN child — per pid, never a process group — and verifies it gone.
//!
//! Gates, in order: the host's switch ("Let my other Macs use this Mac › Run part of a split model", off by
//! default) is the control route's 403 before anything here runs; then this module refuses by
//! name what would break "one engine owns a Mac": a second rank (`alreadyHosting`), this Mac's
//! single engine mounted (`singleEngineMounted`), this Mac's own distributed engine running
//! (`distributedEngineActive`); and it runs only goose-managed interpreters under its own home
//! (`interpreterNotManaged`). While it holds a rank, this Mac's single engine is refused in turn
//! ([`holds_a_rank`]).
//!
//! The lease: every `rankPoll` renews it; [`LINK_LEASE`] without one and the rank is stopped here
//! with the reason recorded — the bound ssh gives a vanished requester.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use std::time::Instant;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::config::{Backend, Runner};
use super::exec::{NodeExec, SystemExec};
use super::launch::{self, RankPhase, RankProcess, RANK_MARKER};
use super::link_control::{
    link_peer, ExecAnswer, ExecRequest, LinkOp, LinkRefusal, ProvisionPollAnswer,
    ProvisionPollRequest, ProvisionStartAnswer, ProvisionStartRequest, RankExit, RankPollRequest,
    RankSnapshot, RankStartAnswer, RankStartRequest, RankStopAnswer, RankStopRequest, Requester,
    LINK_LEASE,
};
use super::node_op::{self, NodeOp};
use super::preflight::now_ms;
use super::provision;
use super::supervisor::{self, StopReport, POLL_INTERVAL};

/// Why a Link op did not run here. The control route maps each to its HTTP class.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostError {
    /// 404: not one of [`LinkOp`] (or not answered by this layer).
    UnknownOp(String),
    /// 400: the body is not the op's request.
    BadRequest(String),
    /// 409: refused by name.
    Refused(LinkRefusal),
    /// 500: tried and failed here.
    Failed(String),
}

impl std::fmt::Display for HostError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HostError::UnknownOp(m) | HostError::BadRequest(m) | HostError::Failed(m) => {
                f.write_str(m)
            }
            HostError::Refused(r) => write!(f, "{}: {}", r.code, r.message),
        }
    }
}

fn refused(code: &str, message: impl Into<String>) -> HostError {
    HostError::Refused(LinkRefusal {
        code: code.to_string(),
        message: message.into(),
    })
}

/// The rank this Mac serves for another Mac, as its own UI shows it ("Rank 1 of <name>'s
/// distributed engine · <model> · JACCL").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostedRankStatus {
    pub rank_id: String,
    pub run_id: String,
    pub requester: Requester,
    pub rank: usize,
    pub size: usize,
    pub model_id: String,
    pub served_model_id: String,
    pub backend: Backend,
    pub runner: Runner,
    pub pid: Option<u32>,
    /// "loading" (weights arriving or warming up) | "serving" (the rank reported ready).
    pub state: String,
    /// "loading" | "warming" | "ready" — the rank's own reports (`RankLive::phase`); a tensor
    /// rank's caps line is its last word before it serves, so it reads "ready" there.
    pub phase: String,
    /// MLX's active bytes on the rank (its own `RANK_MEM`); `None` before its first report.
    pub loaded_bytes: Option<u64>,
    /// The weights the requester's preflight planned on this rank; `None` from a requester
    /// before it sent them.
    pub planned_weight_bytes: Option<u64>,
    pub started_ms: u64,
    /// When the requester last polled (the lease's clock).
    pub last_poll_ms: u64,
}

struct Hosted {
    status: HostedRankStatus,
    process: RankProcess,
    last_poll: Instant,
    exit: Option<RankExit>,
}

impl Hosted {
    fn live(&self) -> bool {
        self.exit.is_none()
    }

    /// Record the rank's own exit if it has happened.
    fn observe_exit(&mut self) {
        if self.exit.is_some() {
            return;
        }
        if let Ok(Some(status)) = self.process.child.try_wait() {
            self.exit = Some(exit_of(status, unasked_end(status)));
        }
    }
}

/// How a rank that nobody here stopped ended, in words: a code of its own, or a signal from
/// outside goose (goose stops ranks only through `stop_hosted`, which records its own reason).
fn unasked_end(status: std::process::ExitStatus) -> String {
    use std::os::unix::process::ExitStatusExt;
    match (status.code(), status.signal()) {
        (Some(code), _) => format!("exited on its own with code {code}"),
        (None, Some(signal)) => format!("killed by signal {signal} from outside goose"),
        (None, None) => "ended without a code or a signal".to_string(),
    }
}

fn exit_of(status: std::process::ExitStatus, reason: String) -> RankExit {
    use std::os::unix::process::ExitStatusExt;
    RankExit {
        code: status.code(),
        signal: status.signal(),
        reason,
    }
}

struct ProvisionJob {
    lines: Vec<String>,
    finished: bool,
    status: Option<i32>,
    error: Option<String>,
}

#[derive(Default)]
struct Host {
    hosted: tokio::sync::Mutex<Option<Hosted>>,
    jobs: StdMutex<HashMap<String, Arc<StdMutex<ProvisionJob>>>>,
    /// The last hosted status, for the synchronous readers (the single engine's mount guard, the
    /// status DTO). Written under `hosted`'s lock.
    snapshot: StdMutex<Option<HostedRankStatus>>,
}

fn host() -> &'static Host {
    static HOST: OnceLock<Host> = OnceLock::new();
    HOST.get_or_init(Host::default)
}

fn next_id(kind: &str) -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    format!(
        "{kind}-{}-{}-{}",
        std::process::id(),
        now_ms(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

type Observer = Box<dyn Fn(Option<&HostedRankStatus>) + Send + Sync>;

static OBSERVER: OnceLock<Observer> = OnceLock::new();

/// Called with the hosted rank each time one starts or ends here (goose publishes it for the
/// other goosed processes on this Mac). One observer per process; a second is ignored.
pub fn observe(observer: Observer) {
    let _ = OBSERVER.set(observer);
}

fn publish(hosted: &Option<Hosted>) {
    let next = hosted.as_ref().filter(|h| h.live()).map(|h| {
        let mut status = h.status.clone();
        status.pid = h.process.pid();
        let live = h.process.live.lock().unwrap();
        let phase = match (status.runner, live.phase()) {
            (Runner::MlxLmTensor, RankPhase::Warming) => RankPhase::Ready,
            (_, phase) => phase,
        };
        status.state = if phase == RankPhase::Ready {
            "serving"
        } else {
            "loading"
        }
        .to_string();
        status.phase = phase.as_str().to_string();
        status.loaded_bytes = live.memory.map(|m| m.active);
        drop(live);
        status
    });
    let changed = {
        let mut snapshot = host().snapshot.lock().unwrap();
        let changed = snapshot.as_ref().map(|s| &s.rank_id) != next.as_ref().map(|s| &s.rank_id);
        *snapshot = next.clone();
        changed
    };
    if changed {
        if let Some(observer) = OBSERVER.get() {
            observer(next.as_ref());
        }
    }
}

/// The rank this Mac serves for another Mac right now, if any.
pub fn hosting() -> Option<HostedRankStatus> {
    host().snapshot.lock().unwrap().clone()
}

/// This Mac holds a rank of another Mac's engine: its single engine must not mount.
pub fn holds_a_rank() -> bool {
    hosting().is_some()
}

fn home() -> Result<String, HostError> {
    dirs::home_dir()
        .map(|h| h.display().to_string())
        .ok_or_else(|| HostError::Failed("this Mac reports no home directory".to_string()))
}

fn decode<T: serde::de::DeserializeOwned>(op: LinkOp, body: Value) -> Result<T, HostError> {
    serde_json::from_value(body)
        .map_err(|e| HostError::BadRequest(format!("{} request: {e}", op.path())))
}

fn encode<T: Serialize>(value: &T) -> Result<Value, HostError> {
    serde_json::to_value(value).map_err(|e| HostError::Failed(format!("encoding the answer: {e}")))
}

/// Run one Link op on this Mac. `Discover` is answered by goose itself (it owns the discovery
/// script); every other op is answered here.
pub async fn dispatch(op: LinkOp, body: Value) -> Result<Value, HostError> {
    match op {
        LinkOp::Discover => Err(HostError::UnknownOp(
            "discover is answered by goose's own discovery, not the sidecar".to_string(),
        )),
        LinkOp::Exec => encode(&exec(decode(op, body)?).await?),
        LinkOp::ProvisionStart => encode(&provision_start(decode(op, body)?)?),
        LinkOp::ProvisionPoll => encode(&provision_poll(decode(op, body)?)?),
        LinkOp::RankStart => encode(&rank_start(decode(op, body)?, &home()?).await?),
        LinkOp::RankPoll => encode(&rank_poll(decode(op, body)?).await?),
        LinkOp::RankStop => encode(&rank_stop(decode(op, body)?).await?),
    }
}

async fn run_here(script: &str) -> Result<crate::distributed::ExecOutput, HostError> {
    SystemExec
        .run(None, script)
        .await
        .map_err(|e| HostError::Failed(format!("{e:#}")))
}

async fn exec(request: ExecRequest) -> Result<ExecAnswer, HostError> {
    let op = request.op;
    let home = home()?;
    let command = match &op {
        NodeOp::Signal { pid, .. } => Some(signal_proof(*pid).await?),
        _ => None,
    };
    let services = match &op {
        NodeOp::Repair { .. } => Some(
            run_here("/usr/sbin/networksetup -listnetworkserviceorder")
                .await?
                .stdout,
        ),
        _ => None,
    };
    if matches!(op, NodeOp::Compact) {
        // This Mac's own guard: never pressure a model loaded here — a rank this goosed hosts, or
        // any MLX engine process (another window's single engine included).
        if let Some(hosted) = hosting() {
            return Err(refused(
                "engineLoaded",
                format!(
                    "this Mac serves rank {} of {}'s distributed engine; compaction never runs \
                     beside a loaded model",
                    hosted.rank, hosted.requester.name
                ),
            ));
        }
        let listing = run_here(super::node_op::PROCESS_LIST_SCRIPT).await?;
        if !listing.success() {
            return Err(refused(
                "processListUnreadable",
                format!(
                    "this Mac's process list could not be read (exit {:?}: {}); compaction never \
                     runs without proof no engine is loaded here",
                    listing.status,
                    listing.stderr.trim()
                ),
            ));
        }
        let engines = super::compaction::engines_in(&listing.stdout);
        if let Some(refusal) = super::compaction::engine_refusal("this Mac", &engines) {
            return Err(refused(&refusal.code, refusal.message));
        }
    }
    let code = match &op {
        NodeOp::Probe { .. } => "interpreterNotManaged",
        NodeOp::Signal { .. } => "notAGooseRank",
        NodeOp::Repair { .. } => "repairNotLicensed",
        _ => "opRefused",
    };
    op.authorize(
        &home,
        |_| Ok(command.clone().flatten()),
        || {
            services
                .clone()
                .ok_or_else(|| anyhow::anyhow!("no service listing"))
        },
    )
    .map_err(|e| refused(code, format!("{e:#}")))?;
    let script = op
        .script()
        .map_err(|e| HostError::BadRequest(format!("{e:#}")))?;
    if matches!(op, NodeOp::Signal { .. } | NodeOp::Repair { .. }) {
        tracing::warn!(
            op = op.kind(),
            "distributed node: a Link requester changes this Mac"
        );
    }
    Ok(run_here(&script).await?.into())
}

/// The command line behind `pid` on this Mac, or `None` when ps PROVED no process holds it. A
/// ps that could not answer is refused here (`pidUnproven`) BEFORE `authorize` — gate 4: an
/// unanswered ps once read as "no such process" and the `/bin/kill` ran on a pid nobody had
/// proven was a goose rank. Nothing is signalled; the requester's next observation of the pid
/// (its `pidRow`) decides what the stop reports.
async fn signal_proof(pid: u32) -> Result<Option<String>, HostError> {
    let answer = match run_here(&format!("/bin/ps -o command= -p {pid}")).await {
        Ok(out) => out
            .ps_answer()
            .map(|line| line.map(str::to_string))
            .map_err(|e| format!("{e:#}")),
        Err(e) => Err(e.to_string()),
    };
    answer.map_err(|why| {
        tracing::warn!(
            event = "link_signal_unproven",
            pid,
            why = %why,
            "distributed node: signal NOT sent: ps could not prove the pid is a goose rank"
        );
        refused(
            "pidUnproven",
            format!("pid {pid}: ps could not prove what runs there ({why}); nothing was signalled"),
        )
    })
}

fn provision_start(request: ProvisionStartRequest) -> Result<ProvisionStartAnswer, HostError> {
    let spec = request.env.spec();
    let python = spec.python(&home()?);
    let job_id = next_id("provision");
    let job = Arc::new(StdMutex::new(ProvisionJob {
        lines: Vec::new(),
        finished: false,
        status: None,
        error: None,
    }));
    host()
        .jobs
        .lock()
        .unwrap()
        .insert(job_id.clone(), Arc::clone(&job));
    tokio::spawn(async move {
        let script = provision::provision_script(&spec);
        let sink = Arc::clone(&job);
        let result = provision::run_streaming(None, &script, move |line| {
            sink.lock().unwrap().lines.push(line.to_string());
        })
        .await;
        let mut job = job.lock().unwrap();
        job.finished = true;
        match result {
            Ok(status) => job.status = status,
            Err(e) => job.error = Some(format!("{e:#}")),
        }
    });
    Ok(ProvisionStartAnswer { job_id, python })
}

fn provision_poll(request: ProvisionPollRequest) -> Result<ProvisionPollAnswer, HostError> {
    let job = host()
        .jobs
        .lock()
        .unwrap()
        .get(&request.job_id)
        .cloned()
        .ok_or_else(|| {
            refused(
                "unknownJob",
                format!(
                    "no provisioning job '{}' on this Mac (its goosed restarted?)",
                    request.job_id
                ),
            )
        })?;
    let job = job.lock().unwrap();
    let since = request.since.min(job.lines.len());
    Ok(ProvisionPollAnswer {
        lines: job.lines[since..].to_vec(),
        next: job.lines.len(),
        finished: job.finished,
        status: job.status,
        error: job.error.clone(),
    })
}

async fn rank_start(request: RankStartRequest, home: &str) -> Result<RankStartAnswer, HostError> {
    let mut guard = host().hosted.lock().await;
    if let Some(hosted) = guard.as_mut() {
        hosted.observe_exit();
        if hosted.live() {
            let s = &hosted.status;
            return Err(refused(
                "alreadyHosting",
                format!(
                    "this Mac already serves rank {} of {}'s distributed engine ({}) since {} — \
                     one engine owns a Mac at a time",
                    s.rank, s.requester.name, s.model_id, s.started_ms
                ),
            ));
        }
    }
    let single = crate::engine::global_manager().status().await;
    if matches!(single.state.as_str(), "running" | "mounting") {
        return Err(refused(
            "singleEngineMounted",
            format!(
                "this Mac's single MLX engine is {} with '{}'; one engine owns a Mac at a time — \
                 unmount it there first",
                single.state,
                single.model_id.as_deref().unwrap_or("<model not reported>")
            ),
        ));
    }
    let own = supervisor::global_manager().status();
    if own.state.owns_the_mac() {
        return Err(refused(
            "distributedEngineActive",
            format!(
                "this Mac runs its own distributed engine ({}, '{}'); one engine owns a Mac at a \
                 time",
                own.state.as_str(),
                own.model_id.as_deref().unwrap_or("<model not reported>")
            ),
        ));
    }
    let spec = &request.spec;
    if spec.rank == 0 || spec.rank >= spec.size {
        return Err(HostError::BadRequest(format!(
            "rank {} of {}: a Link node serves a worker rank (1..{}); rank 0 is the requester",
            spec.rank, spec.size, spec.size
        )));
    }
    if link_peer(request.node.ssh.as_deref()).is_none() {
        return Err(HostError::BadRequest(format!(
            "node '{}' is not described as a Link node (host {:?})",
            request.node.name, request.node.ssh
        )));
    }
    let mut local = request.node.clone();
    local.ssh = None;
    let python = spec
        .interpreter(&local)
        .map_err(|e| HostError::BadRequest(format!("{e:#}")))?;
    node_op::managed_interpreter(home, python)
        .map_err(|e| refused("interpreterNotManaged", format!("{e:#}")))?;
    let process = launch::spawn_rank(&local, spec).map_err(|e| {
        HostError::Failed(format!(
            "spawning rank {} for {}: {e:#}",
            spec.rank, request.requester.name
        ))
    })?;
    let pid = process
        .pid()
        .ok_or_else(|| HostError::Failed("the rank exited before reporting a pid".to_string()))?;
    let started_ms = now_ms();
    let status = HostedRankStatus {
        rank_id: next_id("rank"),
        run_id: request.run_id,
        requester: request.requester,
        rank: spec.rank,
        size: spec.size,
        model_id: request.model_id,
        served_model_id: request.served_model_id,
        backend: spec.backend,
        runner: request.runner,
        pid: Some(pid),
        state: "loading".to_string(),
        phase: RankPhase::Loading.as_str().to_string(),
        loaded_bytes: None,
        planned_weight_bytes: spec.planned_weight_bytes,
        started_ms,
        last_poll_ms: started_ms,
    };
    tracing::info!(
        rank = status.rank,
        requester = %status.requester.name,
        model = %status.model_id,
        pid,
        "distributed node: serving a rank for a Link requester"
    );
    let answer = RankStartAnswer {
        rank_id: status.rank_id.clone(),
        pid,
        lease_ms: LINK_LEASE.as_millis() as u64,
    };
    *guard = Some(Hosted {
        status,
        process,
        last_poll: Instant::now(),
        exit: None,
    });
    publish(&guard);
    drop(guard);
    ensure_lease_watch();
    Ok(answer)
}

fn unknown_rank(rank_id: &str) -> HostError {
    refused(
        "unknownRank",
        format!(
            "this Mac holds no rank '{rank_id}' (its goosed restarted, or another requester's \
             rank replaced it)"
        ),
    )
}

async fn rank_poll(request: RankPollRequest) -> Result<RankSnapshot, HostError> {
    let mut guard = host().hosted.lock().await;
    let hosted = guard
        .as_mut()
        .filter(|h| h.status.rank_id == request.rank_id)
        .ok_or_else(|| unknown_rank(&request.rank_id))?;
    hosted.last_poll = Instant::now();
    hosted.status.last_poll_ms = now_ms();
    hosted.observe_exit();
    let snapshot = {
        let live = hosted.process.live.lock().unwrap();
        RankSnapshot {
            rank_id: hosted.status.rank_id.clone(),
            pid: live.pid.or(hosted.status.pid).unwrap_or_default(),
            lines: live.lines,
            tail: (live.lines > request.known_lines).then(|| live.tail.iter().cloned().collect()),
            group_joined: live.group_joined,
            caps: live.caps.clone(),
            memory: live.memory,
            ready: live.ready,
            exit: hosted.exit.clone(),
            state: live.state.clone(),
            log: live.log.clone(),
        }
    };
    publish(&guard);
    Ok(snapshot)
}

async fn rank_stop(request: RankStopRequest) -> Result<RankStopAnswer, HostError> {
    let mut guard = host().hosted.lock().await;
    let hosted = guard
        .as_mut()
        .filter(|h| h.status.rank_id == request.rank_id)
        .ok_or_else(|| unknown_rank(&request.rank_id))?;
    let why = format!("stopped at {}'s request", hosted.status.requester.name);
    let report = stop_hosted(hosted, request.follow_rank0, why).await;
    let exit = hosted.exit.clone();
    publish(&guard);
    Ok(RankStopAnswer { report, exit })
}

/// THIS Mac's stop sequence for its hosted rank: (pipeline) the grace for rank 0's shutdown
/// broadcast, then SIGTERM → grace → SIGKILL to the rank's own pid through the supervisor's
/// local stop, then `ps` to verify the pid is gone.
async fn stop_hosted(hosted: &mut Hosted, follow_rank0: bool, why: String) -> StopReport {
    hosted.observe_exit();
    let mut report = StopReport {
        steps: Vec::new(),
        verified: true,
    };
    let pid = hosted.process.pid();
    if let Some(exit) = &hosted.exit {
        report
            .steps
            .push(format!("rank already ended ({})", exit.reason));
        return report;
    }
    if follow_rank0 {
        if let Some(status) = supervisor::wait_child_exit(&mut hosted.process.child).await {
            report.steps.push(format!(
                "rank {} pid {} left on rank 0's shutdown broadcast ({status})",
                hosted.status.rank,
                pid.unwrap_or_default()
            ));
            hosted.exit = Some(exit_of(
                status,
                "left on rank 0's shutdown broadcast".into(),
            ));
        } else {
            report.steps.push(format!(
                "rank {} pid {}: still running after rank 0's shutdown broadcast and the grace \
                 window",
                hosted.status.rank,
                pid.unwrap_or_default()
            ));
        }
    }
    if hosted.exit.is_none() {
        let local = supervisor::stop_ranks(
            std::slice::from_mut(&mut hosted.process),
            &SystemExec,
            None,
            false,
        )
        .await;
        report.verified &= local.verified;
        report.steps.extend(local.steps);
        if let Ok(Some(status)) = hosted.process.child.try_wait() {
            hosted.exit = Some(exit_of(status, why));
        }
    }
    if let Some(pid) = pid {
        match run_here(&node_op::pid_row_script(pid)).await {
            Ok(out) => match out
                .ps_answer()
                .and_then(|rows| rows.map_or(Ok(None), super::probe::parse_ps_row))
            {
                Ok(None) => report
                    .steps
                    .push(format!("ps -p {pid}: no such process (verified here)")),
                Ok(Some(row)) if row.zombie() => report
                    .steps
                    .push(format!("ps -p {pid}: a zombie awaiting its reap (gone)")),
                Ok(Some(row)) => {
                    report.verified = false;
                    report
                        .steps
                        .push(format!("ps -p {pid}: STILL RUNNING (stat {})", row.stat));
                }
                Err(e) => {
                    report.verified = false;
                    report.steps.push(format!("ps -p {pid}: unreadable: {e:#}"));
                }
            },
            Err(e) => {
                report.verified = false;
                report.steps.push(format!("ps -p {pid}: {e}"));
            }
        }
    }
    report
}

/// The lease: one watcher per process, on the supervisor's poll cadence. A rank whose requester
/// has not polled for [`LINK_LEASE`] is stopped here, per pid, with the reason recorded; a rank
/// that exited on its own is recorded so this Mac's UI stops showing it.
fn ensure_lease_watch() {
    static WATCH: OnceLock<()> = OnceLock::new();
    WATCH.get_or_init(|| {
        tokio::spawn(async {
            loop {
                tokio::time::sleep(POLL_INTERVAL).await;
                let mut guard = host().hosted.lock().await;
                if let Some(hosted) = guard.as_mut() {
                    hosted.observe_exit();
                    let silent = hosted.last_poll.elapsed();
                    if hosted.live() && silent > LINK_LEASE {
                        let why = format!(
                            "lease expired: no poll from {} for {:.1} s (the lease is {} s — \
                             the bound ssh gives a vanished requester)",
                            hosted.status.requester.name,
                            silent.as_secs_f64(),
                            LINK_LEASE.as_secs()
                        );
                        tracing::warn!("distributed node: {why}");
                        let report = stop_hosted(hosted, false, why).await;
                        tracing::warn!(
                            verified = report.verified,
                            "distributed node: lease stop: {}",
                            report.steps.join("; ")
                        );
                    }
                }
                publish(&guard);
            }
        });
    });
}

/// A rank a previous goosed on this Mac hosted and never stopped: that goosed was killed outright,
/// so neither its exit path nor its lease watcher ran. Stopped here, per pid, only while the pid's
/// command line still carries the goose rank marker (a reused pid is somebody else's process).
/// `Ok(None)` = nothing of ours runs at `pid`; otherwise the step line and whether it is gone.
/// A ps that could not answer is an `Err` (`pidUnproven`, nothing signalled): the caller keeps
/// the record, so the mount stays refused by name and the next reclaim looks again.
pub async fn reclaim_orphan(pid: u32) -> Result<Option<(String, bool)>, HostError> {
    match signal_proof(pid).await? {
        Some(command) if command.contains(RANK_MARKER) => {}
        _ => return Ok(None),
    }
    Ok(Some(
        supervisor::reclaim_rank_pid(&SystemExec, None, "this Mac", pid).await,
    ))
}

/// goosed's exit path: a rank this Mac hosts must not outlive it.
pub async fn shutdown() -> String {
    let mut guard = host().hosted.lock().await;
    let Some(hosted) = guard.as_mut().filter(|h| h.live()) else {
        return "no hosted rank".to_string();
    };
    let rank = hosted.status.rank;
    let report = stop_hosted(hosted, false, "this Mac's goosed is exiting".to_string()).await;
    publish(&guard);
    format!(
        "hosted rank {rank}: {}; {}",
        if report.verified {
            "stopped, verified"
        } else {
            "stopped, NOT verified"
        },
        report.steps.join("; ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_malformed_request_is_a_bad_request_and_discover_is_not_ours() {
        let err = dispatch(LinkOp::RankPoll, serde_json::json!({"nope": 1}))
            .await
            .unwrap_err();
        assert!(matches!(err, HostError::BadRequest(_)), "{err:?}");
        let err = dispatch(LinkOp::Discover, serde_json::json!({}))
            .await
            .unwrap_err();
        assert!(matches!(err, HostError::UnknownOp(_)), "{err:?}");
    }

    #[tokio::test]
    async fn a_poll_for_a_rank_this_mac_never_held_is_refused_by_name() {
        let err = dispatch(
            LinkOp::RankPoll,
            serde_json::json!({"rankId": "rank-x", "knownLines": 0}),
        )
        .await
        .unwrap_err();
        match err {
            HostError::Refused(r) => assert_eq!(r.code, "unknownRank"),
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn a_signal_to_a_process_that_is_not_a_goose_rank_is_refused_here() {
        let err = dispatch(
            LinkOp::Exec,
            serde_json::json!({"op": {"kind": "signal", "pid": std::process::id(), "signal": "TERM"}}),
        )
        .await
        .unwrap_err();
        match err {
            HostError::Refused(r) => assert_eq!(r.code, "notAGooseRank", "{}", r.message),
            other => panic!("{other:?}"),
        }
    }

    /// Gate 4 through the real `/bin/ps`: a pid ps cannot answer for (`process id too large`,
    /// exit 1 with stderr) is refused by name and nothing is signalled — the failed proof never
    /// reads as "no such process". macOS only: Linux procps answers the same pid as absent.
    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn a_signal_whose_pid_ps_cannot_answer_for_is_refused_unsent() {
        let err = dispatch(
            LinkOp::Exec,
            serde_json::json!({"op": {"kind": "signal", "pid": 999_999_999u32, "signal": "TERM"}}),
        )
        .await
        .unwrap_err();
        match err {
            HostError::Refused(r) => {
                assert_eq!(r.code, "pidUnproven", "{}", r.message);
                assert!(r.message.contains("nothing was signalled"), "{}", r.message);
            }
            other => panic!("{other:?}"),
        }
    }

    /// A stand-in pipeline rank under the REAL rank program (rank_env.py + pipeline_rank.py) and
    /// a goose-managed interpreter under a temp home: it joins its "group", reports ready, and
    /// leaves on SIGTERM with 0 — or when the test process that spawned it is gone. The host
    /// holding it is a process global nothing drops, so a test that fails before its stop used to
    /// leave a `/usr/bin/python3 … goose-distributed-rank` orphan behind — the shape of the pid
    /// 9425 found on 2026-09-25, which then blocked a real split's restore as a foreign rank.
    /// macOS only, with the tests that host it: the rank program takes the load lock through
    /// libproc (`rank_load_lock.py`), which Linux does not have.
    #[cfg(target_os = "macos")]
    fn stand_in_node(home: &std::path::Path) -> crate::distributed::NodeConfig {
        let site = home.join("site");
        std::fs::create_dir_all(site.join("mlx")).unwrap();
        std::fs::create_dir_all(site.join("rapid_mlx/distributed")).unwrap();
        std::fs::write(site.join("mlx/__init__.py"), "").unwrap();
        std::fs::write(
            site.join("mlx/core.py"),
            "def get_active_memory(): return 1\ndef get_peak_memory(): return 2\n\
             def get_cache_memory(): return 0\n",
        )
        .unwrap();
        std::fs::write(site.join("rapid_mlx/__init__.py"), "").unwrap();
        std::fs::write(site.join("rapid_mlx/distributed/__init__.py"), "").unwrap();
        std::fs::write(
            site.join("rapid_mlx/distributed/pipeline_qwen4_serve.py"),
            "import os, signal, threading\n\
             from dataclasses import dataclass\n\
             @dataclass\n\
             class _Job:\n\
             \x20   row: object\n\
             \x20   produced: int = 0\n\
             class _Engine:\n\
             \x20   def _start(self, row): pass\n\
             \x20   def prefill(self, words): pass\n\
             def prefill_chunks(start, end, step, split=0): return []\n\
             def _build_app(state, tokenizer, eos_ids, vision=None): pass\n\
             def add_arguments(parser):\n\
             \x20   for flag in ('--model', '--served-model-name', '--host', '--split'):\n\
             \x20       parser.add_argument(flag)\n\
             \x20   for flag in ('--port', '--context', '--slots', '--max-batch', '--prefill-step', '--attention-scores-bytes'):\n\
             \x20       parser.add_argument(flag, type=int)\n\
             def serve(options, emit=None):\n\
             \x20   done = threading.Event()\n\
             \x20   signal.signal(signal.SIGTERM, lambda *a: done.set())\n\
             \x20   emit('RANK_GROUP', {'rank': int(os.environ['MLX_RANK']), 'size': 2})\n\
             \x20   emit('READY', {'port': options.port})\n\
             \x20   parent = os.getppid()\n\
             \x20   while not done.wait(0.2) and os.getppid() == parent: pass\n\
             \x20   return 0\n",
        )
        .unwrap();
        let home_text = home.display().to_string();
        let python = crate::distributed::provision::EnvSpec::pipeline().python(&home_text);
        let bin = std::path::Path::new(&python).parent().unwrap();
        std::fs::create_dir_all(bin).unwrap();
        std::fs::write(
            &python,
            format!(
                "#!/bin/sh\nPYTHONPATH='{}' exec /usr/bin/python3 \"$@\"\n",
                site.display()
            ),
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&python, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut config = crate::distributed::config::tests::two_mac_config();
        let mut node = config.nodes.remove(1);
        node.ssh = Some(crate::distributed::link_control::link_host("peer-node"));
        node.pipeline_python = Some(python);
        node
    }

    #[cfg(target_os = "macos")]
    fn start_request(node: &crate::distributed::NodeConfig) -> RankStartRequest {
        let mut config = crate::distributed::config::tests::two_mac_config();
        config.nodes[1] = node.clone();
        config.nodes[0].pipeline_python = Some("/unused/rank0/python".to_string());
        let spec = launch::pipeline_rank_specs(
            &config,
            &crate::model_identity::ServedNames::only("served-id"),
            8_192,
            "19",
            2_048,
            &[0, 0],
            0.05,
        )
        .remove(1);
        RankStartRequest {
            run_id: "run-1".to_string(),
            requester: Requester {
                node_id: "macbook-1a2b".to_string(),
                hostname: "macbook".to_string(),
                name: "MacBook Pro".to_string(),
            },
            node: node.clone(),
            spec,
            model_id: "rapid-mlx/Qwen3.8-Flash-Next-4bit".to_string(),
            served_model_id: "served-id".to_string(),
            runner: Runner::PipelineQwen4,
        }
    }

    /// The host is one per process: the tests that hold a rank take turns.
    #[cfg(target_os = "macos")]
    static HOSTING_TESTS: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn a_hosted_rank_is_spawned_here_mirrored_refused_twice_and_stopped_verified() {
        let _turn = HOSTING_TESTS.lock().await;
        let home = tempfile::tempdir().unwrap();
        let node = stand_in_node(home.path());
        let home_text = home.path().display().to_string();
        let started = rank_start(start_request(&node), &home_text).await.unwrap();
        assert_eq!(started.lease_ms, LINK_LEASE.as_millis() as u64);

        let second = rank_start(start_request(&node), &home_text)
            .await
            .unwrap_err();
        match second {
            HostError::Refused(r) => {
                assert_eq!(r.code, "alreadyHosting");
                assert!(r.message.contains("rank 1 of MacBook Pro"), "{}", r.message);
            }
            other => panic!("{other:?}"),
        }

        let mut joined = None;
        for _ in 0..100 {
            let snapshot = rank_poll(RankPollRequest {
                rank_id: started.rank_id.clone(),
                known_lines: 0,
            })
            .await
            .unwrap();
            assert!(snapshot.exit.is_none(), "{snapshot:?}");
            if snapshot.group_joined {
                joined = Some(snapshot);
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        let joined = joined.expect("the rank joined its group");
        assert_eq!(joined.pid, started.pid);
        assert!(joined
            .tail
            .unwrap()
            .iter()
            .any(|l| l.starts_with("GOOSE_READY")));
        let shown = hosting().expect("this Mac shows the rank it serves");
        assert_eq!(shown.rank, 1);
        assert_eq!(shown.state, "serving");
        assert_eq!(shown.requester.name, "MacBook Pro");
        assert_eq!(shown.backend, Backend::Jaccl);
        assert!(holds_a_rank());

        let stopped = rank_stop(RankStopRequest {
            rank_id: started.rank_id.clone(),
            follow_rank0: false,
        })
        .await
        .unwrap();
        assert!(stopped.report.verified, "{:?}", stopped.report);
        assert!(
            stopped
                .report
                .steps
                .iter()
                .any(|s| s.contains(&format!("ps -p {}: no such process", started.pid))),
            "{:?}",
            stopped.report.steps
        );
        let exit = stopped.exit.expect("the exit is recorded");
        assert_eq!(exit.code, Some(0), "SIGTERM → serve() returned 0");
        assert_eq!(exit.reason, "stopped at MacBook Pro's request");
        assert!(hosting().is_none() && !holds_a_rank());

        let after = rank_poll(RankPollRequest {
            rank_id: started.rank_id,
            known_lines: joined.lines,
        })
        .await
        .unwrap();
        assert_eq!(after.exit.map(|e| e.code), Some(Some(0)));
        let unix_gone = unsafe { libc::kill(started.pid as libc::pid_t, 0) } != 0;
        assert!(unix_gone, "pid {} still exists", started.pid);

        let foreign = {
            let mut request = start_request(&node);
            request.node.pipeline_python = Some("/bin/sh".to_string());
            request
        };
        match rank_start(foreign, &home_text).await.unwrap_err() {
            HostError::Refused(r) => assert_eq!(r.code, "interpreterNotManaged"),
            other => panic!("{other:?}"),
        }
    }

    /// The mesh, replaced by a loopback into THIS process's host; `down` makes every call fail
    /// the way an unreachable peer does.
    #[cfg(target_os = "macos")]
    struct Loopback {
        down: std::sync::atomic::AtomicBool,
    }

    #[cfg(target_os = "macos")]
    impl crate::distributed::link_control::LinkTransport for Loopback {
        fn call<'a>(
            &'a self,
            _peer: &'a str,
            op: LinkOp,
            body: Value,
        ) -> crate::distributed::exec::BoxFuture<
            'a,
            Result<Value, crate::distributed::link_control::LinkCallError>,
        > {
            use crate::distributed::link_control::LinkCallError;
            Box::pin(async move {
                if self.down.load(Ordering::SeqCst) {
                    return Err(LinkCallError::Unreachable(
                        "SOCKS reply 1 (the mesh is down in this test)".to_string(),
                    ));
                }
                dispatch(op, body).await.map_err(|e| match e {
                    HostError::Refused(r) => LinkCallError::Refused(r),
                    HostError::BadRequest(m) => LinkCallError::BadRequest(m),
                    HostError::UnknownOp(m) => LinkCallError::NotServed(m),
                    HostError::Failed(m) => LinkCallError::Failed(m),
                })
            })
        }

        fn requester(
            &self,
        ) -> crate::distributed::exec::BoxFuture<
            '_,
            Result<Requester, crate::distributed::link_control::LinkCallError>,
        > {
            Box::pin(async {
                Ok(Requester {
                    node_id: "macbook-1a2b".to_string(),
                    hostname: "macbook".to_string(),
                    name: "MacBook Pro".to_string(),
                })
            })
        }
    }

    /// The requester's half against the host's, through the supervisor's own stop sequence: the
    /// relay mirrors the peer rank, a mesh outage is a named control event (never the rank's
    /// death), and the stop is the peer's verified per-pid stop, after which the local session
    /// process exits with the rank's own code.
    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn a_link_rank_is_relayed_survives_a_control_outage_and_stops_verified() {
        use crate::distributed::link_control::{self, ControlEvent};
        let _turn = HOSTING_TESTS.lock().await;
        let loopback = Arc::new(Loopback {
            down: std::sync::atomic::AtomicBool::new(false),
        });
        link_control::install_transport(loopback.clone());
        // The stand-in env lives under a temp home, which only `rank_start`'s parameter reaches
        // (the op reads the process's own `$HOME`): the host is started with it, and the
        // requester adopts that rank exactly as `spawn_link_rank` does after its `rankStart`.
        let home = tempfile::tempdir().unwrap();
        let node = stand_in_node(home.path());
        let request = start_request(&node);
        let started = rank_start(request.clone(), &home.path().display().to_string())
            .await
            .unwrap();
        let mut rank = link_control::adopt_link_rank(&node, &request.spec, "peer-node", started)
            .await
            .unwrap();

        for _ in 0..100 {
            if rank.live.lock().unwrap().group_joined {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        assert!(rank.live.lock().unwrap().group_joined, "{}", rank.tail());
        assert_eq!(rank.pid(), hosting().and_then(|h| h.pid));

        loopback.down.store(true, Ordering::SeqCst);
        tokio::time::sleep(POLL_INTERVAL * 2).await;
        loopback.down.store(false, Ordering::SeqCst);
        tokio::time::sleep(POLL_INTERVAL * 2).await;
        let events = link_control::drain_events(&rank);
        assert!(
            matches!(events.first(), Some(ControlEvent::Lost(e)) if e.contains("SOCKS reply 1")),
            "{events:?}"
        );
        // Silence is measured from the last answered poll (the peer's lease clock), so it spans
        // the whole outage — not just the time since the first failed poll.
        assert!(
            matches!(events.last(), Some(ControlEvent::Restored(silent)) if *silent >= POLL_INTERVAL * 3 / 2),
            "{events:?}"
        );
        assert!(
            rank.child.try_wait().unwrap().is_none(),
            "an outage shorter than the lease is not the rank's death"
        );

        let report = supervisor::stop_ranks(
            std::slice::from_mut(&mut rank),
            &link_control::LinkRoutedExec::new(Arc::new(SystemExec)),
            None,
            false,
        )
        .await;
        assert!(report.verified, "{:?}", report.steps);
        assert!(
            report.steps[0].contains("over LeanZero Link")
                && report.steps[0].contains("verified gone by the peer"),
            "{:?}",
            report.steps
        );
        assert_eq!(
            rank.child.try_wait().unwrap().and_then(|s| s.code()),
            Some(0),
            "the session process ends with the rank's own code"
        );
        assert!(hosting().is_none());
    }

    #[tokio::test]
    async fn a_read_only_op_runs_here_and_answers_the_script_output() {
        let value = dispatch(
            LinkOp::Exec,
            serde_json::json!({"op": {"kind": "pidRow", "pid": std::process::id()}}),
        )
        .await
        .unwrap();
        let answer: ExecAnswer = serde_json::from_value(value).unwrap();
        assert_eq!(answer.status, Some(0));
        let row = crate::distributed::probe::parse_ps_row(&answer.stdout)
            .unwrap()
            .unwrap();
        assert_eq!(row.pid, std::process::id());
    }
}
