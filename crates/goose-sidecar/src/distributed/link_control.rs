//! LeanZero Link as the distributed engine's CONTROL plane (the requester's side, and the wire
//! types both sides speak). The data plane is untouched: tensors/activations stay JACCL over the
//! Thunderbolt RDMA devices (or ring over the TB IPs) that discovery chose; only probes,
//! provisioning, rank start/status/stop ride the mesh, to the PEER'S OWN goosed, which runs its
//! rank locally ([`super::link_host`]).
//!
//! A Link node is named in the config by the host `link:<node id>` (the field that carries an ssh
//! alias for a headless node), so every existing call site already routes to the peer, and a
//! config with ssh aliases behaves exactly as before.
//!
//! Protocol — `POST /v1/swarm/distributed/<op>` on the peer's control service (bearer node
//! token or `?token=`, constant time; an `Origin` header is refused; the peer's switch "Allow this
//! Mac to serve as a distributed node" off → 403), JSON bodies in camelCase, [`LinkOp`] names the
//! op. A refusal the requester must act on is `409 {code, message}` ([`LinkRefusal`]).
//!
//! The rank's session: the peer spawns the rank as its own child and holds it under a LEASE that
//! every `rankPoll` renews. Here a local SESSION PROCESS stands where the ssh client stood on the
//! headless path — its lifetime is the remote rank's: it exits with the rank's own code when the
//! peer reports the exit, and ending it here (the stop sequence's per-pid kill, a dropped run)
//! makes the relay tell the peer to stop the rank, as the pty's hang-up did over ssh. So the
//! supervisor's liveness and stop rules apply unchanged.

use std::collections::HashMap;
use std::process::Stdio;
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use tokio::process::{ChildStdin, Command};

use super::config::{NodeConfig, Runner};
use super::exec::{BoxFuture, ExecOutput, NodeExec};
use super::launch::{RankLive, RankMemory, RankProcess, RankSpec};
use super::node_op::NodeOp;
use super::provision::EnvSpec;
use super::supervisor::{StopReport, POLL_INTERVAL, READY_TICK};

/// The host prefix naming a LeanZero Link node in `NodeConfig::ssh`.
pub const LINK_HOST_PREFIX: &str = "link:";

/// parity: ssh's own bound on a vanished peer — `ServerAliveInterval=5` × `ServerAliveCountMax=3`
/// (`exec::SSH_OPTIONS`). Over ssh, a requester that vanishes for that long loses its session and
/// the pty hang-up takes the remote rank; a Link peer gives its requester the same bound between
/// polls before it stops the rank itself (never an orphaned 20 GB rank).
pub const LINK_LEASE: Duration = Duration::from_secs(5 * 3);

pub fn link_host(node_id: &str) -> String {
    format!("{LINK_HOST_PREFIX}{node_id}")
}

/// The Link node id when `host` names one.
pub fn link_peer(host: Option<&str>) -> Option<&str> {
    host?
        .strip_prefix(LINK_HOST_PREFIX)
        .filter(|id| !id.is_empty())
}

/// The ops of the `/v1/swarm/distributed/<op>` route. One list; the route, the peer's dispatcher
/// and the requester all key on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkOp {
    /// Goose's discovery probe, run by the peer on itself → [`ExecAnswer`].
    Discover,
    /// One [`NodeOp`], authorized and built by the peer → [`ExecAnswer`].
    Exec,
    /// Build one of goose's managed envs on the peer → [`ProvisionStartAnswer`].
    ProvisionStart,
    ProvisionPoll,
    /// Spawn this peer's rank → [`RankStartAnswer`] (or 409 [`LinkRefusal`]).
    RankStart,
    /// The rank's state since `knownLines`; renews the lease → [`RankSnapshot`].
    RankPoll,
    /// Stop the rank per pid and verify it gone → [`RankStopAnswer`].
    RankStop,
}

impl LinkOp {
    pub const ALL: [LinkOp; 7] = [
        LinkOp::Discover,
        LinkOp::Exec,
        LinkOp::ProvisionStart,
        LinkOp::ProvisionPoll,
        LinkOp::RankStart,
        LinkOp::RankPoll,
        LinkOp::RankStop,
    ];

    pub fn path(self) -> &'static str {
        match self {
            LinkOp::Discover => "discover",
            LinkOp::Exec => "exec",
            LinkOp::ProvisionStart => "provisionStart",
            LinkOp::ProvisionPoll => "provisionPoll",
            LinkOp::RankStart => "rankStart",
            LinkOp::RankPoll => "rankPoll",
            LinkOp::RankStop => "rankStop",
        }
    }

    pub fn from_path(path: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|op| op.path() == path)
    }
}

/// A script's outcome as the peer saw it (`exec`, `discover`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecAnswer {
    pub status: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl From<ExecOutput> for ExecAnswer {
    fn from(out: ExecOutput) -> Self {
        Self {
            status: out.status,
            stdout: out.stdout,
            stderr: out.stderr,
        }
    }
}

impl From<ExecAnswer> for ExecOutput {
    fn from(answer: ExecAnswer) -> Self {
        Self {
            status: answer.status,
            stdout: answer.stdout,
            stderr: answer.stderr,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoverRequest {
    #[serde(default)]
    pub extra_roots: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecRequest {
    pub op: NodeOp,
}

/// One of goose's managed envs, named — the peer builds its [`EnvSpec`] from its own pins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ManagedEnv {
    Tensor,
    Pipeline,
}

impl ManagedEnv {
    pub fn spec(self) -> EnvSpec {
        match self {
            ManagedEnv::Tensor => EnvSpec::tensor(),
            ManagedEnv::Pipeline => EnvSpec::pipeline(),
        }
    }

    pub fn of(spec: &EnvSpec) -> Option<Self> {
        [ManagedEnv::Tensor, ManagedEnv::Pipeline]
            .into_iter()
            .find(|env| env.spec().name == spec.name)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProvisionStartRequest {
    pub env: ManagedEnv,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProvisionStartAnswer {
    pub job_id: String,
    /// The interpreter the env provides on the peer.
    pub python: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProvisionPollRequest {
    pub job_id: String,
    /// Lines already read (the answer carries the ones after it).
    pub since: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProvisionPollAnswer {
    pub lines: Vec<String>,
    /// `since` for the next poll.
    pub next: usize,
    pub finished: bool,
    /// The script's exit status once finished (`None` = killed by a signal).
    pub status: Option<i32>,
    /// A failure to run the script at all.
    pub error: Option<String>,
}

/// Who asked a peer for a rank — shown on the peer ("Rank 1 of <name>'s distributed engine").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Requester {
    pub node_id: String,
    pub hostname: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RankStartRequest {
    /// One launch of one run (a restart is a new id).
    pub run_id: String,
    pub requester: Requester,
    /// The peer's node as the requester's config describes it (its paths are the peer's).
    pub node: NodeConfig,
    pub spec: RankSpec,
    pub model_id: String,
    pub served_model_id: String,
    pub runner: Runner,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RankStartAnswer {
    pub rank_id: String,
    pub pid: u32,
    /// The peer's lease: no poll for this long and it stops the rank itself.
    pub lease_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RankPollRequest {
    pub rank_id: String,
    /// The rank's line count the requester already mirrors; the tail is sent only past it.
    pub known_lines: u64,
}

/// How a hosted rank ended, as its host observed it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RankExit {
    pub code: Option<i32>,
    pub signal: Option<i32>,
    /// Who ended it and why ("exited on its own", "stopped at the requester's request", "lease
    /// expired: …").
    pub reason: String,
}

impl RankExit {
    /// The session process's exit code: the rank's own, or the shell convention for a signal.
    pub fn session_code(&self) -> i32 {
        match (self.code, self.signal) {
            (Some(code), _) => code,
            (None, Some(signal)) => 128 + signal,
            (None, None) => SESSION_UNKNOWN_EXIT,
        }
    }
}

/// The session process's code when the rank's own is unknown — ssh's code for "the session
/// ended without the remote command's status".
const SESSION_UNKNOWN_EXIT: i32 = 255;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RankSnapshot {
    pub rank_id: String,
    pub pid: u32,
    pub lines: u64,
    /// The rank's last lines, present when `lines` moved past `knownLines`.
    pub tail: Option<Vec<String>>,
    pub group_joined: bool,
    pub caps: Option<serde_json::Value>,
    pub memory: Option<RankMemory>,
    pub exit: Option<RankExit>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RankStopRequest {
    pub rank_id: String,
    /// Pipeline: rank 0's shutdown broadcast takes every rank — wait the grace for it first.
    pub follow_rank0: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RankStopAnswer {
    /// The peer's own stop sequence (per pid) and its verification.
    pub report: StopReport,
    pub exit: Option<RankExit>,
}

/// A named refusal (HTTP 409): the requester shows `code` and `message` verbatim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkRefusal {
    pub code: String,
    pub message: String,
}

/// Why a Link call did not produce the op's answer. Every variant names its cause; none is ever
/// read as an empty success.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkCallError {
    /// This goosed has no Link, or the mesh is not connected.
    NotConnected(String),
    /// The peer could not be reached over the mesh (or answered outside the contract).
    Unreachable(String),
    /// The peer's switch is off (403).
    Disabled(String),
    /// The peer's goosed does not serve distributed nodes (501), or not this op (404).
    NotServed(String),
    /// The peer refused with a named code (409).
    Refused(LinkRefusal),
    /// The peer read the request as malformed (400).
    BadRequest(String),
    /// The peer failed doing it (500).
    Failed(String),
}

impl std::fmt::Display for LinkCallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LinkCallError::NotConnected(m) => write!(f, "LeanZero Link is not connected: {m}"),
            LinkCallError::Unreachable(m) => write!(f, "unreachable over LeanZero Link: {m}"),
            LinkCallError::Disabled(m) => write!(
                f,
                "servingDisabled: \"Allow this Mac to serve as a distributed node\" is off there \
                 ({m})"
            ),
            LinkCallError::NotServed(m) => write!(f, "not served there: {m}"),
            LinkCallError::Refused(r) => write!(f, "{}: {}", r.code, r.message),
            LinkCallError::BadRequest(m) => write!(f, "bad request: {m}"),
            LinkCallError::Failed(m) => write!(f, "failed there: {m}"),
        }
    }
}

impl std::error::Error for LinkCallError {}

/// The mesh, as the distributed engine needs it. goose implements it over its `LinkManager`
/// (`POST /v1/swarm/distributed/<op>` through the mesh daemon's SOCKS5 listener).
pub trait LinkTransport: Send + Sync {
    fn call<'a>(
        &'a self,
        peer: &'a str,
        op: LinkOp,
        body: serde_json::Value,
    ) -> BoxFuture<'a, Result<serde_json::Value, LinkCallError>>;

    /// This node as a requester names itself to a peer.
    fn requester(&self) -> BoxFuture<'_, Result<Requester, LinkCallError>>;
}

static TRANSPORT: OnceLock<Arc<dyn LinkTransport>> = OnceLock::new();

/// Install the process's Link transport (goose does, once). A second call is ignored.
pub fn install_transport(transport: Arc<dyn LinkTransport>) {
    let _ = TRANSPORT.set(transport);
}

pub fn transport() -> Result<Arc<dyn LinkTransport>, LinkCallError> {
    TRANSPORT.get().cloned().ok_or_else(|| {
        LinkCallError::NotConnected("this goosed runs no LeanZero Link transport".to_string())
    })
}

/// One typed call: serialize the request, decode the answer.
pub async fn call_typed<Req: Serialize, Resp: DeserializeOwned>(
    peer: &str,
    op: LinkOp,
    request: &Req,
) -> Result<Resp, LinkCallError> {
    let transport = transport()?;
    let body = serde_json::to_value(request)
        .map_err(|e| LinkCallError::BadRequest(format!("serializing {}: {e}", op.path())))?;
    let value = transport.call(peer, op, body).await?;
    serde_json::from_value(value).map_err(|e| {
        LinkCallError::Unreachable(format!(
            "{peer} answered {} outside the contract: {e}",
            op.path()
        ))
    })
}

fn link_error(peer: &str, what: &str, error: LinkCallError) -> anyhow::Error {
    anyhow!("{what} on Link node '{peer}': {error}")
}

/// The distributed engine's node transport: `link:` hosts over LeanZero Link, every other host
/// through `inner` (ssh, or `/bin/sh` for this Mac) exactly as before.
pub struct LinkRoutedExec {
    inner: Arc<dyn NodeExec>,
}

impl LinkRoutedExec {
    pub fn new(inner: Arc<dyn NodeExec>) -> Self {
        Self { inner }
    }
}

impl NodeExec for LinkRoutedExec {
    fn run<'a>(
        &'a self,
        host: Option<&'a str>,
        script: &'a str,
    ) -> BoxFuture<'a, Result<ExecOutput>> {
        match link_peer(host) {
            // Loud, never a silent local run: a call site reaching a Link node with a raw script
            // must be moved to `run_op`.
            Some(peer) => Box::pin(async move {
                Err(anyhow!(
                    "refused to send a raw script to Link node '{peer}': goose sends only typed \
                     node operations over LeanZero Link (first line: {})",
                    script.lines().next().unwrap_or_default()
                ))
            }),
            None => self.inner.run(host, script),
        }
    }

    fn run_op<'a>(
        &'a self,
        host: Option<&'a str>,
        op: &'a NodeOp,
    ) -> BoxFuture<'a, Result<ExecOutput>> {
        match link_peer(host) {
            Some(peer) => Box::pin(async move {
                let answer: ExecAnswer =
                    call_typed(peer, LinkOp::Exec, &ExecRequest { op: op.clone() })
                        .await
                        .map_err(|e| link_error(peer, op.kind(), e))?;
                Ok(answer.into())
            }),
            None => self.inner.run_op(host, op),
        }
    }
}

/// Goose's discovery probe on a Link peer (the peer builds and runs the script on itself).
pub async fn discover(peer: &str, extra_roots: &[String]) -> Result<ExecOutput> {
    let answer: ExecAnswer = call_typed(
        peer,
        LinkOp::Discover,
        &DiscoverRequest {
            extra_roots: extra_roots.to_vec(),
        },
    )
    .await
    .map_err(|e| link_error(peer, "discovery", e))?;
    Ok(answer.into())
}

/// Build one of goose's managed envs on a Link peer, handing every output line to `on_line` as the
/// peer reports it — the Link twin of `provision::run_streaming`. No time bound: a cold install
/// downloads wheels; the poll cadence is the supervisor's own.
pub async fn provision(
    peer: &str,
    spec: &EnvSpec,
    mut on_line: impl FnMut(&str) + Send,
) -> Result<Option<i32>> {
    let env = ManagedEnv::of(spec).ok_or_else(|| {
        anyhow!(
            "'{}' is not one of goose's managed envs; a Link node builds only those",
            spec.name
        )
    })?;
    let started: ProvisionStartAnswer =
        call_typed(peer, LinkOp::ProvisionStart, &ProvisionStartRequest { env })
            .await
            .map_err(|e| link_error(peer, "provisioning", e))?;
    let mut since = 0;
    loop {
        tokio::time::sleep(READY_TICK).await;
        let poll: ProvisionPollAnswer = call_typed(
            peer,
            LinkOp::ProvisionPoll,
            &ProvisionPollRequest {
                job_id: started.job_id.clone(),
                since,
            },
        )
        .await
        .map_err(|e| link_error(peer, "provisioning progress", e))?;
        for line in &poll.lines {
            on_line(line);
        }
        since = poll.next;
        if poll.finished {
            if let Some(error) = poll.error {
                return Err(anyhow!("provisioning on Link node '{peer}': {error}"));
            }
            return Ok(poll.status);
        }
    }
}

/// A control-plane event the relay recorded; the supervisor's poll turns it into a run event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlEvent {
    /// The mesh stopped answering for this rank's peer (the error, as first seen).
    Lost(String),
    /// It answers again, after this long.
    Restored(Duration),
}

struct LinkRankState {
    peer: String,
    rank_id: String,
    events: StdMutex<Vec<ControlEvent>>,
    /// The session process's stdin: whoever learns the rank's end first writes the code.
    session: tokio::sync::Mutex<Option<ChildStdin>>,
    /// How the rank ended, when the peer reported it.
    exit: StdMutex<Option<RankExit>>,
}

impl LinkRankState {
    fn event(&self, event: ControlEvent) {
        self.events.lock().unwrap().push(event);
    }

    /// End the local session with `code` (idempotent: the first writer wins).
    async fn finish(&self, code: i32) {
        if let Some(mut stdin) = self.session.lock().await.take() {
            let _ = stdin.write_all(format!("{code}\n").as_bytes()).await;
            let _ = stdin.flush().await;
        }
    }
}

/// Live relays by (host, rank). A restart replaces the entry.
type Relays = StdMutex<HashMap<(String, usize), Arc<LinkRankState>>>;

fn relays() -> &'static Relays {
    static RELAYS: OnceLock<Relays> = OnceLock::new();
    RELAYS.get_or_init(Default::default)
}

fn relay_of(rank: &RankProcess) -> Option<Arc<LinkRankState>> {
    let host = rank.host.clone()?;
    relays().lock().unwrap().get(&(host, rank.rank)).cloned()
}

/// The control events recorded for `rank` since the last drain.
pub fn drain_events(rank: &RankProcess) -> Vec<ControlEvent> {
    relay_of(rank)
        .map(|state| std::mem::take(&mut *state.events.lock().unwrap()))
        .unwrap_or_default()
}

/// What one launch tells every peer about the run.
#[derive(Debug, Clone)]
pub struct LinkLaunch {
    pub run_id: String,
    pub model_id: String,
    pub served_model_id: String,
    pub runner: Runner,
}

/// The session process: waits for one line — the rank's exit code — and exits with it; EOF (the
/// relay gone) is ssh's 255.
const SESSION_SCRIPT: &str = "IFS= read -r code; exit \"${code:-255}\"";

/// Ask the Link peer for its rank and stand its session up here: the returned [`RankProcess`]
/// carries the PEER's rank pid, mirrors the rank's output lines, and its child is the local
/// session process (see the module doc).
pub async fn spawn_link_rank(
    node: &NodeConfig,
    spec: &RankSpec,
    launch: &LinkLaunch,
) -> Result<RankProcess> {
    let host = node
        .ssh
        .clone()
        .ok_or_else(|| anyhow!("node '{}' is this Mac, not a Link node", node.name))?;
    let peer = link_peer(Some(&host))
        .ok_or_else(|| anyhow!("node '{}' ({host}) is not a Link node", node.name))?
        .to_string();
    let transport = transport().map_err(|e| link_error(&peer, "rank start", e))?;
    let requester = transport
        .requester()
        .await
        .map_err(|e| link_error(&peer, "rank start", e))?;
    let started: RankStartAnswer = call_typed(
        &peer,
        LinkOp::RankStart,
        &RankStartRequest {
            run_id: launch.run_id.clone(),
            requester,
            node: node.clone(),
            spec: spec.clone(),
            model_id: launch.model_id.clone(),
            served_model_id: launch.served_model_id.clone(),
            runner: launch.runner,
        },
    )
    .await
    .map_err(|e| {
        anyhow!(
            "{} (rank {}) did not start its rank over LeanZero Link — {e}",
            node.name,
            spec.rank
        )
    })?;
    adopt_link_rank(node, spec, &peer, started).await
}

/// Stand up the local session for a rank the peer has started: the session process, the relay,
/// and the [`RankProcess`] the supervisor holds.
pub async fn adopt_link_rank(
    node: &NodeConfig,
    spec: &RankSpec,
    peer: &str,
    started: RankStartAnswer,
) -> Result<RankProcess> {
    let host = link_host(peer);
    let peer = peer.to_string();
    let mut cmd = Command::new("/bin/sh");
    cmd.arg("-c")
        .arg(SESSION_SCRIPT)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    crate::subprocess::configure_subprocess(&mut cmd);
    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(e) => {
            // The peer holds a rank nobody will poll: stop it now rather than wait out its lease.
            let _: Result<RankStopAnswer, _> = call_typed(
                &peer,
                LinkOp::RankStop,
                &RankStopRequest {
                    rank_id: started.rank_id.clone(),
                    follow_rank0: false,
                },
            )
            .await;
            return Err(anyhow!("spawning the Link session process: {e}"));
        }
    };
    let session_pid = child.id();
    let state = Arc::new(LinkRankState {
        peer: peer.clone(),
        rank_id: started.rank_id.clone(),
        events: StdMutex::new(Vec::new()),
        session: tokio::sync::Mutex::new(child.stdin.take()),
        exit: StdMutex::new(None),
    });
    relays()
        .lock()
        .unwrap()
        .insert((host.clone(), spec.rank), Arc::clone(&state));
    let live = Arc::new(StdMutex::new(RankLive {
        pid: Some(started.pid),
        ..Default::default()
    }));
    tokio::spawn(relay(
        Arc::clone(&state),
        Arc::clone(&live),
        session_pid,
        node.name.clone(),
    ));
    Ok(RankProcess {
        rank: spec.rank,
        node: node.name.clone(),
        host: Some(host),
        child,
        live,
    })
}

fn process_alive(pid: Option<u32>) -> bool {
    let Some(pid) = pid else { return false };
    // SAFETY: signal 0 only checks for existence; no signal is delivered.
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

fn mirror(live: &StdMutex<RankLive>, snapshot: &RankSnapshot) {
    let mut live = live.lock().unwrap();
    live.pid = Some(snapshot.pid);
    live.lines = snapshot.lines;
    live.group_joined = snapshot.group_joined;
    live.caps = snapshot.caps.clone();
    if snapshot.memory.is_some() {
        live.memory = snapshot.memory;
    }
    if let Some(tail) = &snapshot.tail {
        live.tail = tail.iter().cloned().collect();
    }
}

fn note(live: &StdMutex<RankLive>, line: String) {
    live.lock().unwrap().tail.push_back(line);
}

/// The relay: poll the peer at the supervisor's own cadence (its readiness tick until the rank
/// joined the group, then its poll interval), mirror what the rank said, and end the local session
/// when the rank ends. A poll that fails is a CONTROL loss — recorded, never read as the rank's
/// death; only past the peer's lease (when the peer has stopped the rank itself) does the session
/// end here, with ssh's 255.
async fn relay(
    state: Arc<LinkRankState>,
    live: Arc<StdMutex<RankLive>>,
    session_pid: Option<u32>,
    node: String,
) {
    let mut lost_since: Option<(Instant, String)> = None;
    loop {
        let tick = if live.lock().unwrap().group_joined {
            POLL_INTERVAL
        } else {
            READY_TICK
        };
        tokio::time::sleep(tick).await;
        if !process_alive(session_pid) {
            // Ended here (the stop sequence's per-pid kill, or a dropped run): the rank goes too.
            let _: Result<RankStopAnswer, _> = call_typed(
                &state.peer,
                LinkOp::RankStop,
                &RankStopRequest {
                    rank_id: state.rank_id.clone(),
                    follow_rank0: false,
                },
            )
            .await;
            return;
        }
        let known_lines = live.lock().unwrap().lines;
        let poll: Result<RankSnapshot, _> = call_typed(
            &state.peer,
            LinkOp::RankPoll,
            &RankPollRequest {
                rank_id: state.rank_id.clone(),
                known_lines,
            },
        )
        .await;
        match poll {
            Ok(snapshot) => {
                if let Some((since, _)) = lost_since.take() {
                    state.event(ControlEvent::Restored(since.elapsed()));
                }
                mirror(&live, &snapshot);
                if let Some(exit) = snapshot.exit {
                    note(
                        &live,
                        format!(
                            "[LeanZero Link] {node} reports its rank ended: {}",
                            exit.reason
                        ),
                    );
                    let code = exit.session_code();
                    *state.exit.lock().unwrap() = Some(exit);
                    state.finish(code).await;
                    return;
                }
            }
            Err(error) => {
                if lost_since.is_none() {
                    state.event(ControlEvent::Lost(error.to_string()));
                }
                let (since, first) = lost_since
                    .get_or_insert_with(|| (Instant::now(), error.to_string()))
                    .clone();
                if since.elapsed() > LINK_LEASE + tick {
                    note(
                        &live,
                        format!(
                            "[LeanZero Link] no answer from {node} for {:.1} s (first error: \
                             {first}); its lease ({} s without a poll) has stopped the rank there",
                            since.elapsed().as_secs_f64(),
                            LINK_LEASE.as_secs()
                        ),
                    );
                    state.finish(SESSION_UNKNOWN_EXIT).await;
                    return;
                }
            }
        }
    }
}

/// The stop sequence's step for a Link rank: the PEER stops its rank per pid and reports the
/// verified result. Returns the step line and whether the rank was verified gone.
pub async fn stop_link_rank(rank: &RankProcess, follow_rank0: bool) -> (String, bool) {
    let label = format!("rank {} ({})", rank.rank, rank.node);
    let Some(state) = relay_of(rank) else {
        return (
            format!("{label}: no Link session is known for it; nothing to stop over the mesh"),
            false,
        );
    };
    let answer: Result<RankStopAnswer, _> = call_typed(
        &state.peer,
        LinkOp::RankStop,
        &RankStopRequest {
            rank_id: state.rank_id.clone(),
            follow_rank0,
        },
    )
    .await;
    match answer {
        Ok(answer) => {
            let code = answer
                .exit
                .as_ref()
                .map(RankExit::session_code)
                .unwrap_or(SESSION_UNKNOWN_EXIT);
            if let Some(exit) = answer.exit.clone() {
                *state.exit.lock().unwrap() = Some(exit);
            }
            state.finish(code).await;
            (
                format!(
                    "{label} over LeanZero Link: {} → {}",
                    answer.report.steps.join("; "),
                    if answer.report.verified {
                        "verified gone by the peer"
                    } else {
                        "NOT verified by the peer"
                    }
                ),
                answer.report.verified,
            )
        }
        Err(error) => {
            let ended = state.exit.lock().unwrap().clone();
            match ended {
                // The peer already reported the rank's end before the mesh went quiet.
                Some(exit) => (
                    format!(
                        "{label}: ended on {} before the stop ({}); the stop call failed: {error}",
                        state.peer, exit.reason
                    ),
                    true,
                ),
                None => (
                    format!(
                        "{label}: cannot reach {} over LeanZero Link to stop it ({error}); its \
                         lease stops the rank within {} s of the last poll — NOT verified",
                        state.peer,
                        LINK_LEASE.as_secs()
                    ),
                    false,
                ),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::distributed::exec::SSH_OPTIONS;

    #[test]
    fn a_link_host_is_named_and_an_ssh_alias_is_not() {
        assert_eq!(link_host("workhorse-7f3a"), "link:workhorse-7f3a");
        assert_eq!(
            link_peer(Some("link:workhorse-7f3a")),
            Some("workhorse-7f3a")
        );
        assert_eq!(link_peer(Some("workhorse")), None);
        assert_eq!(link_peer(Some("link:")), None);
        assert_eq!(link_peer(None), None);
    }

    #[test]
    fn the_lease_is_the_bound_ssh_gives_a_vanished_requester() {
        let option = |name: &str| {
            SSH_OPTIONS
                .iter()
                .find_map(|o| o.strip_prefix(&format!("{name}=")))
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or_else(|| panic!("{name} left SSH_OPTIONS"))
        };
        assert_eq!(
            LINK_LEASE.as_secs(),
            option("ServerAliveInterval") * option("ServerAliveCountMax")
        );
    }

    #[test]
    fn every_op_round_trips_its_path() {
        for op in LinkOp::ALL {
            assert_eq!(LinkOp::from_path(op.path()), Some(op));
        }
        assert_eq!(LinkOp::from_path("shell"), None);
    }

    #[test]
    fn a_signal_or_an_unknown_end_maps_to_the_shell_convention() {
        let exit = |code, signal| RankExit {
            code,
            signal,
            reason: String::new(),
        };
        assert_eq!(exit(Some(0), None).session_code(), 0);
        assert_eq!(exit(None, Some(15)).session_code(), 143);
        assert_eq!(exit(None, None).session_code(), 255);
    }

    #[test]
    fn managed_envs_are_named_by_their_pins() {
        assert_eq!(ManagedEnv::of(&EnvSpec::tensor()), Some(ManagedEnv::Tensor));
        assert_eq!(
            ManagedEnv::of(&EnvSpec::pipeline()),
            Some(ManagedEnv::Pipeline)
        );
        let mut other = EnvSpec::tensor();
        other.name = "mine".into();
        assert_eq!(ManagedEnv::of(&other), None);
    }

    #[tokio::test]
    async fn a_raw_script_never_reaches_a_link_node() {
        struct Local;
        impl NodeExec for Local {
            fn run<'a>(
                &'a self,
                host: Option<&'a str>,
                _: &'a str,
            ) -> BoxFuture<'a, Result<ExecOutput>> {
                Box::pin(async move {
                    Ok(ExecOutput {
                        status: Some(0),
                        stdout: host.unwrap_or("local").to_string(),
                        stderr: String::new(),
                    })
                })
            }
        }
        let exec = LinkRoutedExec::new(Arc::new(Local));
        let err = exec
            .run(Some("link:peer"), "/bin/rm -rf /\n")
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("typed node operations"), "{err}");
        assert_eq!(
            exec.run(Some("workhorse"), "true").await.unwrap().stdout,
            "workhorse",
            "an ssh alias goes through the inner transport unchanged"
        );
        assert_eq!(exec.run(None, "true").await.unwrap().stdout, "local");
    }

    #[tokio::test]
    async fn the_session_process_exits_with_the_code_it_is_handed() {
        let mut child = Command::new("/bin/sh")
            .arg("-c")
            .arg(SESSION_SCRIPT)
            .stdin(Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take().unwrap();
        stdin.write_all(b"143\n").await.unwrap();
        assert_eq!(child.wait().await.unwrap().code(), Some(143));

        let mut child = Command::new("/bin/sh")
            .arg("-c")
            .arg(SESSION_SCRIPT)
            .stdin(Stdio::piped())
            .spawn()
            .unwrap();
        drop(child.stdin.take());
        assert_eq!(
            child.wait().await.unwrap().code(),
            Some(255),
            "the relay gone without a code reads as ssh's 255"
        );
    }
}
