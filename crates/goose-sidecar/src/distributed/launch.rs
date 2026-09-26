//! The rank processes. goose launches the ranks itself instead of through `mlx.launch`, measured
//! on STEP1b for three reasons: `mlx.launch` exits 0 when a rank dies (the rank's own status is
//! the only truth), SIGTERM to it orphans both ranks (no handler), and its pump threads spin
//! more than one core for the launcher's whole life (`launch.py:192-201`, stdin always writable). What it
//! did is small and reproduced exactly: per rank, `MLX_RANK` plus the backend's env
//! (`MLX_IBV_DEVICES` + `MLX_JACCL_COORDINATOR`, or `MLX_HOSTFILE`), then exec the program — on a
//! peer through `ssh -tt`, whose pty makes the remote rank die with its session (measured
//! 2026-09-24: killing the local ssh client took the remote pid within 1 s).
//!
//! The program is embedded here and passed base64 on the command line, so a node needs nothing
//! installed beyond its interpreter: `rank_env.py` (the backend env, `emit`, the memory reporter —
//! every rank), `rank_formation.py` (a JACCL group's formation handshake, Q-136), `rank_live.py` (rank 0's live request table, Rapid-MLX's `/v1/status` shape),
//! `rank_thinking.py` (a chat request's thinking switch, resolved as the single engine resolves it),
//! then the runner's program — `rank_budget.py` + `rank_prefill.py` + `rank_batch.py` +
//! `rank_state.py` + `rank_tool_stream.py` + `rank_wrapper.py` (`mlx_lm.server`, tensor split, under `NodeConfig::python`; the budget is
//! what an absent max_tokens generates, the prefill modules what a step and a batch may hold) or `pipeline_rank.py` (the fork's `pipeline_qwen4_serve`,
//! layer split, under `NodeConfig::pipeline_python`).

use std::collections::VecDeque;
use std::process::Stdio;
use std::sync::{Arc, Mutex as StdMutex};

use anyhow::{bail, Context, Result};
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::process::{Child, Command};

use super::config::{Backend, DistributedConfig, NodeConfig};
use super::exec::{sh_quote, SSH_OPTIONS};
use super::plan::{RankPlan, TensorPrefill};
use crate::model_identity::ServedNames;

/// The literal every goose rank carries on its command line, so `ps` can name a rank a previous
/// goosed left behind (and `stop` can reclaim it per-pid).
pub const RANK_MARKER: &str = "goose-distributed-rank";

/// The argument after the marker that names WHICH goose launched the rank: this install's owner
/// token (`RankSpec::owner`). The marker alone says "a goose rank" — a cargo test's stand-in rank
/// carries it too (Q-77: pid 9425 was `/usr/bin/python3` running the boot line, not the split's
/// py3.12 rank) — so only this token proves a leftover is this install's own, one preflight may
/// wait for or reclaim; any other rank stays a refusal.
pub const OWNER_ARG_PREFIX: &str = "goose-distributed-owner=";

const TENSOR_PROGRAM: &str = concat!(
    include_str!("rank_load_lock.py"),
    include_str!("rank_env.py"),
    include_str!("rank_formation.py"),
    include_str!("rank_live.py"),
    include_str!("rank_thinking.py"),
    include_str!("rank_budget.py"),
    include_str!("rank_prefill.py"),
    include_str!("rank_batch.py"),
    include_str!("rank_state.py"),
    include_str!("rank_tool_stream.py"),
    include_str!("rank_wrapper.py")
);
const PIPELINE_PROGRAM: &str = concat!(
    include_str!("rank_load_lock.py"),
    include_str!("rank_env.py"),
    include_str!("rank_formation.py"),
    include_str!("rank_live.py"),
    include_str!("rank_thinking.py"),
    include_str!("pipeline_rank.py")
);
const BOOT: &str = "import base64,sys;exec(base64.b64decode(sys.argv[1]))";
const TAIL_LINES: usize = 200;

/// What a rank runs, with the fields only that program reads (flattened into the spec JSON).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "program", rename_all = "camelCase")]
pub enum RankProgram {
    /// `mlx_lm.server` under `rank_wrapper.py` (tensor split). Its in-process memory and wired
    /// limits sit at the node's own GPU ceiling (`max_recommended_working_set_size`, read on the
    /// rank); MLX's free-buffer cache at the plan's transient allowance (`mlx_cache_limit_bytes`).
    /// The prompt cache's two bounds are the same on every rank (see [`TensorLaunch`]).
    ///
    /// Tagged `mlxLmServerDoorbell` since the doorbell (Q-66): an idle worker parks in recv(1)
    /// instead of spinning in JACCL's all_sum, which needs every rank's wrapper to take part.
    /// A Link peer runs its OWN goosed's wrapper, and an older one would never join the
    /// doorbell's port all_sum (a hang). The new tag makes that peer's goosed refuse the spec at
    /// rank start instead (serde: unknown variant; see `older_peer_refusal`). `mlxLmServer`
    /// (an older requester's spec) still reads, with `doorbell` false: upstream waiting.
    ///
    /// Tagged `mlxLmServerBounded` since Q-79: `prompt_cache_live_bound` changes what the prompt
    /// cache evicts, and every rank must evict alike, so a peer whose wrapper cannot bound cached +
    /// live at each insert must refuse the rank rather than join it. `mlxLmServerDoorbell` (a
    /// Q-66..Q-73 requester) still reads, with the bound off: that requester's policy.
    ///
    /// Tagged `mlxLmServerPrefill` since Q-104: `prefill` sizes every prefill step's chunk from
    /// the batch it runs (the chunk a step takes must agree across ranks, or the collectives no
    /// longer pair up) and holds a request rank 0 cannot fit beside the live batch, so a peer
    /// whose wrapper chunks at mlx_lm's fixed step must refuse the rank. `mlxLmServerBounded` (a
    /// Q-79 requester) still reads, with `prefill` absent: upstream chunking, no projection.
    ///
    /// Tagged `mlxLmServerFormation` since Q-136: a JACCL launch's spec carries
    /// [`RankSpec::formation`], and every rank must take part in the handshake (a peer that skips
    /// it leaves the others waiting in a round that never completes). `mlxLmServerPrefill` (a Q-104
    /// requester) still reads, with no formation: that requester's ranks form none either.
    #[serde(
        rename = "mlxLmServerFormation",
        alias = "mlxLmServerPrefill",
        alias = "mlxLmServerBounded",
        alias = "mlxLmServerDoorbell",
        alias = "mlxLmServer"
    )]
    MlxLmServer {
        context_window: u64,
        /// `--prompt-cache-bytes` and the prompt cache's own `max_bytes`
        /// (`RankPlan::prompt_cache_limit_bytes`). Absent only in an older requester's spec.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        prompt_cache_limit_bytes: Option<u64>,
        /// `--prompt-cache-size` (`RankPlan::prompt_cache_entries`), beside the limit.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        prompt_cache_entries: Option<u64>,
        /// An older requester's spec: the one number its own rank 0's wrapper hands mlx_lm as the
        /// flag alone (no count, no max_bytes). A rank reading it runs that same policy, so both
        /// ranks evict alike (each runs its own LRU cache over the same requests); goose itself
        /// never writes it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        prompt_cache_bytes: Option<u64>,
        planned_bytes: u64,
        #[serde(default)]
        doorbell: bool,
        /// `mx.set_cache_limit` on the rank (`RankPlan::mlx_cache_limit_bytes`). Absent in an
        /// older requester's spec: the rank then keeps that requester's rule (its ceiling less
        /// `planned_bytes`).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mlx_cache_limit_bytes: Option<u64>,
        /// The prompt cache holds cached + live KV inside `prompt_cache_limit_bytes` at every
        /// insert, not only at admission.
        #[serde(default)]
        prompt_cache_live_bound: bool,
        /// The plan's prefill figures (Q-104): `--prefill-step-size`, the workspace every step
        /// stays inside, and the per-token costs rank 0 projects a batch's KV with before it
        /// admits a request. Absent in an older requester's spec: upstream chunking, no
        /// projection.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        prefill: Option<TensorPrefill>,
    },
    /// The fork's `pipeline_qwen4_serve.serve` under `pipeline_rank.py` (layer split): the exact
    /// `pipeline_qwen4 serve` arguments, parsed on the rank by the fork's own parser.
    ///
    /// Tagged `pipelineServeFormation` since Q-136, for the same reason as the tensor program's
    /// tag; `pipelineServe` (an older requester's spec) still reads.
    #[serde(rename = "pipelineServeFormation", alias = "pipelineServe")]
    PipelineServe { serve_args: Vec<String> },
}

/// Everything one rank's program reads (base64 JSON, argv[2]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RankSpec {
    pub rank: usize,
    pub size: usize,
    pub backend: Backend,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ibv_devices: Option<Vec<Vec<Option<String>>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coordinator: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ring_hosts: Option<Vec<Vec<String>>>,
    pub served_id: String,
    /// Every other name of the served model (`model_identity::ServedNames::also`): rank 0 answers
    /// to each as to `served_id` (Q-131). Absent in an older requester's spec: the served id only.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub served_aliases: Vec<String>,
    pub model_dir: String,
    pub port: u16,
    pub memory_report_seconds: f64,
    /// The weights preflight planned on this rank (`RankPlan::weights_bytes`): with the rank's own
    /// MLX active bytes it is the load's progress — on this Mac and on a Link peer hosting it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub planned_weight_bytes: Option<u64>,
    /// The launching install's owner token, written on the rank's command line after the marker
    /// (`OWNER_ARG_PREFIX`). A Link peer launches the requester's spec as given, so a hosted rank
    /// carries the REQUESTER's token. `None` = a goose that set none (an older goose, a test)
    /// launched it: its leftovers are never provably this install's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    /// Where the rank takes the Mac's load lock (`rank_load_lock.py`). `None` — every production
    /// spec — is the machine's own lock under the rank's account home, which the rank resolves on
    /// the Mac it runs on (the requester cannot know a peer's home). Test builds point each launch
    /// at a lock of its own, so a stand-in rank never holds this Mac's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub load_lock: Option<String>,
    /// The group's formation handshake (`rank_formation.py`, Q-136): every JACCL launch carries
    /// one, fresh per launch (the supervisor sets it). `None` = a ring launch (TCP, nothing to
    /// absorb), or an older requester's spec.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub formation: Option<GroupFormation>,
    #[serde(flatten)]
    pub program: RankProgram,
}

/// Formation rounds: rank 0's first message (round 1), the workers' first (round 2), and the one
/// after it — a message that went nowhere is absorbed when the next one arrives.
pub const FORMATION_ROUNDS: u32 = 3; // measured: 33 lost first messages, every next one arrived

/// One launch's formation handshake (`rank_formation.py`): `rounds` lockstep all_gathers of
/// `[nonce, round, rank, check]`; a rank that reads a peer's LATER round sends to that peer
/// without receiving until the two are in step, and any part that is not this launch's ends the
/// rank in words.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupFormation {
    /// Fresh per launch, below 2^31 (it rides an int32): a message another launch left on the
    /// connection carries a different one.
    pub nonce: u32,
    pub rounds: u32,
}

impl GroupFormation {
    /// A formation no earlier launch of this process used, and — for the connection's previous
    /// group, launched by any goosed — distinct but by a 2^-31 chance.
    pub fn fresh() -> Self {
        use std::hash::{BuildHasher, Hasher};
        static LAUNCHES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
        hasher.write_u32(std::process::id());
        hasher.write_u64(LAUNCHES.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
        GroupFormation {
            // Never 0: a later collective's zero over a primer's buffer must not read as one.
            nonce: (hasher.finish() as u32) & 0x7fff_ffff | 1,
            rounds: FORMATION_ROUNDS,
        }
    }
}

/// What preflight's plan hands one tensor rank's launch.
///
/// The prompt cache bounds must be identical on every rank: each tensor rank runs its own mlx_lm
/// LRU prompt cache over the same requests, and a rank that evicts differently reuses a different
/// prefix — its prefill then runs a different number of steps than its peers' and the collectives
/// no longer pair up. [`TensorLaunch::for_ranks`] refuses plans whose bounds differ.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TensorLaunch {
    pub planned_bytes: u64,
    pub prompt_cache_limit_bytes: u64,
    pub prompt_cache_entries: u64,
    /// Per rank: it bounds only this process's free buffers, nothing a peer must mirror.
    pub mlx_cache_limit_bytes: u64,
    /// The same on every rank: the smallest workspace any rank's plan affords.
    pub prefill: TensorPrefill,
}

impl TensorLaunch {
    pub fn from_plan(plan: &RankPlan) -> Result<Self> {
        Ok(TensorLaunch {
            planned_bytes: plan.planned_bytes,
            prompt_cache_limit_bytes: plan.prompt_cache_limit_bytes(),
            prompt_cache_entries: plan.prompt_cache_entries,
            mlx_cache_limit_bytes: plan.mlx_cache_limit_bytes(),
            prefill: plan.prefill.context(
                "the plan carries no prefill figures (a plan made before Q-104): run preflight again",
            )?,
        })
    }

    /// Every rank's launch figures. The prefill figures become one set: the smallest workspace
    /// any rank's plan affords (inside every rank's plan), with the per-token costs every rank
    /// shares — plans of different models cannot be launched together.
    pub fn for_ranks(plans: &[&RankPlan]) -> Result<Vec<Self>> {
        let mut launches = plans
            .iter()
            .map(|plan| Self::from_plan(plan))
            .collect::<Result<Vec<Self>>>()?;
        if let Some(first) = launches.first().map(|l| l.prefill) {
            let costs = |p: &TensorPrefill| {
                (
                    p.step,
                    p.pair_bytes,
                    p.kv_bytes_per_token,
                    p.sequence_state_bytes,
                )
            };
            if let Some((rank, other)) = launches
                .iter()
                .enumerate()
                .find(|(_, launch)| costs(&launch.prefill) != costs(&first))
            {
                bail!(
                    "the ranks' prefill costs differ (rank 0: {:?}, rank {rank}: {:?}): every \
                     tensor rank must chunk and admit identically",
                    costs(&first),
                    costs(&other.prefill)
                );
            }
            let workspace = launches
                .iter()
                .map(|l| l.prefill.workspace_bytes)
                .min()
                .unwrap_or(first.workspace_bytes);
            for launch in &mut launches {
                launch.prefill.workspace_bytes = workspace;
            }
        }
        let bounds = |launch: &Self| (launch.prompt_cache_limit_bytes, launch.prompt_cache_entries);
        if let Some(first) = launches.first() {
            if let Some((rank, other)) = launches
                .iter()
                .enumerate()
                .find(|(_, launch)| bounds(launch) != bounds(first))
            {
                bail!(
                    "the ranks' prompt cache bounds differ (rank 0: {} bytes / {} entries, rank \
                     {rank}: {} bytes / {} entries): every tensor rank must evict identically",
                    first.prompt_cache_limit_bytes,
                    first.prompt_cache_entries,
                    other.prompt_cache_limit_bytes,
                    other.prompt_cache_entries
                );
            }
        }
        Ok(launches)
    }
}

/// The per-rank tensor specs. See [`base_specs`] for the backend env.
pub fn rank_specs(
    config: &DistributedConfig,
    served: &ServedNames,
    per_rank: &[TensorLaunch],
    context_window: u64,
    memory_report_seconds: f64,
) -> Vec<RankSpec> {
    base_specs(config, served, memory_report_seconds, |rank, _| {
        let launch = per_rank[rank];
        RankProgram::MlxLmServer {
            context_window,
            prompt_cache_limit_bytes: Some(launch.prompt_cache_limit_bytes),
            prompt_cache_entries: Some(launch.prompt_cache_entries),
            prompt_cache_bytes: None,
            planned_bytes: launch.planned_bytes,
            doorbell: true,
            mlx_cache_limit_bytes: Some(launch.mlx_cache_limit_bytes),
            prompt_cache_live_bound: true,
            prefill: Some(launch.prefill),
        }
    })
}

/// The `pipeline_qwen4 serve` arguments for one rank: this node's own model dir, the served id,
/// rank 0's loopback port, the context preflight allowed, the slots the plan was made for
/// (`--slots`: the fork re-plans at load for that many full-context sequences and admits requests
/// by that KV budget; `--max-batch` = the same count, the rows proven per batch), and the split
/// preflight approved (`--split` = ranks 1..N-1's starts), so the fork loads exactly that split
/// instead of re-balancing on its own load-time figures, the prefill chunk whose attention
/// scores preflight fitted beside the fork's plan (`--prefill-step`, `RankPlan::prefill_step`),
/// and this rank's scores at that chunk (`--attention-scores-bytes`,
/// `RankPlan::attention_scores_bytes`): the fork's buffer cache holds its measured budget less
/// its plan less these, so a prefill's scores and the cache never share the same room (Q-127).
/// `aliases` — every other name of the served model — go to rank 0 only, the one rank that serves
/// HTTP (`--served-model-alias`, fork lz-pipeline-qwen4.3, Q-131).
#[allow(clippy::too_many_arguments)]
pub fn pipeline_serve_args(
    config: &DistributedConfig,
    node: &NodeConfig,
    served_id: &str,
    aliases: &[String],
    context: u64,
    split: &str,
    prefill_step: u64,
    attention_scores_bytes: u64,
) -> Vec<String> {
    let mut args: Vec<String> = [
        "--model",
        &node.model_dir,
        "--served-model-name",
        served_id,
        "--host",
        "127.0.0.1",
        "--port",
        &config.port.to_string(),
        "--context",
        &context.to_string(),
        "--slots",
        &config.slots().to_string(),
        "--max-batch",
        &config.slots().to_string(),
        "--split",
        split,
        "--prefill-step",
        &prefill_step.to_string(),
        "--attention-scores-bytes",
        &attention_scores_bytes.to_string(),
    ]
    .iter()
    .map(|a| a.to_string())
    .collect();
    for alias in aliases {
        args.extend(["--served-model-alias".to_string(), alias.clone()]);
    }
    args
}

/// The per-rank pipeline specs for the plan preflight approved; `attention_scores_bytes[rank]`
/// is that rank's `RankPlan::attention_scores_bytes`.
pub fn pipeline_rank_specs(
    config: &DistributedConfig,
    served: &ServedNames,
    context: u64,
    split: &str,
    prefill_step: u64,
    attention_scores_bytes: &[u64],
    memory_report_seconds: f64,
) -> Vec<RankSpec> {
    base_specs(config, served, memory_report_seconds, |rank, node| {
        RankProgram::PipelineServe {
            serve_args: pipeline_serve_args(
                config,
                node,
                &served.id,
                if rank == 0 { &served.also } else { &[] },
                context,
                split,
                prefill_step,
                attention_scores_bytes[rank],
            ),
        }
    })
}

/// The per-rank backend env, exactly as `mlx.launch` would have described the hostfile: JACCL's
/// device matrix has `null` on the diagonal and host i's own RDMA device elsewhere; ring's host
/// list gives rank r the port `coordinator_port + r` on its TB IP.
fn base_specs(
    config: &DistributedConfig,
    served: &ServedNames,
    memory_report_seconds: f64,
    program: impl Fn(usize, &NodeConfig) -> RankProgram,
) -> Vec<RankSpec> {
    let size = config.size();
    let ibv_devices: Vec<Vec<Option<String>>> = config
        .nodes
        .iter()
        .enumerate()
        .map(|(i, node)| {
            (0..size)
                .map(|j| (i != j).then(|| node.rdma_device.clone()))
                .collect()
        })
        .collect();
    let ring_hosts: Vec<Vec<String>> = config
        .nodes
        .iter()
        .enumerate()
        .map(|(rank, node)| {
            vec![format!(
                "{}:{}",
                node.tb_ip,
                config.coordinator_port as usize + rank
            )]
        })
        .collect();
    #[cfg(not(test))]
    let launch_load_lock: Option<String> = None;
    #[cfg(test)]
    let launch_load_lock = Some(tests::launch_load_lock());
    let formation = (config.backend == Backend::Jaccl).then(GroupFormation::fresh);
    config
        .nodes
        .iter()
        .enumerate()
        .map(|(rank, node)| RankSpec {
            rank,
            size,
            backend: config.backend,
            ibv_devices: (config.backend == Backend::Jaccl).then(|| ibv_devices.clone()),
            coordinator: (config.backend == Backend::Jaccl)
                .then(|| format!("{}:{}", config.nodes[0].tb_ip, config.coordinator_port)),
            ring_hosts: (config.backend == Backend::Ring).then(|| ring_hosts.clone()),
            served_id: served.id.clone(),
            served_aliases: if rank == 0 {
                served.also.clone()
            } else {
                Vec::new()
            },
            model_dir: node.model_dir.clone(),
            port: config.port,
            memory_report_seconds,
            planned_weight_bytes: None,
            owner: None,
            load_lock: launch_load_lock.clone(),
            formation,
            program: program(rank, node),
        })
        .collect()
}

impl RankSpec {
    fn program_source(&self) -> &'static str {
        match self.program {
            RankProgram::MlxLmServer { .. } => TENSOR_PROGRAM,
            RankProgram::PipelineServe { .. } => PIPELINE_PROGRAM,
        }
    }

    /// The interpreter this rank's program needs on `node`: the tensor env, or the fork's.
    pub fn interpreter<'a>(&self, node: &'a NodeConfig) -> Result<&'a str> {
        match self.program {
            RankProgram::MlxLmServer { .. } => Ok(&node.python),
            RankProgram::PipelineServe { .. } => {
                node.pipeline_python.as_deref().with_context(|| {
                    format!(
                    "node '{}' has no pipeline_python: the qwen4_exp pipeline rank runs the fork's \
                     interpreter",
                    node.name
                )
                })
            }
        }
    }
}

/// The interpreter's arguments: the boot one-liner, the program, the spec, the marker, and the
/// owner token when the spec carries one (the rank programs read argv[1] and argv[2] only).
pub fn python_args(spec: &RankSpec) -> Result<Vec<String>> {
    let b64 = base64::engine::general_purpose::STANDARD;
    let mut args = vec![
        "-c".to_string(),
        BOOT.to_string(),
        b64.encode(spec.program_source()),
        b64.encode(serde_json::to_vec(spec)?),
        RANK_MARKER.to_string(),
    ];
    if let Some(owner) = &spec.owner {
        args.push(format!("{OWNER_ARG_PREFIX}{owner}"));
    }
    Ok(args)
}

/// The peer-side command: print the shell's pid (which `exec` hands to the interpreter unchanged)
/// so the supervisor can signal and verify THAT pid over ssh, then become the rank.
pub fn remote_script(python: &str, args: &[String]) -> String {
    let quoted: Vec<String> = args.iter().map(|a| sh_quote(a)).collect();
    format!(
        "echo GOOSE_RANK_PID=$$; exec {} {}",
        sh_quote(python),
        quoted.join(" ")
    )
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RankMemory {
    pub active: u64,
    pub peak: u64,
    pub cache: u64,
}

/// What a rank has told us so far, from its own output.
#[derive(Debug, Default)]
pub struct RankLive {
    /// The rank's own pid (this Mac: the child; a peer: `GOOSE_RANK_PID=`).
    pub pid: Option<u32>,
    pub lines: u64,
    pub tail: VecDeque<String>,
    pub group_joined: bool,
    pub caps: Option<serde_json::Value>,
    pub memory: Option<RankMemory>,
    /// The rank printed its `GOOSE_READY` (the pipeline server, after its warm-up batch).
    pub ready: bool,
    /// The rank's last `GOOSE_RANK_STATE`: where its generation loop was (rank_state.py).
    pub state: Option<serde_json::Value>,
    /// The last round of its group's formation the rank announced (`GOOSE_RANK_FORMING`,
    /// rank_formation.py): what a formation standstill is read from.
    pub forming: Option<u32>,
    /// The rank's `GOOSE_RANK_FORMATION`: its group formed, and how many messages each peer's
    /// connection swallowed on the way (rank_formation.py, Q-136).
    pub formation: Option<FormationReport>,
    /// Where the rank's whole output is kept on the Mac whose goosed reads it (rank_log.rs) —
    /// the file's path, or why there is none.
    pub log: Option<String>,
}

/// A rank's account of its group's formation (`GOOSE_RANK_FORMATION`, rank_formation.py).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct FormationReport {
    pub rank: usize,
    pub rounds: u32,
    /// Per peer (its number as text): that peer's messages to this rank that went nowhere.
    pub lost: std::collections::BTreeMap<String, u32>,
}

impl FormationReport {
    /// What the connections swallowed, in words, or `None` when every first primer arrived.
    pub fn losses(&self) -> Option<String> {
        let lost: Vec<String> = self
            .lost
            .iter()
            .filter(|(_, n)| **n > 0)
            .map(|(src, n)| {
                format!(
                    "rank {src} → rank {}: {n} message(s) went nowhere in {} rounds",
                    self.rank, self.rounds
                )
            })
            .collect();
        (!lost.is_empty()).then(|| lost.join("; "))
    }
}

/// Where a rank's start is, from its own reports. Both rank programs report `RANK_CAPS` once the
/// weights are in (the fork after `load_stage`, the tensor wrapper as its server starts), so
/// before it the rank is loading; the pipeline server then runs a warm-up batch and prints
/// `READY`. The tensor wrapper prints no READY of its own — rank 0's readiness completion is its
/// warm-up, judged by the coordinator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RankPhase {
    Loading,
    Warming,
    Ready,
}

impl RankPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            RankPhase::Loading => "loading",
            RankPhase::Warming => "warming",
            RankPhase::Ready => "ready",
        }
    }
}

impl RankLive {
    pub fn tail_text(&self) -> String {
        self.tail.iter().cloned().collect::<Vec<_>>().join("\n")
    }

    pub fn phase(&self) -> RankPhase {
        if self.ready {
            RankPhase::Ready
        } else if self.caps.is_some() {
            RankPhase::Warming
        } else {
            RankPhase::Loading
        }
    }

    fn take_line(&mut self, line: &str) {
        self.lines += 1;
        if let Some(pid) = line.strip_prefix("GOOSE_RANK_PID=") {
            if let Ok(pid) = pid.trim().parse() {
                self.pid = Some(pid);
            }
        } else if line.starts_with("GOOSE_RANK_GROUP ") {
            self.group_joined = true;
        } else if let Some(caps) = line.strip_prefix("GOOSE_RANK_CAPS ") {
            self.caps = serde_json::from_str(caps).ok();
        } else if line.starts_with("GOOSE_READY ") {
            self.ready = true;
        } else if let Some(memory) = line.strip_prefix("GOOSE_RANK_MEM ") {
            if let Ok(memory) = serde_json::from_str::<RankMemory>(memory) {
                self.memory = Some(memory);
            }
            return;
        } else if let Some(forming) = line.strip_prefix("GOOSE_RANK_FORMING ") {
            if let Ok(forming) = serde_json::from_str::<serde_json::Value>(forming) {
                if let Some(round) = forming["round"].as_u64() {
                    self.forming = u32::try_from(round).ok();
                    return;
                }
            }
        } else if let Some(report) = line.strip_prefix("GOOSE_RANK_FORMATION ") {
            self.formation = serde_json::from_str(report).ok();
        } else if let Some(state) = line.strip_prefix("GOOSE_RANK_STATE ") {
            if let Ok(state) = serde_json::from_str(state) {
                self.state = Some(state);
                return;
            }
        }
        if self.tail.len() == TAIL_LINES {
            self.tail.pop_front();
        }
        self.tail.push_back(line.to_string());
    }
}

pub struct RankProcess {
    pub rank: usize,
    pub node: String,
    /// `None` = this Mac.
    pub host: Option<String>,
    /// The rank itself on this Mac; on a peer, the local ssh client carrying it.
    pub child: Child,
    pub live: Arc<StdMutex<RankLive>>,
    /// The owner token the rank was launched with (`RankSpec::owner`): a stop that must sweep a
    /// node for this rank signals only ranks carrying it.
    pub owner: Option<String>,
}

impl RankProcess {
    pub fn pid(&self) -> Option<u32> {
        self.live.lock().unwrap().pid
    }

    pub fn tail(&self) -> String {
        self.live.lock().unwrap().tail_text()
    }
}

impl RankLive {
    /// Where this rank's whole output is, and its last account of its loop (rank_state.py), in
    /// one line — what a hang or a death names for the post-mortem.
    pub fn evidence(&self) -> String {
        let state = match &self.state {
            Some(state) => {
                let field = |key: &str| match state.get(key) {
                    Some(serde_json::Value::String(text)) => text.clone(),
                    Some(value) => value.to_string(),
                    None => "?".to_string(),
                };
                format!(
                    "steps {}, at {}, mode {}, rows {}, rings {}",
                    field("steps"),
                    field("at"),
                    field("mode"),
                    field("rows"),
                    field("rings")
                )
            }
            None => "no loop state reported".to_string(),
        };
        let log = self
            .log
            .as_deref()
            .unwrap_or("not reported (its goosed keeps no durable rank log)");
        format!("{state}; log {log}")
    }
}

/// The durable log both of a rank's streams append to, or `None` once it failed (the failure is
/// then in the rank's tail and `RankLive::log`).
type SharedLog = Arc<StdMutex<Option<super::rank_log::RankLog>>>;

/// Opens `rank`'s durable log on this Mac. A log that cannot be opened is said, in the tail and in
/// `RankLive::log` — never a silent absence.
fn open_rank_log(rank: usize, node: &str, live: &StdMutex<RankLive>) -> SharedLog {
    let opened = super::rank_log::rank_log_dir()
        .and_then(|dir| super::rank_log::RankLog::open(&dir, rank, node));
    let mut live = live.lock().unwrap();
    match opened {
        Ok(log) => {
            live.log = Some(log.path().display().to_string());
            Arc::new(StdMutex::new(Some(log)))
        }
        Err(e) => {
            let why = format!("unavailable: {e:#}");
            tracing::warn!(rank, node, "distributed engine: rank log {why}");
            live.take_line(&format!("goose: this rank's durable log is {why}"));
            live.log = Some(why);
            Arc::new(StdMutex::new(None))
        }
    }
}

fn record(live: &StdMutex<RankLive>, log: &SharedLog, stream: &str, line: &str) {
    let failed = {
        let mut log = log.lock().unwrap();
        let failed = log.as_mut().and_then(|l| {
            l.append(stream, line)
                .err()
                .map(|e| (l.path().to_owned(), e))
        });
        if failed.is_some() {
            *log = None;
        }
        failed
    };
    let mut live = live.lock().unwrap();
    if let Some((path, e)) = failed {
        let why = format!(
            "stopped: {e:#} (the output up to here is in {})",
            path.display()
        );
        tracing::warn!("distributed engine: rank log {why}");
        live.take_line(&format!("goose: this rank's durable log {why}"));
        live.log = Some(why);
    }
    live.take_line(line);
}

/// Drains one of a rank's streams to EOF: every line into the durable log, then the live view.
/// Bytes that are not UTF-8 are read lossily rather than ending the drain — a reader that stopped
/// would leave the rank blocked on a full pipe at 0% CPU, the very state a hang shows.
fn read_lines<R: AsyncRead + Unpin + Send + 'static>(
    reader: R,
    stream: &'static str,
    live: Arc<StdMutex<RankLive>>,
    log: SharedLog,
) {
    tokio::spawn(async move {
        let mut reader = BufReader::new(reader);
        let mut buf = Vec::new();
        loop {
            buf.clear();
            match reader.read_until(b'\n', &mut buf).await {
                Ok(0) => break,
                Ok(_) => {
                    let text = String::from_utf8_lossy(&buf);
                    let line = text.trim_end_matches('\n').trim_end_matches('\r');
                    record(&live, &log, stream, line);
                }
                Err(e) => {
                    record(
                        &live,
                        &log,
                        stream,
                        &format!("goose: reading the rank's std{stream} failed: {e}"),
                    );
                    break;
                }
            }
        }
    });
}

/// Attaches the drains, and the durable log, to a spawned rank's two streams.
fn drain_rank_output(child: &mut Child, rank: usize, node: &str, live: &Arc<StdMutex<RankLive>>) {
    let log = open_rank_log(rank, node, live);
    if let Some(stdout) = child.stdout.take() {
        read_lines(stdout, "out", Arc::clone(live), Arc::clone(&log));
    }
    if let Some(stderr) = child.stderr.take() {
        read_lines(stderr, "err", Arc::clone(live), log);
    }
}

pub fn spawn_rank(node: &NodeConfig, spec: &RankSpec) -> Result<RankProcess> {
    let python = spec.interpreter(node)?;
    let args = python_args(spec)?;
    let mut cmd = match node.host() {
        None => {
            let mut cmd = Command::new(python);
            cmd.args(&args)
                .env("PATH", crate::engine::sidecar_spawn_path())
                .env("DO_NOT_TRACK", "1");
            cmd
        }
        Some(alias) => {
            let mut cmd = Command::new("/usr/bin/ssh");
            cmd.arg("-tt")
                .args(SSH_OPTIONS)
                .args(["-o", "LogLevel=QUIET"])
                .arg(alias)
                .arg(remote_script(python, &args));
            cmd
        }
    };
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    crate::subprocess::configure_subprocess(&mut cmd);
    let mut child = cmd.spawn().with_context(|| {
        format!(
            "spawning rank {} on {} ({})",
            spec.rank,
            node.name,
            node.host().unwrap_or("this Mac")
        )
    })?;
    let live = Arc::new(StdMutex::new(RankLive {
        pid: if node.is_local() { child.id() } else { None },
        ..Default::default()
    }));
    drain_rank_output(&mut child, spec.rank, &node.name, &live);
    Ok(RankProcess {
        rank: spec.rank,
        node: node.name.clone(),
        host: node.ssh.clone(),
        child,
        live,
        owner: spec.owner.clone(),
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::distributed::config::tests::two_mac_config;
    use crate::distributed::provision::EnvSpec;

    const QWEN38: &str = include_str!("../../tests/fixtures/chat_templates/qwen3.8.jinja");

    /// One launch's own load lock: a test's stand-in ranks never take this Mac's, nor each other's.
    pub(crate) fn launch_load_lock() -> String {
        static LAUNCHES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let launch = LAUNCHES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        std::env::temp_dir()
            .join(format!(
                "goose-sidecar-test-load-locks/{}-{launch}/mlx-load.lock",
                std::process::id()
            ))
            .display()
            .to_string()
    }

    /// The 27B's prefill figures on 2 ranks at E2E #2's 262,144-token plan (plan.rs's fixture):
    /// one row's 2,048-token chunk at the full context.
    pub(crate) fn e2e_prefill() -> TensorPrefill {
        TensorPrefill {
            step: 2_048,
            workspace_bytes: 2_048 * 262_144 * 25,
            pair_bytes: 25,
            kv_bytes_per_token: 32_768,
            sequence_state_bytes: 76_972_032,
            batch_transient_ratio: crate::distributed::BATCH_KV_TRANSIENT_RATIO,
        }
    }

    fn launch(planned_bytes: u64, prompt_cache_limit_bytes: u64) -> TensorLaunch {
        TensorLaunch {
            planned_bytes,
            prompt_cache_limit_bytes,
            prompt_cache_entries: 3,
            mlx_cache_limit_bytes: planned_bytes / 10,
            prefill: e2e_prefill(),
        }
    }

    #[test]
    fn jaccl_specs_reproduce_the_proven_hostfile() {
        let config = two_mac_config();
        let specs = rank_specs(
            &config,
            &ServedNames::only("node-alias"),
            &[launch(20, 2), launch(21, 2)],
            65_536,
            2.0,
        );
        // hostfile-jaccl.json: rdma [null,"rdma_en3"] / ["rdma_en3",null], coordinator hosts[0].ips[0].
        let devices = specs[0].ibv_devices.as_ref().unwrap();
        assert_eq!(devices[0], vec![None, Some("rdma_en3".to_string())]);
        assert_eq!(devices[1], vec![Some("rdma_en3".to_string()), None]);
        assert_eq!(specs[1].coordinator.as_deref(), Some("192.168.0.1:32323"));
        assert_eq!(specs[1].model_dir, config.nodes[1].model_dir);
        assert!(matches!(
            specs[1].program,
            RankProgram::MlxLmServer {
                planned_bytes: 21,
                prompt_cache_limit_bytes: Some(2),
                prompt_cache_entries: Some(3),
                prompt_cache_bytes: None,
                context_window: 65_536,
                doorbell: true,
                mlx_cache_limit_bytes: Some(2),
                prompt_cache_live_bound: true,
                prefill: Some(_),
            }
        ));
        assert!(specs[0].ring_hosts.is_none());
        assert!(
            specs.iter().all(|s| s.served_id == "node-alias"),
            "every rank serves the id it was given, never the HF id {}",
            config.model_id
        );
    }

    #[test]
    fn ring_specs_give_each_rank_its_own_port() {
        let mut config = two_mac_config();
        config.backend = Backend::Ring;
        let specs = rank_specs(
            &config,
            &ServedNames::only("node-alias"),
            &[launch(1, 1), launch(1, 1)],
            8_192,
            2.0,
        );
        assert_eq!(
            specs[0].ring_hosts.as_ref().unwrap(),
            &vec![
                vec!["192.168.0.1:32323".to_string()],
                vec!["192.168.0.2:32324".to_string()]
            ]
        );
        assert!(specs[0].ibv_devices.is_none() && specs[0].coordinator.is_none());
    }

    #[test]
    fn the_remote_command_prints_the_pid_then_execs_the_marked_rank() {
        let config = two_mac_config();
        let spec = &rank_specs(
            &config,
            &ServedNames::only("node-alias"),
            &[launch(1, 1), launch(1, 1)],
            8_192,
            2.0,
        )[1];
        let args = python_args(spec).unwrap();
        let script = remote_script(&config.nodes[1].python, &args);
        assert!(script
            .starts_with("echo GOOSE_RANK_PID=$$; exec '/tmp/jaccl-smoke/.venv/bin/python' '-c' "));
        assert!(script.ends_with(&format!("'{RANK_MARKER}'")));
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(&args[3])
            .unwrap();
        let back: RankSpec = serde_json::from_slice(&decoded).unwrap();
        assert_eq!(&back, spec);
    }

    /// The owner token rides after the marker — on this Mac's rank and inside the ssh carrier's
    /// remote script alike — and `ps` reads it back from both; a spec without one (an older
    /// requester's) still reads, and writes no token.
    #[test]
    fn the_owner_token_follows_the_marker_and_ps_reads_it_back() {
        let config = two_mac_config();
        let mut spec = rank_specs(
            &config,
            &ServedNames::only("node-alias"),
            &[launch(1, 1), launch(1, 1)],
            8_192,
            2.0,
        )
        .remove(1);
        let bare = python_args(&spec).unwrap();
        assert_eq!(bare.last().map(String::as_str), Some(RANK_MARKER));

        spec.owner = Some("0123abcd".to_string());
        let args = python_args(&spec).unwrap();
        assert_eq!(args[args.len() - 2], RANK_MARKER);
        assert_eq!(args[args.len() - 1], format!("{OWNER_ARG_PREFIX}0123abcd"));
        let local = format!("11 {} {}", config.nodes[1].python, args.join(" "));
        let carrier = format!(
            "12 /usr/bin/ssh -tt workhorse {}",
            remote_script(&config.nodes[1].python, &args)
        );
        let ranks = crate::distributed::probe::goose_rank_processes(
            &format!("{local}\n{carrier}\n"),
            None,
            &[],
        );
        let read: Vec<(u32, Option<&str>, bool)> = ranks
            .iter()
            .map(|r| (r.pid, r.owner.as_deref(), r.carrier))
            .collect();
        assert_eq!(
            read,
            vec![(11, Some("0123abcd"), false), (12, Some("0123abcd"), true)]
        );

        let mut older = serde_json::to_value(&spec).unwrap();
        older.as_object_mut().unwrap().remove("owner");
        let back: RankSpec = serde_json::from_value(older).unwrap();
        assert_eq!(back.owner, None);
    }

    #[tokio::test]
    async fn the_boot_line_runs_the_wrapper_it_is_given() {
        // The boot one-liner, fed a stand-in wrapper, sees the spec as argv[2] and the marker as
        // argv[3] — the positions rank_wrapper.py reads.
        let b64 = base64::engine::general_purpose::STANDARD;
        let stand_in = "import sys,base64,json;print(json.loads(base64.b64decode(sys.argv[2]))['rank'], sys.argv[3])";
        let out = tokio::process::Command::new("/usr/bin/python3")
            .args([
                "-c",
                BOOT,
                &b64.encode(stand_in),
                &b64.encode(r#"{"rank":1}"#),
                RANK_MARKER,
            ])
            .output()
            .await
            .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&out.stdout).trim(),
            format!("1 {RANK_MARKER}")
        );
    }

    fn pipeline_config() -> DistributedConfig {
        let mut config = two_mac_config();
        config.nodes[0].pipeline_python =
            Some("/Users/me/.goose/distributed/fork/bin/python".into());
        config.nodes[1].pipeline_python =
            Some("/Users/workhorse/.goose/distributed/fork/bin/python".into());
        config
    }

    /// Q-131: rank 0 — the one rank serving HTTP — is told every other name of the model; the
    /// others are handed nothing new, so a peer running an older program reads the spec it knows.
    #[test]
    fn only_rank_zero_is_told_the_models_other_names() {
        let served = ServedNames {
            id: "node-alias".to_string(),
            also: vec!["Org/Model-HF".to_string()],
        };
        let tensor = rank_specs(
            &two_mac_config(),
            &served,
            &[launch(20, 2), launch(21, 2)],
            65_536,
            2.0,
        );
        assert_eq!(tensor[0].served_aliases, ["Org/Model-HF"]);
        assert!(tensor[1].served_aliases.is_empty());
        let peer = serde_json::to_value(&tensor[1]).unwrap();
        assert!(peer.get("served_aliases").is_none(), "{peer}");
        let pipeline = pipeline_rank_specs(
            &pipeline_config(),
            &served,
            8_192,
            "19",
            2_048,
            &[0, 0],
            2.0,
        );
        let aliases = |spec: &RankSpec| match &spec.program {
            RankProgram::PipelineServe { serve_args } => serve_args
                .windows(2)
                .filter(|w| w[0] == "--served-model-alias")
                .map(|w| w[1].clone())
                .collect::<Vec<_>>(),
            other => panic!("{other:?}"),
        };
        assert_eq!(aliases(&pipeline[0]), ["Org/Model-HF"]);
        assert!(aliases(&pipeline[1]).is_empty());
        assert!(pipeline.iter().all(|s| s.served_id == "node-alias"));
    }

    #[test]
    fn a_pipeline_rank_serves_the_approved_split_under_the_forks_interpreter() {
        let config = pipeline_config();
        let plan = crate::distributed::plan::parse_pipeline_plan(
            crate::distributed::plan::tests::FLASH_PLAN_32K,
        )
        .unwrap();
        let scores = [805_306_368u64, 0];
        let specs = pipeline_rank_specs(
            &config,
            &ServedNames::only("node-alias"),
            32_768,
            &plan.split_arg(),
            2_048,
            &scores,
            2.0,
        );
        for (rank, spec) in specs.iter().enumerate() {
            let RankProgram::PipelineServe { serve_args } = &spec.program else {
                panic!("{spec:?}");
            };
            assert_eq!(
                serve_args,
                &[
                    "--model",
                    config.nodes[rank].model_dir.as_str(),
                    "--served-model-name",
                    "node-alias",
                    "--host",
                    "127.0.0.1",
                    "--port",
                    "8190",
                    "--context",
                    "32768",
                    "--slots",
                    "2",
                    "--max-batch",
                    "2",
                    "--split",
                    "19",
                    "--prefill-step",
                    "2048",
                    "--attention-scores-bytes",
                    scores[rank].to_string().as_str(),
                ]
            );
            assert_eq!(
                spec.interpreter(&config.nodes[rank]).unwrap(),
                config.nodes[rank].pipeline_python.as_deref().unwrap()
            );
        }
        assert_eq!(specs[1].coordinator.as_deref(), Some("192.168.0.1:32323"));
        let args = python_args(&specs[1]).unwrap();
        let b64 = base64::engine::general_purpose::STANDARD;
        let program = String::from_utf8(b64.decode(&args[2]).unwrap()).unwrap();
        assert_eq!(
            program,
            concat!(
                include_str!("rank_load_lock.py"),
                include_str!("rank_env.py"),
                include_str!("rank_formation.py"),
                include_str!("rank_live.py"),
                include_str!("rank_thinking.py"),
                include_str!("pipeline_rank.py")
            )
        );
        let script = remote_script(specs[1].interpreter(&config.nodes[1]).unwrap(), &args);
        assert!(script.starts_with(
            "echo GOOSE_RANK_PID=$$; exec '/Users/workhorse/.goose/distributed/fork/bin/python' '-c' "
        ));

        let mut four = config.clone();
        four.slots = Some(4);
        let RankProgram::PipelineServe { serve_args } = &pipeline_rank_specs(
            &four,
            &ServedNames::only("node-alias"),
            8_192,
            "19",
            2_048,
            &[0, 0],
            2.0,
        )[0]
        .program
        else {
            unreachable!()
        };
        let joined = serve_args.join(" ");
        assert!(
            joined.contains("--slots 4 --max-batch 4 --split 19"),
            "{joined}"
        );

        let mut tensor_only = config.clone();
        tensor_only.nodes[1].pipeline_python = None;
        let err = specs[1]
            .interpreter(&tensor_only.nodes[1])
            .unwrap_err()
            .to_string();
        assert!(err.contains("pipeline_python"), "{err}");
        let tensor = &rank_specs(
            &config,
            &ServedNames::only("node-alias"),
            &[launch(1, 1), launch(1, 1)],
            8_192,
            2.0,
        )[1];
        assert_eq!(
            tensor.interpreter(&config.nodes[1]).unwrap(),
            config.nodes[1].python
        );
    }

    /// The REAL pipeline program (prelude + pipeline_rank.py), booted exactly as a rank is, against
    /// stand-in `mlx.core` and `pipeline_qwen4_serve` modules: it sets the JACCL env, parses goose's
    /// argv with the fork's parser, starts the memory reporter, hands serve() goose's emit, and
    /// exits with serve()'s code — without ever calling mx.distributed.init itself when the spec
    /// carries no formation (an older requester's; the formation path is the next test). macOS only,
    /// like every test that boots a real rank program: it takes the load lock through libproc
    /// (`rank_load_lock.py`), which Linux does not have.
    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn the_pipeline_program_runs_serve_with_the_parsed_args_and_exits_with_its_code() {
        let root = tempfile::tempdir().unwrap();
        let site = root.path();
        std::fs::create_dir_all(site.join("mlx")).unwrap();
        std::fs::create_dir_all(site.join("rapid_mlx/distributed")).unwrap();
        std::fs::write(site.join("mlx/__init__.py"), "").unwrap();
        std::fs::write(
            site.join("mlx/core.py"),
            "class _D:\n    @staticmethod\n    def init(*a, **k):\n        raise SystemExit('the wrapper called mx.distributed.init')\n\
             distributed = _D()\n\
             def get_active_memory(): return 11\n\
             def get_peak_memory(): return 22\n\
             def get_cache_memory(): return 3\n",
        )
        .unwrap();
        std::fs::write(site.join("rapid_mlx/__init__.py"), "").unwrap();
        std::fs::write(site.join("rapid_mlx/distributed/__init__.py"), "").unwrap();
        std::fs::write(
            site.join("rapid_mlx/distributed/pipeline_qwen4_serve.py"),
            "import os, time\n\
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
             \x20   parser.add_argument('--model', required=True)\n\
             \x20   parser.add_argument('--served-model-name', required=True)\n\
             \x20   parser.add_argument('--host', default='127.0.0.1')\n\
             \x20   parser.add_argument('--port', type=int, required=True)\n\
             \x20   parser.add_argument('--context', type=int, required=True)\n\
             \x20   parser.add_argument('--slots', type=int, default=2)\n\
             \x20   parser.add_argument('--max-batch', type=int, default=2)\n\
             \x20   parser.add_argument('--prefill-step', type=int)\n\
             \x20   parser.add_argument('--attention-scores-bytes', type=int, default=0)\n\
             \x20   parser.add_argument('--split')\n\
             def serve(options, emit=None):\n\
             \x20   time.sleep(0.3)\n\
             \x20   emit('READY', dict(vars(options), rank=int(os.environ['MLX_RANK']), \
             coordinator=os.environ.get('MLX_JACCL_COORDINATOR'), devices=open(os.environ['MLX_IBV_DEVICES']).read(), \
             hostfile=os.environ.get('MLX_HOSTFILE'), offline=os.environ.get('HF_HUB_OFFLINE')))\n\
             \x20   return 7\n",
        )
        .unwrap();
        let mut config = pipeline_config();
        config.slots = Some(3);
        config.nodes[1].pipeline_python = Some("/usr/bin/python3".into());
        let mut spec = pipeline_rank_specs(
            &config,
            &ServedNames::only("node-alias"),
            32_768,
            "19",
            2_048,
            &[0, 805_306_368],
            0.05,
        )
        .remove(1);
        spec.memory_report_seconds = 0.05;
        spec.formation = None;
        let out = tokio::process::Command::new(spec.interpreter(&config.nodes[1]).unwrap())
            .args(python_args(&spec).unwrap())
            .env("PYTHONPATH", site)
            .output()
            .await
            .unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert_eq!(
            out.status.code(),
            Some(7),
            "{stdout}{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let mut live = RankLive::default();
        stdout.lines().for_each(|l| live.take_line(l));
        assert_eq!(
            live.memory,
            Some(RankMemory {
                active: 11,
                peak: 22,
                cache: 3
            }),
            "the reporter ran while serve() did: {stdout}"
        );
        let ready: serde_json::Value = serde_json::from_str(
            stdout
                .lines()
                .find_map(|l| l.strip_prefix("GOOSE_READY "))
                .unwrap_or_else(|| panic!("{stdout}")),
        )
        .unwrap();
        assert_eq!(ready["model"], config.nodes[1].model_dir.as_str());
        assert_eq!(ready["served_model_name"], "node-alias");
        assert_eq!(ready["port"], 8190);
        assert_eq!(ready["context"], 32_768);
        assert_eq!(
            ready["slots"], 3,
            "the configured slots, not the fork's default"
        );
        assert_eq!(ready["max_batch"], 3);
        assert_eq!(ready["split"], "19");
        assert_eq!(
            ready["prefill_step"], 2_048,
            "the chunk preflight fitted the attention scores to, not the fork's default"
        );
        assert_eq!(
            ready["attention_scores_bytes"], 805_306_368,
            "this rank's own scores, which the fork's buffer cache leaves room for"
        );
        assert_eq!(ready["rank"], 1);
        assert_eq!(ready["coordinator"], "192.168.0.1:32323");
        assert_eq!(
            ready["devices"],
            r#"[[null, "rdma_en3"], ["rdma_en3", null]]"#
        );
        assert_eq!(
            ready["hostfile"],
            serde_json::Value::Null,
            "one backend's env only"
        );
        assert_eq!(ready["offline"], "1");
    }

    /// rank_formation.py under a real interpreter, over a stand-in transport that behaves as the
    /// Thunderbolt connection did after an abnormal end (Q-136, measured 2026-09-26 with a verbs
    /// logger on both ranks): the first message(s) one way go nowhere while the sender sees them
    /// complete, and everything after arrives in order. The negative control is the Q-136 shift
    /// itself: with no handshake, rank 0's first receive reads rank 1's SECOND message.
    #[test]
    fn the_formation_handshake_absorbs_what_a_connection_swallowed() {
        let prelude = r#"
import json, queue, threading, types

local = threading.local()
DROP, QUEUES, EMITTED, SENT = {}, {}, {}, {}

class Arr:
    def __init__(self, values): self.values = list(values)
    def tolist(self): return list(self.values)

def send(x, dst, stream=None):
    # The key's n-th message goes nowhere when n is in DROP[key]; its sender never knows.
    key = (local.rank, dst)
    SENT[key] = SENT.get(key, 0) + 1
    if SENT[key] not in DROP.get(key, ()):
        QUEUES[key].put(list(x.values))
    return x

def recv(shape, dtype, src, stream=None):
    # The stand-in's own guard against a test that would otherwise hang; nothing in goose.
    return Arr(QUEUES[(src, local.rank)].get(timeout=3))

def all_gather(x, stream=None):
    # jaccl's mesh all_gather: one message to every peer, one read from every peer.
    size = GROUP_SIZE[0]
    for peer in range(size):
        if peer != local.rank:
            send(x, peer)
    parts = []
    for peer in range(size):
        parts += x.values if peer == local.rank else recv(None, None, peer).values
    return Arr(parts)

GROUP_SIZE = [2]
mx = types.SimpleNamespace(
    int32="int32", cpu="cpu", eval=lambda *a: None,
    array=lambda values, dtype=None: Arr(values),
    distributed=types.SimpleNamespace(send=send, recv=recv, all_gather=all_gather),
)

def emit(tag, payload):
    if tag == "RANK_FORMATION":
        EMITTED[local.rank] = payload

class Group:
    def __init__(self, rank, size): self.r, self.s = rank, size
    def rank(self): return self.r
    def size(self): return self.s

def run(size, drops, stale=(), formation=True):
    DROP.clear(); EMITTED.clear(); QUEUES.clear(); SENT.clear()
    GROUP_SIZE[0] = size
    for src in range(size):
        for dst in range(size):
            QUEUES[(src, dst)] = queue.Queue()
    DROP.update(drops)
    for key, message in stale:
        QUEUES[key].put(message)
    got, errors = {}, {}
    def body(rank):
        local.rank = rank
        try:
            if formation:
                form_group(Group(rank, size), {"nonce": 424243, "rounds": 3})
            # The program's first collective (the wrapper's doorbell port all_sum): one tagged
            # message to every peer, one read back from each.
            for peer in range(size):
                if peer != rank:
                    send(Arr([777, rank, peer, 0]), peer)
            got[rank] = {p: recv(None, None, p).tolist() for p in range(size) if p != rank}
        except BaseException as e:
            errors[rank] = f"{type(e).__name__}: {e}"
    threads = [threading.Thread(target=body, args=(r,)) for r in range(size)]
    for t in threads: t.start()
    for t in threads: t.join()
    return got, errors

def pairs(got, size):
    return all(got[r][p] == [777, p, r, 0] for r in range(size) for p in range(size) if p != r)
"#;
        let checks = r#"
# Nothing lost: nothing absorbed, the first collective pairs.
got, errors = run(2, {})
assert not errors and pairs(got, 2), (got, errors)
assert EMITTED == {0: {"rank": 0, "rounds": 3, "lost": {"1": 0}},
                   1: {"rank": 1, "rounds": 3, "lost": {"0": 0}}}, EMITTED

# The Q-136 case: rank 1's first message to rank 0 went nowhere; its next fills rank 0's read,
# and rank 0 sends its last round without reading until the two are in step.
got, errors = run(2, {(1, 0): {1}})
assert not errors and pairs(got, 2), (got, errors)
assert EMITTED[0]["lost"] == {"1": 1} and EMITTED[1]["lost"] == {"0": 0}, EMITTED

# Rank 0's first message went nowhere (2 of the 33 measured).
got, errors = run(2, {(0, 1): {1}})
assert not errors and pairs(got, 2), (got, errors)
assert EMITTED[1]["lost"] == {"0": 1} and EMITTED[0]["lost"] == {"1": 0}, EMITTED

# Both first messages went nowhere: rank 0 leads, so the two never wait on each other.
got, errors = run(2, {(0, 1): {1}, (1, 0): {1}})
assert not errors and pairs(got, 2), (got, errors)
assert EMITTED[0]["lost"] == {"1": 1} and EMITTED[1]["lost"] == {"0": 1}, EMITTED

# Negative control: the same loss with no formation shifts every later pairing by one — rank 0
# never reads rank 1's first collective message (here the stand-in's guard ends its wait).
got, errors = run(2, {(1, 0): {1}}, formation=False)
assert got[1][0] == [777, 0, 1, 0] and "Empty" in errors.get(0, ""), (got, errors)

# A message another launch left on the connection is refused in words, never read as data.
got, errors = run(2, {}, stale=[((0, 1), [999, 1, 0, 1])])
assert "not this launch's part" in errors.get(1, ""), errors

# Three ranks: formed in step; a loss cannot be skipped around, so it ends the rank in words.
got, errors = run(3, {})
assert not errors and pairs(got, 3), (got, errors)
got, errors = run(3, {(2, 0): {1}})
assert "cannot skip one peer's receive" in errors.get(0, ""), errors

# The residuals, stated (neither ever measured): two losses in a row one way, or both ways losing
# the same crossing round, leave the two waiting for each other — goosed's formation standstill
# names it, and the restart forms a fresh group.
for drops in ({(1, 0): {1, 2}}, {(0, 1): {2}, (1, 0): {1}}):
    got, errors = run(2, drops)
    assert "Empty" in errors.get(0, "") and "Empty" in errors.get(1, ""), (drops, errors)
print("ok")
"#;
        let out = std::process::Command::new("/usr/bin/python3")
            .arg("-c")
            .arg(format!(
                "{prelude}{}{checks}",
                include_str!("rank_formation.py")
            ))
            .output()
            .expect("/usr/bin/python3 runs the rank programs' pure half");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "ok");
    }

    /// The REAL pipeline program with a JACCL launch's formation: it initialises the group on the
    /// spec's backend and runs the handshake BEFORE serve() — whose own `init(strict=True)` then
    /// finds MLX's cached group — so the fork's first collective never meets what a connection
    /// swallowed. The stand-in group is a group of one (no peer here); the handshake's pairing is
    /// `the_formation_handshake_absorbs_what_a_connection_swallowed`.
    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn the_pipeline_program_forms_its_group_before_serve() {
        let root = tempfile::tempdir().unwrap();
        let site = root.path();
        std::fs::create_dir_all(site.join("mlx")).unwrap();
        std::fs::create_dir_all(site.join("rapid_mlx/distributed")).unwrap();
        std::fs::write(site.join("mlx/__init__.py"), "").unwrap();
        std::fs::write(
            site.join("mlx/core.py"),
            "class _G:\n\
             \x20   def rank(self): return 0\n\
             \x20   def size(self): return 1\n\
             class _D:\n\
             \x20   @staticmethod\n\
             \x20   def init(strict=False, backend='any'):\n\
             \x20       print('STANDIN_INIT ' + backend, flush=True)\n\
             \x20       return _G()\n\
             distributed = _D()\n\
             def get_active_memory(): return 11\n\
             def get_peak_memory(): return 22\n\
             def get_cache_memory(): return 3\n",
        )
        .unwrap();
        std::fs::write(site.join("rapid_mlx/__init__.py"), "").unwrap();
        std::fs::write(site.join("rapid_mlx/distributed/__init__.py"), "").unwrap();
        std::fs::write(
            site.join("rapid_mlx/distributed/pipeline_qwen4_serve.py"),
            "class _Job:\n\
             \x20   def __init__(self, row): self.row = row\n\
             class _Engine:\n\
             \x20   def _start(self, row): pass\n\
             \x20   def prefill(self, words): pass\n\
             def prefill_chunks(start, end, step, split=0): return []\n\
             def _build_app(state, tokenizer, eos_ids, vision=None): pass\n\
             def add_arguments(parser):\n\
             \x20   for flag in ('--model', '--served-model-name', '--host', '--port', '--context', \
             '--slots', '--max-batch', '--prefill-step', '--attention-scores-bytes', '--split'):\n\
             \x20       parser.add_argument(flag)\n\
             def serve(options, emit=None):\n\
             \x20   emit('READY', {'rank': 0})\n\
             \x20   return 7\n",
        )
        .unwrap();
        let mut config = pipeline_config();
        config.nodes[0].pipeline_python = Some("/usr/bin/python3".into());
        let mut spec = pipeline_rank_specs(
            &config,
            &ServedNames::only("node-alias"),
            32_768,
            "19",
            2_048,
            &[0, 805_306_368],
            0.05,
        )
        .remove(0);
        spec.memory_report_seconds = 0.05;
        let formation = spec
            .formation
            .expect("a JACCL pipeline launch forms its group");
        let out = tokio::process::Command::new(spec.interpreter(&config.nodes[0]).unwrap())
            .args(python_args(&spec).unwrap())
            .env("PYTHONPATH", site)
            .output()
            .await
            .unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert_eq!(
            out.status.code(),
            Some(7),
            "{stdout}{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let order: Vec<&str> = stdout
            .lines()
            .filter(|l| {
                l.starts_with("STANDIN_INIT")
                    || l.starts_with("GOOSE_RANK_FORMATION")
                    || l.starts_with("GOOSE_READY")
            })
            .collect();
        assert_eq!(
            order,
            [
                "STANDIN_INIT jaccl",
                &format!(
                    r#"GOOSE_RANK_FORMATION {{"rank": 0, "rounds": {}, "lost": {{}}}}"#,
                    formation.rounds
                ),
                r#"GOOSE_READY {"rank": 0}"#,
            ],
            "{stdout}"
        );
    }

    /// A JACCL launch carries a fresh formation (the nonce differs launch to launch, never 0); a
    /// ring launch carries none. The rank's report reads
    /// back as the losses a start names.
    #[test]
    fn every_jaccl_launch_forms_its_group_afresh_and_its_losses_are_named() {
        let config = two_mac_config();
        let launch_once = || {
            rank_specs(
                &config,
                &ServedNames::only("node-alias"),
                &[launch(1, 2), launch(3, 2)],
                8_192,
                2.0,
            )
        };
        let first = launch_once();
        let second = launch_once();
        let formation = first[0].formation.expect("a JACCL launch forms its group");
        assert_eq!(
            first[1].formation,
            Some(formation),
            "one formation per launch"
        );
        assert_eq!(formation.rounds, FORMATION_ROUNDS);
        assert!(formation.nonce < 1 << 31, "the nonce rides an int32");
        assert_ne!(
            formation.nonce, 0,
            "a zero over a part's buffer never reads as one"
        );
        assert_ne!(
            second[0].formation.unwrap().nonce,
            formation.nonce,
            "the next launch's parts are told from this one's"
        );
        let json = serde_json::to_value(&first[1]).unwrap();
        assert_eq!(json["formation"]["rounds"], 3);
        let mut ring = config.clone();
        ring.backend = Backend::Ring;
        let spec = &rank_specs(
            &ring,
            &ServedNames::only("node-alias"),
            &[launch(1, 2), launch(3, 2)],
            8_192,
            2.0,
        )[0];
        assert_eq!(
            spec.formation, None,
            "ring runs over TCP: nothing to absorb"
        );

        let mut live = RankLive::default();
        live.take_line(r#"GOOSE_RANK_FORMING {"rank": 0, "round": 1}"#);
        assert_eq!(live.forming, Some(1));
        assert!(
            live.tail.is_empty(),
            "a round's announcement stays out of the tail"
        );
        live.take_line(r#"GOOSE_RANK_FORMATION {"rank": 0, "rounds": 3, "lost": {"1": 0}}"#);
        assert_eq!(live.formation.as_ref().unwrap().losses(), None);
        live.take_line(r#"GOOSE_RANK_FORMATION {"rank": 0, "rounds": 3, "lost": {"1": 1}}"#);
        assert_eq!(
            live.formation.unwrap().losses().as_deref(),
            Some("rank 1 → rank 0: 1 message(s) went nowhere in 3 rounds")
        );
    }

    /// rank_live.py's two functions under a real interpreter, on the instants the 2-rank 27B
    /// tensor split measured (2026-09-24: prefill from 0.52 s, 2,048 tokens reported at 13.53 s,
    /// the first token at 21.069 s, the third at 21.354 s).
    #[test]
    fn the_live_request_table_reads_prefill_and_generation_apart() {
        let checks = r#"
queued = live_request("r", 0.0, 0.5, max_tokens=60)
assert (queued["phase"], queued["status"]) == ("queued", "waiting"), queued
assert queued["prompt_tokens_per_second"] is None and queued["tokens_per_second"] is None
reading = live_request("r", 0.0, 13.53, prompt_tokens=3249, prefill_started=0.52, prefilled=2048)
assert (reading["phase"], reading["status"]) == ("prefill", "running"), reading
assert reading["prompt_tokens_per_second"] == round(2048 / 13.01, 2), reading
assert reading["tokens_per_second"] is None
writing = live_request("r", 0.0, 21.354, prompt_tokens=3249, prefill_started=0.52,
                       prefilled=3249, first_token=21.069, last_token=21.354, completion=3)
assert writing["phase"] == "generation", writing
assert writing["prompt_tokens_per_second"] == round(3249 / (21.069 - 0.52), 2), writing
assert writing["tokens_per_second"] == round(2 / (21.354 - 21.069), 2), writing
assert writing["ttft_s"] == 21.069
one = live_request("r", 0.0, 21.1, first_token=21.069, last_token=21.069, completion=1)
assert one["tokens_per_second"] is None, "one token has no decode span"
busy = live_status({"num_running": 1, "num_waiting": 0}, [writing, queued])
assert busy["status"] == "generating" and busy["generation_tps"] == writing["tokens_per_second"]
assert busy["num_running"] == 1 and len(busy["requests"]) == 2
idle = live_status({"num_running": 0, "num_waiting": 0}, [])
assert (idle["status"], idle["generation_tps"], idle["requests"]) == ("idle", None, [])
# A restored 48,647-token prefix: the prefill starts there and one range ends at 50,695.
cached = live_request("r", 0.0, 10.52, prompt_tokens=52001, cached_tokens=48647,
                      prefill_started=0.52, prefilled=50695)
assert cached["prompt_tokens_per_second"] == round(2048 / 10.0, 2), "only the read tokens count"
assert cached["cached_tokens"] == 48647 and cached["prefilled_tokens"] == 50695
restored_whole = live_request("r", 0.0, 1.0, prompt_tokens=48648, cached_tokens=48647,
                              prefill_started=0.5, prefilled=48647)
assert restored_whole["prompt_tokens_per_second"] is None, "nothing read yet: no rate"
print("ok")
"#;
        let out = std::process::Command::new("/usr/bin/python3")
            .arg("-c")
            .arg(format!("{}{checks}", include_str!("rank_live.py")))
            .output()
            .expect("/usr/bin/python3 runs the rank programs' pure half");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "ok");
    }

    /// rank_budget.py under a real interpreter, on Q-65's turn (2026-09-25, the 2-rank 27B split
    /// launched for 262,144 tokens): a 49,939-token prompt with no max_tokens stopped at exactly
    /// 512 — mlx_lm's `--max-tokens` default. The budget is the window's room instead.
    #[test]
    fn a_request_without_max_tokens_runs_to_the_windows_room_not_512() {
        let checks = r#"
assert generation_budget(262144, 49939, None) == 262144 - 49939
assert generation_budget(262144, 49939, None) > 512
assert generation_budget(262144, 49939, 4096) == 4096, "a client's own max_tokens stands"
assert generation_budget(262144, 262000, 4096) == 144, "never past the window"
assert generation_budget(262144, 262143, None) == 1
for full in (262144, 300000):
    try:
        generation_budget(262144, full, None)
    except ContextFull as refusal:
        assert "262144" in str(refusal) and str(full) in str(refusal), refusal
        assert "maximum context length" in str(refusal), "goose reads it as context-exceeded"
    else:
        raise AssertionError(f"a {full}-token prompt has no room")
print("ok")
"#;
        let out = std::process::Command::new("/usr/bin/python3")
            .arg("-c")
            .arg(format!("{}{checks}", include_str!("rank_budget.py")))
            .output()
            .expect("/usr/bin/python3 runs the rank programs' pure half");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "ok");
    }

    /// rank_thinking.py under a real interpreter, on Q-135's request (2026-09-26): goose's first
    /// agent call of the Jira brief — 27 tools, no chat_template_kwargs because the 27B's profile
    /// leaves thinking on auto. The single engine rendered it with thinking OFF; the split's
    /// mlx_lm.server rendered it ON at effort xhigh and thought 7k–10.4k+ tokens. Every rank now
    /// resolves it as the single engine does.
    #[test]
    fn a_rank_resolves_the_thinking_switch_as_the_single_engine_does() {
        let checks = format!(
            "QWEN38 = {}\n{}",
            serde_json::to_string(QWEN38).unwrap(),
            r#"
tools = [{"type": "function", "function": {"name": "shell"}}]
assert template_reasons(QWEN38), "Qwen3.8 carries the XML tool and think contracts"
assert not template_reasons("{{ messages }}") and not template_reasons(None)
agent = {"messages": [], "tools": tools}
assert resolved_template_kwargs(agent, True) == {"enable_thinking": False}, "auto + tools: off"
assert resolved_template_kwargs({"messages": []}, True) == {"enable_thinking": False}, "auto, no tools: off"
assert resolved_template_kwargs({"messages": []}, False) == {}, "no reasoning parser: the template decides"
on = {"messages": [], "tools": tools, "chat_template_kwargs": {"enable_thinking": True, "reasoning_effort": "low"}}
assert resolved_template_kwargs(on, True) == {"enable_thinking": True, "reasoning_effort": "low"}
effort_only = {"messages": [], "tools": tools, "chat_template_kwargs": {"reasoning_effort": "low"}}
assert resolved_template_kwargs(effort_only, True) == {"enable_thinking": False, "reasoning_effort": "low"}, \
    "an effort alone is no pin: the single engine turns thinking off and the level is inert"
assert resolved_template_kwargs({"messages": [], "chat_template_kwargs": {"enable_thinking": "false"}}, True) \
    == {"enable_thinking": False}, "the string form becomes the boolean the template tests"
assert resolved_template_kwargs({"messages": [], "enable_thinking": True, "tools": tools}, True) \
    == {"enable_thinking": True}, "a top-level pin reaches mlx_lm, which reads only the kwargs"
assert resolved_template_kwargs({"messages": [], "tools": tools, "tool_choice": "none"}, True) == {}, \
    "tool_choice none: prose on tool definitions keeps the template's default"
assert resolved_template_kwargs({"messages": [], "tools": tools, "reasoning_effort": "none"}, True) \
    == {"enable_thinking": False}
for asked in ({"reasoning_effort": "high"}, {"reasoning_max_tokens": 512}, {"reasoning": {"effort": "low"}}):
    try:
        resolved_template_kwargs({"messages": [], **asked}, True)
    except ThinkingRefused as refusal:
        assert "does not translate" in str(refusal) and list(asked)[0] in str(refusal), refusal
    else:
        raise AssertionError(f"{asked} would be dropped silently")
print("ok")
"#
        );
        let out = std::process::Command::new("/usr/bin/python3")
            .arg("-c")
            .arg(format!("{}{checks}", include_str!("rank_thinking.py")))
            .output()
            .expect("/usr/bin/python3 runs the rank programs' pure half");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "ok");
    }

    /// rank_prefill.py under a real interpreter, on E2E #2's plan (the 27B over 2 ranks at
    /// 262,144 tokens: KV charge 17,333,813,248 B, workspace one row's 2,048-token chunk) and
    /// Q-104's repro (turn 6: a ~51k-token turn with four summaries raised a rank 35 → 44.8 GB in
    /// one second — five rows padded to the turn's width, their scores at once).
    #[test]
    fn the_prefill_plan_sizes_every_step_and_holds_what_the_batch_cannot_fit() {
        let checks = r#"
p = {"step": 2048, "workspace_bytes": 2048 * 262144 * 25, "pair_bytes": 25,
     "kv_bytes_per_token": 32768, "sequence_state_bytes": 76972032,
     "batch_transient_ratio": 2.2}
limit = 17333813248
assert prefill_chunk(p, 1, 262144, 256) == 2048, "the plan's own chunk at the full window"
assert prefill_chunk(p, 5, 51200, 256) == 2048
assert prefill_chunk(p, 5, 100000, 256) == 1024, "wider batches take smaller chunks"
assert prefill_chunk(p, 8, 262144, 256) == 256
assert prefill_chunk(p, 40, 262144, 256) == 51, "under one KV step: whole tokens, not zero"
for rows, width in ((1, 262144), (5, 51200), (5, 100000), (8, 262144), (40, 262144)):
    chunk = prefill_chunk(p, rows, width, 256)
    assert chunk_overruns(p, rows, width, chunk) == 0, (rows, width, chunk)
assert chunk_overruns(p, 1, 262144, 4096) == 2048 * 262144 * 25, "past the workspace: named"
assert admits(p, limit, 0, 0, 262143), "an idle engine takes a full-window prompt"
assert batch_kv_charge(p, 1, 262144) == 262144 * 32768 + 76972032, "a lone row: its KV once"
# Q-104's turn 6: the ~51k-token turn joins four live summaries — five rows padded to 51,200:
# 2.2 × 5 × 1.755 GB = 19.3 GB > 17.33. Four rows (13.8 GB... 15.4 GB) still fit.
assert not admits(p, limit, 4, 72, 51200)
assert admits(p, limit, 3, 72, 51200)
assert batch_kv_charge(p, 5, 51200) > limit >= batch_kv_charge(p, 4, 51200)
# E2E #2's 22:40:48 burst (the 36,027-token turn and four summaries): fits, 13.8 GB of 17.33.
assert admits(p, limit, 4, 36027, 300)
print("ok")
"#;
        let out = std::process::Command::new("/usr/bin/python3")
            .arg("-c")
            .arg(format!("{}{checks}", include_str!("rank_prefill.py")))
            .output()
            .expect("/usr/bin/python3 runs the rank programs' pure half");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "ok");
    }

    /// The interpreter of a goose-managed env on this Mac (or the one `override_var` names) whose
    /// provisioning proof — the exact import preflight runs — prints the pinned answer. `None`,
    /// said out loud, when it is absent or stale: preflight refuses a stale env before any rank
    /// runs, so the shipped rank programs never execute under one.
    fn proven_env(spec: &EnvSpec, override_var: &str) -> Option<String> {
        let python = std::env::var(override_var)
            .unwrap_or_else(|_| spec.python(&dirs::home_dir().unwrap().display().to_string()));
        if !std::path::Path::new(&python).exists() {
            eprintln!("skipped: {python} absent");
            return None;
        }
        let out = std::process::Command::new(&python)
            .arg("-c")
            .arg(&spec.check)
            .output()
            .unwrap_or_else(|e| panic!("{python} runs: {e}"));
        let answer = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if answer != spec.expect {
            eprintln!(
                "skipped: {python} imports '{answer}', the pinned env is '{}' — provision it again \
                 (or point {override_var} at one that is){}",
                spec.expect,
                String::from_utf8_lossy(&out.stderr)
            );
            return None;
        }
        Some(python)
    }

    /// Runs `program` as a rank runs its own — the spec base64 in argv[2] — but WITHOUT the rank
    /// marker, so no sweep ever takes it for a goose rank (Q-77). Returns its GOOSE_TEST line.
    fn run_against_real_packages(
        python: &str,
        program: &str,
        spec: &RankSpec,
    ) -> serde_json::Value {
        let tmp = tempfile::tempdir().unwrap();
        let out = std::process::Command::new(python)
            .arg("-c")
            .arg(program)
            .arg("goose-sidecar-test")
            .arg(
                base64::engine::general_purpose::STANDARD.encode(serde_json::to_vec(spec).unwrap()),
            )
            .env("TMPDIR", tmp.path())
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success(), "{stdout}{stderr}");
        let line = stdout
            .lines()
            .find_map(|l| l.strip_prefix("GOOSE_TEST "))
            .unwrap_or_else(|| panic!("{stdout}{stderr}"));
        serde_json::from_str(line).unwrap()
    }

    /// The pipeline program as shipped — the load lock, the env prelude, the live table and
    /// pipeline_rank.py up to where serve() needs a model and a group — against the REAL fork at
    /// the pinned commit (`EnvSpec::pipeline()`'s proof). Its stand-in test replaced the whole fork
    /// with a module of the same names; here the fork's own seams are what goose patches: the
    /// attribute guard, `_Job` (a dataclass whose `produced` field LiveJob turns into a
    /// property), `_Engine._start`/`_Engine.prefill` (Q-134's continuous admission: a row
    /// prefills alone, then joins the running batch) measured over the fork's own
    /// `prefill_chunks`, `_Joining` and `_Row`, the fork's own argparse reading goose's serve
    /// argv, and the fork's own `_build_app` serving `/v1/status` through goose's replacement
    /// route (only the tokenizer — the model's — is a stand-in). Negative controls, measured
    /// 2026-09-26: the same program under an env still on 2f02ac645 exits at the guard ("has no
    /// prefill_chunks"); the 66ccd37a6 env's module has no `_Engine`, the guard's first new name.
    #[test]
    fn the_pipeline_program_patches_the_real_forks_seams() {
        let Some(python) = proven_env(&EnvSpec::pipeline(), "GOOSE_TEST_PIPELINE_PYTHON") else {
            return;
        };
        let config = pipeline_config();
        let served = ServedNames {
            id: "node-alias".to_string(),
            also: vec!["Org/Model-HF".to_string()],
        };
        let spec =
            pipeline_rank_specs(&config, &served, 32_768, "19", 2_048, &[0, 0], 2.0).remove(0);
        let rank = include_str!("pipeline_rank.py");
        let serves = rank
            .find("threading.Thread(target=report_memory")
            .expect("the program starts the reporter, then serve()");
        let checks = r#"
import asyncio
from fastapi.testclient import TestClient

serve = pipeline_qwen4_serve
assert serve._Job is LiveJob, "the job seam is goose's"
assert serve._Engine._start is _start and serve._Engine.prefill is prefill, "the engine seams"
assert serve._build_app is _build_app, "the app seam is goose's"
starts = serve.pipe._parse_starts(options.split)

loop = asyncio.new_event_loop()
row = serve._Row(list(range(10)), 8, 0.0, 1.0)
job = serve._Job(row, loop, asyncio.Queue())
assert type(job) is LiveJob and jobs_by_row[id(row)] is job and job.produced == 0, job

# The fork's engine, reduced to what goose measures: the row starts prefilling in its own cache
# (its ranges the fork's own prefill_chunks, to the prompt's end), one collective per range, the
# last one sampling its first token.
def start(engine, row):
    ranges = serve.prefill_chunks(0, len(row.ids), engine.prefill_step, 0)
    engine.joining = serve._Joining(row, None, None, None, None, ranges)
def chunk(engine, words):
    engine.joining.ranges.pop(0)
    return (0 if not engine.joining.ranges else None), [0, 0]
fork_start, fork_prefill = start, chunk
engine = serve._Engine(type("Stage", (), {"is_first": True})(), None, 4)
serve._Engine._start(engine, row)
assert job.prefill_started is not None and job.prefilled == 0, vars(job)
walked = []
while engine.joining.ranges:
    serve._Engine.prefill(engine, None)
    walked.append(job.prefilled)
job.produced += 1

class Tokenizer:
    chat_template = ""
    eos_token_ids = [0]

state = serve._State(served=options.served_model_name, aliases=tuple(options.served_model_alias or ()),
                     context=options.context, max_batch=options.max_batch)
app = serve._build_app(state, Tokenizer(), {0})
client = TestClient(app)
models = [m["id"] for m in client.get("/v1/models").json()["data"]]
# The stand-in tokenizer declares no tool contract, so the tools refusal that follows the fork's
# model check says a name got past it without generating.
def chat(model):
    answer = client.post("/v1/chat/completions", json={"model": model, "messages": [{"role": "user",
        "content": "hi"}], "tools": [{"type": "function", "function": {"name": "f", "parameters": {}}}]})
    return [answer.status_code, answer.json()["error"]]
names = {name: chat(name) for name in ("node-alias", "Org/Model-HF", "other")}
state.active = [job]
state.jobs.put(serve._Job(serve._Row([1] * 5, 4, 0.0, 1.0), loop, asyncio.Queue()))
status = client.get("/v1/status").json()
print("GOOSE_TEST " + json.dumps({"options": vars(options), "starts": starts, "walked": walked,
      "first_token": job.first_token is not None, "prefilled": job.prefilled, "status": status,
      "models": models, "names": names}))
"#;
        let program = format!(
            "{}{}{}{}{}{checks}",
            include_str!("rank_load_lock.py"),
            include_str!("rank_env.py"),
            include_str!("rank_live.py"),
            include_str!("rank_thinking.py"),
            &rank[..serves]
        );
        let seen = run_against_real_packages(&python, &program, &spec);
        let options = &seen["options"];
        assert_eq!(options["model"], config.nodes[0].model_dir.as_str());
        assert_eq!(options["served_model_name"], "node-alias");
        assert_eq!(
            options["served_model_alias"],
            serde_json::json!(["Org/Model-HF"]),
            "rank 0 is told every other name of the model"
        );
        assert_eq!(options["port"], 8190);
        assert_eq!(options["context"], 32_768);
        assert_eq!(
            seen["models"],
            serde_json::json!(["node-alias", "Org/Model-HF"]),
            "the fork lists the served id first, then the alias"
        );
        let names = &seen["names"];
        for accepted in ["node-alias", "Org/Model-HF"] {
            assert_eq!(
                names[accepted][1]["type"], "tools_unsupported",
                "{accepted} passes the fork's model check: {names}"
            );
        }
        assert_eq!(
            names["other"],
            serde_json::json!([404, {"message": "model 'other' is not served here; this engine \
                serves 'node-alias' (also answering to 'Org/Model-HF')", "type": "model_not_found"}]),
            "another model is still refused, naming every name served"
        );
        assert_eq!(options["slots"], 2);
        assert_eq!(options["max_batch"], 2);
        assert_eq!(options["prefill_step"], 2_048);
        assert_eq!(
            seen["starts"],
            serde_json::json!([0, 19]),
            "the fork loads the split preflight approved"
        );
        assert_eq!(
            seen["walked"],
            serde_json::json!([4, 8, 10]),
            "each prefill range's end, read from the fork's own prefill_chunks — to the prompt's \
             end, the last range sampling the first token (Q-134)"
        );
        assert_eq!(seen["first_token"], true);
        assert_eq!(seen["prefilled"], 10);
        let status = &seen["status"];
        assert_eq!(status["status"], "generating", "{status}");
        assert_eq!(
            (&status["num_running"], &status["num_waiting"]),
            (&serde_json::json!(1), &serde_json::json!(1)),
            "the fork's own counters ride under goose's table: {status}"
        );
        assert_eq!(
            status["prefix_cache"]["enabled"], false,
            "the fork's own /v1/status body: {status}"
        );
        let phases: Vec<&str> = status["requests"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["phase"].as_str().unwrap())
            .collect();
        assert_eq!(phases, ["generation", "queued"]);
    }

    /// Q-135: the same request and the same setting render the SAME prompt on every way, through
    /// each way's own code, on the Qwen3.8 template the 27B ships (CPU, a stand-in vocabulary — only
    /// the template renders): the single engine's /v1/chat/completions resolution (Rapid-MLX's own
    /// helpers — byte-identical at the pinned fork commit and the single engine's tag, measured
    /// 2026-09-26) into its apply_chat_template; mlx_lm 0.31.3's own `_tokenize` with the kwargs
    /// rank_wrapper.py hands it; and goose's pipeline route over the fork's own handler. Negative
    /// control: the kwargs as goose sends them, unresolved, render the split's pre-fix prompt — ON
    /// at effort xhigh where the single engine renders OFF.
    #[test]
    fn every_way_renders_the_same_prompt_for_the_same_setting() {
        let Some(python) = proven_env(&EnvSpec::pipeline(), "GOOSE_TEST_PIPELINE_PYTHON") else {
            return;
        };
        let spec = pipeline_rank_specs(
            &pipeline_config(),
            &ServedNames::only("node-alias"),
            32_768,
            "19",
            2_048,
            &[0, 0],
            2.0,
        )
        .remove(0);
        let rank = include_str!("pipeline_rank.py");
        let serves = rank
            .find("threading.Thread(target=report_memory")
            .expect("the program starts the reporter, then serve()");
        let checks = r#"
import copy
import types

import mlx_lm.server as mlx_server
from fastapi.testclient import TestClient
from rapid_mlx.api.models import ChatCompletionRequest
from rapid_mlx.api.tool_calling import convert_tools_for_template
from rapid_mlx.config.server_config import get_config
from rapid_mlx.service import helpers
from rapid_mlx.utils.chat_template import apply_chat_template
from tokenizers import Tokenizer, models
from transformers import PreTrainedTokenizerFast

SERVED = options.served_model_name


def qwen38_tokenizer():
    tokenizer = PreTrainedTokenizerFast(
        tokenizer_object=Tokenizer(models.WordLevel({"[UNK]": 0}, unk_token="[UNK]"))
    )
    tokenizer.chat_template = QWEN38
    return tokenizer


hf = qwen38_tokenizer()


def single(body):
    # routes/chat.py: effort translation, the tools gate, the casual gate, then the resolved switch
    # into the engine's apply_chat_template. goose's model_parsers.rs gives this template the
    # deepseek_r1 reasoning parser.
    get_config().reasoning_parser_name = "deepseek_r1"
    request = ChatCompletionRequest(**copy.deepcopy(body))
    helpers.maybe_apply_reasoning_effort(request, chat_template=QWEN38)
    helpers.maybe_auto_disable_thinking_for_tools(request)
    helpers.maybe_auto_disable_thinking_for_casual_chat(request)
    return apply_chat_template(
        hf,
        [m.model_dump(exclude_none=True) for m in request.messages],
        tools=convert_tools_for_template(body.get("tools")),
        enable_thinking=helpers._resolve_enable_thinking(request),
        model_name=SERVED,
        chat_template_kwargs=request.chat_template_kwargs or None,
    )


class Recording:
    has_chat_template = True
    has_tool_calling = True
    has_thinking = False

    def __init__(self):
        self.prompts = []

    def apply_chat_template(self, messages, **kwargs):
        text = hf.apply_chat_template(messages, **{**kwargs, "tokenize": False})
        if kwargs.get("add_generation_prompt"):
            self.prompts.append(text)
        return [ord(c) for c in text]


def tensor(body, kwargs):
    # mlx_lm's own _tokenize under goose's argv (no --chat-template-args: its parser's "{}").
    responses = mlx_server.ResponseGenerator.__new__(mlx_server.ResponseGenerator)
    responses.model_provider = types.SimpleNamespace(
        cli_args=types.SimpleNamespace(chat_template_args=json.loads("{}"))
    )
    request = mlx_server.CompletionRequest(
        "chat", "", copy.deepcopy(body["messages"]), body.get("tools") or None, None
    )
    recording = Recording()
    responses._tokenize(recording, request, types.SimpleNamespace(chat_template_kwargs=kwargs))
    return recording.prompts[0]


class Rendered(Exception):
    pass


pipe_tokenizer = qwen38_tokenizer()
rendered = []


def encode(text, *args, **kwargs):
    rendered.append(text)
    raise Rendered()


pipe_tokenizer.encode = encode
state = pipeline_qwen4_serve._State(served=SERVED, context=options.context, max_batch=options.max_batch)
client = TestClient(pipeline_qwen4_serve._build_app(state, pipe_tokenizer, {0}), raise_server_exceptions=False)


def pipeline(body):
    rendered.clear()
    reply = client.post("/v1/chat/completions", json=body)
    return rendered[0] if rendered else [reply.status_code, reply.json()]


tools = [{"type": "function", "function": {"name": "shell", "description": "Run a command",
          "parameters": {"type": "object", "properties": {"command": {"type": "string"}},
                         "required": ["command"]}}}]
messages = [{"role": "system", "content": "You are goose."},
            {"role": "user", "content": "Write notes/kickoff.md from the kickoff notes."}]
cases = {
    "auto_tools": {},
    "auto_no_tools": {"tools": None},
    "on": {"chat_template_kwargs": {"enable_thinking": True}},
    "off": {"chat_template_kwargs": {"enable_thinking": False}},
    "on_low": {"chat_template_kwargs": {"enable_thinking": True, "reasoning_effort": "low"}},
    "auto_low": {"chat_template_kwargs": {"reasoning_effort": "low"}},
    "string_false": {"chat_template_kwargs": {"enable_thinking": "false"}},
    "tool_choice_none": {"tool_choice": "none"},
    "effort_none": {"reasoning_effort": "none"},
}
seen = {}
reasons = template_reasons(QWEN38)
for name, extra in cases.items():
    body = {"model": SERVED, "messages": messages, "tools": tools, **extra}
    if body["tools"] is None:
        del body["tools"]
    reference = single(body)
    seen[name] = {
        "tensor": tensor(body, resolved_template_kwargs(body, reasons)) == reference,
        "pipeline": pipeline(body) == reference,
        "unresolved_tensor": tensor(body, body.get("chat_template_kwargs")) == reference,
        "thinks": reference.endswith("<|im_start|>assistant\n<think>\n"),
        "xhigh": "Reasoning effort is set to xhigh" in reference,
    }
seen["untranslated"] = pipeline({"model": SERVED, "messages": messages, "reasoning_max_tokens": 512})
print("GOOSE_TEST " + json.dumps(seen))
"#;
        let program = format!(
            "{}{}{}{}{}QWEN38 = {}\n{checks}",
            include_str!("rank_load_lock.py"),
            include_str!("rank_env.py"),
            include_str!("rank_live.py"),
            include_str!("rank_thinking.py"),
            &rank[..serves],
            serde_json::to_string(QWEN38).unwrap(),
        );
        let seen = run_against_real_packages(&python, &program, &spec);
        // (thinks, xhigh, the split's pre-fix prompt was already the single engine's)
        let expected = [
            ("auto_tools", false, false, false),
            ("auto_no_tools", false, false, false),
            ("on", true, true, true),
            ("off", false, false, true),
            ("on_low", true, false, true),
            ("auto_low", false, false, false),
            ("string_false", false, false, false),
            ("tool_choice_none", true, true, true),
            ("effort_none", false, false, false),
        ];
        for (case, thinks, xhigh, unresolved_matched) in expected {
            let row = &seen[case];
            assert_eq!(
                row["tensor"], true,
                "{case}: mlx_lm renders the single engine's prompt"
            );
            assert_eq!(
                row["pipeline"], true,
                "{case}: the pipeline renders the single engine's prompt"
            );
            assert_eq!(row["thinks"], thinks, "{case}");
            assert_eq!(row["xhigh"], xhigh, "{case}");
            assert_eq!(row["unresolved_tensor"], unresolved_matched, "{case}");
        }
        assert_eq!(seen["untranslated"][0], 400, "{}", seen["untranslated"]);
        assert_eq!(
            seen["untranslated"][1]["error"]["code"],
            "unsupported_parameter"
        );
    }

    /// The tensor program past its prelude — every class and patch rank_wrapper.py lays over
    /// mlx_lm, up to `server.main()` — against the REAL mlx_lm 0.31.3, on the CPU, with no model
    /// and no group (MLX's group is the one stand-in). Its stand-in boot tests ran these lines
    /// against a hand-written mlx_lm; here mlx_lm's own parser reads the wrapper's argv, mlx_lm's
    /// own prompt loop takes the chunk the wrapper sets (and, unpatched, takes the whole slice),
    /// mlx_lm's own BatchGenerator / LRUPromptCache carry the live-batch bound, and mlx_lm's own
    /// HTTP handler serves the wrapper's routes and refusals — the `Refused` BaseException
    /// passing through `handle_completion`'s `except Exception` is upstream's code, not a copy.
    /// Q-131: the spec names the model twice (the node alias it is served under, its HF id);
    /// mlx_lm's own handler admits both, the request every rank receives names the served id, and
    /// another model is refused naming both.
    #[test]
    fn the_wrapper_serves_through_real_mlx_lm() {
        let Some(python) = proven_env(&EnvSpec::tensor(), "GOOSE_TEST_TENSOR_PYTHON") else {
            return;
        };
        let served = ServedNames {
            id: "node-alias".to_string(),
            also: vec!["Org/Model-HF".to_string()],
        };
        let config = two_mac_config();
        let tensor = TensorLaunch {
            planned_bytes: 27_456_216_576,
            prompt_cache_limit_bytes: 9_431_744_512,
            prompt_cache_entries: 110,
            mlx_cache_limit_bytes: 2_745_621_658,
            // One row's 600 tokens of scores at a 1,000-token width: a 512-token chunk.
            prefill: TensorPrefill {
                workspace_bytes: 1_000 * 600 * 25,
                ..e2e_prefill()
            },
        };
        let mut spec = rank_specs(&config, &served, &[tensor, tensor], 141_568, 2.0).remove(0);
        if let RankProgram::MlxLmServer { doorbell, .. } = &mut spec.program {
            *doorbell = false;
        }
        let wrapper = include_str!("rank_wrapper.py");
        let start = wrapper
            .find("import faulthandler  # noqa")
            .expect("the wrapper's body starts after the group check");
        let end = wrapper
            .find("server.main()")
            .expect("the wrapper ends in mlx_lm's main");
        let checks = r#"
import argparse
import http.server
import types
import urllib.error
import urllib.request
from queue import Queue

mx.set_default_device(mx.cpu)
from mlx_lm.generate import GenerationBatch, SequenceStateMachine
from mlx_lm.models.cache import LRUPromptCache

assert server.run is run and issubclass(server.BatchGenerator, mlx_generate.BatchGenerator)
assert ArraysCache.advance is advance
assert issubclass(server.LRUPromptCache, LRUPromptCache)
assert mlx_generate.PromptProcessingBatch.prompt is prompt
assert mlx_generate.PromptProcessingBatch.split is split

# The argv the wrapper hands mlx_lm, through mlx_lm's OWN parser: main() builds it, and parsing
# is the last thing main does before it touches the device.
class Parsed(Exception):
    pass

real_parse_args = argparse.ArgumentParser.parse_args

def parse_and_stop(self, args=None, namespace=None):
    raise Parsed(real_parse_args(self, args, namespace))

argparse.ArgumentParser.parse_args = parse_and_stop
try:
    server.main()
    raise SystemExit("mlx_lm's main() never parsed its argv")
except Parsed as parsed:
    cli = parsed.args[0]
argparse.ArgumentParser.parse_args = real_parse_args

def chunks_of(step):
    taken = []
    batch = mlx_generate.PromptProcessingBatch.__new__(mlx_generate.PromptProcessingBatch)
    batch.model = lambda tokens, cache: taken.append(tokens.shape[1])
    batch.uids, batch.tokens, batch.prompt_cache = [0], [[]], []
    batch.prefill_step_size = int(prefill["step"])
    step(batch, [[1] * 1000])
    return taken

generator = mlx_generate.BatchGenerator.__new__(server.BatchGenerator)
generator._old_wired_limit = None
generator.max_tokens, generator.logits_processors, generator._uid_count = 128, [], 0
generator._default_state_machine = SequenceStateMachine({}, initial="normal")
generator._unprocessed_sequences, generator._currently_processing = deque(), []
generator._prompt_batch = mlx_generate.PromptProcessingBatch.empty(None, None)
generator._generation_batch = GenerationBatch.empty(None, None)
generator.insert_segments(segments=[[[1] * 100]], caches=[[KVCache()]], all_tokens=[[]], max_tokens=[5])

def entry(first):
    layer = KVCache()
    layer.update_and_fetch(mx.zeros((1, 2, 64, 8), dtype=mx.bfloat16), mx.zeros((1, 2, 64, 8), dtype=mx.bfloat16))
    return list(range(first, first + 64)), [layer]

# mlx_lm's KVCache grows in 256-token steps: a 64-token entry holds a whole step until `compact`
# gives it exactly its own tokens.
raw_entry_bytes = sum(layer.nbytes for layer in entry(0)[1])
compacted = entry(0)[1]
compact(compacted)
entry_bytes = sum(layer.nbytes for layer in compacted)
prompt_cache_limit = generator.prompt_cache_nbytes + entry_bytes
cache = server.LRUPromptCache(int(spec["prompt_cache_entries"]))
cache.insert_cache("m", *entry(0))
cache.insert_cache("m", *entry(100))
alone = [len(cache), cache.nbytes]
live_batch[:] = [generator]
cache.insert_cache("m", *entry(200))
beside_batch = [len(cache), cache.nbytes]
live_batch.clear()

responses = server.ResponseGenerator.__new__(server.ResponseGenerator)
responses.model_provider = types.SimpleNamespace(cli_args=cli, tokenizer=types.SimpleNamespace(chat_template=QWEN38))
responses.requests = Queue()
httpd = http.server.ThreadingHTTPServer(
    ("127.0.0.1", 0),
    lambda *args, **kwargs: server.APIHandler(responses, *args, system_fingerprint="test", **kwargs),
)
threading.Thread(target=httpd.serve_forever, daemon=True).start()
shared = []
shared_kwargs = []

shared_models = []

def generation_thread():
    while True:
        rqueue, request, args = responses.requests.get()
        shared.append(args.max_tokens)
        shared_models.append(args.model.model)
        shared_kwargs.append(args.chat_template_kwargs)
        rqueue.put(ContextFull("no room"))

threading.Thread(target=generation_thread, daemon=True).start()

def call(path, body=None):
    url = f"http://127.0.0.1:{httpd.server_address[1]}{path}"
    data = None if body is None else json.dumps(body).encode()
    try:
        with urllib.request.urlopen(urllib.request.Request(url, data=data), timeout=30) as reply:
            return [reply.status, json.loads(reply.read())]
    except urllib.error.HTTPError as error:
        return [error.code, json.loads(error.read())]

messages = [{"role": "user", "content": "hi"}]
print("GOOSE_TEST " + json.dumps({
    "cli": {k: v for k, v in vars(cli).items() if isinstance(v, (int, str, type(None)))},
    "chunks": chunks_of(mlx_generate.PromptProcessingBatch.prompt),
    "upstream_chunks": chunks_of(upstream_prompt),
    "raw_entry_bytes": raw_entry_bytes,
    "entry_bytes": entry_bytes,
    "alone": alone,
    "beside_batch": beside_batch,
    "models": call("/v1/models"),
    "wrong_model": call("/v1/chat/completions", {"model": "other", "messages": messages}),
    "untranslated": call("/v1/chat/completions", {"model": served, "messages": messages, "reasoning_max_tokens": 512}),
    "no_room": call("/v1/chat/completions", {"model": served, "messages": messages}),
    "alias_no_room": call("/v1/chat/completions", {"model": "Org/Model-HF", "messages": messages}),
    "shared_models": shared_models,
    "shared_max_tokens": shared,
    "shared_kwargs": shared_kwargs,
    "status": call("/v1/status"),
}))
"#;
        let program = format!(
            "{}{}{}{}{}{}{}{}{}\
             class _Group:\n    def rank(self): return 0\n    def size(self): return 2\n\
             group = _Group()\n{}QWEN38 = {qwen}\n{checks}",
            include_str!("rank_load_lock.py"),
            include_str!("rank_env.py"),
            include_str!("rank_live.py"),
            include_str!("rank_thinking.py"),
            include_str!("rank_budget.py"),
            include_str!("rank_prefill.py"),
            include_str!("rank_batch.py"),
            include_str!("rank_state.py"),
            include_str!("rank_tool_stream.py"),
            &wrapper[start..end],
            qwen = serde_json::to_string(QWEN38).unwrap(),
        );
        let seen = run_against_real_packages(&python, &program, &spec);
        let cli = &seen["cli"];
        assert_eq!(cli["model"], config.nodes[0].model_dir.as_str());
        assert_eq!(cli["port"], 8190);
        assert_eq!(cli["prompt_cache_size"], 110);
        assert_eq!(
            cli["prompt_cache_bytes"], 9_431_744_512u64,
            "mlx_lm's own size parser reads the plan's KV charge"
        );
        assert_eq!(cli["prefill_step_size"], 2_048);
        assert_eq!(
            seen["chunks"],
            serde_json::json!([512, 488]),
            "mlx_lm's own prompt loop takes the chunk the plan's workspace affords"
        );
        assert_eq!(
            seen["upstream_chunks"],
            serde_json::json!([1_000]),
            "unpatched, the same loop reads the whole slice in one step"
        );
        let entry = seen["entry_bytes"].as_u64().unwrap();
        assert_eq!(
            (seen["raw_entry_bytes"].as_u64().unwrap(), entry),
            (256 * 2 * 8 * 2 * 2, 64 * 2 * 8 * 2 * 2),
            "compact leaves an entry exactly its 64 tokens of bf16 keys and values, not its step"
        );
        assert_eq!(seen["alone"], serde_json::json!([2, 2 * entry]));
        assert_eq!(
            seen["beside_batch"],
            serde_json::json!([1, entry]),
            "beside the live batch the cache keeps what the KV charge leaves"
        );
        assert_eq!(seen["models"][0], 200);
        let listed: Vec<(&str, u64)> = seen["models"][1]["data"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| {
                (
                    m["id"].as_str().unwrap(),
                    m["context_window"].as_u64().unwrap(),
                )
            })
            .collect();
        assert_eq!(
            listed,
            [("node-alias", 141_568), ("Org/Model-HF", 141_568)],
            "the served id first, then every other name of the same model"
        );
        assert_eq!(
            seen["wrong_model"],
            serde_json::json!([404, {"error": {"message": "model 'other' is not served here; this \
                distributed engine serves 'node-alias' (also answering to 'Org/Model-HF')"}}]),
            "another model is refused, naming every name that is served"
        );
        let no_room = serde_json::json!([400, {"error": {"message": "no room",
            "code": "context_length_exceeded", "type": "invalid_request_error"}}]);
        assert_eq!(
            seen["no_room"], no_room,
            "the refusal passes mlx_lm's own handle_completion to do_POST"
        );
        assert_eq!(
            seen["alias_no_room"], no_room,
            "the HF id passes the same validation to the same generation"
        );
        assert_eq!(
            seen["shared_models"],
            serde_json::json!(["node-alias", "node-alias"]),
            "every rank is handed the served id, whichever name the client used"
        );
        assert_eq!(
            seen["shared_max_tokens"],
            serde_json::json!([null, null]),
            "an absent max_tokens stays absent through mlx_lm's own validation"
        );
        assert_eq!(
            seen["shared_kwargs"],
            serde_json::json!([{"enable_thinking": false}, {"enable_thinking": false}]),
            "auto reaches every rank as the single engine's answer, set before the request is shared"
        );
        assert_eq!(seen["untranslated"][0], 400);
        assert_eq!(
            seen["untranslated"][1]["error"]["code"], "unsupported_parameter",
            "{}",
            seen["untranslated"]
        );
        assert_eq!(seen["status"][1]["status"], "idle");
    }

    /// Q-141: the streamer against the REAL qwen3_coder parser of mlx_lm 0.31.3. For every call
    /// shape the parser accepts — a long file write (quotes, backslashes, tabs, non-ASCII, text
    /// that looks like the close tag and the next header), typed parameters, the word "null", an
    /// undeclared tool, no tools at all, no newlines, no parameters — and every chunking (the
    /// whole text, one character at a time, seeded random pieces), what was streamed is exactly
    /// `json.dumps` of what the parser reads from the whole text, and a long string value was
    /// sent while it was written (all but the held tail before its close). What the parser
    /// refuses (a typed value it cannot convert, a call cut before `</function>`) or reads
    /// differently from what was already sent (a parameter written twice) is named, and what was
    /// streamed stays unterminated, so the client fails the call instead of running it.
    #[test]
    fn the_tool_stream_sends_exactly_what_mlx_lms_parser_reads() {
        let Some(python) = proven_env(&EnvSpec::tensor(), "GOOSE_TEST_TENSOR_PYTHON") else {
            return;
        };
        let checks = r#"
import random
from mlx_lm.tool_parsers import qwen3_coder as q

def schema(name, **props):
    return {"type": "function", "function": {"name": name, "parameters": {"type": "object", "properties": props}}}

TOOLS = [
    schema("shell", command={"type": "string"}, timeout={"type": "integer"}, ratio={"type": "number"},
           force={"type": "boolean"}, env={"type": "object"}, paths={"type": "array"}, note={}),
    schema("write", path={"type": "string"}, content={"type": "string"}),
]
LONG = "".join(
    f'line {i}: echo "quoted" \\ back ünïcödé \U0001f9a2\ttab </param <parameter=x> </function>\n'
    for i in range(300)
)

def call(name, *params):
    body = "".join(f"<parameter={key}>\n{value}\n</parameter>\n" for key, value in params)
    return f"\n<function={name}>\n{body}</function>\n"

EXACT = {
    "shell": (call("shell", ("command", "cd /work && ls -la")), TOOLS),
    "long_write": (call("write", ("path", "/tmp/x.py"), ("content", LONG)), TOOLS),
    "typed": (call("shell", ("command", "echo a command long enough to stream"), ("timeout", "30"),
                   ("ratio", "2.5"), ("force", "true"), ("env", '{"A": 1}'), ("paths", '["a", "b"]')), TOOLS),
    "typed_first": (call("shell", ("timeout", "7"), ("command", "echo the string after a typed value")), TOOLS),
    "null_word": (call("shell", ("command", "null")), TOOLS),
    "null_upper": (call("shell", ("command", "NULL")), TOOLS),
    "untyped_note": (call("shell", ("note", '{"looks": "like json but the schema has no type"}')), TOOLS),
    "undeclared": (call("bash", ("cmd", "rm -rf build && make -j8 all install")), TOOLS),
    "no_tools": (call("shell", ("command", "echo no tools were declared at all")), None),
    "no_newlines": ("<function=shell><parameter=command>ls -la /very/long/path/somewhere</parameter></function>", TOOLS),
    "two_newlines": ("<function=shell><parameter=command>\n\nkeeps one newline each side\n\n</parameter></function>", TOOLS),
    "no_parameters": ("\n<function=list_files>\n</function>\n", TOOLS),
    "short": (call("shell", ("command", "ls")), TOOLS),
}
REFUSED = {
    "bad_int": (call("shell", ("command", "echo sleep then stop"), ("timeout", "soon")), TOOLS),
    "cut_mid_value": (call("write", ("path", "/tmp/y"), ("content", LONG))[:-2000], TOOLS),
    "written_twice": (call("shell", ("command", "echo the first of two values"), ("command", "echo the second")), TOOLS),
}

def chunkings(text):
    yield "whole", [text]
    yield "chars", list(text)
    rng = random.Random(141)
    for seed in range(3):
        pieces, i = [], 0
        while i < len(text):
            n = rng.randint(1, 9)
            pieces.append(text[i:i + n])
            i += n
        yield f"random{seed}", pieces

def run(pieces, tools):
    stream = ToolCallStream(q._convert_param_value, q._get_arguments_config)
    opened, sent = [], ""
    for piece in pieces:
        name, fragment = stream.feed(piece, tools)
        if name is not None:
            opened.append(name)
        sent += fragment
    rest, why = stream.close(q.parse_tool_call, tools)
    return opened, sent, rest, why

for case, (text, tools) in EXACT.items():
    whole = q.parse_tool_call(text, tools)
    expected = json.dumps(whole["arguments"], ensure_ascii=False)
    for how, pieces in chunkings(text):
        assert "".join(pieces) == text
        opened, sent, rest, why = run(pieces, tools)
        assert why is None, (case, how, why)
        assert opened == [whole["name"]], (case, how, opened)
        assert sent + rest == expected, (case, how, sent + rest, expected)
        if case == "long_write" and how != "whole":
            assert len(rest) <= 2 * ToolCallStream.HOLD, (how, len(rest), "the long value was held back")

for case, (text, tools) in REFUSED.items():
    for how, pieces in chunkings(text):
        opened, sent, rest, why = run(pieces, tools)
        assert rest is None and why, (case, how, rest)
        try:
            json.loads(sent)
        except json.JSONDecodeError:
            pass
        else:
            raise AssertionError(f"{case}/{how}: the client could run {sent!r}")
print("ok")
"#;
        let out = std::process::Command::new(&python)
            .arg("-c")
            .arg(format!("{}{checks}", include_str!("rank_tool_stream.py")))
            .output()
            .unwrap();
        assert!(
            out.status.success() && String::from_utf8_lossy(&out.stdout).trim() == "ok",
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// Q-141 through the REAL mlx_lm 0.31.3 handler: a streamed chat answer whose tool call is
    /// written token by token (the generation is a stand-in feeding mlx_lm's own Response objects
    /// through its own control-token buffer). The generation pauses halfway through the call until
    /// the client has seen part of its arguments. Through the wrapper the client sees them — the
    /// open frame (id, `shell`) and dozens of argument fragments before `</tool_call>` — and the
    /// call it assembles is exactly the parser's; the answer's words, finish_reason `tool_calls`
    /// and the one call are mlx_lm's. NEGATIVE CONTROL: the same generation through mlx_lm's own
    /// handle_completion sends nothing until the call closes (E2E #3c's 18 minutes of silence),
    /// then the whole call in one frame.
    #[test]
    fn a_streamed_tool_call_reaches_the_client_while_it_is_written() {
        let Some(python) = proven_env(&EnvSpec::tensor(), "GOOSE_TEST_TENSOR_PYTHON") else {
            return;
        };
        let config = two_mac_config();
        let tensor = TensorLaunch {
            planned_bytes: 27_456_216_576,
            prompt_cache_limit_bytes: 9_431_744_512,
            prompt_cache_entries: 110,
            mlx_cache_limit_bytes: 2_745_621_658,
            prefill: e2e_prefill(),
        };
        let mut spec = rank_specs(
            &config,
            &ServedNames::only("node-alias"),
            &[tensor, tensor],
            141_568,
            2.0,
        )
        .remove(0);
        if let RankProgram::MlxLmServer { doorbell, .. } = &mut spec.program {
            *doorbell = false;
        }
        let wrapper = include_str!("rank_wrapper.py");
        let start = wrapper
            .find("import faulthandler  # noqa")
            .expect("the wrapper's body starts after the group check");
        let end = wrapper
            .find("server.main()")
            .expect("the wrapper ends in mlx_lm's main");
        let checks = r#"
import argparse
import http.server
import types
import urllib.request
from queue import Queue

mx.set_default_device(mx.cpu)

class Parsed(Exception):
    pass

real_parse_args = argparse.ArgumentParser.parse_args

def parse_and_stop(self, args=None, namespace=None):
    raise Parsed(real_parse_args(self, args, namespace))

argparse.ArgumentParser.parse_args = parse_and_stop
try:
    server.main()
    raise SystemExit("mlx_lm's main() never parsed its argv")
except Parsed as parsed:
    cli = parsed.args[0]
argparse.ArgumentParser.parse_args = real_parse_args

TOOLS = [{"type": "function", "function": {"name": "shell", "parameters": {
    "type": "object", "properties": {"command": {"type": "string"}}}}}]
COMMAND = "".join(
    f'printf "%s\\n" "row {i}: ünï \U0001f9a2 \\\\ \\"q\\"" >> /tmp/out.txt\n' for i in range(120)
).rstrip("\n")
TOOL = f"\n<function=shell>\n<parameter=command>\n{COMMAND}\n</parameter>\n</function>\n"
TOKENS = [TOOL[i:i + 3] for i in range(0, len(TOOL), 3)]
HALF = len(TOKENS) // 2
EXPECTED = json.dumps(qwen3_coder.parse_tool_call(TOOL, TOOLS)["arguments"], ensure_ascii=False)

responses = server.ResponseGenerator.__new__(server.ResponseGenerator)
responses.model_provider = types.SimpleNamespace(cli_args=cli, tokenizer=types.SimpleNamespace(chat_template=QWEN38))
responses.requests = Queue()
httpd = http.server.ThreadingHTTPServer(
    ("127.0.0.1", 0),
    lambda *args, **kwargs: server.APIHandler(responses, *args, system_fingerprint="test", **kwargs),
)
threading.Thread(target=httpd.serve_forever, daemon=True).start()
seen_args = []
waited = []

def token(text, state, match=None, finish=None):
    return server.Response(text, 7, state, match, 0.0, finish, ())

def generation_thread():
    while True:
        rqueue, request, args = responses.requests.get()
        rqueue.put(server.GenerationContext(
            has_tool_calling=True, has_thinking=False, tool_parser=qwen3_coder.parse_tool_call,
            sequences={(1,): "<tool_call>", (2,): "</tool_call>", (3,): "<|im_end|>"},
            prompt=[0] * 8, prompt_cache_count=0,
        ))
        for piece in ("I'll", " write", " it.\n"):
            rqueue.put(token(piece, "normal"))
        rqueue.put(token("<tool_call>", "tool", (1,)))
        for piece in TOKENS[:HALF]:
            rqueue.put(token(piece, "tool"))
        # The client's own reading decides: seen while the call is still being written, or not.
        waited.append(seen_args[-1].wait(3))
        for piece in TOKENS[HALF:]:
            rqueue.put(token(piece, "tool"))
        rqueue.put(token("</tool_call>", "normal", (2,)))
        rqueue.put(token("<|im_end|>", None, (3,), "stop"))
        rqueue.put(None)

threading.Thread(target=generation_thread, daemon=True).start()

def stream():
    seen = threading.Event()
    seen_args.append(seen)
    body = {"model": served, "stream": True, "tools": TOOLS,
            "messages": [{"role": "user", "content": "write the rows"}]}
    url = f"http://127.0.0.1:{httpd.server_address[1]}/v1/chat/completions"
    request = urllib.request.Request(url, data=json.dumps(body).encode())
    text, calls, finish, arg_frames = "", {}, None, 0
    with urllib.request.urlopen(request, timeout=60) as reply:
        for raw in reply:
            line = raw.decode().strip()
            if not line.startswith("data: ") or line == "data: [DONE]":
                continue
            frame = json.loads(line[len("data: "):])
            if not frame["choices"]:
                continue
            choice = frame["choices"][0]
            finish = choice["finish_reason"] or finish
            text += choice["delta"].get("content", "")
            for delta in choice["delta"].get("tool_calls", []):
                call = calls.setdefault(delta["index"], {"ids": [], "names": [], "arguments": ""})
                if "id" in delta:
                    call["ids"].append(delta["id"])
                if "name" in delta["function"]:
                    call["names"].append(delta["function"]["name"])
                call["arguments"] += delta["function"].get("arguments", "")
                arg_frames += 1
            if any(len(call["arguments"]) > len("{") for call in calls.values()):
                seen.set()
    return {"text": text, "calls": [calls[i] for i in sorted(calls)], "finish": finish,
            "arg_frames": arg_frames, "seen_while_written": waited[-1]}

streamed = stream()
server.APIHandler.handle_completion = original_handle_completion
upstream = stream()
print("GOOSE_TEST " + json.dumps({"streamed": streamed, "upstream": upstream, "expected": EXPECTED,
                                   "command": COMMAND}))
"#;
        let program = format!(
            "{}{}{}{}{}{}{}{}{}\
             class _Group:\n    def rank(self): return 0\n    def size(self): return 2\n\
             group = _Group()\n{}QWEN38 = {qwen}\n{checks}",
            include_str!("rank_load_lock.py"),
            include_str!("rank_env.py"),
            include_str!("rank_live.py"),
            include_str!("rank_thinking.py"),
            include_str!("rank_budget.py"),
            include_str!("rank_prefill.py"),
            include_str!("rank_batch.py"),
            include_str!("rank_state.py"),
            include_str!("rank_tool_stream.py"),
            &wrapper[start..end],
            qwen = serde_json::to_string(QWEN38).unwrap(),
        );
        let seen = run_against_real_packages(&python, &program, &spec);
        let expected = seen["expected"].as_str().unwrap();
        let streamed = &seen["streamed"];
        assert_eq!(
            streamed["seen_while_written"], true,
            "the client read the call's arguments while the model was still writing them"
        );
        assert!(
            streamed["arg_frames"].as_u64().unwrap() > 50,
            "argument fragments arrive as they are written: {streamed}"
        );
        assert_eq!(streamed["text"], "I'll write it.\n");
        assert_eq!(streamed["finish"], "tool_calls");
        let calls = streamed["calls"].as_array().unwrap();
        assert_eq!(calls.len(), 1, "{streamed}");
        assert_eq!(calls[0]["names"], serde_json::json!(["shell"]));
        assert_eq!(
            calls[0]["ids"].as_array().unwrap().len(),
            1,
            "one open frame"
        );
        assert_eq!(
            calls[0]["arguments"], expected,
            "the call assembled from the fragments is the parser's own"
        );
        let arguments: serde_json::Value =
            serde_json::from_str(calls[0]["arguments"].as_str().unwrap()).unwrap();
        assert_eq!(arguments["command"], seen["command"]);

        let upstream = &seen["upstream"];
        assert_eq!(
            upstream["seen_while_written"], false,
            "unpatched mlx_lm sends nothing while the call is written"
        );
        assert_eq!(
            upstream["arg_frames"], 1,
            "then the whole call in one frame"
        );
        assert_eq!(upstream["calls"][0]["arguments"], expected);
        assert_eq!(upstream["text"], streamed["text"]);
        assert_eq!(upstream["finish"], streamed["finish"]);
    }

    /// The tensor wrapper's own module prelude — its imports and both upstream-attribute checks,
    /// exactly as shipped — run against the REAL mlx_lm 0.31.3. The stubbed tests never executed
    /// these lines, so 3.0.44 shipped `import mlx_lm.generate as mlx_generate`, which binds the
    /// re-exported `generate` function, and every split died at startup.
    #[test]
    fn the_wrapper_prelude_binds_real_mlx_lm_modules() {
        let Some(python) = proven_env(&EnvSpec::tensor(), "GOOSE_TEST_TENSOR_PYTHON") else {
            return;
        };
        let wrapper = include_str!("rank_wrapper.py");
        let start = wrapper
            .find("import mlx_lm  # noqa")
            .expect("the wrapper imports mlx_lm");
        let end = start
            + wrapper[start..]
                .find("\nserved = spec[")
                .expect("the prelude ends where the spec is read");
        let program = format!(
            "import types\nfrom mlx_lm.models.cache import ArraysCache, BatchKVCache\n\
             spec = {{\"prefill\": {{}}, \"prompt_cache_limit_bytes\": 1}}\n{}\n\
             assert isinstance(mlx_generate, types.ModuleType), mlx_generate\n\
             assert isinstance(server, types.ModuleType), server\nprint(\"ok\")\n",
            &wrapper[start..end]
        );
        let out = std::process::Command::new(&python)
            .arg("-c")
            .arg(program)
            .output()
            .unwrap();
        assert!(
            out.status.success() && String::from_utf8_lossy(&out.stdout).contains("ok"),
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// Q-114's cause against the REAL mlx_lm 0.31.3 and MLX 0.32.2 allocator: the 27B's decode
    /// step shape — mlx_lm's own `_make_cache` over 48 linear-attention caches, each advanced one
    /// token a step, only the first one's counter read (as `create_ssm_mask` reads
    /// `cache[ssm_idx]`). Upstream's `advance` pins one Metal buffer per unread counter per step
    /// until MLX refuses an allocation at its `resource_limit` (the rank's
    /// `[metal::malloc] Resource limit (499000) exceeded`, E2E #3b at 10,447 tokens); with the
    /// wrapper's `advance` + `settle_counters` per step the same steps run past that point.
    /// The step count is derived from the device's own limit, not typed.
    #[test]
    fn a_step_settles_the_counters_it_advanced_so_metal_resources_stay_flat() {
        let Some(python) = proven_env(&EnvSpec::tensor(), "GOOSE_TEST_TENSOR_PYTHON") else {
            return;
        };
        let wrapper = include_str!("rank_wrapper.py");
        let start = wrapper
            .find("upstream_advance = ArraysCache.advance")
            .expect("the wrapper keeps upstream's advance");
        let end = start
            + wrapper[start..]
                .find("ArraysCache.advance = advance\n")
                .expect("the wrapper installs its advance")
            + "ArraysCache.advance = advance\n".len();
        let checks = r#"
import json
import types
mx.set_default_device(mx.cpu)
from mlx_lm.generate import _make_cache

LAYERS = 48
limit = int(mx.device_info(mx.gpu)["resource_limit"])

def caches():
    model = types.SimpleNamespace(make_cache=lambda: [ArraysCache(2) for _ in range(LAYERS)])
    return _make_cache(model, [0], None)

def run(steps, advance_one, after_step):
    layers = caches()
    for step in range(1, steps + 1):
        mx.eval(layers[0].make_mask(1))
        for layer in layers:
            advance_one(layer)
        after_step()
    return [int(layers[0].left_padding.item()), int(layers[-1].left_padding.item())]

# Past the step at which upstream exhausts the device's resources, with margin.
steps = limit // (LAYERS - 1) + limit // (10 * (LAYERS - 1))
settled = run(steps, lambda layer: layer.advance(1), settle_counters)
threw_at = None
try:
    run(steps, lambda layer: upstream_advance(layer, 1), lambda: None)
except RuntimeError as refusal:
    threw_at = str(refusal)
print("GOOSE_TEST " + json.dumps({"steps": steps, "limit": limit, "settled": settled, "upstream": threw_at}))
"#;
        let program = format!(
            "import mlx.core as mx\n{}{}{checks}",
            include_str!("rank_batch.py"),
            &wrapper[start..end]
        );
        let out = std::process::Command::new(&python)
            .arg("-c")
            .arg(program)
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success(), "{stdout}{stderr}");
        let seen: serde_json::Value = serde_json::from_str(
            stdout
                .lines()
                .find_map(|l| l.strip_prefix("GOOSE_TEST "))
                .unwrap_or_else(|| panic!("{stdout}{stderr}")),
        )
        .unwrap();
        let steps = seen["steps"].as_i64().unwrap();
        assert_eq!(
            seen["settled"],
            serde_json::json!([-steps, -steps]),
            "every counter still reads its true value after the settled steps: {seen}"
        );
        let upstream = seen["upstream"].as_str().unwrap_or_default();
        assert!(
            upstream.contains("Resource limit"),
            "negative control: upstream's advance exhausts MLX's resources within the same steps: {seen}"
        );
    }

    /// rank_batch.py against the REAL mlx_lm 0.31.3 (goose's provisioned tensor venv, when this
    /// Mac has one): the split that moves every row leaves exactly what upstream's deep copy
    /// leaves — uids, tokens, samplers, caches, offsets, the shared left padding dropped — while
    /// holding the very cache objects; a partial split is upstream's; and a BatchGenerator's shape
    /// counts a queued row at its cached prefix plus its prompt.
    #[test]
    fn the_moving_split_leaves_what_mlx_lms_split_leaves() {
        let Some(python) = proven_env(&EnvSpec::tensor(), "GOOSE_TEST_TENSOR_PYTHON") else {
            return;
        };
        let checks = r#"
import mlx.core as mx

# On the CPU: the cache operations are what is proven here, and a test never waits on the GPU.
mx.set_default_device(mx.cpu)
from mlx_lm.generate import BatchGenerator, PromptProcessingBatch
from mlx_lm.models.cache import BatchKVCache

def row(width, seed):
    mx.random.seed(seed)
    cache = []
    for layer in range(4):
        if layer % 2:
            c = KVCache()
            c.keys = mx.random.normal((1, 2, width, 8)).astype(mx.bfloat16)
            c.values = mx.random.normal((1, 2, width, 8)).astype(mx.bfloat16)
            c.offset = width
        else:
            c = ArraysCache(2)
            c[0] = mx.random.normal((1, 3, 16))
            c[1] = mx.random.normal((1, 2, 4, 4))
        cache.append(c)
    return cache

def batch(widths, seed):
    return PromptProcessingBatch(None, list(range(len(widths))), [row(w, seed + i) for i, w in enumerate(widths)],
                                 tokens=[[7] * w for w in widths], max_tokens=[5] * len(widths))

def arrays(caches):
    out = []
    for c in caches:
        state = c.state if isinstance(c.state, (list, tuple)) else [c.state]
        out += [x for x in state if isinstance(x, mx.array)]
        if isinstance(c, BatchKVCache):
            out += [c.offset, c.left_padding, mx.array(c._idx)]
    return out

def same(a, b):
    fields = ("uids", "tokens", "samplers", "logits_processors", "max_tokens")
    assert all(getattr(a, f) == getattr(b, f) for f in fields), [(f, getattr(a, f), getattr(b, f)) for f in fields]
    xs, ys = arrays(a.prompt_cache), arrays(b.prompt_cache)
    assert len(xs) == len(ys) and all(x.shape == y.shape and mx.array_equal(x, y).item() for x, y in zip(xs, ys))

for widths, leave in (([64], [0]), ([64, 8, 8], [0, 1, 2]), ([64, 32, 8], [1]), ([64, 32, 8], [0, 2])):
    up, ours = batch(widths, 3), batch(widths, 3)
    held = [id(c) for c in ours.prompt_cache]
    up_left, ours_left = PromptProcessingBatch.split(up, leave), moving_split(ours, leave, PromptProcessingBatch.split)
    same(up, ours)
    same(up_left, ours_left)
    if len(leave) == len(widths):
        assert [id(c) for c in ours_left.prompt_cache] == held, "every row left: the caches moved"
        assert ours.prompt_cache == [] and ours.uids == []

# Rows sharing left padding (the shift upstream's filter makes), then every row leaves.
up, ours = batch([64, 40], 5), batch([64, 40], 5)
for b in (up, ours):
    for c in b.prompt_cache:
        if isinstance(c, BatchKVCache):
            c.keys = mx.pad(c.keys, [(0, 0), (0, 0), (3, 0), (0, 0)])
            c.values = mx.pad(c.values, [(0, 0), (0, 0), (3, 0), (0, 0)])
            c.left_padding = c.left_padding + 3
            c._idx += 3
same(PromptProcessingBatch.split(up, [0, 1]), moving_split(ours, [0, 1], PromptProcessingBatch.split))

# BatchGenerator's constructor reads Metal's working set; its queue is what is read here, filled
# by its own insert_segments.
from collections import deque
from mlx_lm.generate import GenerationBatch, SequenceStateMachine
generator = BatchGenerator.__new__(BatchGenerator)
generator._old_wired_limit = None
generator.max_tokens, generator.logits_processors, generator._uid_count = 128, [], 0
generator._default_state_machine = SequenceStateMachine({}, initial="normal")
generator._unprocessed_sequences, generator._currently_processing = deque(), []
generator._prompt_batch = PromptProcessingBatch.empty(None, None)
generator._generation_batch = GenerationBatch.empty(None, None)
generator.insert_segments(segments=[[[1] * 100]], caches=[row(64, 9)], all_tokens=[[1] * 64], max_tokens=[5])
generator.insert_segments(segments=[[[1] * 10]], caches=[row(8, 11)], all_tokens=[[1] * 8], max_tokens=[5])
assert batch_shape(generator) == (2, 164), batch_shape(generator)
print("ok")
"#;
        let out = std::process::Command::new(&python)
            .arg("-c")
            .arg(format!(
                "{}{}{checks}",
                include_str!("rank_prefill.py"),
                include_str!("rank_batch.py")
            ))
            .output()
            .expect("the tensor venv's python runs");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "ok");
    }

    /// The tensor program a rank receives carries the budget ahead of the wrapper that calls it,
    /// and the spec hands it the launch's window under the key the wrapper reads
    /// (`spec["context_window"]`).
    #[test]
    fn the_tensor_program_carries_the_budget_and_the_launch_window() {
        let config = two_mac_config();
        let specs = rank_specs(
            &config,
            &ServedNames::only("node-alias"),
            &[launch(1, 2), launch(3, 2)],
            262_144,
            2.0,
        );
        let args = python_args(&specs[1]).unwrap();
        let b64 = base64::engine::general_purpose::STANDARD;
        let program = String::from_utf8(b64.decode(&args[2]).unwrap()).unwrap();
        assert_eq!(
            program,
            concat!(
                include_str!("rank_load_lock.py"),
                include_str!("rank_env.py"),
                include_str!("rank_formation.py"),
                include_str!("rank_live.py"),
                include_str!("rank_thinking.py"),
                include_str!("rank_budget.py"),
                include_str!("rank_prefill.py"),
                include_str!("rank_batch.py"),
                include_str!("rank_state.py"),
                include_str!("rank_tool_stream.py"),
                include_str!("rank_wrapper.py")
            )
        );
        let spec: serde_json::Value =
            serde_json::from_slice(&b64.decode(&args[3]).unwrap()).unwrap();
        assert_eq!(spec["context_window"], 262_144);
    }

    /// Q-66's doorbell needs every rank's wrapper to take part, so the spec says so twice: the
    /// `doorbell` flag the wrapper reads, and a program tag an older peer's goosed cannot parse
    /// (it refuses at rank start instead of joining a launch it would hang).
    #[test]
    fn the_tensor_spec_asks_for_the_doorbell_and_an_older_peer_refuses_it() {
        let config = two_mac_config();
        let spec = rank_specs(
            &config,
            &ServedNames::only("node-alias"),
            &[launch(1, 2), launch(3, 2)],
            8_192,
            2.0,
        )
        .remove(1);
        let json = serde_json::to_value(&spec).unwrap();
        assert_eq!(json["program"], "mlxLmServerFormation");
        assert_eq!(json["formation"]["rounds"], FORMATION_ROUNDS);

        // A Q-104 requester's spec (the prefill tag, no formation) forms no group here: its own
        // ranks run no handshake either.
        let mut prefill = json.clone();
        prefill["program"] = "mlxLmServerPrefill".into();
        prefill.as_object_mut().unwrap().remove("formation");
        let read: RankSpec = serde_json::from_value(prefill).unwrap();
        assert_eq!(read.formation, None);
        assert!(matches!(
            read.program,
            RankProgram::MlxLmServer {
                prefill: Some(_),
                ..
            }
        ));

        // A Q-104 peer's goosed (its enum knows the prefill tag, not the formation one) refuses
        // this spec: its wrapper would skip the handshake the other ranks wait in.
        #[derive(Debug, Deserialize)]
        #[serde(tag = "program", rename_all = "camelCase")]
        #[allow(dead_code)]
        enum PrefillProgram {
            #[serde(
                rename = "mlxLmServerPrefill",
                alias = "mlxLmServerBounded",
                alias = "mlxLmServerDoorbell",
                alias = "mlxLmServer"
            )]
            MlxLmServer {
                planned_bytes: u64,
            },
            PipelineServe {
                serve_args: Vec<String>,
            },
        }
        let refused = serde_json::from_value::<PrefillProgram>(json.clone()).unwrap_err();
        let why = link_control_refusal(&refused.to_string());
        assert!(why.is_some_and(|w| w.contains("update goose")), "{refused}");
        assert_eq!(json["doorbell"], true);
        assert_eq!(json["prompt_cache_live_bound"], true);
        assert_eq!(json["prefill"]["step"], 2_048);
        assert_eq!(json["prefill"]["workspace_bytes"], 2_048u64 * 262_144 * 25);

        // A Q-79 requester's spec (the bounded tag, no prefill plan) runs upstream chunking here.
        let mut bounded = json.clone();
        bounded["program"] = "mlxLmServerBounded".into();
        bounded.as_object_mut().unwrap().remove("prefill");
        let read: RankSpec = serde_json::from_value(bounded).unwrap();
        assert!(matches!(
            read.program,
            RankProgram::MlxLmServer {
                prompt_cache_live_bound: true,
                prefill: None,
                ..
            }
        ));

        // A Q-79 peer's goosed (its enum knows the bounded tag, not the prefill one) refuses
        // this spec: its wrapper would chunk at mlx_lm's fixed step while this Mac's shrinks it.
        #[derive(Debug, Deserialize)]
        #[serde(tag = "program", rename_all = "camelCase")]
        #[allow(dead_code)]
        enum BoundedProgram {
            #[serde(
                rename = "mlxLmServerBounded",
                alias = "mlxLmServerDoorbell",
                alias = "mlxLmServer"
            )]
            MlxLmServer {
                planned_bytes: u64,
            },
            PipelineServe {
                serve_args: Vec<String>,
            },
        }
        let refused = serde_json::from_value::<BoundedProgram>(json.clone()).unwrap_err();
        let why = link_control_refusal(&refused.to_string());
        assert!(why.is_some_and(|w| w.contains("update goose")), "{refused}");

        // A Q-66..Q-73 requester's spec (the doorbell tag, no live bound, no MLX cache figure)
        // runs that requester's own eviction policy here.
        let mut doorbell = json.clone();
        doorbell["program"] = "mlxLmServerDoorbell".into();
        let fields = doorbell.as_object_mut().unwrap();
        fields.remove("prompt_cache_live_bound");
        fields.remove("mlx_cache_limit_bytes");
        let read: RankSpec = serde_json::from_value(doorbell).unwrap();
        assert!(matches!(
            read.program,
            RankProgram::MlxLmServer {
                doorbell: true,
                prompt_cache_live_bound: false,
                mlx_cache_limit_bytes: None,
                ..
            }
        ));

        // A Q-66..Q-73 peer's goosed (its enum knows the doorbell tag, not the bounded one)
        // refuses this spec, and the requester says what to do about it.
        #[derive(Debug, Deserialize)]
        #[serde(tag = "program", rename_all = "camelCase")]
        #[allow(dead_code)]
        enum DoorbellProgram {
            #[serde(rename = "mlxLmServerDoorbell", alias = "mlxLmServer")]
            MlxLmServer {
                planned_bytes: u64,
            },
            PipelineServe {
                serve_args: Vec<String>,
            },
        }
        let refused = serde_json::from_value::<DoorbellProgram>(json.clone()).unwrap_err();
        let why = link_control_refusal(&refused.to_string());
        assert!(why.is_some_and(|w| w.contains("update goose")), "{refused}");

        // An older requester's spec (no doorbell, the old tag, its one prompt cache number)
        // still starts a rank here, waiting the upstream way.
        let mut older = json.clone();
        older["program"] = "mlxLmServer".into();
        let fields = older.as_object_mut().unwrap();
        fields.remove("doorbell");
        fields.remove("prompt_cache_limit_bytes");
        fields.remove("prompt_cache_entries");
        fields.insert("prompt_cache_bytes".into(), 4_715_872_256u64.into());
        let read: RankSpec = serde_json::from_value(older).unwrap();
        assert!(matches!(
            read.program,
            RankProgram::MlxLmServer {
                doorbell: false,
                prompt_cache_bytes: Some(4_715_872_256),
                prompt_cache_limit_bytes: None,
                prompt_cache_entries: None,
                ..
            }
        ));

        // An older peer's goosed (its enum has no doorbell tag) cannot read this spec, and the
        // requester says what to do about it.
        #[derive(Debug, Deserialize)]
        #[serde(tag = "program", rename_all = "camelCase")]
        #[allow(dead_code)]
        enum OlderProgram {
            MlxLmServer {
                context_window: u64,
                prompt_cache_bytes: u64,
                planned_bytes: u64,
            },
            PipelineServe {
                serve_args: Vec<String>,
            },
        }
        let refused = serde_json::from_value::<OlderProgram>(json).unwrap_err();
        let why = link_control_refusal(&refused.to_string());
        assert!(why.is_some_and(|w| w.contains("update goose")), "{refused}");
    }

    fn link_control_refusal(error: &str) -> Option<&'static str> {
        super::super::link_control::older_peer_refusal(error)
    }

    /// The REAL tensor program, booted as a rank is (doorbell off: the stand-in has no peer),
    /// against stand-in `mlx.core` and `mlx_lm` modules whose `run` builds its prompt cache
    /// exactly as mlx_lm 0.31.3's does (`LRUPromptCache(cli_args.prompt_cache_size)`,
    /// server.py:1743). Returns what that `run` saw — argv, the cache's max_size / max_bytes, the
    /// trims an insert made beside a live batch of 7 bytes — plus the rank's `caps`, its `fatal`
    /// line and exit `code`. `generation_dies`: the stand-in generation loop raises a Metal OOM.
    #[cfg(target_os = "macos")]
    async fn boot_against_stand_ins(
        mut spec: RankSpec,
        generation_dies: bool,
    ) -> serde_json::Value {
        let root = tempfile::tempdir().unwrap();
        let site = root.path();
        std::fs::create_dir_all(site.join("mlx")).unwrap();
        std::fs::create_dir_all(site.join("mlx_lm")).unwrap();
        std::fs::write(site.join("mlx/__init__.py"), "").unwrap();
        std::fs::write(
            site.join("mlx/core.py"),
            "import os\n\
             __version__ = '0.32.2'\n\
             class _G:\n\
             \x20   def rank(self): return int(os.environ['MLX_RANK'])\n\
             \x20   def size(self): return 2\n\
             class _D:\n\
             \x20   @staticmethod\n\
             \x20   def init(strict=False, backend='any'): return _G()\n\
             distributed = _D()\n\
             def device_info(): return {'memory_size': 128, 'max_recommended_working_set_size': 100}\n\
             def set_memory_limit(n): pass\n\
             def set_wired_limit(n): pass\n\
             def set_cache_limit(n): pass\n\
             def get_active_memory(): return 1\n\
             def get_peak_memory(): return 1\n\
             def get_cache_memory(): return 0\n\
             def clear_cache(): pass\n\
             class array: pass\n\
             def contiguous(x): return x\n\
             def eval(*arrays): pass\n\
             def async_eval(*arrays): pass\n",
        )
        .unwrap();
        std::fs::write(site.join("mlx_lm/__init__.py"), "__version__ = '0.31.3'\n").unwrap();
        std::fs::create_dir_all(site.join("mlx_lm/models")).unwrap();
        std::fs::write(site.join("mlx_lm/models/__init__.py"), "").unwrap();
        std::fs::write(
            site.join("mlx_lm/models/cache.py"),
            "class KVCache: pass\n\
             class ArraysCache:\n\
             \x20   def advance(self, N): pass\n\
             class BatchKVCache:\n\
             \x20   step = 256\n",
        )
        .unwrap();
        std::fs::create_dir_all(site.join("mlx_lm/tool_parsers")).unwrap();
        std::fs::write(site.join("mlx_lm/tool_parsers/__init__.py"), "").unwrap();
        std::fs::write(
            site.join("mlx_lm/tool_parsers/qwen3_coder.py"),
            "def parse_tool_call(model_output, tools=None): pass\n\
             def _convert_param_value(param_value, param_name, param_config): pass\n\
             def _get_arguments_config(func_name, tools): pass\n",
        )
        .unwrap();
        std::fs::write(
            site.join("mlx_lm/generate.py"),
            "class PromptProcessingBatch:\n\
             \x20   def prompt(self, tokens): pass\n\
             \x20   def split(self, indices): pass\n\
             \x20   def filter(self, keep): pass\n",
        )
        .unwrap();
        std::fs::write(
            site.join("mlx_lm/server.py"),
            "import argparse, json, os, sys\n\
             class LRUPromptCache:\n\
             \x20   def __init__(self, max_size=10, max_bytes=1 << 63):\n\
             \x20       self.max_size, self.max_bytes, self.trims = max_size, max_bytes, []\n\
             \x20   def insert_cache(self, model, tokens, prompt_cache, *, cache_type='assistant'): pass\n\
             \x20   def trim_to(self, *, n_sequences=None, n_bytes=None): self.trims.append(n_bytes)\n\
             class _Rows(list):\n\
             \x20   prompt_cache = []\n\
             class BatchGenerator:\n\
             \x20   prompt_cache_nbytes = 7\n\
             \x20   def __init__(self):\n\
             \x20       self._generation_batch, self._prompt_batch = _Rows(), _Rows()\n\
             \x20       self._unprocessed_sequences, self._currently_processing = [], []\n\
             \x20   def close(self): pass\n\
             \x20   def remove(self, uids): pass\n\
             \x20   def next(self): return [], []\n\
             class ResponseGenerator:\n\
             \x20   def _next_request(self, timeout=None): pass\n\
             \x20   def generate(self, request, args, progress_callback=None): pass\n\
             \x20   def _tokenize(self, tokenizer, request, args): pass\n\
             \x20   def _share_request(self, request): pass\n\
             \x20   def _generate(self):\n\
             \x20       if os.environ.get('STANDIN_GENERATION_DIES'):\n\
             \x20           raise RuntimeError('[METAL] Command buffer execution failed: Insufficient Memory (00000008:kIOGPUCommandBufferCallbackErrorOutOfMemory)')\n\
             class APIHandler:\n\
             \x20   def do_GET(self): pass\n\
             \x20   def do_POST(self): pass\n\
             \x20   def validate_model_parameters(self): pass\n\
             \x20   def _set_completion_headers(self, status): pass\n\
             \x20   def handle_completion(self, request, stop_words): pass\n\
             \x20   def generate_response(self, text, finish_reason, **kwargs): pass\n\
             class ModelProvider:\n\
             \x20   def __init__(self, cli_args): self.cli_args, self._model_map = cli_args, {}\n\
             \x20   def load(self, *a): pass\n\
             def run(host, port, model_provider):\n\
             \x20   cache = LRUPromptCache(model_provider.cli_args.prompt_cache_size)\n\
             \x20   BatchGenerator()\n\
             \x20   cache.insert_cache('model', [1, 2], [], cache_type='user')\n\
             \x20   print('GOOSE_STANDIN ' + json.dumps({'argv': sys.argv[1:], 'max_size': cache.max_size, \
             'max_bytes': cache.max_bytes, 'trims': cache.trims, 'served': model_provider._model_map}), flush=True)\n\
             \x20   ResponseGenerator()._generate()\n\
             def main():\n\
             \x20   p = argparse.ArgumentParser()\n\
             \x20   p.add_argument('--model'); p.add_argument('--host'); p.add_argument('--port', type=int)\n\
             \x20   p.add_argument('--prompt-cache-size', type=int, default=10)\n\
             \x20   p.add_argument('--prompt-cache-bytes', type=int)\n\
             \x20   p.add_argument('--prefill-step-size', type=int)\n\
             \x20   args = p.parse_args()\n\
             \x20   run(args.host, args.port, ModelProvider(args))\n",
        )
        .unwrap();
        spec.memory_report_seconds = 0.05;
        if let RankProgram::MlxLmServer { doorbell, .. } = &mut spec.program {
            *doorbell = false;
        }
        spec.formation = None;
        let mut command = tokio::process::Command::new("/usr/bin/python3");
        command
            .args(python_args(&spec).unwrap())
            .env("PYTHONPATH", site)
            .kill_on_drop(true);
        if generation_dies {
            command.env("STANDIN_GENERATION_DIES", "1");
        }
        let out = command.output().await.unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert_eq!(
            out.status.success(),
            !generation_dies,
            "{stdout}{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let line = |tag: &str| {
            stdout
                .lines()
                .find_map(|l| l.strip_prefix(tag))
                .map(|json| serde_json::from_str::<serde_json::Value>(json).unwrap())
        };
        let mut served = line("GOOSE_STANDIN ").unwrap_or_else(|| panic!("{stdout}"));
        served["caps"] = line("GOOSE_RANK_CAPS ").unwrap_or_else(|| panic!("{stdout}"));
        served["fatal"] = line("GOOSE_RANK_FATAL ").unwrap_or_default();
        served["code"] = out.status.code().into();
        served
    }

    #[cfg(target_os = "macos")]
    fn flag(served: &serde_json::Value, name: &str) -> Option<String> {
        let argv: Vec<String> = serde_json::from_value(served["argv"].clone()).unwrap();
        let at = argv.iter().position(|a| a == name)?;
        Some(argv[at + 1].clone())
    }

    /// The server receives both flags from the spec (E2E #1's figures), and the cache it builds is
    /// bounded by the same bytes, which mlx_lm itself never passes.
    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn the_tensor_program_hands_mlx_lm_both_prompt_cache_bounds() {
        let config = two_mac_config();
        let e2e = TensorLaunch {
            planned_bytes: 27_456_216_576,
            prompt_cache_limit_bytes: 9_431_744_512,
            prompt_cache_entries: 110,
            mlx_cache_limit_bytes: 2_745_621_658,
            prefill: e2e_prefill(),
        };
        let spec = rank_specs(
            &config,
            &ServedNames::only("node-alias"),
            &[e2e, e2e],
            141_568,
            2.0,
        )
        .remove(1);
        let served = boot_against_stand_ins(spec, false).await;
        assert_eq!(
            served["caps"]["cache_limit"], 2_745_621_658u64,
            "MLX's free-buffer cache holds the plan's transient allowance, not the ceiling's rest"
        );
        assert_eq!(
            served["trims"],
            serde_json::json!([9_431_744_512u64 - 7]),
            "an insert beside a live batch keeps cached + live inside the plan's KV charge"
        );
        assert_eq!(flag(&served, "--prompt-cache-size").as_deref(), Some("110"));
        assert_eq!(
            flag(&served, "--prefill-step-size").as_deref(),
            Some("2048"),
            "mlx_lm runs the chunk the plan charged, whatever its own default becomes"
        );
        assert_eq!(
            flag(&served, "--prompt-cache-bytes").as_deref(),
            Some("9431744512")
        );
        assert_eq!(served["max_size"], 110);
        assert_eq!(
            served["max_bytes"], 9_431_744_512u64,
            "the cache mlx_lm builds is bounded between admissions too"
        );
        assert_eq!(
            served["served"]["node-alias"],
            config.nodes[1].model_dir.as_str()
        );
    }

    /// An older requester's spec runs the policy its own rank 0 runs — its one number as the flag,
    /// mlx_lm's default count, no max_bytes — so the two ranks evict alike.
    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn an_older_requesters_spec_runs_its_own_prompt_cache_policy() {
        let config = two_mac_config();
        let mut spec = rank_specs(
            &config,
            &ServedNames::only("node-alias"),
            &[launch(1, 1), launch(1, 1)],
            8_192,
            2.0,
        )
        .remove(1);
        if let RankProgram::MlxLmServer {
            prompt_cache_limit_bytes,
            prompt_cache_entries,
            prompt_cache_bytes,
            mlx_cache_limit_bytes,
            prompt_cache_live_bound,
            prefill,
            ..
        } = &mut spec.program
        {
            *prompt_cache_limit_bytes = None;
            *prompt_cache_entries = None;
            *prompt_cache_bytes = Some(4_715_872_256);
            *mlx_cache_limit_bytes = None;
            *prompt_cache_live_bound = false;
            *prefill = None;
        }
        let served = boot_against_stand_ins(spec, false).await;
        assert_eq!(
            flag(&served, "--prefill-step-size"),
            None,
            "that requester chunks at mlx_lm's own step, so this rank does too"
        );
        assert_eq!(
            served["caps"]["cache_limit"], 99,
            "that requester's rule: the ceiling (100) less the planned bytes (1)"
        );
        assert_eq!(served["trims"], serde_json::json!([]));
        assert_eq!(
            flag(&served, "--prompt-cache-bytes").as_deref(),
            Some("4715872256")
        );
        assert_eq!(flag(&served, "--prompt-cache-size"), None);
        assert_eq!(served["max_size"], 10);
        assert_eq!(served["max_bytes"], serde_json::json!(1u64 << 63));
    }

    /// E2E #2's Studio rank: mlx_lm's generation thread raised a Metal OOM, the worker's main
    /// thread only joins it, and the rank exited 0. Now the death is named and the exit is not a
    /// success.
    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn a_generation_thread_that_dies_ends_the_rank_named_and_non_zero() {
        let config = two_mac_config();
        let spec = rank_specs(
            &config,
            &ServedNames::only("node-alias"),
            &[launch(1, 1), launch(1, 1)],
            8_192,
            2.0,
        )
        .remove(1);
        let served = boot_against_stand_ins(spec, true).await;
        assert_eq!(served["code"], 70);
        assert_eq!(served["fatal"]["thread"], "generation");
        assert_eq!(served["fatal"]["out_of_memory"], true);
        assert!(
            served["fatal"]["error"]
                .as_str()
                .is_some_and(|e| e.contains("kIOGPUCommandBufferCallbackErrorOutOfMemory")),
            "{}",
            served["fatal"]
        );
    }

    /// The phases a pipeline rank walks, from its own lines (the 27B's figures as a reporter thread
    /// read them mid-`mx.eval`, 2026-09-24): loading until RANK_CAPS, warming until READY.
    #[test]
    fn a_ranks_phase_is_its_own_reports() {
        let mut live = RankLive::default();
        assert_eq!(live.phase(), RankPhase::Loading);
        live.take_line("GOOSE_RANK_GROUP {\"rank\": 1, \"size\": 2}");
        live.take_line(
            "GOOSE_RANK_MEM {\"active\": 21861391624, \"peak\": 21861391624, \"cache\": 0}",
        );
        assert_eq!(live.phase(), RankPhase::Loading);
        assert_eq!(live.memory.map(|m| m.active), Some(21_861_391_624));
        live.take_line("GOOSE_RANK_CAPS {\"memory_limit\": 1, \"planned\": 2}");
        assert_eq!(live.phase(), RankPhase::Warming);
        live.take_line("GOOSE_READY {\"rank\": 1, \"pid\": 7, \"layers\": [20, 48]}");
        assert_eq!(live.phase(), RankPhase::Ready);
    }

    /// rank_state.py under a real interpreter: a row's token trail checkpoints at every power of
    /// two, so two ranks' trails bracket where their samples diverged; a published snapshot never
    /// changes after the reporter may hold it; a row that ended — sampled to a stop, or removed —
    /// stays visible as `ended`.
    #[test]
    fn a_ranks_loop_state_says_where_it_is_and_what_it_sampled() {
        let checks = r#"
import json
t, u = TokenTrail(), TokenTrail()
for token in (5, 7, 9, 11, 13):
    t.fold(token)
for token in (5, 7, 9, 12, 13):
    u.fold(token)
assert t.generated == 5 and sorted(t.checkpoints) == ["1", "2", "4"], t.checkpoints
assert t.checkpoints["2"] == u.checkpoints["2"], "the same first two tokens, the same checkpoint"
assert t.checkpoints["4"] != u.checkpoints["4"], "the fourth token differs: the bracket is (2, 4]"
loop = LoopState(1)
loop.fold(0, 5, None)
loop.publish("batch")
first = published_state[0]
loop.fold(0, 7, "stop")
loop.steps = 2
loop.publish("idle")
assert first["trails"][0]["generated"] == 1 and first["at"] == "batch", first
now = published_state[0]
assert now["trails"] == [] and now["ended"]["how"] == "stop" and now["ended"]["generated"] == 2, now
loop.new_batch()
loop.fold(0, 3, None)
loop.drop([0])
loop.publish("doorbell")
assert published_state[0]["ended"]["how"] == "removed" and published_state[0]["at"] == "doorbell"
json.dumps(published_state[0])
print("ok")
"#;
        let out = std::process::Command::new("/usr/bin/python3")
            .arg("-c")
            .arg(format!(
                "published_state = [None]\n{}{checks}",
                include_str!("rank_state.py")
            ))
            .output()
            .expect("/usr/bin/python3 runs the rank programs' pure half");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "ok");
    }

    async fn wait_for<T>(what: &str, mut probe: impl FnMut() -> Option<T>) -> T {
        for _ in 0..600 {
            if let Some(found) = probe() {
                return found;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        panic!("never saw {what}");
    }

    /// The drain never stops before EOF: a line that is not UTF-8 is read lossily. The old
    /// `lines()` loop ended at the first such line, and a rank whose pipe nobody drains blocks on
    /// its next write at 0% CPU — Q-114's rank 1 state. Every line also lands in the durable log,
    /// the memory and state reports the tail leaves out included, tagged with its stream.
    #[tokio::test]
    async fn a_rank_line_that_is_not_utf8_never_ends_the_drain() {
        let mut child = tokio::process::Command::new("/bin/sh")
            .args([
                "-c",
                r#"printf 'first\n\377\376 torn\nGOOSE_RANK_STATE {"steps": 7, "at": "doorbell"}\n'; printf 'on stderr\n' >&2"#,
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let live = Arc::new(StdMutex::new(RankLive::default()));
        drain_rank_output(&mut child, 1, "studio", &live);
        assert!(child.wait().await.unwrap().success());
        wait_for("four lines", || {
            (live.lock().unwrap().lines == 4).then_some(())
        })
        .await;
        let live = live.lock().unwrap();
        assert_eq!(live.state.as_ref().unwrap()["steps"], 7);
        let tail = live.tail_text();
        assert!(
            tail.contains("first") && tail.contains("\u{FFFD}\u{FFFD} torn"),
            "{tail}"
        );
        assert!(
            tail.contains("on stderr") && !tail.contains("GOOSE_RANK_STATE"),
            "{tail}"
        );
        let log = std::fs::read_to_string(live.log.as_deref().unwrap()).unwrap();
        assert_eq!(log.lines().count(), 4, "{log}");
        assert!(log.contains(" out GOOSE_RANK_STATE {\"steps\": 7"), "{log}");
        assert!(log.contains(" err on stderr"), "{log}");
    }

    /// Q-114's missing evidence, reproduced on the REAL tensor wrapper against the real mlx_lm
    /// 0.31.3 (CPU, no model, MLX's group the one stand-in): a worker rank takes three steps while
    /// a batch runs, then its loop asks with a timeout — the batch is over on this rank — and it
    /// parks on the doorbell of a rank 0 that never rings. goosed's drain keeps its output in the
    /// durable log; the rank's last GOOSE_RANK_STATE there says it is parked at the doorbell after
    /// step 3 with no ring received, and the SIGTERM goosed sends a hung pair leaves every thread's
    /// stack there too — Doorbell.wait under _next_request — before the rank dies of the signal as
    /// before. On 3.0.45 the same stall left nothing: no state line, no stack, no file.
    #[tokio::test]
    async fn a_parked_worker_leaves_its_step_and_doorbell_state_in_its_durable_log() {
        let Some(python) = proven_env(&EnvSpec::tensor(), "GOOSE_TEST_TENSOR_PYTHON") else {
            return;
        };
        let config = two_mac_config();
        let mut spec = rank_specs(
            &config,
            &ServedNames::only("node-alias"),
            &[launch(1, 1), launch(1, 1)],
            8_192,
            2.0,
        )
        .remove(1);
        spec.memory_report_seconds = 0.05;
        let wrapper = include_str!("rank_wrapper.py");
        let start = wrapper
            .find("import faulthandler  # noqa")
            .expect("the wrapper's body starts after the group check");
        let end = wrapper
            .find("server.main()")
            .expect("the wrapper ends in mlx_lm's main");
        let prelude = r#"
import socket
mx.set_default_device(mx.cpu)

class _Group:
    def rank(self): return 1
    def size(self): return 2

group = _Group()
# MLX's own collectives run in a group of one here (no JACCL devices): an all_sum is the identity.
os.environ.pop("MLX_IBV_DEVICES")
os.environ.pop("MLX_RANK")
# Rank 0's side of the doorbell, played here: it accepts the worker and never rings.
rank0 = socket.create_server(("127.0.0.1", 0))
os.environ["MLX_JACCL_COORDINATOR"] = "127.0.0.1:1"
peers = []
threading.Thread(target=lambda: peers.append(rank0.accept()), daemon=True).start()
real_all_sum = mx.distributed.all_sum
mx.distributed.all_sum = lambda x, *a, **k: real_all_sum(x, *a, **k) + rank0.getsockname()[1]
"#;
        let steps = r#"
mx.distributed.all_sum = real_all_sum
assert doorbell is not None and doorbell.link is not None
responses = server.ResponseGenerator.__new__(server.ResponseGenerator)
responses._is_distributed, responses._rank = True, 1
threading.Thread(target=report_memory, daemon=True).start()
for _ in range(3):
    assert responses._next_request(None) is None
threading.Thread(target=responses._next_request, args=(0.1,), daemon=True).start()
threading.Event().wait()
"#;
        let program = format!(
            "{}{}{}{}{}{}{}{}{}{prelude}{}{steps}",
            include_str!("rank_load_lock.py"),
            include_str!("rank_env.py"),
            include_str!("rank_live.py"),
            include_str!("rank_thinking.py"),
            include_str!("rank_budget.py"),
            include_str!("rank_prefill.py"),
            include_str!("rank_batch.py"),
            include_str!("rank_state.py"),
            include_str!("rank_tool_stream.py"),
            &wrapper[start..end]
        );
        let tmp = tempfile::tempdir().unwrap();
        let mut child = tokio::process::Command::new(&python)
            .arg("-c")
            .arg(program)
            .arg("goose-sidecar-test")
            .arg(
                base64::engine::general_purpose::STANDARD
                    .encode(serde_json::to_vec(&spec).unwrap()),
            )
            .env("TMPDIR", tmp.path())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let live = Arc::new(StdMutex::new(RankLive::default()));
        drain_rank_output(&mut child, 1, "Work’s Mac Studio", &live);
        let parked = wait_for("the worker parked at the doorbell", || {
            let live = live.lock().unwrap();
            live.state
                .clone()
                .filter(|state| state["at"] == "doorbell")
                .or_else(|| {
                    assert!(
                        !live.tail_text().contains("Traceback"),
                        "{}",
                        live.tail_text()
                    );
                    None
                })
        })
        .await;
        assert_eq!(parked["rank"], 1);
        assert_eq!(
            parked["steps"], 3,
            "its own step count: three busy steps, then parked"
        );
        assert_eq!(parked["mode"], "idle", "{parked}");
        assert_eq!(parked["rings"], 0, "no ring ever came");
        let evidence = live.lock().unwrap().evidence();
        assert!(
            evidence.contains("steps 3, at doorbell, mode idle, rows 0, rings 0; log /"),
            "{evidence}"
        );

        let pid = child.id().unwrap() as libc::pid_t;
        // SAFETY: the test's own child, signalled by its pid alone.
        assert_eq!(unsafe { libc::kill(pid, libc::SIGTERM) }, 0);
        let status = child.wait().await.unwrap();
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(
            status.signal(),
            Some(libc::SIGTERM),
            "the rank still dies of the signal"
        );
        let path = live.lock().unwrap().log.clone().unwrap();
        let log = wait_for("the SIGTERM stack dump in the durable log", || {
            let log = std::fs::read_to_string(&path).unwrap();
            log.contains("in _next_request").then_some(log)
        })
        .await;
        assert!(
            log.contains(" err Thread 0x") || log.contains(" err Current thread 0x"),
            "{log}"
        );
        let lines: Vec<&str> = log.lines().collect();
        let caller = lines
            .iter()
            .position(|l| l.ends_with(" in _next_request"))
            .unwrap_or_else(|| panic!("{log}"));
        assert!(
            lines[caller - 1].ends_with(" in wait"),
            "most recent call first: Doorbell.wait under _next_request: {log}"
        );
        let last_state = log
            .lines()
            .filter_map(|l| l.split_once(" out GOOSE_RANK_STATE ").map(|(_, json)| json))
            .next_back()
            .unwrap_or_else(|| panic!("{log}"));
        let last_state: serde_json::Value = serde_json::from_str(last_state).unwrap();
        assert_eq!(
            (&last_state["at"], &last_state["steps"]),
            (&serde_json::json!("doorbell"), &serde_json::json!(3))
        );
    }

    #[test]
    fn rank_output_markers_are_read_and_memory_lines_stay_out_of_the_tail() {
        let mut live = RankLive::default();
        live.take_line("GOOSE_RANK_PID=4242");
        live.take_line("GOOSE_RANK_GROUP {\"rank\": 1, \"size\": 2}");
        live.take_line("GOOSE_RANK_MEM {\"active\": 10, \"peak\": 20, \"cache\": 1}");
        live.take_line("Traceback (most recent call last):");
        assert_eq!(live.pid, Some(4242));
        assert!(live.group_joined);
        assert_eq!(live.memory.unwrap().peak, 20);
        assert!(!live.tail_text().contains("GOOSE_RANK_MEM"));
        assert!(live.tail_text().contains("Traceback"));
    }
}
