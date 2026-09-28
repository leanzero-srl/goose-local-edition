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
//! then the runner's program — `rank_request.py` (the request fields rank 0 refuses by name before
//! any rank sees the request, Q-177) + `rank_sampling.py` (a request's absent sampling fields, resolved as
//! the single engine resolves them, Q-159) + `rank_budget.py` + `rank_prefill.py` + `rank_batch.py` +
//! `rank_state.py` + `rank_prompt_search.py` (the prompt cache's nearest-entry search in linear time, Q-162) +
//! `rank_boundary.py` + `rank_tool_schema.py` (a tool parameter's type read through a union or a
//! reference, Q-232) + `rank_tool_stream.py` + `rank_stream_watch.py` + `rank_xml_guard.py` + `rank_wrapper.py` (`mlx_lm.server`, tensor split, under `NodeConfig::python`; the budget is
//! what an absent max_tokens generates, the prefill modules what a step and a batch may hold, the
//! boundary where a chat request's reusable prefix ends, the XML guard what a tool call may be
//! followed by, Q-161) or `pipeline_rank.py` (the fork's `pipeline_qwen4_serve`,
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
use super::sampling::SamplingDefaults;
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
    include_str!("rank_request.py"),
    include_str!("rank_sampling.py"),
    include_str!("rank_budget.py"),
    include_str!("rank_prefill.py"),
    include_str!("rank_batch.py"),
    include_str!("rank_state.py"),
    include_str!("rank_prompt_search.py"),
    include_str!("rank_boundary.py"),
    include_str!("rank_tool_schema.py"),
    include_str!("rank_tool_stream.py"),
    include_str!("rank_stream_watch.py"),
    include_str!("rank_xml_guard.py"),
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
    ///
    /// Tagged `mlxLmServerTransientTail` since Q-142: `transient_tail_boundary` ends a prompt
    /// segment — and so a prefill chunk — at a chat request's stable boundary, and a peer whose
    /// wrapper does not cut there would run a different number of prefill steps (the collectives
    /// no longer pair up). `mlxLmServerFormation` (a Q-136 requester) still reads, with the
    /// boundary off: that requester's rank 0 never declares the tail, so goose never sends one.
    ///
    /// Tagged `mlxLmServerSkeletonGuard` since Q-161: `xml_skeleton_guard` masks, at the XML tool
    /// call's fixed positions, every token its template does not allow — it changes what is
    /// sampled, and every rank samples the same token from the same logits, so a peer whose wrapper
    /// does not mask would sample differently and the ranks would diverge. `mlxLmServerTransientTail`
    /// (a Q-142 requester) still reads, with the guard off: that requester's ranks mask nothing.
    ///
    /// Tagged `mlxLmServerRowProcessors` since Q-161's reopening: `row_processors` keeps each
    /// generating row's own logits processors where mlx_lm 0.31.3 hands a joining row the list of
    /// a row that already left (E2E #3f: the guard ran on one token of the tool answer). Which row
    /// runs which processors decides what is sampled, so a peer whose wrapper keeps upstream's
    /// lists would sample differently and the ranks would diverge. `mlxLmServerSkeletonGuard` (a
    /// 3.0.57 requester) still reads, with `row_processors` off: that requester's ranks keep
    /// upstream's lists alike.
    ///
    /// Tagged `mlxLmServerNewestPrefix` since Q-182: `keep_newest_prefix` makes every rank's prompt
    /// cache evict the newest stable prefix last (`rank_boundary.py` `pop_keeping_newest_prefix`).
    /// It changes what is evicted, and each rank runs its own cache over the same requests, so a
    /// peer whose wrapper evicts by mlx_lm's type counts alone would reuse a different prefix and
    /// run a different number of prefill steps. `mlxLmServerRowProcessors` (a 3.0.59 requester)
    /// still reads, with `keep_newest_prefix` off: that requester's ranks evict upstream's way.
    ///
    /// Tagged `mlxLmServerPrefillYield` since Q-231: `prefill_step_yields` ends mlx_lm's step loop
    /// after every step that read prompt tokens, so rows whose client left are removed and a new
    /// request is taken between prompt slices, not after the loop's whole count of them. Which step
    /// each rank runs next changes, and every rank must end the loop at the same step (a peer that
    /// ran on would be in the model's collectives while this Mac shares the removals), so a peer
    /// whose wrapper keeps upstream's loop must refuse the rank. `mlxLmServerNewestPrefix` (a 3.0.60
    /// requester) still reads, with `prefill_step_yields` off: that requester's ranks loop upstream's
    /// way alike.
    ///
    /// Tagged `mlxLmServerConversationPrefix` since Q-294: `keep_conversation_prefix` makes every
    /// rank's prompt cache keep the stable prefix a request naming its transient tail cut
    /// (`rank_boundary.py` `ConversationPrefix`) — not the newest "user" entry, which an
    /// end-of-turn helper's segment took over on E2E #3o. It changes what is evicted, and each rank
    /// runs its own cache over the same requests, so a peer whose wrapper keeps the newest "user"
    /// entry would reuse a different prefix and run a different number of prefill steps.
    /// `mlxLmServerPrefillYield` (a 3.0.66 requester) still reads, with `keep_conversation_prefix`
    /// off: that requester's ranks keep the newest "user" entry alike.
    ///
    /// Tagged `mlxLmServerStableHead` since Q-347: `keep_stable_head` makes every rank end a
    /// prefill segment where an agent request's system prompt and tools end, and keep that entry
    /// after the conversation prefix (`rank_boundary.py` `cut_at_head`). A peer that did not cut
    /// there would run a different number of prefill steps (the collectives no longer pair up),
    /// and one that evicted it would reuse a different prefix. `mlxLmServerConversationPrefix` (a
    /// 3.0.69 requester) still reads, with `keep_stable_head` off: that requester's ranks cut and
    /// keep no head alike.
    #[serde(
        rename = "mlxLmServerStableHead",
        alias = "mlxLmServerConversationPrefix",
        alias = "mlxLmServerPrefillYield",
        alias = "mlxLmServerNewestPrefix",
        alias = "mlxLmServerRowProcessors",
        alias = "mlxLmServerSkeletonGuard",
        alias = "mlxLmServerTransientTail",
        alias = "mlxLmServerFormation",
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
        /// Rank 0 declares `rapid_mlx_transient_tail` (+ `_on_tool`) on `/v1/models`, and every
        /// rank keeps a chat request's prefix up to the text its tail names (`rank_boundary.py`):
        /// the entry goose's next agent request extends on this non-trimmable hybrid cache.
        #[serde(default)]
        transient_tail_boundary: bool,
        /// Every rank holds a tool request's decode to the Qwen3-Coder XML call's skeleton
        /// (`rank_xml_guard.py`, the single engine's lz.7 guard): after `</parameter>` and
        /// `</function>` at a line start inside a call, after `<tool_call>` and after
        /// `</tool_call>`, only the template's own continuation (or the end of the turn) may come.
        #[serde(default)]
        xml_skeleton_guard: bool,
        /// Every rank keeps each generating row's own logits processors (`rank_batch.py`
        /// `row_processors`): mlx_lm 0.31.3's `GenerationBatch.filter` leaves the lists of rows
        /// that left whenever the rows kept carry none, and the next row to join reads one of them.
        #[serde(default)]
        row_processors: bool,
        /// Every rank's prompt cache evicts the newest "user" entry — the stable prefix the latest
        /// conversation request left, which its next request extends — only when nothing else is
        /// left (`rank_boundary.py` `pop_keeping_newest_prefix`, Q-182).
        #[serde(default)]
        keep_newest_prefix: bool,
        /// Every rank ends mlx_lm's step loop after a step that read prompt tokens
        /// (`rank_wrapper.py` `PromptStepBudget`, Q-231): the loop's removals and the next request
        /// are handled between prompt slices.
        #[serde(default)]
        prefill_step_yields: bool,
        /// Every rank's prompt cache keeps the stable prefix the latest request naming its
        /// transient tail cut (goose's agent requests) while anything else is left to evict, and
        /// rank 0 admits a request into a live batch only while the batch leaves room for it
        /// (`rank_boundary.py` `KeptEntry`, Q-294). Supersedes `keep_newest_prefix`.
        #[serde(default)]
        keep_conversation_prefix: bool,
        /// Every rank ends a prefill segment where an agent request's stable head ends — its
        /// system prompt and tools, where mlx_lm's own system segment would end — keeps that entry
        /// while anything but the conversation prefix is left to evict, and rank 0 admits a
        /// request into a live batch only while the batch leaves room for it beside the prefix
        /// (`rank_boundary.py` `cut_at_head`, Q-347): a request whose messages changed — the first
        /// after a compaction, a new chat with the same tools — reads the head from the cache.
        #[serde(default)]
        keep_stable_head: bool,
        /// goose's sampling profile for the model (Q-159, `rank_sampling.py`): the layer rank 0
        /// resolves between a request's own fields and the checkpoint's generation_config.json.
        /// Rank 0 only — the workers sample from the arguments rank 0 shares — so it needs no new
        /// tag: rank 0 is always the requester's own Mac, whose wrapper reads it.
        #[serde(default, skip_serializing_if = "SamplingDefaults::is_empty")]
        sampling_defaults: Box<SamplingDefaults>,
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
            transient_tail_boundary: true,
            xml_skeleton_guard: true,
            row_processors: true,
            keep_newest_prefix: true,
            prefill_step_yields: true,
            keep_conversation_prefix: true,
            keep_stable_head: true,
            sampling_defaults: Box::default(),
        }
    })
}

/// The `pipeline_qwen4 serve` arguments for one rank: this node's own model dir, the served id,
/// rank 0's loopback port, the context preflight allowed, the slots the plan was made for
/// (`--slots`: the fork re-plans at load for that many full-context sequences and admits each
/// request by what it needs of that KV budget — its prompt and max_tokens beside what the running
/// rows will still grow into; no row count is passed: since lz-pipeline-qwen4.11 the fork derives
/// the rows its plan header names from the same budget, Q-160), and the split
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

    /// goose's sampling profile for the split's model (Q-159), on rank 0 — the one rank that
    /// resolves a request's absent sampling fields and shares the result: the tensor wrapper reads
    /// it from the spec (`rank_sampling.py`), the pipeline fork as `serve --default-*`
    /// (lz-pipeline-qwen4.9). The other ranks never see it, so a peer's own program is not asked
    /// to parse a flag it may predate.
    pub fn set_sampling_defaults(&mut self, sampling: SamplingDefaults) {
        if self.rank != 0 {
            return;
        }
        match &mut self.program {
            RankProgram::MlxLmServer {
                sampling_defaults, ..
            } => **sampling_defaults = sampling,
            RankProgram::PipelineServe { serve_args } => serve_args.extend(sampling.serve_flags()),
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

    /// The rank's last loop state names a place it works on its CPU alone (`CPU_ONLY_PHASES`).
    pub fn in_cpu_only_phase(&self) -> bool {
        self.state
            .as_ref()
            .and_then(|state| state.get("at"))
            .and_then(serde_json::Value::as_str)
            .is_some_and(|at| super::CPU_ONLY_PHASES.contains(&at))
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
/// would leave the rank blocked on a full pipe at 0% CPU, the very state a hang shows. The handle
/// ends at EOF, once every line is in the log and the live view.
fn read_lines<R: AsyncRead + Unpin + Send + 'static>(
    reader: R,
    stream: &'static str,
    live: Arc<StdMutex<RankLive>>,
    log: SharedLog,
) -> tokio::task::JoinHandle<()> {
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
    })
}

/// Attaches the drains, and the durable log, to a spawned rank's two streams. The handles end when
/// both streams have reached EOF — what a reader of the whole output waits on.
fn drain_rank_output(
    child: &mut Child,
    rank: usize,
    node: &str,
    live: &Arc<StdMutex<RankLive>>,
) -> Vec<tokio::task::JoinHandle<()>> {
    let log = open_rank_log(rank, node, live);
    let mut drains = Vec::new();
    if let Some(stdout) = child.stdout.take() {
        drains.push(read_lines(
            stdout,
            "out",
            Arc::clone(live),
            Arc::clone(&log),
        ));
    }
    if let Some(stderr) = child.stderr.take() {
        drains.push(read_lines(stderr, "err", Arc::clone(live), log));
    }
    drains
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
                transient_tail_boundary: true,
                xml_skeleton_guard: true,
                row_processors: true,
                keep_newest_prefix: true,
                prefill_step_yields: true,
                keep_conversation_prefix: true,
                keep_stable_head: true,
                sampling_defaults: _,
            }
        ));
        assert!(
            specs.iter().all(|s| matches!(&s.program,
                RankProgram::MlxLmServer { sampling_defaults, .. } if sampling_defaults.is_empty())),
            "the profile is set per launch, on rank 0 only (`set_sampling_defaults`)"
        );
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
        assert!(joined.contains("--slots 4 --split 19"), "{joined}");

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
             \x20   stream: object = None\n\
             @dataclass\n\
             class _State:\n\
             \x20   waiting: list = None\n\
             @dataclass\n\
             class _Row:\n\
             \x20   sampling: object = None\n\
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
        assert!(
            ready.get("max_batch").is_none(),
            "no row count is passed: the fork derives it from the KV budget (Q-160)"
        );
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
            "from dataclasses import dataclass\n\
             @dataclass\n\
             class _Job:\n\
             \x20   row: object\n\
             \x20   stream: object = None\n\
             @dataclass\n\
             class _State:\n\
             \x20   waiting: list = None\n\
             @dataclass\n\
             class _Row:\n\
             \x20   sampling: object = None\n\
             class _Engine:\n\
             \x20   def _start(self, row): pass\n\
             \x20   def prefill(self, words): pass\n\
             def prefill_chunks(start, end, step, split=0): return []\n\
             def _build_app(state, tokenizer, eos_ids, vision=None): pass\n\
             def add_arguments(parser):\n\
             \x20   for flag in ('--model', '--served-model-name', '--host', '--port', '--context', \
             '--slots', '--prefill-step', '--attention-scores-bytes', '--split'):\n\
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
# Q-231, E2E #3m: a fact check goose dropped (POST 20:16:14.250Z, its stop named 3.642 s later)
# still holds the batch; the user's call waits behind it without being held for room.
dropped = row_handling("127.0.0.1:50003", {"reason": "cancelled_by_client"}, 3.892, True, 0.25, False)
assert dropped == {"client": "127.0.0.1:50003", "held_for_room": False,
                   "stopped": {"reason": "cancelled_by_client"}, "stopped_after_s": 3.642,
                   "leaving": True}, dropped
waiting = row_handling("127.0.0.1:50100", None, None, False, 0.0, True)
assert waiting["stopped_after_s"] is None and waiting["held_for_room"] and not waiting["leaving"]
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

    /// rank_sampling.py under a real interpreter (Q-159): the checkpoint's generation_config.json
    /// read and filtered as the single engine's utils/generation_config.py filters it, every way it
    /// can fail to be read named, and each field resolved request > profile > config > fallback.
    #[test]
    fn a_rank_reads_the_checkpoints_sampling_as_the_single_engine_does() {
        let checks = r#"
import tempfile
root = tempfile.mkdtemp()

def checkpoint(content):
    path = tempfile.mkdtemp(dir=root)
    if content is not None:
        with open(os.path.join(path, "generation_config.json"), "w") as handle:
            handle.write(content)
    return path

qwen = checkpoint('{"do_sample": true, "temperature": 1.0, "top_k": 20, "top_p": 0.95, "eos_token_id": [1, 2]}')
path, values, ignored, error = generation_config_sampling(qwen)
assert values == {"temperature": 1.0, "top_k": 20, "top_p": 0.95} and ignored == [] and error is None, values
assert isinstance(values["top_k"], int)
odd = checkpoint('{"temperature": true, "top_p": NaN, "top_k": 20.5, "min_p": "0.1", "repetition_penalty": 1.05, "top_k_extra": 3}')
_, values, ignored, error = generation_config_sampling(odd)
assert values == {"repetition_penalty": 1.05}, values
assert ignored == ["temperature=True", "top_p=nan", "top_k=20.5", "min_p='0.1'"], ignored
assert error is None
for content, said in ((None, "is absent"), ("{not json", "is unreadable"), ("[1]", "holds no JSON object")):
    _, values, _, error = generation_config_sampling(checkpoint(content))
    assert values == {} and said in error, (content, error)

defaults = SamplingDefaults({"top_p": 0.8, "min_p": None, "bogus": 1}, qwen)
assert defaults.profile == {"top_p": 0.8}, "unset profile fields and unknown keys are no layer"
resolved = defaults.resolve({"temperature": None, "top_k": 5})
assert resolved["temperature"] == (1.0, "generation_config"), "a null is no value"
assert resolved["top_k"] == (5, "request")
assert resolved["top_p"] == (0.8, "profile"), "the profile sits above the checkpoint"
assert resolved["min_p"] == (None, "unset")
assert defaults.resolve({"temperature": 0})["temperature"] == (0, "request"), "0 is the client's greedy"
bare = SamplingDefaults(None, checkpoint(None))
assert bare.resolve({})["temperature"] == (0.7, "engine_fallback")
assert bare.resolve({})["top_p"] == (0.9, "engine_fallback")
assert bare.report()["generation_config_error"].endswith("is absent")
print("ok")
"#;
        let out = std::process::Command::new("/usr/bin/python3")
            .arg("-c")
            .arg(format!("{}{checks}", include_str!("rank_sampling.py")))
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
# Q-182, E2E #3h: a joining row must leave the cache one prefix as wide as the batch. 15:51:45's
# 95,514-token agent call joining a live 154-token tool label: 14.11 GB of batch + 3.21 GB of
# prefix = 17.317 GB, inside by 17 MB. 15:52:14's 95,789-token call beside a label: 17.365 GB, so
# it waits for the label (seconds) instead of pushing the cache below its own prefix.
assert kept_prefix_bytes(p, 95514) == batch_kv_charge(p, 1, 95514) == 95514 * 32768 + 76972032
assert admits(p, limit, 1, 154, 95514)
assert not admits(p, limit, 1, 154, 95789)
assert not admits(p, limit, 1, 95789, 123), "a label arriving during the call waits for it"
assert batch_kv_charge(p, 2, 95789) <= limit, "the old rule would have taken it"
# 14:56:45: a 70,824-token call and two labels arrived together; three rows padded to 70,824 charge
# 15.83 GB and left 1.5 GB for its 2.4 GB prefix. The second label now waits.
assert admits(p, limit, 1, 70824, 157)
assert not admits(p, limit, 2, 70824, 192)
assert batch_kv_charge(p, 3, 70824) <= limit, "the old rule would have taken it"
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
        run_against_real_packages_printing(python, program, spec).0
    }

    /// The same, with every line the program printed (its GOOSE_* lines are what a rank's durable
    /// log would hold).
    fn run_against_real_packages_printing(
        python: &str,
        program: &str,
        spec: &RankSpec,
    ) -> (serde_json::Value, String) {
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
        (serde_json::from_str(line).unwrap(), stdout.into_owned())
    }

    /// The pipeline program as shipped — the load lock, the env prelude, the live table and
    /// pipeline_rank.py up to where serve() needs a model and a group — against the REAL fork at
    /// the pinned commit (`EnvSpec::pipeline()`'s proof). Its stand-in test replaced the whole fork
    /// with a module of the same names; here the fork's own seams are what goose patches: the
    /// attribute guard, `_Job` (a dataclass whose `produced` field LiveJob turns into a
    /// property), `_Engine._start`/`_Engine.prefill` (Q-134's continuous admission: a row
    /// prefills in its own cache, then joins the running batch; since Q-145 several rows may
    /// prefill — `_Engine.prefilling`, and `_Engine.joining` is the read-only pick whose chunk
    /// runs next) measured over the fork's own `prefill_chunks`, `_Joining` and `_Row`, the
    /// fork's own argparse reading goose's serve argv, and the fork's own `_build_app` serving
    /// `/v1/status` through goose's replacement route, which lists rank 0's `_State.waiting`
    /// (Q-145; `held` is gone) and the queue (only the tokenizer — the model's — is a stand-in).
    /// Negative controls, measured 2026-09-26: the same program under an env still on 2f02ac645
    /// exits at the guard ("has no prefill_chunks"); the 66ccd37a6 env's module has no `_Engine`,
    /// the guard's first new name; on 419306f70 the pre-Q-145 stand-in (`engine.joining = …`)
    /// raised "property 'joining' of '_Engine' object has no setter".
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
        let mut spec =
            pipeline_rank_specs(&config, &served, 32_768, "19", 2_048, &[0, 0], 2.0).remove(0);
        spec.set_sampling_defaults(SamplingDefaults {
            top_p: Some(0.8),
            top_k: Some(20),
            ..SamplingDefaults::default()
        });
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
    engine.prefilling.append(serve._Joining(row, None, None, None, None, ranges))
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
# Q-159: the fork keeps each admitted row's resolved sampling for goose's status rows.
row.sampling = {"temperature": {"value": 1.0, "from": "generation_config"}}

class Tokenizer:
    chat_template = ""
    eos_token_ids = [0]

state = serve._State(served=options.served_model_name, aliases=tuple(options.served_model_alias or ()),
                     context=options.context, max_batch=1)
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
# Q-145: rank 0 moves queued jobs to `waiting`; the queue holds what it has not taken yet.
state.waiting = [serve._Job(serve._Row([2] * 6, 4, 0.0, 1.0), loop, asyncio.Queue())]
state.jobs.put(serve._Job(serve._Row([1] * 5, 4, 0.0, 1.0), loop, asyncio.Queue()))
# Q-178: a streamed chat job carries the fork's StreamWatch; goose's row carries its report.
from rapid_mlx.distributed.pipeline_stream import StreamWatch
job.stream = StreamWatch(lambda tag, payload: None, job.id)
job.stream.take("tool", "<tool_call>\n<function=write>")
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
        assert!(
            options.get("max_batch").is_none(),
            "the fork's own parser takes no row count since Q-160"
        );
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
            (&serde_json::json!(1), &serde_json::json!(2)),
            "the fork's own counters (the queue + rank 0's waiting list, Q-145) ride under \
             goose's table: {status}"
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
        assert_eq!(
            phases,
            ["generation", "queued", "queued"],
            "goose's table lists the waiting job and the queued one: {status}"
        );
        // Q-159: rank 0's argv carries goose's profile in the fork's own flags, the fork's status
        // names its sampling layers, and each row carries the fields it samples with.
        assert_eq!(options["default_top_p"], 0.8);
        assert_eq!(options["default_top_k"], 20);
        assert_eq!(options["default_temperature"], serde_json::Value::Null);
        assert_eq!(
            status["requests"][0]["sampling"],
            serde_json::json!({"temperature": {"value": 1.0, "from": "generation_config"}})
        );
        assert_eq!(status["requests"][1]["sampling"], serde_json::Value::Null);
        assert!(
            status["sampling_defaults"]["generation_config_error"].is_string(),
            "a state built with no checkpoint names it: {status}"
        );
        // Q-178: the streamed job's row carries what its client has and has not been sent; a job
        // that is not a streamed chat says so with null; the fork's own body names the last answer
        // the engine ended itself (none yet).
        let stream = &status["requests"][0]["stream"];
        assert_eq!(stream["parser_state"], "tool", "{status}");
        assert_eq!(stream["tail"], "<tool_call>\n<function=write>");
        assert_eq!(
            (&stream["generated_chars"], &stream["sent_chars"]),
            (&serde_json::json!(28), &serde_json::json!(0))
        );
        assert_eq!(stream["tool_call"]["streamed"], false, "{stream}");
        assert_eq!(status["requests"][1]["stream"], serde_json::Value::Null);
        assert_eq!(status["last_engine_stop"], serde_json::Value::Null);
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
state = pipeline_qwen4_serve._State(served=SERVED, context=options.context, max_batch=1)
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

# Q-162: the cache searches through the linear trie, mlx_lm's own search answers the same over the
# same nodes, and the loop says `cache_lookup` while the search runs, then where it was.
from mlx_lm.models.cache import PromptTrie as UpstreamTrie
searched_at = []
linear_nearest = nearest_prompt

def watched_nearest(*args):
    searched_at.append(published_state[0]["at"])
    return linear_nearest(*args)

nearest_prompt = watched_nearest
loop.publish("batch")
leaving = list(range(200, 230)) + [999]
_, lookup_rest = cache.fetch_nearest_cache("m", leaving)
lookup = {
    "trie": type(cache._trie).__name__,
    "at": searched_at + [published_state[0]["at"]],
    "rest": lookup_rest,
    "same": all(
        UpstreamTrie.search(cache._trie, "m", probe) == cache._trie.search("m", probe)
        for probe in (leaving, list(range(200, 264)), list(range(200, 300)), [7], [])
    ),
}
nearest_prompt = linear_nearest

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
    "lookup": lookup,
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
            "{}\
             class _Group:\n    def rank(self): return 0\n    def size(self): return 2\n\
             group = _Group()\n{}QWEN38 = {qwen}\n{checks}",
            tensor_modules(),
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
        assert_eq!(
            seen["lookup"],
            serde_json::json!({"trie": "LinearPromptTrie", "at": ["cache_lookup", "batch"],
                "rest": [999], "same": true}),
            "mlx_lm's own cache searches through the linear trie (Q-162), answering what mlx_lm's \
             own search answers, and the loop reports the search as `cache_lookup` while it runs"
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
        for model in seen["models"][1]["data"].as_array().unwrap() {
            assert_eq!(
                model["request_extensions"],
                serde_json::json!([
                    "rapid_mlx_transient_tail",
                    "rapid_mlx_transient_tail_on_tool"
                ]),
                "goose's omlx provider names its turn-context tail to an engine that declares it \
                 (Q-142), on the tool results too (Q-94)"
            );
            assert_eq!(
                model["capabilities"],
                serde_json::json!(["text", "tools"]),
                "mlx_lm.server reads text only, and says so: goose's omlx provider sends an \
                 image's placeholder, never the image (Q-260)"
            );
        }
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

    /// rank_boundary.py under a real interpreter: the tail goose names comes off the message it
    /// ends (a tool message too), a user message that held only the tail goes whole, a tail that
    /// is not that message's exact end is named instead of guessed at, and the boundary becomes a
    /// segment end mlx_lm snapshots — with nothing past it but one segment of volatile text.
    /// Q-347: the stable head ends where mlx_lm's own system segment would, becomes a segment end
    /// on a tool step too, and is kept after the conversation prefix.
    #[test]
    fn a_rank_keeps_the_prefix_before_the_transient_tail() {
        let checks = r#"
tail = "\n<turn-context>\n<current-time>2026-09-26 20:53:00</current-time>\n</turn-context>"
tool = [{"role": "system", "content": "s"}, {"role": "user", "content": "q"},
        {"role": "assistant", "content": "", "tool_calls": []},
        {"role": "tool", "tool_call_id": "c", "content": "IDENTICAL" + tail}]
stable = stable_messages(tool, tail)
assert stable[-1] == {"role": "tool", "tool_call_id": "c", "content": "IDENTICAL"}, stable
assert stable[:3] == tool[:3] and tool[-1]["content"].endswith(tail), "the request is untouched"
asked = [{"role": "user", "content": "Write it." + tail}]
assert stable_messages(asked, tail) == [{"role": "user", "content": "Write it."}]
alone = [{"role": "user", "content": "q"}, {"role": "assistant", "content": "a"}, {"role": "user", "content": tail}]
assert stable_messages(alone, tail) == alone[:2], "a user turn of nothing but the block goes whole"
for messages, why in (
    ([{"role": "tool", "content": "IDENTICAL" + tail + " "}], "exact end of message #0 (tool"),
    ([{"role": "tool", "content": [{"type": "text", "text": "x"}]}], "list"),
    ([{"role": "assistant", "content": tail}], "no user or tool message"),
):
    try:
        stable_messages(messages, tail)
    except TailIgnored as ignored:
        assert why in str(ignored), (why, str(ignored))
    else:
        raise AssertionError(f"{messages} would be cut somewhere it does not end")

prompt = list(range(100))
assert stable_boundary(prompt, prompt[:60] + [-1] * 40) == 60 - BOUNDARY_REPLAY_TOKENS
assert stable_boundary(prompt, [-1] * 100) == 0

# mlx_lm's own segmentation of a request ending on a user message (system, context, think tail)
# and of one ending on tool results (one segment).
system, context, think = list(range(0, 30)), list(range(30, 90)), list(range(90, 100))
cut, kinds = cut_at_boundary([system, context, think], ["system", "user", "assistant"], 70)
assert cut == [system, list(range(30, 70)), list(range(70, 100))], cut
assert kinds == ["system", "user", "assistant"], kinds
cut, kinds = cut_at_boundary([prompt], ["assistant"], 70)
assert cut == [prompt[:70], prompt[70:]] and kinds == ["user", "assistant"], (cut, kinds)
cut, kinds = cut_at_boundary([system, context, think], ["system", "user", "assistant"], 30)
assert cut == [system, prompt[30:]] and kinds == ["system", "assistant"], "an end already there stays"
for outside in (0, 100, 120):
    assert cut_at_boundary([prompt], ["assistant"], outside) == ([prompt], ["assistant"]), outside

# Q-294: the conversation prefix is the key a cut named, matched by its tokens, held by identity.
from collections import deque


class Order:
    def __init__(self):
        self._lrus = {kind: deque() for kind in ("assistant", "user", "system")}

    def __len__(self):
        return sum(len(lru) for lru in self._lrus.values())


def oldest_user_first(order):
    for kind in ("user", "assistant", "system"):
        if order._lrus[kind]:
            return order._lrus[kind].popleft()


def upstream_order(order):
    """mlx_lm 0.31.3's `CacheOrder.pop`: by type counts, assistant, then user, then system."""
    kinds = ("assistant", "user", "system")
    for a, b in zip(kinds, kinds[1:]):
        if order._lrus[a] and len(order._lrus[a]) >= len(order._lrus[b]):
            return order._lrus[a].popleft()
    return order._lrus["system"].popleft()


prefix = KeptEntry()
conversation = list(range(60))
prefix.cut(conversation)
prefix.inserted(list(range(59)) + [-1], 7)
assert prefix.tokens is None, "same length, other tokens: not the conversation's"
helper, older, key = [-5] * 10, [-6] * 20, conversation[:]
prefix.inserted(key, 42)
assert prefix.tokens is key and prefix.nbytes == 42 and not prefix.cut_keys
order = Order()
order._lrus["user"].extend([("m", older), ("m", key), ("m", helper)])
order._lrus["system"].append(("m", [-7] * 5))
assert pop_keeping(order, oldest_user_first, (prefix,)) == ("m", older)
assert [t for _, t in order._lrus["user"]] == [key, helper], "held back in place"
assert pop_keeping(order, oldest_user_first, (prefix,)) == ("m", helper)
assert pop_keeping(order, oldest_user_first, (prefix,))[1] == [-7] * 5
assert pop_keeping(order, oldest_user_first, (prefix,)) == ("m", key)
assert prefix.tokens is None and prefix.nbytes == 0, "the last entry goes when the bound needs it"
prefix.cut(conversation)
prefix.inserted(key, 42)
order._lrus["user"].append(("m", key[:]))
order._lrus["assistant"].append(("m", [-8] * 3))
assert pop_keeping(order, oldest_user_first, (prefix,))[1] == key
assert prefix.tokens is None, "a key the cache replaced is no longer held"

# Q-347: the stable head. mlx_lm's probe is the leading system messages + an empty user turn.
chat = [{"role": "system", "content": "s"}, {"role": "user", "content": "q"}, {"role": "tool", "content": "r"}]
assert head_probe(chat) == [chat[0], {"role": "user", "content": ""}]
assert head_probe(chat[1:]) is None, "a conversation opening on no system message has no head"
assert head_end(prompt, prompt[:40] + [-1] * 5) == 40
assert head_end(prompt, prompt[:40]) == 0, "mlx_lm cuts no system segment when nothing differs"
assert head_end(prompt[:40], prompt) == 0
# A request ending on a user message: mlx_lm already ends its system segment at the head.
cut, kinds = cut_at_boundary([system, context, think], ["system", "user", "assistant"], 70)
assert cut_at_head(cut, kinds, 30) == (cut, kinds), "upstream's own cut, unchanged"
# A tool step (one segment): the head becomes a "system" segment end before the boundary.
cut, kinds = cut_at_head(*cut_at_boundary([prompt], ["assistant"], 70), 30)
assert cut == [prompt[:30], prompt[30:70], prompt[70:]], cut
assert kinds == ["system", "user", "assistant"], kinds
cut, kinds = cut_at_head([prompt], ["assistant"], 30)
assert cut == [prompt[:30], prompt[30:]] and kinds == ["system", "assistant"], "no boundary to keep"
for outside in (0, 100, 130):
    assert cut_at_head([prompt], ["assistant"], outside) == ([prompt], ["assistant"]), outside

# Both kept: the conversation prefix (most precious), then the head; every other entry goes first,
# each held back in place; with only the two left the head goes first, then the prefix.
conversation, head = KeptEntry(), KeptEntry()
long_key, head_key = list(range(80)), list(range(30))
conversation.cut(long_key)
head.cut(head_key)
conversation.inserted(long_key, 80)
head.inserted(head_key, 30)
assert (conversation.tokens, head.tokens) == (long_key, head_key)
order = Order()
helper_system, helper_user, end = [-9] * 4, [-9] * 6, list(range(90))
order._lrus["system"].extend([("m", head_key), ("m", helper_system)])
order._lrus["user"].extend([("m", long_key), ("m", helper_user)])
order._lrus["assistant"].append(("m", end))
kept = (conversation, head)
evicted = [pop_keeping(order, upstream_order, kept)[1] for _ in range(3)]
assert evicted == [end, helper_user, helper_system], evicted
assert [t for _, t in order._lrus["system"]] == [head_key]
assert [t for _, t in order._lrus["user"]] == [long_key]
assert pop_keeping(order, upstream_order, kept)[1] is head_key and head.tokens is None
assert conversation.tokens is long_key, "the prefix outlives the head"
assert pop_keeping(order, upstream_order, kept)[1] is long_key and conversation.tokens is None
# A head the cache dropped another way (a trimmable entry's prefixes) is forgotten, not reserved.
head.cut(head_key)
head.inserted(head_key, 30)
forget_dropped(order, kept)
assert head.tokens is None and head.nbytes == 0
print("ok")
"#;
        let out = std::process::Command::new("/usr/bin/python3")
            .arg("-c")
            .arg(format!("{}{checks}", include_str!("rank_boundary.py")))
            .output()
            .expect("/usr/bin/python3 runs the rank programs' pure half");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "ok");
    }

    /// Q-182 through the REAL mlx_lm 0.31.3 `LRUPromptCache`: E2E #3h's agent calls from 15:49:18
    /// to 15:59:24 replayed with their measured sizes (the 27B over 2 ranks: 32,768 B of KV per
    /// token + 76,972,032 B of state, the 17,333,813,248 B KV plan), each beside the tool label
    /// that joined it (154 tokens). Entries are non-trimmable, as the hybrid's are. At every step
    /// the order is the rank's: the label's system snapshot, the agent's fetch and admission, its
    /// stable prefix (typed "user"), the label's end, the agent's end — each insert trimmed to the
    /// plan less the live batch. The NEGATIVE CONTROL is mlx_lm's own eviction with the old
    /// admission: it reproduces the run exactly — 92,443 … 93,875 read, then 0 at 95,789 — and
    /// past ~95.6k tokens it misses every call. Each half of the fix alone still misses; both
    /// read every prefix.
    #[test]
    fn a_tool_label_beside_an_agent_call_leaves_its_prefix_cached_through_real_mlx_lm() {
        let Some(python) = proven_env(&EnvSpec::tensor(), "GOOSE_TEST_TENSOR_PYTHON") else {
            return;
        };
        let checks = r#"
from mlx_lm.models.cache import LRUPromptCache

p = {"kv_bytes_per_token": 32768, "sequence_state_bytes": 76972032, "batch_transient_ratio": 2.2}
limit = 17333813248


class Layer:
    def __init__(self, tokens):
        self.nbytes = batch_kv_charge(p, 1, tokens)

    def is_trimmable(self):
        return False


# (prompt, the next call's cache_read = this call's stable prefix, output) from calls.csv; the
# 95,514-token call's prefix is its prompt less the 582 tokens the call before it left out.
run = [(93025, 92443, 108), (93168, 92586, 178), (93410, 92828, 213), (93967, 93385, 198),
       (94229, 93647, 117), (94457, 93875, 235), (95514, 94932, 167), (95789, 95207, 98)]
grown = run + [(95789 + 700 * k, 95789 + 700 * k - 582, 150) for k in range(1, 30)]
LABEL_SYSTEM, LABEL, LABEL_OUT = 46, 154, 9
upstream_pop = LRUPromptCache.CacheOrder.pop


def replay(steps, keep, gated):
    LRUPromptCache.CacheOrder.pop = upstream_pop
    if keep:
        keep_newest_prefix(LRUPromptCache.CacheOrder)
    cache = LRUPromptCache(max_size=limit // batch_kv_charge(p, 1, 256), max_bytes=limit)
    conversation = list(range(steps[-1][0]))
    label_system = [-1] * LABEL_SYSTEM
    reads = []
    for i, (prompt_tokens, stable_tokens, out) in enumerate(steps):
        stable = conversation[:stable_tokens]
        prompt = stable + [-(1000 + i)] * (prompt_tokens - stable_tokens)
        label = label_system + [-(2000 + i)] * (LABEL - LABEL_SYSTEM + LABEL_OUT)
        cache.insert_cache("m", label_system[:], [Layer(LABEL_SYSTEM)], cache_type="system")
        cache.trim_to(n_bytes=limit - batch_kv_charge(p, 1, LABEL))
        found, rest = cache.fetch_nearest_cache("m", prompt)
        reads.append(prompt_tokens - len(rest) if found is not None else 0)
        joins = admits(p, limit, 1, LABEL, prompt_tokens) if gated else True
        if not joins:
            cache.insert_cache("m", label, [Layer(LABEL + LABEL_OUT)])
        room = limit - batch_kv_charge(p, 2 if joins else 1, prompt_tokens)
        cache.trim_to(n_bytes=room)
        cache.insert_cache("m", stable[:], [Layer(stable_tokens)], cache_type="user")
        cache.trim_to(n_bytes=room)
        if joins:
            cache.insert_cache("m", label, [Layer(LABEL + LABEL_OUT)])
            cache.trim_to(n_bytes=room)
        cache.insert_cache("m", prompt + [-(3000 + i)] * out, [Layer(prompt_tokens + out)])
        cache.trim_to(n_bytes=limit - batch_kv_charge(p, 1, prompt_tokens + out))
    return reads


prefixes = [0] + [stable for _, stable, _ in grown[:-1]]
live = [0, 92443, 92586, 92828, 93385, 93647, 93875, 0]
assert replay(run, keep=False, gated=False) == live, replay(run, keep=False, gated=False)
upstream = replay(grown, keep=False, gated=False)
assert upstream[8:] == [0] * (len(grown) - 8), upstream
assert replay(grown, keep=True, gated=True) == prefixes
assert replay(grown, keep=True, gated=False)[7] == prefixes[7], "95,789 reads 94,932"
assert replay(grown, keep=True, gated=False)[8:] == [0] * (len(grown) - 8), "wider, the room is gone"
assert replay(grown, keep=False, gated=True)[7] == 0, "admission alone: the old eviction order"
LRUPromptCache.CacheOrder.pop = upstream_pop
print("ok")
"#;
        let out = std::process::Command::new(&python)
            .arg("-c")
            .arg(format!(
                "{}{}{checks}",
                include_str!("rank_prefill.py"),
                include_str!("rank_boundary.py")
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

    /// Q-294 through the REAL mlx_lm 0.31.3 `LRUPromptCache`: E2E #3o's turn 6 → 7 (rank0 log
    /// 06:08:30–06:09:24Z) replayed with its measured sizes. The agent's last call of turn 6 read
    /// 116,845 tokens and wrote 540; its stable prefix ends 668 tokens before its end (a turn-4
    /// call sent 108,076 and the next read 107,408). goose's end-of-turn helpers then arrived one
    /// after another — req-204..211, the CANCELLED lines' prompt_tokens, req-204 a 150-token
    /// label — each with its own 380-token system segment (every fact check's first prefill
    /// report) and a context segment ending 384 tokens before its prompt does (1,452 → 1,068,
    /// 1,248 → 864, 2,144 → 1,760 prefilled when goose dropped them). The NEGATIVE CONTROL is the
    /// 3.0.66 rule (Q-182's newest "user" entry, a batch leaving room as wide as itself): all
    /// eight join, the helpers' context segments take the protection, and the agent's next call
    /// reads 0 — the run's own shape (06:09:24: user 4 sequences, 0.47 GB, no prefix). Each half
    /// of the fix alone still reads 0; both read the whole prefix, the eighth helper waiting.
    /// E2E #3h (Q-182's replay) still reads every prefix under the new rule.
    #[test]
    fn end_of_turn_helpers_leave_the_conversation_prefix_cached_through_real_mlx_lm() {
        let Some(python) = proven_env(&EnvSpec::tensor(), "GOOSE_TEST_TENSOR_PYTHON") else {
            return;
        };
        let checks = r#"
from mlx_lm.models.cache import LRUPromptCache

p = {"kv_bytes_per_token": 32768, "sequence_state_bytes": 76972032, "batch_transient_ratio": 2.2}
limit = 17333813248


class Layer:
    def __init__(self, tokens):
        self.nbytes = batch_kv_charge(p, 1, tokens)

    def is_trimmable(self):
        return False


AGENT, AGENT_OUT, TAIL = 116845, 540, 668
HELPERS = [150, 6462, 2144, 3331, 1452, 7611, 23686, 1248]
HELPER_SYSTEM, HELPER_TAIL = 380, 384
NEXT_TURN = 1956
upstream_pop = LRUPromptCache.CacheOrder.pop


def replay(keep, gated):
    """What the agent's first call of the next turn reads, the helpers that joined, the helpers
    held, and the cache it meets. `keep`: the conversation rule's eviction (else Q-182's);
    `gated`: rank 0's admission leaves room for the conversation prefix."""
    LRUPromptCache.CacheOrder.pop = upstream_pop
    prefix = KeptEntry()
    if keep:
        keep_entries(LRUPromptCache.CacheOrder, prefix)
    else:
        keep_newest_prefix(LRUPromptCache.CacheOrder)
    cache = LRUPromptCache(max_size=limit // batch_kv_charge(p, 1, 256), max_bytes=limit)
    conversation = list(range(AGENT + NEXT_TURN))
    stable = conversation[: AGENT - TAIL]

    def insert(tokens, cache_type, room):
        layers = [Layer(len(tokens))]
        prefix.inserted(tokens, sum(layer.nbytes for layer in layers))
        cache.insert_cache("m", tokens, layers, cache_type=cache_type)
        cache.trim_to(n_bytes=room)

    prefix.cut(stable)
    insert(stable[:], "user", limit - batch_kv_charge(p, 1, AGENT))
    insert(stable + [-1] * (TAIL + AGENT_OUT), "assistant", limit - batch_kv_charge(p, 1, AGENT))
    rows, width, held = 0, 0, []
    for i, tokens in enumerate(HELPERS):
        if not admits(p, limit, rows, width, tokens, prefix.nbytes if gated else 0):
            held.append(tokens)
            continue
        rows, width = rows + 1, max(width, tokens)
        room = limit - batch_kv_charge(p, rows, width)
        cache.trim_to(n_bytes=room)
        system = [-(100 + i)] * HELPER_SYSTEM
        insert(system, "system", room)
        insert(system + [-(200 + i)] * (tokens - HELPER_TAIL - HELPER_SYSTEM), "user", room)
    found, rest = cache.fetch_nearest_cache("m", conversation)
    read = len(conversation) - len(rest) if found is not None else 0
    return read, rows, held, cache


read, rows, held, cache = replay(keep=False, gated=False)
assert (read, rows, held) == (0, len(HELPERS), []), (read, rows, held)
users = [len(tokens) for _, tokens in cache._lru._lrus["user"]]
assert users and max(users) < max(HELPERS), f"the helpers' segments only: {users}"
assert replay(keep=True, gated=False)[:3] == (0, len(HELPERS), []), "the eviction alone"
assert replay(keep=False, gated=True)[:3] == (0, 7, [1248]), "the admission alone"
assert replay(keep=True, gated=True)[:3] == (AGENT - TAIL, 7, [1248])

run = [(93025, 92443, 108), (93168, 92586, 178), (93410, 92828, 213), (93967, 93385, 198),
       (94229, 93647, 117), (94457, 93875, 235), (95514, 94932, 167), (95789, 95207, 98)]
grown = run + [(95789 + 700 * k, 95789 + 700 * k - 582, 150) for k in range(1, 30)]
LABEL_SYSTEM, LABEL, LABEL_OUT = 46, 154, 9


def replay_3h(steps):
    LRUPromptCache.CacheOrder.pop = upstream_pop
    prefix = KeptEntry()
    keep_entries(LRUPromptCache.CacheOrder, prefix)
    cache = LRUPromptCache(max_size=limit // batch_kv_charge(p, 1, 256), max_bytes=limit)

    def insert(tokens, layers, cache_type="assistant"):
        prefix.inserted(tokens, sum(layer.nbytes for layer in layers))
        cache.insert_cache("m", tokens, layers, cache_type=cache_type)

    conversation = list(range(steps[-1][0]))
    label_system = [-1] * LABEL_SYSTEM
    reads = []
    for i, (prompt_tokens, stable_tokens, out) in enumerate(steps):
        stable = conversation[:stable_tokens]
        prompt = stable + [-(1000 + i)] * (prompt_tokens - stable_tokens)
        label = label_system + [-(2000 + i)] * (LABEL - LABEL_SYSTEM + LABEL_OUT)
        insert(label_system[:], [Layer(LABEL_SYSTEM)], "system")
        cache.trim_to(n_bytes=limit - batch_kv_charge(p, 1, LABEL))
        found, rest = cache.fetch_nearest_cache("m", prompt)
        reads.append(prompt_tokens - len(rest) if found is not None else 0)
        joins = admits(p, limit, 1, LABEL, prompt_tokens, prefix.nbytes)
        if not joins:
            insert(label, [Layer(LABEL + LABEL_OUT)])
        room = limit - batch_kv_charge(p, 2 if joins else 1, prompt_tokens)
        cache.trim_to(n_bytes=room)
        prefix.cut(stable)
        insert(stable[:], [Layer(stable_tokens)], "user")
        cache.trim_to(n_bytes=room)
        if joins:
            insert(label, [Layer(LABEL + LABEL_OUT)])
            cache.trim_to(n_bytes=room)
        insert(prompt + [-(3000 + i)] * out, [Layer(prompt_tokens + out)])
        cache.trim_to(n_bytes=limit - batch_kv_charge(p, 1, prompt_tokens + out))
    return reads


assert replay_3h(grown) == [0] + [stable for _, stable, _ in grown[:-1]]
LRUPromptCache.CacheOrder.pop = upstream_pop
print("ok")
"#;
        let out = std::process::Command::new(&python)
            .arg("-c")
            .arg(format!(
                "{}{}{checks}",
                include_str!("rank_prefill.py"),
                include_str!("rank_boundary.py")
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

    /// Q-142 through the REAL mlx_lm 0.31.3 on the CPU: its HTTP handler, its generation loop,
    /// its BatchGenerator and LRU prompt cache under the wrapper, serving a tiny random qwen3_5 —
    /// the 27B's architecture, GatedDeltaNet layers whose ArraysCache cannot be trimmed beside
    /// attention layers — through the Qwen3.8 template (one token per byte). goose's agent
    /// requests are replayed as E2E #3c sent them: a question, then tool steps, each request
    /// ending on its turn-context block, which the next request no longer carries. Named
    /// (`rapid_mlx_transient_tail`), each step reads the previous request's stable prefix from
    /// the cache; the NEGATIVE CONTROL — the same wrapper, the same steps, the tail not named, the
    /// shape every 3.0.51 split request had — reads only the system prompt on every step (E2E
    /// #3c: 31,385 of 58,379 / 58,774 / 59,600). And the state restored at the boundary is that
    /// prefix's own: the next logits from it equal a cold read of the whole prompt.
    /// Q-342: the conversation's compaction is its next request. The summary request that extends
    /// the chat's (same system, tools, messages and template switches, the instruction last)
    /// reads the whole prefix; the transcript request E2E #3p sent (0 of 97,590) and the same
    /// messages under a thinking switch the chat's requests did not render read nothing.
    #[test]
    fn a_tool_step_reads_the_prefix_before_the_turn_context_through_real_mlx_lm() {
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
        spec.load_lock = Some(launch_load_lock());
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
import tempfile
import urllib.error
import urllib.request

# The generation thread is not a daemon: a failed check must end the process, not wait on it.
sys.excepthook = lambda *failure: (traceback.print_exception(*failure), sys.stderr.flush(), os._exit(1))
mx.set_default_device(mx.cpu)
# CPU only: mlx_lm wires the GPU's working set whenever Metal answers (BatchGenerator.__init__).
mx.metal.is_available = lambda: False
for name in ("MLX_RANK", "MLX_IBV_DEVICES", "MLX_JACCL_COORDINATOR", "MLX_HOSTFILE"):
    os.environ.pop(name, None)

from mlx_lm.models import qwen3_5
from mlx_lm.models.cache import make_prompt_cache
from mlx_lm.utils import save_config, save_model
from tokenizers import Tokenizer, decoders, models, pre_tokenizers
from transformers import PreTrainedTokenizerFast

assert server.ResponseGenerator._tokenize is _tokenize
assert server.APIHandler.handle_chat_completions is handle_chat_completions

alphabet = sorted(pre_tokenizers.ByteLevel.alphabet())
core = Tokenizer(models.BPE(vocab={c: i for i, c in enumerate(alphabet)}, merges=[]))
core.pre_tokenizer = pre_tokenizers.ByteLevel(add_prefix_space=False, use_regex=False)
core.decoder = decoders.ByteLevel()
hf = PreTrainedTokenizerFast(tokenizer_object=core, eos_token="<|im_end|>")
hf.add_special_tokens({"additional_special_tokens": ["<|im_start|>", "<think>", "</think>"]})
hf.chat_template = QWEN38

model_dir = tempfile.mkdtemp()
mx.random.seed(142)
config = {"model_type": "qwen3_5", "text_config": {
    "model_type": "qwen3_5", "hidden_size": 64, "intermediate_size": 128, "num_hidden_layers": 4,
    "num_attention_heads": 2, "num_key_value_heads": 1, "head_dim": 32, "vocab_size": len(hf),
    "linear_num_value_heads": 2, "linear_num_key_heads": 1, "linear_key_head_dim": 16,
    "linear_value_head_dim": 16, "linear_conv_kernel_dim": 4, "full_attention_interval": 2,
    "tie_word_embeddings": False,
}}
tiny = qwen3_5.Model(qwen3_5.ModelArgs.from_dict(config))
mx.eval(tiny.parameters())
save_model(model_dir, tiny)
save_config(config, os.path.join(model_dir, "config.json"))
hf.save_pretrained(model_dir)

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
cli.model = model_dir

provider = server.ModelProvider(cli)
provider._model_map[served] = model_dir
cache = server.LRUPromptCache(cli.prompt_cache_size)
responses = server.ResponseGenerator(provider, cache)
httpd = http.server.ThreadingHTTPServer(
    ("127.0.0.1", 0),
    lambda *args, **kwargs: server.APIHandler(responses, *args, system_fingerprint="test", **kwargs),
)
threading.Thread(target=httpd.serve_forever, daemon=True).start()

def call(path, body=None):
    url = f"http://127.0.0.1:{httpd.server_address[1]}{path}"
    data = None if body is None else json.dumps(body).encode()
    try:
        with urllib.request.urlopen(urllib.request.Request(url, data=data), timeout=600) as reply:
            return [reply.status, json.loads(reply.read())]
    except urllib.error.HTTPError as error:
        return [error.code, json.loads(error.read())]

TOOLS = [{"type": "function", "function": {"name": "shell", "description": "Run a shell command",
          "parameters": {"type": "object", "properties": {"command": {"type": "string"}}}}}]

def block(minute):
    # goose's turn-context block (agents/moim.rs compose_moim) as it joins the message it ends.
    return ("\n<turn-context>\n<current-time>2026-09-26 20:%02d:00</current-time>\n"
            "<working-directory>/Users/mihaiperdum</working-directory>\n\n<ledger>\n"
            "This chat keeps its own ledger.\n</ledger>\n</turn-context>" % minute)

def conversation(system, steps):
    """goose's requests for one question and `steps` tool steps: request k ends on the question
    (k = 0) or tool result k with minute k's block joined to it; the next request carries the
    same message without it (inject_moim moves the block to the newest message)."""
    messages = [{"role": "system", "content": system},
                {"role": "user", "content": "Write the users script and run it."}]
    requests = []
    for k in range(steps + 1):
        tail = block(51 + k)
        sent = json.loads(json.dumps(messages))
        sent[-1]["content"] += tail
        requests.append((sent, tail))
        messages.append({"role": "assistant", "content": f"Step {k}.", "tool_calls": [
            {"id": f"call-{k}", "type": "function",
             "function": {"name": "shell", "arguments": json.dumps({"command": f"node step{k}.js"})}}]})
        messages.append({"role": "tool", "tool_call_id": f"call-{k}", "content": f"step {k} ok\n" * 40})
    return requests

def render(messages, generation=True, tail=None):
    messages = json.loads(json.dumps(messages))
    server.process_message_content(messages)
    if tail is not None:
        messages = [*stable_messages(messages, tail), {"role": "assistant", "content": BOUNDARY_PROBE}]
    kwargs = resolved_template_kwargs({"messages": messages, "tools": TOOLS}, True)
    return provider.tokenizer.apply_chat_template(messages, tools=TOOLS,
        add_generation_prompt=generation, tokenize=True, **kwargs)

def replay(requests, named):
    seen = []
    for messages, tail in requests:
        body = {"model": served, "messages": messages, "tools": TOOLS, "max_tokens": 3, "temperature": 0.0}
        if named:
            body[TRANSIENT_TAIL] = tail
        status, reply = call("/v1/chat/completions", body)
        assert status == 200, reply
        seen.append([reply["usage"]["prompt_tokens"], reply["usage"]["prompt_tokens_details"]["cached_tokens"]])
    return seen

goose = "You are goose, a general-purpose agent. " * 40
named = conversation("A. " + goose, 3)
tailed = replay(named, True)
kept = conversation_prefix.tokens
kept_prefix = [len(kept), conversation_prefix.nbytes] if kept is not None else None
untailed = replay(conversation("B. " + goose, 3), False)
system_end = next(i for i, (a, b) in enumerate(zip(
    render(named[0][0]), render(named[0][0][:1] + [{"role": "user", "content": ""}], generation=False))) if a != b)
bounds = [stable_boundary(render(messages), render(messages, False, tail)) for messages, tail in named]
prompts = [len(render(messages)) for messages, _ in named]

last = render(named[-1][0])
restored, rest = cache.fetch_nearest_cache(provider.model_key, last)
warm = provider.model(mx.array([rest]), cache=restored)[0, -1]
cold = provider.model(mx.array([last]), cache=make_prompt_cache(provider.model))[0, -1]

# Q-342: the auto-compaction at the next reply's start. The last step was answered in words, the
# person's next message arrived, and goose sends ONE summary request, no tail named.
INSTRUCTION = ("Task Context:\n- An llm context limit was reached when a user was in a working "
               "session with an agent (you)\n- Generate a version of the messages above with only "
               "the most verbose parts removed\n- Summarize now, as your reply: call no tool")
answered = [*stable_messages(json.loads(json.dumps(named[-1][0])), named[-1][1]),
            {"role": "assistant", "content": "The script ran: 414 users written."},
            {"role": "user", "content": "Now plan the migration waves.\n" + INSTRUCTION}]
transcript = [{"role": "system", "content": INSTRUCTION + "\n\n**Conversation History:**\n"
               + "\n".join(f"[{m['role']}]: {m.get('content')}" for m in answered[1:-1])},
              {"role": "user", "content": "Please summarize the conversation history provided in "
               "the system prompt."}]

def summarize(messages, tools, kwargs):
    body = {"model": served, "messages": messages, "max_tokens": 3, "temperature": 0.0}
    if tools:
        body["tools"] = tools
    if kwargs:
        body["chat_template_kwargs"] = kwargs
    status, reply = call("/v1/chat/completions", body)
    assert status == 200, reply
    return [reply["usage"]["prompt_tokens"], reply["usage"]["prompt_tokens_details"]["cached_tokens"]]

compaction = {}
compaction["extends_chat"] = summarize(answered, TOOLS, None)
compaction["helper_switch"] = summarize(answered, TOOLS, {"enable_thinking": False})
compaction["thinking_on"] = summarize(answered, TOOLS, {"enable_thinking": True})
compaction["transcript"] = summarize(transcript, None, {"enable_thinking": False})

print("GOOSE_TEST " + json.dumps({
    "compaction": compaction,
    "tailed": tailed,
    "untailed": untailed,
    "system_end": system_end,
    "bounds": bounds,
    "prompts": prompts,
    "kept_prefix": kept_prefix,
    "kept_after_untailed": conversation_prefix.tokens is kept,
    "restored_kinds": sorted({type(layer).__name__ for layer in restored}),
    "warm_equals_cold": bool(mx.allclose(warm, cold, atol=1e-4).item())
        and int(mx.argmax(warm).item()) == int(mx.argmax(cold).item()),
    "not_a_string": call("/v1/chat/completions", {"model": served,
        "messages": [{"role": "user", "content": "hi"}], TRANSIENT_TAIL: 7}),
}), flush=True)
os._exit(0)
"#;
        let program = format!(
            "{}\
             class _Group:\n    def rank(self): return 0\n    def size(self): return 2\n\
             group = _Group()\n{}QWEN38 = {qwen}\n{checks}",
            tensor_modules(),
            &wrapper[start..end],
            qwen = serde_json::to_string(QWEN38).unwrap(),
        );
        let seen = run_against_real_packages(&python, &program, &spec);
        let pairs = |key: &str| -> Vec<(u64, u64)> {
            serde_json::from_value(seen[key].clone()).unwrap_or_else(|e| panic!("{key}: {e}"))
        };
        let bounds: Vec<u64> = serde_json::from_value(seen["bounds"].clone()).unwrap();
        let prompts: Vec<u64> = serde_json::from_value(seen["prompts"].clone()).unwrap();
        let system_end = seen["system_end"].as_u64().unwrap();
        let tailed = pairs("tailed");
        let untailed = pairs("untailed");
        assert_eq!(
            tailed.iter().map(|(prompt, _)| *prompt).collect::<Vec<_>>(),
            prompts,
            "the rank served the prompt goose's messages render to"
        );
        assert_eq!(tailed[0].1, 0, "the question arrives cold");
        for step in 1..tailed.len() {
            assert_eq!(
                tailed[step].1,
                bounds[step - 1],
                "step {step} reads the previous request's prefix up to its turn-context block: \
                 {tailed:?} vs boundaries {bounds:?}"
            );
            assert!(
                bounds[step - 1] > system_end && prompts[step] - tailed[step].1 < prompts[step] / 3,
                "step {step} re-reads only the new tool step: {tailed:?}"
            );
            assert_eq!(
                untailed[step].1, system_end,
                "the negative control, the tail unnamed, reads only the system prompt at step \
                 {step} — E2E #3c's shape: {untailed:?}"
            );
        }
        assert_eq!(
            seen["kept_prefix"][0].as_u64(),
            bounds.last().copied(),
            "Q-294: the conversation prefix is the last tail-naming request's boundary: {}",
            seen["kept_prefix"]
        );
        assert!(seen["kept_prefix"][1]
            .as_u64()
            .is_some_and(|bytes| bytes > 0));
        assert_eq!(
            seen["kept_after_untailed"], true,
            "requests that name no tail (goose's helpers) never become the conversation prefix"
        );
        assert_eq!(
            seen["restored_kinds"],
            serde_json::json!(["ArraysCache", "KVCache"]),
            "the cache that was reused holds the hybrid's non-trimmable state"
        );
        assert_eq!(
            seen["warm_equals_cold"], true,
            "the state restored at the boundary continues exactly as a cold read of the prompt"
        );
        assert_eq!(seen["not_a_string"][0], 400, "{}", seen["not_a_string"]);

        let compaction = |shape: &str| -> (u64, u64) {
            serde_json::from_value(seen["compaction"][shape].clone())
                .unwrap_or_else(|e| panic!("{shape}: {e}"))
        };
        let (prompt, read) = compaction("extends_chat");
        assert_eq!(
            read,
            *bounds.last().unwrap(),
            "Q-342: the summary request that extends the chat's request reads the conversation \
             prefix its last call left: {}",
            seen["compaction"]
        );
        assert!(
            prompt - read < prompt / 3,
            "and prefills only the answer and the instruction: {}",
            seen["compaction"]
        );
        assert!(
            compaction("helper_switch").1 >= read,
            "Q-135: a request carrying tools renders thinking-off whether it pins the switch or \
             not, so on this engine the helper's switch did not break #3p's compaction — its \
             shape did: {}",
            seen["compaction"]
        );
        assert_eq!(
            compaction("thinking_on").1,
            0,
            "NEGATIVE CONTROL: a thinking switch the chat's requests did not render reads nothing \
             — Qwen3.8 writes its reasoning-effort line into the system block — so the summary \
             request carries the chat's own switches: {}",
            seen["compaction"]
        );
        assert_eq!(
            compaction("transcript").1,
            0,
            "NEGATIVE CONTROL: the transcript request (E2E #3p: 0 of 97,590) reads nothing: {}",
            seen["compaction"]
        );
    }

    /// Q-347 through the REAL mlx_lm 0.31.3 `LRUPromptCache`, at E2E #3p's measured sizes (the 27B
    /// over 2 ranks: 32,768 B of KV per token + 76,972,032 B of state, the 11,830,886,400 B plan of
    /// #3p's RANK_ADMISSION lines). The chat's stable head is 40,361 tokens — the 27B's own template
    /// over the Q-342 captures renders one head (system prompt + 81 tools) for req6, req8 and the
    /// post-compaction request, thinking off as the split renders them (that render gives exactly the
    /// engine's 44,053 and 139,503) — so its entry holds 1,399,521,280 B (#3p 12:35:27: system 1
    /// sequence, 1.40 GB). The session opens cold at 41,137 tokens (10:54:29), grows by agent steps
    /// each beside its tool label (Q-182's 46-token system segment), and every turn ends in the
    /// helper burst of 12:21:19 (the CANCELLED lines' 2,027 … 10,751 … 7,250 tokens, each a fact
    /// check's 380-token system segment and a context segment ending 384 tokens early) until the
    /// conversation prefix reaches #3p's 4.74 GB; then the compaction (Q-342's shape: the chat's
    /// request + the instruction) and the first request of the compacted chat (44,053 tokens). The
    /// NEGATIVE CONTROL is the 3.0.69 rule (the conversation prefix kept, no head): it reads 0 of
    /// 44,053, #3p's own number. Holding the head in eviction alone, or leaving it room at
    /// admission alone, still reads 0; both read the whole head, with no agent step reading less,
    /// no insert past the plan, and at most two helpers of a burst waiting for the batch to drain.
    #[test]
    fn a_compacted_chat_reads_its_head_at_e2e_3p_sizes_through_real_mlx_lm() {
        let Some(python) = proven_env(&EnvSpec::tensor(), "GOOSE_TEST_TENSOR_PYTHON") else {
            return;
        };
        let checks = r#"
from mlx_lm.models.cache import LRUPromptCache

p = {"kv_bytes_per_token": 32768, "sequence_state_bytes": 76972032, "batch_transient_ratio": 2.2}
limit = 11830886400


class Layer:
    def __init__(self, tokens):
        self.nbytes = batch_kv_charge(p, 1, tokens)

    def is_trimmable(self):
        return False


HEAD, FIRST, AFTER = 40361, 41137, 44053
TAIL, OUT = 668, 150
LABEL_SYSTEM, LABEL, LABEL_OUT = 46, 154, 9
HELPERS = [2027, 1525, 1981, 986, 922, 10751, 6340, 7250]
HELPER_SYSTEM, HELPER_TAIL, HELPER_OUT = 380, 384, 20
# 12 turns of 5 steps from 41,137 tokens: the last step sends 142,957, its prefix 142,289 — #3p's
# conversation prefix at the compaction (4,738,646,016 B = 142,263 tokens).
TURNS, STEPS, STEP = 12, 5, 1697
upstream_pop = LRUPromptCache.CacheOrder.pop


def replay(keep_head, reserve_head):
    LRUPromptCache.CacheOrder.pop = upstream_pop
    conversation, head = KeptEntry(), KeptEntry()
    keep_entries(LRUPromptCache.CacheOrder, *((conversation, head) if keep_head else (conversation,)))
    cache = LRUPromptCache(max_size=limit // batch_kv_charge(p, 1, LABEL_SYSTEM), max_bytes=limit)
    tokens = list(range(FIRST + TURNS * STEPS * STEP))
    serial = [0]
    over = []

    def fresh(n):
        serial[0] += 1
        return [-serial[0] * 1000000 - i for i in range(n)]

    def insert(key, kind, room):
        layers = [Layer(len(key))]
        for entry in (conversation, head):
            entry.inserted(key, sum(layer.nbytes for layer in layers))
        cache.insert_cache("m", key, layers, cache_type=kind)
        cache.trim_to(n_bytes=room)
        forget_dropped(cache._lru, (conversation, head))
        if cache.nbytes > room:
            over.append((len(key), cache.nbytes, room))

    def agent(prompt_tokens, ends_on_user):
        prompt = tokens[:prompt_tokens]
        found, rest = cache.fetch_nearest_cache("m", prompt)
        read = prompt_tokens - len(rest) if found is not None else 0
        room = limit - batch_kv_charge(p, 1, prompt_tokens)
        cache.trim_to(n_bytes=room)
        stable = prompt[: prompt_tokens - TAIL]
        head.cut(prompt[:HEAD])
        conversation.cut(stable)
        # mlx_lm cuts the system segment itself on a request ending on a user message; a tool step
        # ends one there only under the new rule. Either is snapshotted only if read short of it.
        if read < HEAD and (ends_on_user or keep_head):
            insert(prompt[:HEAD], "system", room)
        if read < len(stable):
            insert(stable[:], "user", room)
        insert(prompt + fresh(OUT), "assistant", limit - batch_kv_charge(p, 1, prompt_tokens + OUT))
        return read

    def burst():
        rows, width, held = 0, 0, []
        reserve = head.nbytes if reserve_head else 0
        for size in HELPERS:
            if not admits(p, limit, rows, width, size, conversation.nbytes, reserve):
                held.append(size)
                continue
            rows, width = rows + 1, max(width, size)
            room = limit - batch_kv_charge(p, rows, width)
            cache.trim_to(n_bytes=room)
            system = fresh(HELPER_SYSTEM)
            insert(system, "system", room)
            insert(system + fresh(size - HELPER_TAIL - HELPER_SYSTEM), "user", room)
        for size in HELPERS:
            if size not in held:
                insert(fresh(size + HELPER_OUT), "assistant", limit - batch_kv_charge(p, 1, width))
        return held

    reads, held = [agent(FIRST, True)], []
    width = FIRST
    for _ in range(TURNS):
        for _ in range(STEPS):
            width += STEP
            label = fresh(LABEL_SYSTEM)
            insert(label, "system", limit - batch_kv_charge(p, 1, LABEL))
            reads.append(agent(width, False))
            insert(label + fresh(LABEL - LABEL_SYSTEM + LABEL_OUT), "assistant",
                   limit - batch_kv_charge(p, 1, width))
        held.append(burst())
    compaction = tokens[:width] + fresh(200)
    found, rest = cache.fetch_nearest_cache("m", compaction)
    compaction_read = len(compaction) - len(rest) if found is not None else 0
    insert(compaction[:-4], "user", limit - batch_kv_charge(p, 1, len(compaction)))
    insert(compaction + fresh(2500), "assistant", limit)
    after = tokens[:HEAD] + fresh(AFTER - HEAD)
    found, rest = cache.fetch_nearest_cache("m", after)
    return {
        "after": AFTER - len(rest) if found is not None else 0,
        "compaction": compaction_read,
        "reads": reads,
        "held": [len(h) for h in held],
        "over": over,
        "head": head.nbytes,
        "prefix": conversation.nbytes,
        "width": width,
    }


control = replay(keep_head=False, reserve_head=False)
fixed = replay(keep_head=True, reserve_head=True)
assert control["width"] - TAIL == 142289 and control["compaction"] == 142289, control["compaction"]
assert control["after"] == 0, f"E2E 3p read 0 of 44,053; the replay read {control['after']}"
assert replay(keep_head=True, reserve_head=False)["after"] == 0, "the eviction alone"
assert replay(keep_head=False, reserve_head=True)["after"] == 0, "the room alone"
assert fixed["after"] == HEAD, fixed["after"]
assert fixed["head"] == HEAD * 32768 + 76972032 == 1399521280, fixed["head"]
assert fixed["reads"] == control["reads"], "no agent step reads less"
assert all(read == FIRST + (i - 1) * STEP - TAIL for i, read in enumerate(fixed["reads"]) if i)
assert fixed["compaction"] == control["compaction"]
assert not fixed["over"] and not control["over"], (fixed["over"], control["over"])
assert max(fixed["held"]) <= 2, fixed["held"]
assert fixed["prefix"] + fixed["head"] <= limit
LRUPromptCache.CacheOrder.pop = upstream_pop
print("ok", fixed["held"], control["held"])
"#;
        let out = std::process::Command::new(&python)
            .arg("-c")
            .arg(format!(
                "{}{}{checks}",
                include_str!("rank_prefill.py"),
                include_str!("rank_boundary.py")
            ))
            .output()
            .expect("the tensor venv's python runs");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            String::from_utf8_lossy(&out.stdout).starts_with("ok"),
            "{}",
            String::from_utf8_lossy(&out.stdout)
        );
    }

    /// The tiny-qwen3_5 server the real-mlx_lm replays drive (see
    /// `a_tool_step_reads_the_prefix_before_the_turn_context_through_real_mlx_lm`): mlx_lm 0.31.3's
    /// own HTTP handler, generation loop, BatchGenerator and LRU prompt cache under the wrapper, on
    /// the CPU, through the Qwen3.8 template with one token per byte; `conversation` renders
    /// goose's agent requests (a question, then tool steps, each ending on its turn-context block).
    const TINY_QWEN35_SERVER: &str = r#"
import argparse
import http.server
import tempfile
import urllib.error
import urllib.request

# The generation thread is not a daemon: a failed check must end the process, not wait on it.
sys.excepthook = lambda *failure: (traceback.print_exception(*failure), sys.stderr.flush(), os._exit(1))
mx.set_default_device(mx.cpu)
# CPU only: mlx_lm wires the GPU's working set whenever Metal answers (BatchGenerator.__init__).
mx.metal.is_available = lambda: False
for name in ("MLX_RANK", "MLX_IBV_DEVICES", "MLX_JACCL_COORDINATOR", "MLX_HOSTFILE"):
    os.environ.pop(name, None)

from mlx_lm.models import qwen3_5
from mlx_lm.models.cache import make_prompt_cache
from mlx_lm.utils import save_config, save_model
from tokenizers import Tokenizer, decoders, models, pre_tokenizers
from transformers import PreTrainedTokenizerFast

assert server.ResponseGenerator._tokenize is _tokenize
assert server.APIHandler.handle_chat_completions is handle_chat_completions

alphabet = sorted(pre_tokenizers.ByteLevel.alphabet())
core = Tokenizer(models.BPE(vocab={c: i for i, c in enumerate(alphabet)}, merges=[]))
core.pre_tokenizer = pre_tokenizers.ByteLevel(add_prefix_space=False, use_regex=False)
core.decoder = decoders.ByteLevel()
hf = PreTrainedTokenizerFast(tokenizer_object=core, eos_token="<|im_end|>")
hf.add_special_tokens({"additional_special_tokens": ["<|im_start|>", "<think>", "</think>"]})
hf.chat_template = QWEN38

model_dir = tempfile.mkdtemp()
mx.random.seed(142)
config = {"model_type": "qwen3_5", "text_config": {
    "model_type": "qwen3_5", "hidden_size": 64, "intermediate_size": 128, "num_hidden_layers": 4,
    "num_attention_heads": 2, "num_key_value_heads": 1, "head_dim": 32, "vocab_size": len(hf),
    "linear_num_value_heads": 2, "linear_num_key_heads": 1, "linear_key_head_dim": 16,
    "linear_value_head_dim": 16, "linear_conv_kernel_dim": 4, "full_attention_interval": 2,
    "tie_word_embeddings": False,
}}
tiny = qwen3_5.Model(qwen3_5.ModelArgs.from_dict(config))
mx.eval(tiny.parameters())
save_model(model_dir, tiny)
save_config(config, os.path.join(model_dir, "config.json"))
hf.save_pretrained(model_dir)

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
cli.model = model_dir

provider = server.ModelProvider(cli)
provider._model_map[served] = model_dir
cache = server.LRUPromptCache(cli.prompt_cache_size)
responses = server.ResponseGenerator(provider, cache)
httpd = http.server.ThreadingHTTPServer(
    ("127.0.0.1", 0),
    lambda *args, **kwargs: server.APIHandler(responses, *args, system_fingerprint="test", **kwargs),
)
threading.Thread(target=httpd.serve_forever, daemon=True).start()

def call(path, body=None):
    url = f"http://127.0.0.1:{httpd.server_address[1]}{path}"
    data = None if body is None else json.dumps(body).encode()
    try:
        with urllib.request.urlopen(urllib.request.Request(url, data=data), timeout=600) as reply:
            return [reply.status, json.loads(reply.read())]
    except urllib.error.HTTPError as error:
        return [error.code, json.loads(error.read())]

TOOLS = [{"type": "function", "function": {"name": "shell", "description": "Run a shell command",
          "parameters": {"type": "object", "properties": {"command": {"type": "string"}}}}}]

def block(minute):
    # goose's turn-context block (agents/moim.rs compose_moim) as it joins the message it ends.
    return ("\n<turn-context>\n<current-time>2026-09-26 20:%02d:00</current-time>\n"
            "<working-directory>/Users/mihaiperdum</working-directory>\n\n<ledger>\n"
            "This chat keeps its own ledger.\n</ledger>\n</turn-context>" % minute)

def conversation(system, steps):
    """goose's requests for one question and `steps` tool steps: request k ends on the question
    (k = 0) or tool result k with minute k's block joined to it; the next request carries the
    same message without it (inject_moim moves the block to the newest message)."""
    messages = [{"role": "system", "content": system},
                {"role": "user", "content": "Write the users script and run it."}]
    requests = []
    for k in range(steps + 1):
        tail = block(51 + k)
        sent = json.loads(json.dumps(messages))
        sent[-1]["content"] += tail
        requests.append((sent, tail))
        messages.append({"role": "assistant", "content": f"Step {k}.", "tool_calls": [
            {"id": f"call-{k}", "type": "function",
             "function": {"name": "shell", "arguments": json.dumps({"command": f"node step{k}.js"})}}]})
        messages.append({"role": "tool", "tool_call_id": f"call-{k}", "content": f"step {k} ok\n" * 40})
    return requests

def render(messages, generation=True, tail=None):
    messages = json.loads(json.dumps(messages))
    server.process_message_content(messages)
    if tail is not None:
        messages = [*stable_messages(messages, tail), {"role": "assistant", "content": BOUNDARY_PROBE}]
    kwargs = resolved_template_kwargs({"messages": messages, "tools": TOOLS}, True)
    return provider.tokenizer.apply_chat_template(messages, tools=TOOLS,
        add_generation_prompt=generation, tokenize=True, **kwargs)

"#;

    /// Q-347 through the REAL mlx_lm 0.31.3 server on a tiny qwen3_5 (the 27B's hybrid: its
    /// linear-attention state cannot be trimmed), with the prompt cache bounded to a few entries so
    /// goose's end-of-turn side calls (fact checks: a system prompt of their own, ending on a user
    /// message) press on it after every agent step, as E2E #3p's did. Then the chat is compacted —
    /// the same system prompt and tools, the summary as the first message (#3p 12:32:50: 44,053
    /// tokens, 0 read, 150 s) — and a new chat opens on the same system prompt and tools. With
    /// `keep_stable_head` the compacted chat and the new chat each read exactly the stable head
    /// (mlx_lm's own system-segment end: the system prompt + tools), the head survived every
    /// side-call burst, the state restored there continues exactly as a cold read, and a tool step
    /// that reads less than the head (the cache emptied) cuts and re-makes it. The NEGATIVE
    /// CONTROL — the same wrapper, the same requests, a 3.0.69 spec (no `keep_stable_head`) — reads
    /// 0 after the compaction: the side calls' system segments took the head's only entry.
    #[test]
    fn a_compacted_chat_reads_its_system_prompt_and_tools_from_the_cache_through_real_mlx_lm() {
        let Some(python) = proven_env(&EnvSpec::tensor(), "GOOSE_TEST_TENSOR_PYTHON") else {
            return;
        };
        let config = two_mac_config();
        let tensor = TensorLaunch {
            planned_bytes: 27_456_216_576,
            prompt_cache_limit_bytes: 9_431_744_512,
            prompt_cache_entries: 6,
            mlx_cache_limit_bytes: 2_745_621_658,
            prefill: e2e_prefill(),
        };
        let spec_with = |keep_stable_head: bool| {
            let mut spec = rank_specs(
                &config,
                &ServedNames::only("node-alias"),
                &[tensor, tensor],
                141_568,
                2.0,
            )
            .remove(0);
            if let RankProgram::MlxLmServer {
                doorbell,
                keep_stable_head: keep,
                ..
            } = &mut spec.program
            {
                *doorbell = false;
                *keep = keep_stable_head;
            }
            spec.load_lock = Some(launch_load_lock());
            spec
        };
        let wrapper = include_str!("rank_wrapper.py");
        let start = wrapper
            .find("import faulthandler  # noqa")
            .expect("the wrapper's body starts after the group check");
        let end = wrapper
            .find("server.main()")
            .expect("the wrapper ends in mlx_lm's main");
        let checks = r#"
def send(messages, tail=None, tools=TOOLS):
    body = {"model": served, "messages": messages, "max_tokens": 2, "temperature": 0.0}
    if tools:
        body["tools"] = tools
    if tail is not None:
        body[TRANSIENT_TAIL] = tail
    status, reply = call("/v1/chat/completions", body)
    assert status == 200, reply
    return [reply["usage"]["prompt_tokens"], reply["usage"]["prompt_tokens_details"]["cached_tokens"]]

# goose's helpers as E2E #3p sent them (the Q-342 captures req0-req9): three system prompts of their
# own — the fact checker, the reviewer, the tool labeler — no tools, no tail named.
HELPERS = ["You are goose's end-of-turn fact checker. Check each reply against the tool results. " * 2,
           "You are goose's end-of-turn reviewer. You are shown the FACTS of one turn. " * 2,
           "Summarize this tool call in a short lowercase phrase (3-8 words). No punctuation. "]

def side_call(k):
    return send([{"role": "system", "content": HELPERS[k % len(HELPERS)]},
                 {"role": "user", "content": f"Reply {k}: the script wrote {k} users.\n" * 3}], tools=None)

def kinds():
    return {kind: [len(tokens) for _, tokens in lru] for kind, lru in cache._lru._lrus.items()}

SYSTEM = "A. " + "You are goose, a general-purpose agent. " * 40
steps = conversation(SYSTEM, 3)
agent, side, head_seen = [], [], []
for messages, tail in steps:
    agent.append(send(messages, tail))
    # mlx_lm's own system-segment end (the model, and its tokenizer, load at the first request).
    system_end = next(i for i, (a, b) in enumerate(zip(
        render(steps[0][0]), render(steps[0][0][:1] + [{"role": "user", "content": ""}], generation=False))) if a != b)
    side.extend(side_call(len(side)) for _ in range(3))
    head_seen.append(any(len(tokens) == system_end for lru in cache._lru._lrus.values() for _, tokens in lru))
after_bursts = kinds()

def compacted(summary, ask, minute):
    return [{"role": "system", "content": SYSTEM},
            {"role": "user", "content": summary},
            {"role": "assistant", "content": "Your context was compacted. The previous message contains a summary of the conversation so far."},
            {"role": "user", "content": ask + block(minute)}], block(minute)

after = send(*compacted("<analysis>\nThe users script ran in four steps.\n</analysis>", "Run the plan on the fake data.", 58))
fresh = send([{"role": "system", "content": SYSTEM}, {"role": "user", "content": "A new question." + block(59)}], block(59))

# The state restored at the head continues exactly as a cold read (a compacted chat not yet sent).
other, _ = compacted("<analysis>\nAnother summary.\n</analysis>", "And the totals?", 57)
prompt = render(other)
restored, rest = cache.fetch_nearest_cache(provider.model_key, prompt)
warm = provider.model(mx.array([rest]), cache=restored)[0, -1] if restored is not None else None
cold = provider.model(mx.array([prompt]), cache=make_prompt_cache(provider.model))[0, -1]
restored_at = len(prompt) - len(rest) if restored is not None else 0

# A tool step that reads less than the head (the cache emptied) cuts and re-makes it.
cache.trim_to(n_sequences=0)
emptied = stable_head.tokens if stable_head is not None else "off"
tool_messages, tool_tail = conversation(SYSTEM, 1)[1]
assert tool_messages[-1]["role"] == "tool"
remade = send(tool_messages, tool_tail)

print("GOOSE_TEST " + json.dumps({
    "system_end": system_end,
    "agent": agent,
    "side": side,
    "head_seen": head_seen,
    "after_bursts": after_bursts,
    "after": after,
    "fresh": fresh,
    "restored_at": restored_at,
    "warm_equals_cold": warm is not None and bool(mx.allclose(warm, cold, atol=1e-4).item())
        and int(mx.argmax(warm).item()) == int(mx.argmax(cold).item()),
    "head": [len(stable_head.tokens), stable_head.nbytes] if stable_head is not None and stable_head.tokens is not None else None,
    "emptied": emptied is None,
    "remade": remade,
    "remade_head": len(stable_head.tokens) if stable_head is not None and stable_head.tokens is not None else None,
    "remade_kinds": kinds(),
}), flush=True)
os._exit(0)
"#;
        let run = |keep_stable_head: bool| {
            let program = format!(
                "{}\
                 class _Group:\n    def rank(self): return 0\n    def size(self): return 2\n\
                 group = _Group()\n{}QWEN38 = {qwen}\n{TINY_QWEN35_SERVER}{checks}",
                tensor_modules(),
                &wrapper[start..end],
                qwen = serde_json::to_string(QWEN38).unwrap(),
            );
            run_against_real_packages(&python, &program, &spec_with(keep_stable_head))
        };
        let pair = |seen: &serde_json::Value, key: &str| -> (u64, u64) {
            serde_json::from_value(seen[key].clone()).unwrap_or_else(|e| panic!("{key}: {e}"))
        };

        let kept = run(true);
        let system_end = kept["system_end"].as_u64().unwrap();
        assert_eq!(
            kept["head_seen"],
            serde_json::json!([true, true, true, true]),
            "the head's entry is in the cache after every agent step and its side calls: {}",
            kept["after_bursts"]
        );
        assert_eq!(
            pair(&kept, "after").1,
            system_end,
            "Q-347: the first request of the compacted chat reads exactly the system prompt + tools \
             (#3p: 0 of 44,053): {kept}"
        );
        assert_eq!(
            pair(&kept, "fresh").1,
            system_end,
            "a new chat with the same system prompt and tools reads it too: {kept}"
        );
        assert_eq!(kept["restored_at"].as_u64(), Some(system_end));
        assert_eq!(
            kept["warm_equals_cold"], true,
            "the state restored at the head continues exactly as a cold read of the prompt"
        );
        assert_eq!(
            kept["head"][0].as_u64(),
            Some(system_end),
            "the kept head is the system prompt + tools: {}",
            kept["head"]
        );
        assert!(kept["head"][1].as_u64().is_some_and(|bytes| bytes > 0));
        assert_eq!(kept["emptied"], true, "an emptied cache holds no head");
        assert_eq!(
            pair(&kept, "remade").1,
            0,
            "the tool step after the cache emptied reads nothing"
        );
        assert_eq!(
            kept["remade_head"].as_u64(),
            Some(system_end),
            "and a request ending on tool results cuts the head and re-makes it: {}",
            kept["remade_kinds"]
        );
        let side: Vec<(u64, u64)> = serde_json::from_value(kept["side"].clone()).unwrap();
        let agent: Vec<(u64, u64)> = serde_json::from_value(kept["agent"].clone()).unwrap();

        let control = run(false);
        assert_eq!(
            serde_json::from_value::<Vec<(u64, u64)>>(control["agent"].clone()).unwrap(),
            agent,
            "the head changes nothing the agent's own steps read"
        );
        assert_eq!(
            serde_json::from_value::<Vec<(u64, u64)>>(control["side"].clone()).unwrap(),
            side,
            "nor what the side calls read"
        );
        assert_eq!(
            pair(&control, "after").1,
            0,
            "NEGATIVE CONTROL (a 3.0.69 spec): the side calls took the head's only entry and the \
             compacted chat reads nothing — #3p's shape: {control}"
        );
        assert_eq!(control["head"], serde_json::Value::Null);
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
install_positional_parameters(q)

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

strays = []

def run(pieces, tools):
    stream = ToolCallStream(q._convert_param_value, q._get_arguments_config)
    opened, sent = [], ""
    strays.clear()
    for piece in pieces:
        name, fragment = stream.feed(piece, tools)
        if name is not None:
            opened.append(name)
        sent += fragment
        strays.append(stream.stray())
    rest, why = stream.close(q.parse_tool_call, tools)
    return opened, sent, rest, why

for case, (text, tools) in EXACT.items():
    whole = q.parse_tool_call(text, tools)
    expected = json.dumps(whole["arguments"], ensure_ascii=False)
    for how, pieces in chunkings(text):
        assert "".join(pieces) == text
        opened, sent, rest, why = run(pieces, tools)
        # Q-146: a call the parser reads is never reported as text the streamer cannot read.
        assert strays == [None] * len(pieces), (case, how, [s for s in strays if s])
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

# Q-146: a call written as JSON inside <tool_call> reads as stray from its first character on, and
# so does prose between two parameters.
JSON_CALL = '\n{"name": "shell", "arguments": {"command": "ls"}}\n'
run(list(JSON_CALL), TOOLS)
assert all(s is not None for s in strays[1:]) and strays[-1].startswith('{"name"'), strays[-3:]
PROSE = call("shell", ("command", "ls"))[:-len("</function>\n")] + "and now the timeout: 30\n"
run(list(PROSE), TOOLS)
assert strays[-1] == "and now the timeout: 30\n", strays[-1]
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

    /// Q-371 / Q-372: the text of a written file goes through the split's tool-call path (mlx_lm
    /// 0.31.3's qwen3_coder parse as the wrapper installs it, and the streamer, in every chunking)
    /// byte for byte — arrows and comparisons (`=>`, `->`, `>=`, `<=`, `>>>`, `a=>b`), HTML
    /// entities, header-shaped text (`<parameter=x>`, `<function=…>`, `</function>`,
    /// `</tool_call>`) and the close tag itself inside a string (`"</parameter>"`), with the value
    /// written first or last. Q-371 (E2E #3r, sessions.db 771288) lost 15 of 16 `=>` — the rank's
    /// token trail proved the MODEL sampled them away, not this path; this pins that the path
    /// keeps them. NEGATIVE CONTROL: mlx_lm's own first-`</parameter>` reading (before
    /// `install_positional_parameters`) cuts the same content at `const close = "` and still
    /// returns the call as a success — Q-372, the silent truncation this fixes.
    #[test]
    fn a_written_file_crosses_the_tensor_tool_path_byte_for_byte() {
        let Some(python) = proven_env(&EnvSpec::tensor(), "GOOSE_TEST_TENSOR_PYTHON") else {
            return;
        };
        let checks = r##"
import random
from mlx_lm.tool_parsers import qwen3_coder as q

TOOLS = [{"type": "function", "function": {"name": "write", "parameters": {"type": "object",
    "properties": {"path": {"type": "string"}, "content": {"type": "string"}}}}}]
CONTENT = "\n".join([
    "const rnd = () => ((_s = (_s * 1664525 + 1013904223) >>> 0) / 4294967296);",
    "const pick = (a) => a[Math.floor(rnd() * a.length)];",
    "rows.map((r) =>\n  [r.username].join(','));",
    "x->y; a >= b; a <= b; z >>> 0; a=>b; f(a)=>a; if (a<b && c>d) {}",
    "html: &lt;div&gt; &amp; &quot;q&quot; &#62; &#x3E; &gt;= <br/>",
    "fake markup: <parameter=x> <function=shell> </function> </tool_call> <parameter=path",
    'const close = "</parameter>";  // the close tag inside a string',
    "  </parameter>  (indented, still the file's own text)",
    "tail => done",
])

def call(*params):
    body = "".join(f"<parameter={key}>\n{value}\n</parameter>\n" for key, value in params)
    return f"\n<function=write>\n{body}</function>\n"

LAST = call(("path", "/tmp/gen.js"), ("content", CONTENT))
FIRST = call(("content", CONTENT), ("path", "/tmp/gen.js"))

shipped = q.parse_tool_call(LAST, TOOLS)["arguments"]["content"]
assert shipped == CONTENT[:CONTENT.index('"</parameter>') + 1], ("the shipped reading", shipped[-60:])

install_positional_parameters(q)

def chunkings(text):
    yield "whole", [text]
    yield "chars", list(text)
    rng = random.Random(371)
    for seed in range(4):
        pieces, i = [], 0
        while i < len(text):
            n = rng.randint(1, 7)
            pieces.append(text[i:i + n])
            i += n
        yield f"random{seed}", pieces

for label, text in (("content_last", LAST), ("content_first", FIRST)):
    parsed = q.parse_tool_call(text, TOOLS)
    assert parsed["name"] == "write", parsed
    assert parsed["arguments"] == {"path": "/tmp/gen.js", "content": CONTENT}, (label, parsed)
    expected = json.dumps(parsed["arguments"], ensure_ascii=False)
    for how, pieces in chunkings(text):
        stream = ToolCallStream(q._convert_param_value, q._get_arguments_config)
        sent = ""
        streamed_before_first_close = None
        for piece in pieces:
            _, fragment = stream.feed(piece, TOOLS)
            sent += fragment
            if streamed_before_first_close is None and '"</parameter>' in stream.text:
                streamed_before_first_close = len(sent)
        rest, why = stream.close(q.parse_tool_call, TOOLS)
        assert why is None, (label, how, why)
        assert sent + rest == expected, (label, how, sent + rest, expected)
        assert json.loads(sent + rest)["content"] == CONTENT, (label, how)
        assert streamed_before_first_close > len(CONTENT[:CONTENT.index("</parameter>")]) // 2, (
            label, how, "the value streamed while it was written", streamed_before_first_close)
print("ok")
"##;
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

    /// Q-232: rank_tool_schema.py under a plain interpreter — the type each schema shape names.
    /// A nullable union names its one other type and "string" wins any union holding it; anyOf /
    /// oneOf / allOf branches and a local `$ref` are read by the same rule; a union of two other
    /// types, an unresolvable or cyclic reference and a schema naming nothing name NO type, and
    /// such a property reaches the parser exactly as written. The request's tools are never
    /// changed.
    #[test]
    fn a_tool_parameters_type_is_read_through_its_union_or_reference() {
        let checks = r##"
import copy

DEFS = {"Crop": {"type": "object", "properties": {"x": {"type": "integer"}}},
        "Action": {"type": "string", "enum": ["enable", "disable"]},
        "Loop": {"$ref": "#/$defs/Loop"},
        "Wrapped": {"allOf": [{"$ref": "#/definitions/Crop"}]}}
DEFS_OLD = {"Crop": DEFS["Crop"]}
cases = [
    ({"type": "string"}, "string"),
    ({"type": ["string", "null"]}, "string"),
    ({"type": ["null", "string"]}, "string"),
    ({"type": ["integer", "null"]}, "integer"),
    ({"type": ["boolean", "null"]}, "boolean"),
    ({"type": ["object", "null"]}, "object"),
    ({"type": ["array", "null"]}, "array"),
    ({"type": ["string", "integer"]}, "string"),
    ({"type": ["integer", "boolean"]}, None),
    ({"type": ["null"]}, None),
    ({"type": "null"}, "null"),
    ({"anyOf": [{"type": "string"}, {"type": "null"}]}, "string"),
    ({"anyOf": [{"type": "integer"}, {"type": "null"}]}, "integer"),
    ({"anyOf": [{"type": "string", "format": "uri"}, {"type": "array"}]}, "string"),
    ({"anyOf": [{"type": "integer"}, {"type": "array"}]}, None),
    ({"anyOf": [{"type": "integer"}, {}]}, None),
    ({"anyOf": [{"type": "string"}, {}]}, "string"),
    ({"oneOf": [{"$ref": "#/$defs/Crop"}, {"type": "null"}]}, "object"),
    ({"$ref": "#/$defs/Crop", "default": None}, "object"),
    ({"$ref": "#/$defs/Action"}, "string"),
    ({"$ref": "#/$defs/Wrapped"}, "object"),
    ({"$ref": "#/$defs/Loop"}, None),
    ({"$ref": "#/$defs/Missing"}, None),
    ({"$ref": "https://example.com/schema.json"}, None),
    ({}, None),
    ({"description": "no type"}, None),
]
for schema, expected in cases:
    got = schema_type(schema, DEFS)
    assert got == expected, (schema, got, expected)
assert schema_type({"$ref": "#/definitions/Crop"}, DEFS_OLD) == "object"

def upstream(func_name, tools):
    for tool in tools or []:
        if tool["function"]["name"] == func_name:
            return tool["function"]["parameters"].get("properties", {})
    return {}

TOOLS = [{"type": "function", "function": {"name": "read_image", "parameters": {
    "type": "object",
    "properties": {"source": {"type": "string"}, "note": {}, "crop": {"$ref": "#/$defs/Crop", "default": None},
                   "when": {"type": ["string", "null"], "description": "d"},
                   "global": {"type": ["boolean", "null"]}, "either": {"type": ["integer", "boolean"]}},
    "$defs": DEFS}}}]
before = copy.deepcopy(TOOLS)
config = typed_arguments_config(upstream)
assert config.__wrapped__ is upstream
typed = config("read_image", TOOLS)
assert TOOLS == before, "the request's tools are never changed"
assert typed["source"] is TOOLS[0]["function"]["parameters"]["properties"]["source"], "a typed property is the same object"
assert typed["note"] == {} and typed["either"] == {"type": ["integer", "boolean"]}, "no type named: as written"
assert typed["crop"] == {"$ref": "#/$defs/Crop", "default": None, "type": "object"}, typed["crop"]
assert typed["when"] == {"type": "string", "description": "d"}, typed["when"]
assert typed["global"] == {"type": "boolean"}, typed["global"]
assert config("undeclared", TOOLS) == {} and config("read_image", None) == {}
print("ok")
"##;
        let out = std::process::Command::new("/usr/bin/python3")
            .arg("-c")
            .arg(format!("{}{checks}", include_str!("rank_tool_schema.py")))
            .output()
            .expect("/usr/bin/python3 runs the rank programs' pure half");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "ok");
    }

    /// Q-232 against the REAL qwen3_coder parser of mlx_lm 0.31.3 (the tensor split's): a word
    /// under `{"type": ["string", "null"]}` — schemars' `Option<String>` — and the call is lost.
    /// NEGATIVE CONTROL, the parser as shipped: `10m` raises SyntaxError (literal_eval), `true`
    /// under ["boolean", "null"] ValueError, `42` becomes the integer 42, `'x'` loses its quotes,
    /// and read_image's `crop` (a `$ref`, as goose puts it on the wire) arrives as its JSON text.
    /// With rank_tool_schema.py installed as the wrapper installs it, every one of those is read
    /// as its type — the call Rapid-MLX's parser (single engine, pipeline fork) reads — while
    /// every shape the parser already read (plain types, undeclared and untyped parameters, the
    /// word "null", a string/array union) reads exactly as before; and a streamed nullable string
    /// streams while it is written and closes byte-identical to the whole-call parse.
    #[test]
    fn a_nullable_string_argument_keeps_its_call_on_the_tensor_parser() {
        let Some(python) = proven_env(&EnvSpec::tensor(), "GOOSE_TEST_TENSOR_PYTHON") else {
            return;
        };
        let checks = r##"
import copy
from mlx_lm.tool_parsers import qwen3_coder as q

def schema(name, props, defs=None):
    parameters = {"type": "object", "properties": props}
    if defs:
        parameters["$defs"] = defs
    return {"type": "function", "function": {"name": name, "parameters": parameters}}

CROP = {"type": "object", "properties": {k: {"type": "integer", "format": "uint32", "minimum": 0}
        for k in ("x", "y", "width", "height")}, "required": ["x", "y", "width", "height"]}
TOOLS = [
    schema("sleep_for", {
        "duration": {"type": ["string", "null"]}, "reverse": {"type": ["null", "string"]},
        "plain": {"type": "string"}, "count": {"type": ["integer", "null"]},
        "ratio": {"type": ["number", "null"]}, "is_global": {"type": ["boolean", "null"]},
        "env": {"type": ["object", "null"]}, "tags": {"type": ["array", "null"]},
        "limit": {"anyOf": [{"type": "integer"}, {"type": "null"}]},
        "url": {"anyOf": [{"type": "string", "format": "uri"}, {"type": "array", "items": {"type": "string"}}]},
        "timeout": {"type": "integer"}, "force": {"type": "boolean"}, "opts": {"type": "object"},
        "either": {"type": ["integer", "boolean"]}, "note": {},
    }),
    schema("read_image", {"source": {"type": "string"},
                          "crop": {"description": "Optional crop rectangle in pixels.", "default": None,
                                   "$ref": "#/$defs/CropParams"}}, {"CropParams": CROP}),
    schema("manage_extensions", {"action": {"$ref": "#/$defs/Action"}, "extension_name": {"type": "string"}},
           {"Action": {"type": "string", "enum": ["enable", "disable"]}}),
]

def call(name, *params):
    body = "".join(f"<parameter={key}>\n{value}\n</parameter>\n" for key, value in params)
    return f"\n<function={name}>\n{body}</function>\n"

CROP_TEXT = '{"x": 0, "y": 0, "width": 640, "height": 480}'
CASES = {
    "word": (call("sleep_for", ("duration", "10m")), {"duration": "10m"}),
    "sentence": (call("sleep_for", ("duration", "fix the bug in main.rs")), {"duration": "fix the bug in main.rs"}),
    "null_first": (call("sleep_for", ("reverse", "10m")), {"reverse": "10m"}),
    "digits": (call("sleep_for", ("duration", "42")), {"duration": "42"}),
    "quoted": (call("sleep_for", ("duration", "'quoted'")), {"duration": "'quoted'"}),
    "null_word": (call("sleep_for", ("duration", "null")), {"duration": None}),
    "bool": (call("sleep_for", ("is_global", "true")), {"is_global": True}),
    "int": (call("sleep_for", ("count", "5")), {"count": 5}),
    "number": (call("sleep_for", ("ratio", "2.5")), {"ratio": 2.5}),
    "object": (call("sleep_for", ("env", '{"A": true, "B": null}')), {"env": {"A": True, "B": None}}),
    "array": (call("sleep_for", ("tags", '["a", null]')), {"tags": ["a", None]}),
    "anyof_int": (call("sleep_for", ("limit", "7")), {"limit": 7}),
    "crop": (call("read_image", ("source", "/tmp/shot.png"), ("crop", CROP_TEXT)),
             {"source": "/tmp/shot.png", "crop": {"x": 0, "y": 0, "width": 640, "height": 480}}),
    # Read exactly as before:
    "plain": (call("sleep_for", ("plain", "10m")), {"plain": "10m"}),
    "url_array": (call("sleep_for", ("url", '["https://a.example"]')), {"url": '["https://a.example"]'}),
    "typed": (call("sleep_for", ("timeout", "30"), ("force", "true"), ("opts", '{"a": 1}')),
              {"timeout": 30, "force": True, "opts": {"a": 1}}),
    "either": (call("sleep_for", ("either", "5")), {"either": 5}),
    "note": (call("sleep_for", ("note", '{"looks": "like json"}')), {"note": '{"looks": "like json"}'}),
    "undeclared": (call("sleep_for", ("cwd", "/w")), {"cwd": "/w"}),
    "enum_ref": (call("manage_extensions", ("action", "enable"), ("extension_name", "git")),
                 {"action": "enable", "extension_name": "git"}),
}

def parse(text):
    try:
        return q.parse_tool_call(text, TOOLS)["arguments"]
    except Exception as refusal:
        return f"REFUSED {type(refusal).__name__}"

shipped = {case: parse(text) for case, (text, _) in CASES.items()}
assert shipped["word"] == "REFUSED SyntaxError", shipped["word"]
assert shipped["sentence"] == "REFUSED SyntaxError", shipped["sentence"]
assert shipped["null_first"] == "REFUSED SyntaxError", shipped["null_first"]
assert shipped["bool"] == "REFUSED ValueError", shipped["bool"]
assert shipped["object"] == "REFUSED ValueError", shipped["object"]
assert shipped["digits"] == {"duration": 42}, shipped["digits"]
assert shipped["quoted"] == {"duration": "quoted"}, shipped["quoted"]
assert shipped["anyof_int"] == {"limit": "7"}, shipped["anyof_int"]
assert shipped["crop"]["crop"] == CROP_TEXT, shipped["crop"]

before = copy.deepcopy(TOOLS)
q._get_arguments_config = typed_arguments_config(q._get_arguments_config)
fixed = {case: parse(text) for case, (text, _) in CASES.items()}
for case, (text, expected) in CASES.items():
    assert fixed[case] == expected, (case, fixed[case], expected)
for case in ("plain", "url_array", "typed", "either", "note", "undeclared", "enum_ref", "int", "number",
             "null_word"):
    assert fixed[case] == shipped[case], (case, "read exactly as before", shipped[case], fixed[case])
assert TOOLS == before, "the request's tools are never changed"

LONG = "".join(f"step {i}: wait for the deploy, then \"retry\" \\ ünï\n" for i in range(80))
text = call("sleep_for", ("duration", LONG), ("is_global", "false"))
expected = json.dumps(q.parse_tool_call(text, TOOLS)["arguments"], ensure_ascii=False)
stream = ToolCallStream(q._convert_param_value, q._get_arguments_config)
sent = ""
before_close = 0
for i in range(0, len(text), 5):
    _, fragment = stream.feed(text[i:i + 5], TOOLS)
    sent += fragment
    if "</parameter>" not in text[:i + 5]:
        before_close = len(sent)
rest, why = stream.close(q.parse_tool_call, TOOLS)
assert why is None, why
assert sent + rest == expected, (sent + rest, expected)
assert before_close > len(LONG) // 2, ("the nullable string streamed while it was written", before_close)
print("ok")
"##;
        let out = std::process::Command::new(&python)
            .arg("-c")
            .arg(format!(
                "{}{}{checks}",
                include_str!("rank_tool_schema.py"),
                include_str!("rank_tool_stream.py")
            ))
            .output()
            .unwrap();
        assert!(
            out.status.success() && String::from_utf8_lossy(&out.stdout).trim() == "ok",
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// Q-232 through the REAL mlx_lm 0.31.3 handler, as the wrapper ships: one call to a tool
    /// whose parameters are `Option<String>` / `Option<bool>` as schemars writes them
    /// (["string", "null"], ["boolean", "null"]) and read_image's `$ref` crop, written by the
    /// stand-in generation, reaches the client — streamed and not — with exactly the arguments the
    /// model wrote, typed. NEGATIVE CONTROL: the parser's own `_get_arguments_config` put back,
    /// the same answer loses its call both ways: the stream's arguments never close, and the
    /// non-streamed request's SyntaxError escapes mlx_lm's ToolCallFormatter (it skips only
    /// ValueError) — the connection closed with no reply until Q-233 made it a named 500.
    #[test]
    fn a_call_with_optional_arguments_reaches_the_client_through_the_wrapper() {
        let Some(python) = proven_env(&EnvSpec::tensor(), "GOOSE_TEST_TENSOR_PYTHON") else {
            return;
        };
        let checks = r##"
import http.client

TOOLS = [{"type": "function", "function": {"name": "schedule", "parameters": {
    "type": "object", "required": ["what"],
    "properties": {"what": {"type": "string"}, "every": {"type": ["string", "null"], "default": None},
                   "is_global": {"type": ["boolean", "null"], "default": None},
                   "crop": {"default": None, "$ref": "#/$defs/Crop"}},
    "$defs": {"Crop": {"type": "object", "properties": {"x": {"type": "integer"}, "y": {"type": "integer"}}}}}}}]
CALL = ("\n<function=schedule>\n<parameter=what>\nre-run the census\n</parameter>\n"
        "<parameter=every>\n10m\n</parameter>\n<parameter=is_global>\nfalse\n</parameter>\n"
        "<parameter=crop>\n{\"x\": 3, \"y\": 4}\n</parameter>\n</function>\n")
EXPECTED = {"what": "re-run the census", "every": "10m", "is_global": False, "crop": {"x": 3, "y": 4}}

def generation_thread():
    while True:
        rqueue, request, args = responses.requests.get()
        rqueue.put(server.GenerationContext(
            has_tool_calling=True, has_thinking=False, tool_parser=qwen3_coder.parse_tool_call,
            sequences={(1,): "<tool_call>", (2,): "</tool_call>", (3,): "<|im_end|>"},
            prompt=[0] * 8, prompt_cache_count=0,
        ))
        rqueue.put(token("<tool_call>", "tool", (1,)))
        for i in range(0, len(CALL), 4):
            rqueue.put(token(CALL[i:i + 4], "tool"))
        rqueue.put(token("</tool_call>", "normal", (2,)))
        rqueue.put(token("<|im_end|>", None, (3,), "stop"))
        rqueue.put(None)

threading.Thread(target=generation_thread, daemon=True).start()

def body(stream):
    return {"model": served, "stream": stream, "tools": TOOLS,
            "messages": [{"role": "user", "content": "re-run the census every ten minutes"}]}

def streamed():
    url = f"http://127.0.0.1:{httpd.server_address[1]}/v1/chat/completions"
    request = urllib.request.Request(url, data=json.dumps(body(True)).encode())
    calls, finish = {}, None
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
            for delta in choice["delta"].get("tool_calls", []):
                entry = calls.setdefault(delta["index"], {"names": [], "arguments": ""})
                if "name" in delta["function"]:
                    entry["names"].append(delta["function"]["name"])
                entry["arguments"] += delta["function"].get("arguments", "")
    return {"calls": [calls[i] for i in sorted(calls)], "finish": finish}

def whole():
    try:
        status, text = post("/v1/chat/completions", body(False))
    except (ConnectionError, http.client.HTTPException) as dropped:
        return {"dropped": type(dropped).__name__, "calls": []}
    try:
        message = json.loads(text)["choices"][0]["message"]
    except (ValueError, KeyError, IndexError):
        return {"status": status, "body": text[:400], "calls": []}
    return {"status": status, "calls": [
        {"names": [c["function"]["name"]], "arguments": c["function"]["arguments"]}
        for c in message.get("tool_calls") or []]}

fixed = {"streamed": streamed(), "whole": whole()}
qwen3_coder._get_arguments_config = qwen3_coder._get_arguments_config.__wrapped__
shipped = {"streamed": streamed(), "whole": whole()}
print("GOOSE_TEST " + json.dumps({"fixed": fixed, "shipped": shipped, "expected": EXPECTED}))
"##;
        let (seen, _) = run_wrapper_checks(&python, checks);
        let expected = &seen["expected"];
        for way in ["streamed", "whole"] {
            let fixed = &seen["fixed"][way];
            let calls = fixed["calls"].as_array().unwrap();
            assert_eq!(calls.len(), 1, "{way}: {fixed}");
            assert_eq!(calls[0]["names"], serde_json::json!(["schedule"]), "{way}");
            let arguments: serde_json::Value =
                serde_json::from_str(calls[0]["arguments"].as_str().unwrap())
                    .unwrap_or_else(|e| panic!("{way}: the client's call closes: {e}: {fixed}"));
            assert_eq!(&arguments, expected, "{way}");
        }
        assert_eq!(seen["fixed"]["streamed"]["finish"], "tool_calls");

        let shipped = &seen["shipped"];
        let streamed = shipped["streamed"]["calls"].as_array().unwrap();
        assert!(
            streamed
                .iter()
                .all(|c| serde_json::from_str::<serde_json::Value>(
                    c["arguments"].as_str().unwrap()
                )
                .is_err()),
            "negative control: the parser as shipped never closes the streamed call: {shipped}"
        );
        assert_eq!(
            shipped["whole"]["status"], 500,
            "negative control: the parser as shipped loses the non-streamed call — since Q-233 a \
             named 500, before it a connection closed with no reply: {shipped}"
        );
        assert!(
            shipped["whole"]["body"]
                .as_str()
                .unwrap()
                .contains("SyntaxError: invalid decimal literal"),
            "{shipped}"
        );
    }

    /// Q-338 through the REAL mlx_lm 0.31.3 handler: a prompt whose start the prefix cache restored
    /// reports its POSITION — the restored prefix included, the contract rank_live.py and the
    /// pipeline already keep — so the desktop's two-part bar (Q-337) and the prefill rate measure
    /// the tokens actually read. mlx_lm's progress counts only the tokens it computes (`total` =
    /// the prompt past the prefix): E2E #3p's turn 7 (113,824 of 114,948 cached) read as
    /// "1,124 of 114,948" with no prefill rate (rank_live's `prefilled - cached` went negative).
    /// The stand-in generation hands back a 12-token prompt with 8 restored, then mlx_lm's own
    /// progress tuple for 2 of the remaining 4. Before the fix the same test read a position of 0
    /// and then 2 — under the cached 8 — and no rate.
    #[test]
    fn a_restored_prefix_is_inside_the_reported_prompt_position() {
        let Some(python) = proven_env(&EnvSpec::tensor(), "GOOSE_TEST_TENSOR_PYTHON") else {
            return;
        };
        let checks = r#"
steps = [threading.Event() for _ in range(2)]
proceed = [threading.Event() for _ in range(2)]

def generation_thread():
    rqueue, request, args = responses.requests.get()
    rqueue.put(server.GenerationContext(
        has_tool_calling=False, has_thinking=False, tool_parser=None,
        sequences={(3,): "<|im_end|>"}, prompt=[0] * 12, prompt_cache_count=8,
    ))
    steps[0].set()
    proceed[0].wait(30)
    rqueue.put((2, 4))
    steps[1].set()
    proceed[1].wait(30)
    rqueue.put(token("done", "normal"))
    rqueue.put(token("<|im_end|>", None, (3,), "stop"))
    rqueue.put(None)

threading.Thread(target=generation_thread, daemon=True).start()
reply = []
threading.Thread(
    target=lambda: reply.append(post("/v1/chat/completions", {
        "model": served, "messages": [{"role": "user", "content": "go"}]})),
    daemon=True,
).start()

def row_when(check):
    url = f"http://127.0.0.1:{httpd.server_address[1]}/v1/status"
    for _ in range(3000):
        with urllib.request.urlopen(url, timeout=30) as answer:
            rows = json.loads(answer.read())["requests"]
        if rows and check(rows[0]):
            return rows[0]
        time.sleep(0.01)
    raise AssertionError(f"the status never showed it: {rows}")

assert steps[0].wait(30)
started = row_when(lambda r: r["prompt_tokens"] == 12)
proceed[0].set()
assert steps[1].wait(30)
reading = row_when(lambda r: r["prefilled_tokens"] != started["prefilled_tokens"])
proceed[1].set()
for _ in range(3000):
    if reply:
        break
    time.sleep(0.01)
print("GOOSE_TEST " + json.dumps({"started": started, "reading": reading, "status": reply[0][0]}))
"#;
        let (seen, _) = run_wrapper_checks(&python, checks);
        let started = &seen["started"];
        assert_eq!(started["cached_tokens"], 8, "{seen}");
        assert_eq!(
            started["prefilled_tokens"], 8,
            "the read starts after the restored prefix: {seen}"
        );
        let reading = &seen["reading"];
        assert_eq!(reading["phase"], "prefill", "{seen}");
        assert_eq!(
            reading["prefilled_tokens"], 10,
            "the position: 8 restored + 2 of the 4 computed: {seen}"
        );
        let read = reading["prefilled_tokens"].as_u64().unwrap()
            - reading["cached_tokens"].as_u64().unwrap();
        assert_eq!(read, 2, "{seen}");
        assert!(
            reading["prompt_tokens_per_second"].as_f64().unwrap() > 0.0,
            "a rate over the 2 tokens read: {seen}"
        );
        assert_eq!(seen["status"], 200, "{seen}");
    }

    /// Q-233 through the REAL mlx_lm 0.31.3 handler: a NON-streamed answer whose tool call the
    /// parser refuses is a 500 naming the parser and its words (`code` `tool_call_unparsed`, the
    /// call's text as `tool_text`), and the rank's log says so (GOOSE_RANK_TOOL_CALL_UNPARSED
    /// `stream` false, GOOSE_RANK_REQUEST_FAILED). Both refusals the qwen3_coder parser raises:
    /// a word under ["integer", "boolean"] (a union Q-232 leaves as written) — literal_eval's
    /// SyntaxError, which escaped mlx_lm's ToolCallFormatter (it skips only ValueError) and, the
    /// 200 already buffered, closed the connection with no reply (RemoteDisconnected, measured by
    /// Q-232's negative control); and a word under "integer" — int()'s ValueError, which mlx_lm
    /// skipped: a 200 whose call was silently gone. POSITIVE CONTROL: a call the parser reads is
    /// answered 200 with it. NEGATIVE CONTROL: mlx_lm's own formatter put back, the ValueError is
    /// that silent 200. A STREAMED answer with the same refused call is exactly what it was
    /// (mlx_lm's formatter or the wrapper's, byte-identical frames): the streamer already fails
    /// the call loudly by leaving its arguments unterminated.
    #[test]
    fn a_non_streamed_call_the_parser_refuses_is_a_named_500() {
        let Some(python) = proven_env(&EnvSpec::tensor(), "GOOSE_TEST_TENSOR_PYTHON") else {
            return;
        };
        let checks = r##"
import http.client

TOOLS = [{"type": "function", "function": {"name": "schedule", "parameters": {
    "type": "object", "required": ["what"],
    "properties": {"what": {"type": "string"}, "count": {"type": ["integer", "boolean"]},
                   "limit": {"type": "integer"}}}}}]

def call(*params):
    body = "".join(f"<parameter={key}>\n{value}\n</parameter>\n" for key, value in params)
    return f"\n<function=schedule>\n{body}</function>\n"

CALLS = {
    "syntax": call(("what", "re-run the census"), ("count", "10m")),
    "value": call(("what", "re-run the census"), ("limit", "ten")),
    "read": call(("what", "re-run the census"), ("count", "5"), ("limit", "7")),
}

def generation_thread():
    while True:
        rqueue, request, args = responses.requests.get()
        text = CALLS[request.messages[-1]["content"]]
        rqueue.put(server.GenerationContext(
            has_tool_calling=True, has_thinking=False, tool_parser=qwen3_coder.parse_tool_call,
            sequences={(1,): "<tool_call>", (2,): "</tool_call>", (3,): "<|im_end|>"},
            prompt=[0] * 8, prompt_cache_count=0,
        ))
        rqueue.put(token("<tool_call>", "tool", (1,)))
        for i in range(0, len(text), 4):
            rqueue.put(token(text[i:i + 4], "tool"))
        rqueue.put(token("</tool_call>", "normal", (2,)))
        rqueue.put(token("<|im_end|>", None, (3,), "stop"))
        rqueue.put(None)

threading.Thread(target=generation_thread, daemon=True).start()

def body(case, stream):
    return {"model": served, "stream": stream, "tools": TOOLS,
            "messages": [{"role": "user", "content": case}]}

def whole(case):
    try:
        status, text = post("/v1/chat/completions", body(case, False))
    except (ConnectionError, http.client.HTTPException) as dropped:
        return {"dropped": type(dropped).__name__}
    reply = json.loads(text)
    if status != 200:
        return {"status": status, "error": reply["error"]}
    message = reply["choices"][0]["message"]
    return {"status": status, "finish": reply["choices"][0]["finish_reason"],
            "calls": [c["function"] for c in message.get("tool_calls") or []]}

def streamed(case):
    url = f"http://127.0.0.1:{httpd.server_address[1]}/v1/chat/completions"
    request = urllib.request.Request(url, data=json.dumps(body(case, True)).encode())
    frames = []
    with urllib.request.urlopen(request, timeout=60) as reply:
        status = reply.status
        for raw in reply:
            line = raw.decode().strip()
            if line.startswith("data: ") and line != "data: [DONE]":
                frame = json.loads(line[len("data: "):])
                for key in ("id", "created", "system_fingerprint"):
                    frame.pop(key, None)
                for choice in frame["choices"]:
                    for delta in choice.get("delta", {}).get("tool_calls", []):
                        delta.pop("id", None)
                frames.append(frame)
            elif line == "data: [DONE]":
                frames.append("[DONE]")
    return {"status": status, "frames": frames}

fixed = {case: whole(case) for case in CALLS}
fixed["streamed"] = streamed("syntax")
server.ToolCallFormatter = upstream_tool_call_formatter
shipped = {case: whole(case) for case in CALLS}
shipped["streamed"] = streamed("syntax")
print("GOOSE_TEST " + json.dumps({"fixed": fixed, "shipped": shipped, "calls": CALLS}))
"##;
        let (seen, printed) = run_wrapper_checks(&python, checks);
        let fixed = &seen["fixed"];
        for (case, words) in [
            ("syntax", "SyntaxError: invalid decimal literal"),
            (
                "value",
                "ValueError: invalid literal for int() with base 10: 'ten'",
            ),
        ] {
            let answer = &fixed[case];
            assert_eq!(answer["status"], 500, "{case}: {answer}");
            let error = &answer["error"];
            assert_eq!(error["type"], "server_error", "{case}");
            assert_eq!(error["code"], "tool_call_unparsed", "{case}");
            assert_eq!(error["tool_text"], seen["calls"][case], "{case}");
            let message = error["message"].as_str().unwrap();
            assert!(
                message.contains("mlx_lm.tool_parsers.qwen3_coder") && message.contains(words),
                "{case}: the parser and its words: {message}"
            );
        }
        let read = &fixed["read"];
        assert_eq!(read["status"], 200, "{read}");
        assert_eq!(read["finish"], "tool_calls");
        assert_eq!(read["calls"][0]["name"], "schedule");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(
                read["calls"][0]["arguments"].as_str().unwrap()
            )
            .unwrap(),
            serde_json::json!({"what": "re-run the census", "count": 5, "limit": 7})
        );
        let unparsed: Vec<serde_json::Value> = printed
            .lines()
            .filter_map(|l| l.strip_prefix("GOOSE_RANK_TOOL_CALL_UNPARSED "))
            .map(|l| serde_json::from_str(l).unwrap())
            .filter(|e: &serde_json::Value| e["stream"] == false)
            .collect();
        assert_eq!(unparsed.len(), 2, "{printed}");
        assert!(
            unparsed[0]["why"]
                .as_str()
                .unwrap()
                .starts_with("SyntaxError"),
            "{unparsed:?}"
        );
        assert!(
            printed
                .lines()
                .filter(|l| l.starts_with("GOOSE_RANK_REQUEST_FAILED ")
                    && l.contains("tool_call_unparsed"))
                .count()
                >= 2,
            "{printed}"
        );

        let shipped = &seen["shipped"];
        assert_eq!(shipped["value"]["status"], 200, "{shipped}");
        assert_eq!(
            shipped["value"]["calls"],
            serde_json::json!([]),
            "negative control: mlx_lm's formatter skips a ValueError — the call is silently gone"
        );
        assert_eq!(shipped["read"], fixed["read"]);
        assert_eq!(
            fixed["streamed"], shipped["streamed"],
            "a streamed answer is untouched"
        );
        assert_eq!(fixed["streamed"]["status"], 200);
    }

    /// Q-232: the tensor program as shipped carries rank_tool_schema.py before the streamer and
    /// the wrapper, and the wrapper installs it over the parser's `_get_arguments_config` after
    /// importing the parser and checking mlx_lm has it, before anything parses a call.
    #[test]
    fn the_tensor_program_reads_tool_parameter_types_through_unions() {
        let schema = include_str!("rank_tool_schema.py");
        let at = |needle: &str| {
            TENSOR_PROGRAM
                .find(needle)
                .unwrap_or_else(|| panic!("the tensor program carries {needle:?}"))
        };
        assert!(at(schema) < at(include_str!("rank_tool_stream.py")));
        let install =
            "qwen3_coder._get_arguments_config = typed_arguments_config(qwen3_coder._get_arguments_config)";
        assert_eq!(TENSOR_PROGRAM.matches(install).count(), 1);
        assert!(at("from mlx_lm.tool_parsers import qwen3_coder  # noqa: E402") < at(install));
        assert!(at("(qwen3_coder, \"_get_arguments_config\"),") < at(install));
        assert!(at(install) < at("class StreamedCall:"));
        assert!(
            !PIPELINE_PROGRAM.contains(schema),
            "the pipeline fork's parser reads these itself"
        );
    }

    /// Q-372: the tensor program installs the positional parameter reading over mlx_lm's
    /// `_parse_xml_function_call` once, after checking mlx_lm has it and after Q-232's schema
    /// install (the reading looks both up on the module per call), before anything parses a call.
    #[test]
    fn the_tensor_program_reads_parameter_values_positionally() {
        let at = |needle: &str| {
            TENSOR_PROGRAM
                .find(needle)
                .unwrap_or_else(|| panic!("the tensor program carries {needle:?}"))
        };
        let install = "\ninstall_positional_parameters(qwen3_coder)\n";
        assert_eq!(TENSOR_PROGRAM.matches(install).count(), 1);
        assert!(at("def install_positional_parameters(qwen3_coder):") < at(install));
        assert!(at("(qwen3_coder, \"_parse_xml_function_call\"),") < at(install));
        assert!(at("qwen3_coder._get_arguments_config = typed_arguments_config(") < at(install));
        assert!(at(install) < at("class StreamedCall:"));
        assert!(
            !PIPELINE_PROGRAM.contains(install),
            "the pipeline fork's parser reads values positionally itself"
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
            "{}\
             class _Group:\n    def rank(self): return 0\n    def size(self): return 2\n\
             group = _Group()\n{}QWEN38 = {qwen}\n{checks}",
            tensor_modules(),
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

    /// `checks` run inside the tensor wrapper's body as rank 0 of a stand-in group of size 2, on
    /// the spec `rank_specs` gives rank 0 of a real launch — every flag the shipped program reads
    /// — except the doorbell: nothing forms a group, binds a node's interface or waits for a peer
    /// (the doorbell's server would accept a worker that never comes). Returns the GOOSE_TEST
    /// line and every line printed.
    fn run_wrapper_checks(python: &str, checks: &str) -> (serde_json::Value, String) {
        run_wrapper_checks_with(python, checks, |_| {})
    }

    /// `run_wrapper_checks` under a spec `edit` changes first (a requester's older program).
    fn run_wrapper_checks_with(
        python: &str,
        checks: &str,
        edit: impl FnOnce(&mut RankProgram),
    ) -> (serde_json::Value, String) {
        let mut spec = rank_specs(
            &two_mac_config(),
            &ServedNames::only("node-alias"),
            &[launch(1, 2), launch(3, 2)],
            141_568,
            2.0,
        )
        .remove(0);
        if let RankProgram::MlxLmServer { doorbell, .. } = &mut spec.program {
            *doorbell = false;
        }
        edit(&mut spec.program);
        run_against_real_packages_printing(python, &wrapper_program(checks), &spec)
    }

    /// Every module the tensor program concatenates before the wrapper, as shipped: a program a
    /// test assembles around the wrapper's body takes them from here, so a module added to
    /// `TENSOR_PROGRAM` reaches every such test (Q-177: six hand-kept lists missed rank_request.py).
    fn tensor_modules() -> &'static str {
        TENSOR_PROGRAM
            .strip_suffix(include_str!("rank_wrapper.py"))
            .expect("the tensor program ends in the wrapper")
    }

    /// The shipped tensor program with its group, formation and doorbell head replaced by a
    /// stand-in group of `size` 2 (rank 0) — every module it concatenates before the wrapper, as
    /// shipped, so a module added to `TENSOR_PROGRAM` is here too — and `checks` run where mlx_lm's
    /// main() would start. The checks see the real mlx_lm 0.31.3 modules as the wrapper patched
    /// them.
    fn wrapper_program(checks: &str) -> String {
        let wrapper = include_str!("rank_wrapper.py");
        let modules = tensor_modules();
        let start = wrapper
            .find("import faulthandler  # noqa")
            .expect("the wrapper's body starts after the group check");
        let end = wrapper
            .find("server.main()")
            .expect("the wrapper ends in mlx_lm's main");
        format!(
            "{modules}\
             class _Group:\n    def rank(self): return 0\n    def size(self): return 2\n\
             group = _Group()\n{}QWEN38 = {qwen}\n{SCAFFOLD}{checks}",
            &wrapper[start..end],
            qwen = serde_json::to_string(QWEN38).unwrap(),
        )
    }

    /// mlx_lm's own argv parsed (its main() stopped at parse_args), a ResponseGenerator with no
    /// generation thread of its own, and rank 0's HTTP handler serving it on an ephemeral port.
    const SCAFFOLD: &str = r#"
import argparse
import http.server
import types
import urllib.error
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

responses = server.ResponseGenerator.__new__(server.ResponseGenerator)
responses.model_provider = types.SimpleNamespace(
    cli_args=cli, model_key=("goose-test", None, None),
    tokenizer=types.SimpleNamespace(chat_template=QWEN38, encode=lambda text: [ord(c) for c in text]),
)
responses.requests = Queue()
responses._is_distributed = False
httpd = http.server.ThreadingHTTPServer(
    ("127.0.0.1", 0),
    lambda *args, **kwargs: server.APIHandler(responses, *args, system_fingerprint="test", **kwargs),
)
threading.Thread(target=httpd.serve_forever, daemon=True).start()

def post(path, body):
    url = f"http://127.0.0.1:{httpd.server_address[1]}{path}"
    try:
        with urllib.request.urlopen(urllib.request.Request(url, data=json.dumps(body).encode()), timeout=60) as reply:
            return reply.status, reply.read().decode()
    except urllib.error.HTTPError as refused:
        return refused.code, refused.read().decode()

def token(text, state, match=None, finish=None):
    return server.Response(text, 7, state, match, 0.0, finish, ())
"#;

    /// Q-161 reopened through the REAL mlx_lm 0.31.3 handler: E2E #3f turn 0's words, streamed. The
    /// stand-in generation writes one `write` call, `</tool_call>`, then what goose's forming panel
    /// showed beside it — `!\n\n</parameter>\n</function>\n!\n</parameter>\n</function>\n!…` — and
    /// /v1/status's tail cycle `!\n</function>\n` up to the panel's 3,251 chars, never another
    /// call. Through the wrapper the write reaches the client whole, the text stops once one span
    /// written back to back covers most of the text outside the calls, the generation is told to
    /// stop there, the rank's log names it (GOOSE_RANK_TEXT_CYCLE, the span and its copies) and
    /// /v1/status keeps it after the request left (`last_engine_stop`). An answer that says a line
    /// twice in passing runs to its end. NEGATIVE CONTROL: mlx_lm's own handle_completion streams
    /// all 3,251 chars.
    #[test]
    fn text_written_over_and_over_beside_the_calls_ends_the_answer() {
        let Some(python) = proven_env(&EnvSpec::tensor(), "GOOSE_TEST_TENSOR_PYTHON") else {
            return;
        };
        let checks = r##"
import re

TOOLS = [{"type": "function", "function": {"name": "write", "parameters": {"type": "object", "properties": {
    "path": {"type": "string"}, "content": {"type": "string"}}}}}]
NOTES = "# Kickoff\n\n## Decisions\n- Confluence is out of scope for now\n- inactive = no login in 24 months\n"
WRITE = f"\n<function=write>\n<parameter=path>\n/w/notes/kickoff.md\n</parameter>\n<parameter=content>\n{NOTES}\n</parameter>\n</function>\n"
PANEL = ("!\n\n</parameter>\n</function>\n!\n</parameter>\n</function>\n!\n</parameter>\n!\n</function>\n"
         "!\n</parameter>\n!\n</function>\n!\n</parameter>\n</function>\n!\n</parameter>\n</function>\n"
         "!\n</parameter>\n!\n</function>\n!\n</function>\n!\n</parameter>\n!\n</function>\n")
CYCLE = "!\n</function>\n"
RUNAWAY = PANEL + CYCLE * ((3251 - len(PANEL)) // len(CYCLE))
PASSING = ("Wrote notes/kickoff.md.\n- TBD\n- TBD\nDecisions, actions and open questions are in it; "
           "the two TBD lines are the owners Aoife has not named yet.\n")
ANSWERS = {"runaway": RUNAWAY, "passing": PASSING}
PIECE = re.compile(r"</|<|parameter|function|>|\n\n|\n|!|[^<>\n!]+")
fed = {}

def generation_thread():
    while True:
        rqueue, request, args = responses.requests.get()
        answer = request.messages[-1]["content"]
        gets = {"n": 0}
        read = threading.Condition()
        real_get = rqueue.get

        def counted_get(*a, **k):
            with read:
                gets["n"] += 1
                read.notify_all()
            return real_get(*a, **k)

        rqueue.get = counted_get
        ctx = server.GenerationContext(
            has_tool_calling=True, has_thinking=False, tool_parser=qwen3_coder.parse_tool_call,
            sequences={(1,): "<tool_call>", (2,): "</tool_call>", (3,): "<|im_end|>"},
            prompt=[0] * 8, prompt_cache_count=0,
        )
        puts = [0]

        def put(item):
            puts[0] += 1
            rqueue.put(item)
            # Until the handler has done all it does with this piece — it asks for the one after
            # (its read of the context began before the count did) — or was told to stop.
            deadline = time.monotonic() + 10
            with read:
                while not (gets["n"] > puts[0] or ctx._should_stop) and time.monotonic() < deadline:
                    read.wait(0.01)

        rqueue.put(ctx)
        put(token("<tool_call>", "tool", (1,)))
        for i in range(0, len(WRITE), 3):
            put(token(WRITE[i:i + 3], "tool"))
        put(token("", "normal", (2,)))
        fed[answer] = ""
        for piece in PIECE.findall(ANSWERS[answer]):
            if ctx._should_stop:
                break
            put(token(piece, "normal"))
            fed[answer] += piece
        else:
            rqueue.put(token("", None, (3,), "stop"))
        rqueue.put(None)

threading.Thread(target=generation_thread, daemon=True).start()

def stream(answer):
    body = {"model": served, "stream": True, "tools": TOOLS, "messages": [{"role": "user", "content": answer}]}
    url = f"http://127.0.0.1:{httpd.server_address[1]}/v1/chat/completions"
    calls, content, finish = {}, "", None
    with urllib.request.urlopen(urllib.request.Request(url, data=json.dumps(body).encode()), timeout=60) as reply:
        for raw in reply:
            line = raw.decode().strip()
            if not line.startswith("data: ") or line == "data: [DONE]":
                continue
            frame = json.loads(line[len("data: "):])
            if not frame["choices"]:
                continue
            choice = frame["choices"][0]
            finish = choice["finish_reason"] or finish
            content += choice["delta"].get("content") or ""
            for delta in choice["delta"].get("tool_calls", []):
                entry = calls.setdefault(delta["index"], {"names": [], "arguments": ""})
                if "name" in delta["function"]:
                    entry["names"].append(delta["function"]["name"])
                entry["arguments"] += delta["function"].get("arguments", "")
    return {"calls": [calls[i] for i in sorted(calls)], "content": content, "finish": finish,
            "fed": fed[answer], "total": ANSWERS[answer]}

def status():
    url = f"http://127.0.0.1:{httpd.server_address[1]}/v1/status"
    with urllib.request.urlopen(url, timeout=10) as reply:
        return json.loads(reply.read())

runaway = stream("runaway")
after_runaway = status()["last_engine_stop"]
passing = stream("passing")
server.APIHandler.handle_completion = original_handle_completion
upstream = stream("runaway")
print("GOOSE_TEST " + json.dumps({"runaway": runaway, "status": after_runaway, "passing": passing,
    "upstream": upstream, "write": json.dumps(qwen3_coder.parse_tool_call(WRITE, TOOLS)["arguments"], ensure_ascii=False)}))
"##;
        let (seen, printed) = run_wrapper_checks(&python, checks);
        let runaway = &seen["runaway"];
        let (fed, total) = (
            runaway["fed"].as_str().unwrap(),
            runaway["total"].as_str().unwrap(),
        );
        assert!(
            fed.len() < total.len() / 10,
            "the generation was told to stop early in #3f's {} chars, not at their end: fed {fed:?}",
            total.len()
        );
        assert!(total.starts_with(fed));
        assert!(
            fed.starts_with(runaway["content"].as_str().unwrap()),
            "the client got only text the model wrote: {runaway}"
        );
        assert_eq!(runaway["calls"].as_array().unwrap().len(), 1);
        assert_eq!(runaway["calls"][0]["names"], serde_json::json!(["write"]));
        assert_eq!(runaway["calls"][0]["arguments"], seen["write"]);
        assert_eq!(
            runaway["finish"], "tool_calls",
            "mlx_lm names the answer by the call it closed: {runaway}"
        );
        let cycle = printed
            .lines()
            .find_map(|l| l.strip_prefix("GOOSE_RANK_TEXT_CYCLE "))
            .unwrap_or_else(|| panic!("the stop is named in the rank's log: {printed}"));
        let cycle: serde_json::Value = serde_json::from_str(cycle).unwrap();
        assert_eq!(cycle["reason"], "text_cycle");
        assert_eq!(cycle["unit"], "</parameter>\n</function>\n!\n");
        assert_eq!(cycle["copies"], 2);
        assert_eq!(cycle["calls"], 1);
        let outside = &total[..cycle["outside_chars"].as_u64().unwrap() as usize];
        assert!(
            outside.ends_with(&"</parameter>\n</function>\n!\n".repeat(2)),
            "the stop read #3f's own words: {outside:?}"
        );
        let content = runaway["content"].as_str().unwrap();
        assert!(
            outside.starts_with(content) && content.len() < outside.len(),
            "the piece that completed the cycle is not handed on: {content:?}"
        );
        assert_eq!(
            seen["status"], cycle,
            "/v1/status keeps the stop after the request left"
        );

        let passing = &seen["passing"];
        assert_eq!(
            passing["fed"], passing["total"],
            "a line said twice in passing is no cycle: {passing}"
        );
        assert_eq!(passing["content"], passing["total"]);

        let upstream = &seen["upstream"];
        assert_eq!(
            upstream["fed"], upstream["total"],
            "negative control: unpatched mlx_lm streams the whole runaway"
        );
        assert_eq!(upstream["content"], upstream["total"]);
    }

    /// Q-161 through the REAL mlx_lm 0.31.3 handler: E2E #3e's turn 0 was ONE streamed answer of
    /// 649 calls — the same `write` of notes/kickoff.md and the same `mkdir -p …/notes`, 324 times
    /// each — 221,604 tokens over ~6.5 h. The stand-in generation writes that answer (write, mkdir,
    /// write, mkdir, … six pairs, then `<|im_end|>`), one call at a time, each only once the
    /// handler has read the last. Through the wrapper the client receives the first write and the
    /// first mkdir, whole and exactly as the parser reads them, and never the repeat: the
    /// generation is told to stop right after the third call (the first verbatim repeat) closes,
    /// the answer ends `tool_calls`, and the rank's log names it (GOOSE_RANK_TOOL_CALL_REPEATED,
    /// the words included). A call that starts as the first write and differs late (a REWRITE of
    /// the same file) is held until it differs, then sent whole with its own index, and the answer
    /// runs to its end. NEGATIVE CONTROL: mlx_lm's own handle_completion delivers all twelve calls
    /// and generates to the end.
    #[test]
    fn a_call_written_again_word_for_word_ends_the_answer_unsent() {
        let Some(python) = proven_env(&EnvSpec::tensor(), "GOOSE_TEST_TENSOR_PYTHON") else {
            return;
        };
        let checks = r##"
TOOLS = [
    {"type": "function", "function": {"name": "write", "parameters": {"type": "object", "properties": {
        "path": {"type": "string"}, "content": {"type": "string"}}}}},
    {"type": "function", "function": {"name": "shell", "parameters": {"type": "object", "properties": {
        "command": {"type": "string"}}}}},
]
NOTES = "# Kickoff\n\n## Decisions\n- Confluence is out of scope for now\n- inactive = no login in 24 months\n"

def call(name, *params):
    body = "".join(f"<parameter={k}>\n{v}\n</parameter>\n" for k, v in params)
    return f"\n<function={name}>\n{body}</function>\n"

WRITE = call("write", ("path", "/w/notes/kickoff.md"), ("content", NOTES))
MKDIR = call("shell", ("command", "mkdir -p /w/notes"))
REWRITE = call("write", ("path", "/w/notes/kickoff.md"), ("content", NOTES + "- next call fri 2/10\n"))
ANSWERS = {"runaway": [WRITE, MKDIR] * 6, "rewrite": [WRITE, MKDIR, REWRITE]}
fed = {}

def generation_thread():
    while True:
        rqueue, request, args = responses.requests.get()
        answer = request.messages[-1]["content"]
        gets = {"n": 0}
        read = threading.Condition()
        real_get = rqueue.get

        def counted_get(*a, **k):
            with read:
                gets["n"] += 1
                read.notify_all()
            return real_get(*a, **k)

        rqueue.get = counted_get
        ctx = server.GenerationContext(
            has_tool_calling=True, has_thinking=False, tool_parser=qwen3_coder.parse_tool_call,
            sequences={(1,): "<tool_call>", (2,): "</tool_call>", (3,): "<|im_end|>"},
            prompt=[0] * 8, prompt_cache_count=0,
        )
        puts = [0]

        def put(item):
            puts[0] += 1
            rqueue.put(item)

        put(ctx)
        fed[answer] = 0
        for text in ANSWERS[answer]:
            if ctx._should_stop:
                break
            put(token("<tool_call>", "tool", (1,)))
            for i in range(0, len(text), 3):
                put(token(text[i:i + 3], "tool"))
            put(token("</tool_call>", "normal", (2,)))
            put(token("\n", "normal"))
            fed[answer] += 1
            # Until the handler asks for the call's last piece (it asks only once it has handled the
            # `</tool_call>` before it, where a repeat stops) or was told to stop. The handler's
            # first read (the context) may come before `get` is counted, hence `>=`; a stop
            # notifies nothing here, so the wait looks again every few milliseconds.
            deadline = time.monotonic() + 10
            with read:
                while not (gets["n"] >= puts[0] or ctx._should_stop) and time.monotonic() < deadline:
                    read.wait(0.01)
        else:
            put(token("<|im_end|>", None, (3,), "stop"))
        rqueue.put(None)

threading.Thread(target=generation_thread, daemon=True).start()

def stream(answer):
    body = {"model": served, "stream": True, "tools": TOOLS, "messages": [{"role": "user", "content": answer}]}
    url = f"http://127.0.0.1:{httpd.server_address[1]}/v1/chat/completions"
    calls, finish = {}, None
    with urllib.request.urlopen(urllib.request.Request(url, data=json.dumps(body).encode()), timeout=60) as reply:
        for raw in reply:
            line = raw.decode().strip()
            if not line.startswith("data: ") or line == "data: [DONE]":
                continue
            frame = json.loads(line[len("data: "):])
            if not frame["choices"]:
                continue
            choice = frame["choices"][0]
            finish = choice["finish_reason"] or finish
            for delta in choice["delta"].get("tool_calls", []):
                entry = calls.setdefault(delta["index"], {"names": [], "arguments": ""})
                if "name" in delta["function"]:
                    entry["names"].append(delta["function"]["name"])
                entry["arguments"] += delta["function"].get("arguments", "")
    return {"calls": [calls[i] for i in sorted(calls)], "indexes": sorted(calls), "finish": finish,
            "fed": fed[answer]}

def parsed(text):
    return json.dumps(qwen3_coder.parse_tool_call(text, TOOLS)["arguments"], ensure_ascii=False)

runaway = stream("runaway")
rewrite = stream("rewrite")
server.APIHandler.handle_completion = original_handle_completion
upstream = stream("runaway")
print("GOOSE_TEST " + json.dumps({"runaway": runaway, "rewrite": rewrite, "upstream": upstream,
    "write": parsed(WRITE), "mkdir": parsed(MKDIR), "rewritten": parsed(REWRITE)}))
"##;
        let (seen, printed) = run_wrapper_checks(&python, checks);
        let (write, mkdir) = (
            seen["write"].as_str().unwrap(),
            seen["mkdir"].as_str().unwrap(),
        );

        let runaway = &seen["runaway"];
        assert_eq!(
            runaway["fed"], 3,
            "the generation was told to stop right after the first repeat closed: {runaway}"
        );
        assert_eq!(runaway["finish"], "tool_calls");
        let calls = runaway["calls"].as_array().unwrap();
        assert_eq!(
            calls.len(),
            2,
            "the repeat never reached the client: {runaway}"
        );
        assert_eq!(calls[0]["names"], serde_json::json!(["write"]));
        assert_eq!(calls[0]["arguments"], write);
        assert_eq!(calls[1]["names"], serde_json::json!(["shell"]));
        assert_eq!(calls[1]["arguments"], mkdir);
        let repeated = printed
            .lines()
            .find_map(|l| l.strip_prefix("GOOSE_RANK_TOOL_CALL_REPEATED "))
            .unwrap_or_else(|| panic!("the repeat is named in the rank's log: {printed}"));
        let repeated: serde_json::Value = serde_json::from_str(repeated).unwrap();
        assert_eq!(repeated["name"], "write");
        assert_eq!(repeated["repeat_of"], 1);
        assert_eq!(repeated["calls"], 2);
        assert!(
            repeated["tail"]
                .as_str()
                .unwrap()
                .contains("mkdir -p /w/notes"),
            "the words the model wrote: {repeated}"
        );

        let rewrite = &seen["rewrite"];
        assert_eq!(
            rewrite["fed"], 3,
            "a call that differs is no repeat: {rewrite}"
        );
        assert_eq!(rewrite["finish"], "tool_calls");
        assert_eq!(rewrite["indexes"], serde_json::json!([0, 1, 2]));
        let calls = rewrite["calls"].as_array().unwrap();
        assert_eq!(
            calls[2]["names"],
            serde_json::json!(["write"]),
            "one open frame"
        );
        assert_eq!(calls[2]["arguments"], seen["rewritten"]);

        let upstream = &seen["upstream"];
        assert_eq!(
            upstream["fed"], 12,
            "unpatched mlx_lm generates the whole runaway"
        );
        assert_eq!(upstream["calls"].as_array().unwrap().len(), 12);
    }

    /// Q-181 through the REAL mlx_lm 0.31.3 handler: after goose's Stop the tensor split kept
    /// generating the cancelled request — /v1/status 'generating', 48,466 completion tokens, 4,413 s,
    /// no ESTABLISHED client on 8091 — because nothing was written while its text was withheld, and
    /// mlx_lm notices a closed connection only when a write fails. The stand-in generation writes a
    /// call whose typed value (an array) the relay withholds until `</parameter>`, one piece at a
    /// time, each once the handler has read the last; the client reads what it was sent and closes
    /// the socket. Through the wrapper the generation is told to stop within a few pieces of the
    /// close, the rank's log names it (GOOSE_RANK_CANCELLED_BY_CLIENT: the phase, the withholding
    /// mode, generated vs sent) and /v1/status keeps it (`last_engine_stop`). The same holds for a
    /// non-streamed answer (nothing is written before its end) and for a client that leaves while
    /// its prompt is read. NEGATIVE CONTROL: mlx_lm's own handle_completion generates the withheld
    /// call to the harness's end.
    #[test]
    fn a_client_that_leaves_while_its_text_is_withheld_ends_the_generation() {
        let Some(python) = proven_env(&EnvSpec::tensor(), "GOOSE_TEST_TENSOR_PYTHON") else {
            return;
        };
        let checks = r##"
import socket as sk

TOOLS = [{"type": "function", "function": {"name": "shell", "parameters": {"type": "object", "properties": {
    "command": {"type": "string"}, "paths": {"type": "array"}}}}}]
HEAD = "\n<function=shell>\n<parameter=paths>\n["
# Harness bounds, never the engine's: the pieces a generation nobody stops writes, and how many of
# them the client waits for before it leaves.
CAP = 1500
LEAVE_AFTER = 40
fed, leave_now, finished = {}, {}, {}

def generation_thread():
    while True:
        rqueue, request, args = responses.requests.get()
        answer = request.messages[-1]["content"]
        gets = {"n": 0}
        read = threading.Condition()
        real_get = rqueue.get

        def counted_get(*a, **k):
            with read:
                gets["n"] += 1
                read.notify_all()
            return real_get(*a, **k)

        rqueue.get = counted_get
        ctx = server.GenerationContext(
            has_tool_calling=True, has_thinking=False, tool_parser=qwen3_coder.parse_tool_call,
            sequences={(1,): "<tool_call>", (2,): "</tool_call>", (3,): "<|im_end|>"},
            prompt=[0] * 8, prompt_cache_count=0,
        )
        puts = [0]

        def put(item):
            puts[0] += 1
            rqueue.put(item)
            deadline = time.monotonic() + 10
            with read:
                while not (gets["n"] > puts[0] or ctx._should_stop) and time.monotonic() < deadline:
                    read.wait(0.01)

        rqueue.put(ctx)
        fed[answer] = 0
        kind = answer.split()[0]
        if kind == "call":
            put(token("<tool_call>", "tool", (1,)))
            for i in range(0, len(HEAD), 3):
                put(token(HEAD[i:i + 3], "tool"))
        for i in range(CAP):
            if ctx._should_stop:
                break
            if kind == "call":
                put(token(f'"p{i}", ', "tool"))
            elif kind == "text":
                put(token(f"word{i} ", "normal"))
            else:
                put((i, CAP * 10))
            fed[answer] += 1
            if fed[answer] == LEAVE_AFTER:
                leave_now[answer].set()
        rqueue.put(None)
        finished[answer].set()

threading.Thread(target=generation_thread, daemon=True).start()

def leave_while_withheld(answer, stream):
    leave_now[answer], finished[answer] = threading.Event(), threading.Event()
    body = json.dumps({"model": served, "stream": stream, "tools": TOOLS,
                       "messages": [{"role": "user", "content": answer}]}).encode()
    client = sk.create_connection(("127.0.0.1", httpd.server_address[1]))
    client.sendall(
        b"POST /v1/chat/completions HTTP/1.1\r\nHost: goose\r\nContent-Type: application/json\r\n"
        + f"Content-Length: {len(body)}\r\n\r\n".encode() + body
    )
    assert leave_now[answer].wait(30), answer
    client.setblocking(False)
    received = b""
    try:
        while chunk := client.recv(65536):
            received += chunk
    except BlockingIOError:
        pass
    at_close = fed[answer]
    client.close()
    assert finished[answer].wait(60), answer
    return {"received": received.decode(errors="replace"), "at_close": at_close, "fed": fed[answer]}

def status():
    url = f"http://127.0.0.1:{httpd.server_address[1]}/v1/status"
    with urllib.request.urlopen(url, timeout=10) as reply:
        return json.loads(reply.read())["last_engine_stop"]

arms = {}
for name, answer, stream in (("streamed", "call streamed", True), ("whole", "text whole", False),
                             ("prefill", "prefill whole", False)):
    arms[name] = leave_while_withheld(answer, stream)
    arms[name]["status"] = status()
server.APIHandler.handle_completion = original_handle_completion
upstream = leave_while_withheld("call upstream", True)
print("GOOSE_TEST " + json.dumps({"arms": arms, "upstream": upstream, "cap": CAP, "leave_after": LEAVE_AFTER}))
"##;
        let (seen, printed) = run_wrapper_checks(&python, checks);
        let cap = seen["cap"].as_u64().unwrap();
        let leave_after = seen["leave_after"].as_u64().unwrap();
        let stops: Vec<serde_json::Value> = printed
            .lines()
            .filter_map(|l| l.strip_prefix("GOOSE_RANK_CANCELLED_BY_CLIENT "))
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(
            stops.len(),
            3,
            "one named stop per arm that left, none for the negative control: {printed}"
        );
        for ((name, phase), stop) in [
            ("streamed", "generation"),
            ("whole", "generation"),
            ("prefill", "prefill"),
        ]
        .into_iter()
        .zip(&stops)
        {
            let arm = &seen["arms"][name];
            let (at_close, fed) = (
                arm["at_close"].as_u64().unwrap(),
                arm["fed"].as_u64().unwrap(),
            );
            // A share of what was left, never a piece count: under a loaded test run the handler
            // polled the socket 4 pieces after the close (merged main, 2026-09-27) where alone it was
            // 1–2; without the fix every arm runs to the cap.
            assert!(
                fed < cap && (fed - at_close) * 10 < cap - at_close,
                "{name}: the generation was told to stop within a few pieces of the close \
                 (fed {at_close} at the close, {fed} in all, the harness ends at {cap}): {arm}"
            );
            assert_eq!(stop["reason"], "cancelled_by_client", "{name}: {stop}");
            assert_eq!(stop["phase"], phase, "{name}: {stop}");
            let how = stop["how"].as_str().unwrap();
            assert!(
                how == "eof" || how.starts_with("ConnectionResetError"),
                "{name}: the socket said how the client left: {how}"
            );
            assert_eq!(
                arm["status"], *stop,
                "{name}: /v1/status keeps the stop after the request left"
            );
        }
        let streamed = &stops[0];
        assert_eq!(
            streamed["withholding"], "tool_typed_value",
            "the stop names what was being withheld when the client left: {streamed}"
        );
        assert!(
            streamed["sent_chars"].as_u64().unwrap()
                < streamed["generated_chars"].as_u64().unwrap()
        );
        assert!(streamed["completion_tokens"].as_u64().unwrap() >= leave_after);
        let received = seen["arms"]["streamed"]["received"].as_str().unwrap();
        assert!(
            received.contains("\"name\": \"shell\"") && !received.contains("p0"),
            "the client had the call's open frame and none of the withheld value: {received}"
        );
        assert!(
            stops[1].get("withholding").is_none(),
            "a non-streamed answer has no stream watch: {}",
            stops[1]
        );
        assert_eq!(stops[2]["completion_tokens"], 0);

        let upstream = &seen["upstream"];
        assert_eq!(
            upstream["fed"].as_u64().unwrap(),
            cap,
            "negative control: unpatched mlx_lm generates the withheld call for a client that left: \
             {upstream}"
        );
    }

    /// Q-231 through the REAL mlx_lm 0.31.3 generation loop (`ResponseGenerator._generate` as the
    /// wrapper runs it, a one-layer model on the CPU, the budget a group's COUNT of steps): E2E #3m
    /// turn 3 replayed in its order. Three end-of-turn fact checks are admitted together (#3m:
    /// 1,775 / 1,770 / 5,453 tokens at 20:16:14Z; here 60 / 60 / 80 read four tokens a step),
    /// goose drops all three before the engine's first prompt step reports (turn_priority,
    /// 20:16:17.067Z), and the user's call arrives while that step runs (20:16:20.9Z). With
    /// `prefill_step_yields` the three rows are in ONE prompt step and never generate; the loop
    /// removes them at that step's end and the user's row is read next. NEGATIVE CONTROL, the
    /// 3.0.63 program (`prefill_step_yields` off): mlx_lm's loop runs its whole count — the three
    /// dropped rows are read to their end and generate, #3m's `steps` frozen at 1116 and uid 29
    /// "generated 1, removed" — and only then takes the user's row. In both, /v1/status lists every
    /// row the batch holds: while the loop keeps the dropped rows, each is `leaving`, names its
    /// client and its `cancelled_by_client` stop, the user's row says it waits unheld, and the
    /// rank's state names each row's request; a request rank 0 holds for room says
    /// `held_for_room`. When the rows are gone the table is empty.
    #[test]
    fn a_dropped_request_leaves_the_batch_at_the_next_prompt_step_and_is_listed_until_it_does() {
        let Some(python) = proven_env(&EnvSpec::tensor(), "GOOSE_TEST_TENSOR_PYTHON") else {
            return;
        };
        let checks = r#"
import socket as sk
from mlx_lm.models import llama

# MLX's CPU reports no working-set size; BatchGenerator asks for it only to wire memory.
mx.metal.is_available = lambda: False
VOCAB = 64
END = VOCAB - 1
SYSTEM = 12


class Detokenizer:
    last_segment = ""

    def add_token(self, token):
        self.last_segment = "w"


class Tokenizer:
    has_tool_calling = False
    has_thinking = False
    tool_parser = None
    eos_token_ids = {END}
    eos_token_id = END

    def encode(self, text, add_special_tokens=False):
        return [1]

    def convert_ids_to_tokens(self, ids):
        return "<end>"

    @property
    def detokenizer(self):
        return Detokenizer()


tok = Tokenizer()
model = llama.Model(llama.ModelArgs(model_type="llama", hidden_size=16, num_hidden_layers=1,
    intermediate_size=32, num_attention_heads=2, rms_norm_eps=1e-5, vocab_size=VOCAB, num_key_value_heads=2))
mx.eval(model.parameters())

provider = responses.model_provider
provider.tokenizer = tok
provider.is_batchable = True
provider.load_default = lambda: None
provider.load = lambda *names: (model, tok)
cli.prefill_step_size = 4
responses.prompt_cache = server.LRUPromptCache(cli.prompt_cache_size)
responses._state_machine_cache = {}
responses._stop = False
responses._rank = 0
# A group's budget (TimeBudget on a distributed group): a count of steps, re-fitted every ten loops,
# at mlx_lm's own defaults. Built without its __init__, which asks mx.distributed.init() for the
# group — under the spec's JACCL env that waits for a peer this stand-in group never has.
responses._time_budget = server.TimeBudget.__new__(server.TimeBudget)
defaults = inspect.signature(server.TimeBudget.__init__).parameters
responses._time_budget.__dict__.update(
    _is_distributed=True, _budget=defaults["budget"].default,
    _iterations=defaults["iterations"].default, _sync_frequency=defaults["sync_frequency"].default,
    _start=None, _current_iterations=None, _loops=0, _time_spent=0)
original_tokenize = lambda self, tokenizer, request, args: (list(request.ids), None, None, None)
responses._tokenize = lambda tokenizer, request, args: (
    list(request.ids), [list(request.ids[:SYSTEM]), list(request.ids[SYSTEM:])], ["system", "user"], "normal")


def arguments(max_tokens):
    return server.GenerationArguments(
        model=server.ModelDescription("goose-test", None, None),
        sampling=server.SamplingArguments(0.0, 1.0, 0, 0.0, 0.0, 0.0),
        # The stand-in model never ends an answer itself: every row runs to what the loop decides.
        logits=server.LogitsProcessorArguments({END: -1e9}, 1.0, 20, 0.0, 20, 0.0, 20),
        stop_words=[], max_tokens=max_tokens, num_draft_tokens=0, logprobs=False, top_logprobs=0,
        seed=None, chat_template_kwargs=None)


steps, hooks, uid_request, generated = [], {}, {}, {}
wrapper_prompt = mlx_generate.PromptProcessingBatch.prompt


def hooked(self, tokens):
    if tokens:
        steps.append([batch_rows.get(uid) for uid in self.uids])
        uid_request.update({uid: batch_rows.get(uid) for uid in self.uids})
        hook = hooks.pop(len(steps), None)
        if hook is not None:
            hook()
    return wrapper_prompt(self, tokens)


mlx_generate.PromptProcessingBatch.prompt = hooked
tracked_next = server.BatchGenerator.next


def counting_next(self):
    stepped = tracked_next(self)
    for r in stepped[1]:
        generated[uid_request.get(r.uid)] = generated.get(uid_request.get(r.uid), 0) + 1
    return stepped


server.BatchGenerator.next = counting_next


def ask(length, first, max_tokens, port):
    ours, theirs = sk.socketpair()
    call = {"theirs": theirs, "started": threading.Event(), "done": threading.Event(), "pieces": 0,
            "gone": None, "request": types.SimpleNamespace(
                ids=[(first + i) % (VOCAB - 2) + 1 for i in range(length)], tools=None, request_type="chat")}
    waiting = responses.requests.qsize()

    def run():
        try:
            ctx, tokens = responses.generate(call["request"], arguments(max_tokens), None,
                                             client=ours, address=("127.0.0.1", port))
            call["started"].set()
            for _ in tokens:
                call["pieces"] += 1
        except ClientGone as gone:
            call["gone"] = str(gone)
        finally:
            call["started"].set()
            call["done"].set()

    threading.Thread(target=run, daemon=True).start()
    while responses.requests.qsize() == waiting and not call["started"].is_set():
        time.sleep(0.001)
    return call


def status():
    url = f"http://127.0.0.1:{httpd.server_address[1]}/v1/status"
    with urllib.request.urlopen(url, timeout=10) as reply:
        return json.loads(reply.read())


seen = {}
checks = [ask(60, 0, 50, 50001), ask(60, 20, 50, 50002), ask(80, 40, 50, 50003)]
user = {}


user_sent = threading.Event()


def first_step():
    # goose drops the three calls (the sockets close) once the engine has them, before its first
    # prompt step reports; the user's call arrives while that step runs.
    for call in checks:
        assert call["started"].wait(30)
        call["theirs"].close()
    user["call"] = ask(30, 7, 3, 50100)
    user_sent.set()


def second_step():
    for call in checks:
        assert call["done"].wait(30)
    seen["second_step"] = status()
    seen["second_step_state"] = published_state[0]


hooks[1] = first_step
hooks[2] = second_step
engine = threading.Thread(target=responses._generate, daemon=True)
engine.start()
assert user_sent.wait(60), "the engine reached its first prompt step"
assert user["call"]["done"].wait(60), "the user's call is answered"
ids = {call["request"].goose_request_id for call in checks}
user_id = user["call"]["request"].goose_request_id
check_steps = [i + 1 for i, rows in enumerate(steps) if ids & set(rows)]
user_steps = [i + 1 for i, rows in enumerate(steps) if user_id in rows]
seen.update(
    check_steps=len(check_steps), last_check_step=max(check_steps), user_first_step=min(user_steps),
    check_generated=sum(generated.get(i, 0) for i in ids), user_pieces=user["call"]["pieces"],
    gone=[call["gone"] for call in checks], check_ids=sorted(ids), user_id=user_id,
)

if spec.get("prefill_step_yields"):
    # A request rank 0 takes while the batch has no room for it waits HELD: its row says so.
    limit = prompt_cache_limit

    def take_the_room():
        global prompt_cache_limit
        prompt_cache_limit = 1
        seen["held_call"] = ask(20, 3, 2, 50300)

    def held_step():
        global prompt_cache_limit
        seen["held"] = status()
        prompt_cache_limit = limit

    hooks[len(steps) + 1] = take_the_room
    hooks[len(steps) + 2] = held_step
    long_call = ask(40, 11, 2, 50200)
    assert long_call["done"].wait(60)
    held_call = seen.pop("held_call")
    assert held_call["done"].wait(60)
    seen["held_id"] = held_call["request"].goose_request_id

seen["final"] = status()
responses._stop = True
engine.join(30)
print("GOOSE_TEST " + json.dumps(seen))
"#;
        let plan = |program: &mut RankProgram, yields: bool| {
            if let RankProgram::MlxLmServer {
                prompt_cache_limit_bytes,
                prefill_step_yields,
                ..
            } = program
            {
                // The 27B split's KV plan (Q-182's E2E #3h figure): three fact checks and the
                // user's call fit beside each other, as on #3m (rows 3, then the user's alone).
                *prompt_cache_limit_bytes = Some(17_333_813_248);
                *prefill_step_yields = yields;
            }
        };
        let (fixed, fixed_printed) =
            run_wrapper_checks_with(&python, checks, |program| plan(program, true));
        let (upstream, upstream_printed) =
            run_wrapper_checks_with(&python, checks, |program| plan(program, false));

        for (arm, seen) in [("yields", &fixed), ("3.0.63", &upstream)] {
            assert_eq!(
                seen["gone"],
                serde_json::json!(["eof", "eof", "eof"]),
                "{arm}: each dropped call was told its client left: {seen}"
            );
            assert_eq!(
                seen["user_pieces"], 3,
                "{arm}: the user's call answered: {seen}"
            );
            let last = &seen["final"];
            assert_eq!(
                last["requests"],
                serde_json::json!([]),
                "{arm}: once every row has left, no request is listed: {last}"
            );
            assert_eq!(last["num_running"], 0, "{arm}: {last}");
            assert_eq!(
                last["last_engine_stop"]["reason"], "cancelled_by_client",
                "{arm}: {last}"
            );
        }

        assert_eq!(
            fixed["check_steps"], 1,
            "the dropped calls are in the one prompt step that was running when they left: {fixed}"
        );
        assert_eq!(
            fixed["check_generated"], 0,
            "they never reach generation: {fixed}"
        );
        assert_eq!(
            fixed["user_first_step"], 2,
            "the user's call is read at the very next step: {fixed}"
        );
        let second = &fixed["second_step"];
        let listed: Vec<&str> = second["requests"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["request_id"].as_str().unwrap())
            .collect();
        assert_eq!(
            listed,
            [fixed["user_id"].as_str().unwrap()],
            "at the user's first step the dropped rows are gone: {second}"
        );

        let reads = upstream["check_steps"].as_u64().unwrap();
        assert!(
            reads >= 15,
            "negative control: mlx_lm's loop reads the dropped prompts to their end (80 tokens, 4 a \
             step, after a 12-token system segment): {upstream}"
        );
        assert!(
            upstream["check_generated"].as_u64().unwrap() >= 3,
            "negative control: the dropped rows reach generation before the loop ends: {upstream}"
        );
        assert!(
            upstream["user_first_step"].as_u64().unwrap()
                > upstream["last_check_step"].as_u64().unwrap(),
            "negative control: the user's call is read only after them: {upstream}"
        );

        // What the 3.0.63 loop held the user's call behind is now on /v1/status.
        let second = &upstream["second_step"];
        let rows = second["requests"].as_array().unwrap();
        let leaving: Vec<&serde_json::Value> =
            rows.iter().filter(|r| r["leaving"] == true).collect();
        assert_eq!(
            leaving.len(),
            3,
            "every row the batch still holds is listed: {second}"
        );
        let mut clients: Vec<&str> = leaving
            .iter()
            .map(|r| r["client"].as_str().unwrap())
            .collect();
        clients.sort();
        assert_eq!(
            clients,
            ["127.0.0.1:50001", "127.0.0.1:50002", "127.0.0.1:50003"]
        );
        for row in &leaving {
            assert_eq!(row["stopped"]["reason"], "cancelled_by_client", "{row}");
            assert_eq!(row["stopped"]["phase"], "prefill", "{row}");
            assert!(row["stopped_after_s"].is_number(), "{row}");
            assert_eq!(row["status"], "running", "{row}");
        }
        let waiting = rows
            .iter()
            .find(|r| r["request_id"] == upstream["user_id"])
            .unwrap_or_else(|| panic!("the user's call is listed: {second}"));
        assert_eq!(
            (
                &waiting["phase"],
                &waiting["held_for_room"],
                &waiting["leaving"]
            ),
            (
                &serde_json::json!("queued"),
                &serde_json::json!(false),
                &serde_json::json!(false)
            ),
            "the user's call waits for the engine's step, not for room: {waiting}"
        );
        assert_eq!(waiting["client"], "127.0.0.1:50100");
        assert_eq!(
            second["num_running"], 1,
            "only the user's call is waited for: the three rows still held are listed `leaving` \
             and not counted (Q-403; no HTTP handler is in flight here): {second}"
        );
        assert_eq!(second["status"], "generating");
        let named: std::collections::BTreeSet<&str> = second_state_requests(&upstream);
        let ids: std::collections::BTreeSet<&str> = upstream["check_ids"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(
            named, ids,
            "GOOSE_RANK_STATE names the request of every row: {}",
            upstream["second_step_state"]
        );
        let left: Vec<serde_json::Value> = upstream_printed
            .lines()
            .filter_map(|l| l.strip_prefix("GOOSE_RANK_ROW_LEFT "))
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(left.len(), 3, "{upstream_printed}");
        for row in &left {
            assert_eq!(row["stopped"], "cancelled_by_client", "{row}");
            assert_eq!(row["how"], "removed", "{row}");
            assert!(row["held_after_answer_s"].is_number(), "{row}");
        }

        let held = &fixed["held"];
        let row = held["requests"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["request_id"] == fixed["held_id"])
            .unwrap_or_else(|| panic!("the held request is listed: {held}"));
        assert_eq!(
            (&row["phase"], &row["held_for_room"]),
            (&serde_json::json!("queued"), &serde_json::json!(true)),
            "a request rank 0 holds for room says so: {row}"
        );
        assert!(
            fixed_printed.contains("GOOSE_RANK_ADMISSION"),
            "{fixed_printed}"
        );
    }

    /// Q-403 through the REAL mlx_lm 0.31.3 generation loop and rank 0's real HTTP handler (every
    /// dropped call is a raw socket POSTing /v1/chat/completions, so do_POST's in-flight count is
    /// the one the router read): E2E #3r's 13:40 turn replayed in its order. Two end-of-turn
    /// reviewers are read together (#3r: seven rows in step 23862, 13:40:34.8 → 13:41:17.6Z), a
    /// third arrives while no room is left and is HELD (#3r's req-580, 16,396 tokens), a fourth is
    /// still queued; goose closes all four mid-step (turn_priority, 13:40:37.6Z), and the router
    /// reads /v1/status before it sends the chat's own call (13:40:41.9Z, `free_slots 0`). Now the
    /// status reads every live socket before it counts: num_running is 0 while the step still runs,
    /// the two batch rows are listed stopped `cancelled_by_client`, and at the step's end they
    /// leave; the held and the queued request are dropped unshared — never in a prompt step — and
    /// the user's call is read at the very next step (#3r: req-580 was released, read a 2,048-token
    /// chunk, and held the chat's call 6.5 s). Each departure is named exactly once. The pre-fix
    /// wrapper (3.0.7x) counts the four do_POST handlers (num_running 4 for the whole step), releases
    /// the held call and admits the queued one into step 3 beside the user's.
    #[test]
    fn a_client_that_leaves_during_prefill_frees_its_slot_before_the_step_ends() {
        let Some(python) = proven_env(&EnvSpec::tensor(), "GOOSE_TEST_TENSOR_PYTHON") else {
            return;
        };
        let checks = r#"
import socket as sk
from mlx_lm.models import llama

mx.metal.is_available = lambda: False
VOCAB = 64
END = VOCAB - 1
SYSTEM = 12


class Detokenizer:
    last_segment = ""

    def add_token(self, token):
        self.last_segment = "w"


class Tokenizer:
    has_tool_calling = False
    has_thinking = False
    tool_parser = None
    eos_token_ids = {END}
    eos_token_id = END
    chat_template = QWEN38

    def encode(self, text, add_special_tokens=False):
        return [1]

    def convert_ids_to_tokens(self, ids):
        return "<end>"

    @property
    def detokenizer(self):
        return Detokenizer()


tok = Tokenizer()
model = llama.Model(llama.ModelArgs(model_type="llama", hidden_size=16, num_hidden_layers=1,
    intermediate_size=32, num_attention_heads=2, rms_norm_eps=1e-5, vocab_size=VOCAB, num_key_value_heads=2))
mx.eval(model.parameters())

provider = responses.model_provider
provider.tokenizer = tok
provider.is_batchable = True
provider.load_default = lambda: None
provider.load = lambda *names: (model, tok)
cli.prefill_step_size = 4
responses.prompt_cache = server.LRUPromptCache(cli.prompt_cache_size)
responses._state_machine_cache = {}
responses._stop = False
responses._rank = 0
responses._time_budget = server.TimeBudget.__new__(server.TimeBudget)
defaults = inspect.signature(server.TimeBudget.__init__).parameters
responses._time_budget.__dict__.update(
    _is_distributed=True, _budget=defaults["budget"].default,
    _iterations=defaults["iterations"].default, _sync_frequency=defaults["sync_frequency"].default,
    _start=None, _current_iterations=None, _loops=0, _time_spent=0)


def prompt_ids(request):
    # A direct call names its ids; an HTTP chat request says "<length> <first>".
    ids = getattr(request, "ids", None)
    if ids is None:
        length, first = map(int, request.messages[-1]["content"].split())
        ids = [(first + i) % (VOCAB - 2) + 1 for i in range(length)]
    return list(ids)


original_tokenize = lambda self, tokenizer, request, args: (prompt_ids(request), None, None, None)
responses._tokenize = lambda tokenizer, request, args: (
    prompt_ids(request), [prompt_ids(request)[:SYSTEM], prompt_ids(request)[SYSTEM:]],
    ["system", "user"], "normal")


def arguments(max_tokens):
    return server.GenerationArguments(
        model=server.ModelDescription("goose-test", None, None),
        sampling=server.SamplingArguments(0.0, 1.0, 0, 0.0, 0.0, 0.0),
        logits=server.LogitsProcessorArguments({END: -1e9}, 1.0, 20, 0.0, 20, 0.0, 20),
        stop_words=[], max_tokens=max_tokens, num_draft_tokens=0, logprobs=False, top_logprobs=0,
        seed=None, chat_template_kwargs=None)


steps, hooks = [], {}
wrapper_prompt = mlx_generate.PromptProcessingBatch.prompt


def hooked(self, tokens):
    if tokens:
        steps.append([batch_rows.get(uid) for uid in self.uids])
        hook = hooks.pop(len(steps), None)
        if hook is not None:
            hook()
    return wrapper_prompt(self, tokens)


mlx_generate.PromptProcessingBatch.prompt = hooked


def until(what, done):
    # A harness bound, never the engine's.
    deadline = time.monotonic() + 20
    while not done():
        assert time.monotonic() < deadline, what
        time.sleep(0.002)


def post(length, first):
    # goose's HTTP client, as a raw socket the test can close mid-request.
    body = json.dumps({"model": served, "max_tokens": 50, "stream": False,
                       "messages": [{"role": "user", "content": f"{length} {first}"}]}).encode()
    client = sk.create_connection(("127.0.0.1", httpd.server_address[1]))
    waiting = responses.requests.qsize()
    client.sendall(
        b"POST /v1/chat/completions HTTP/1.1\r\nHost: goose\r\nContent-Type: application/json\r\n"
        + f"Content-Length: {len(body)}\r\n\r\n".encode() + body
    )
    port = f"127.0.0.1:{client.getsockname()[1]}"
    until(f"{port} reaches the queue", lambda: responses.requests.qsize() > waiting)
    with lock:
        request_id = next(rid for rid, h in handling.items() if h["client"] == port)
    return client, request_id


def ask(length, first, max_tokens, port):
    ours, theirs = sk.socketpair()
    call = {"theirs": theirs, "done": threading.Event(), "pieces": 0, "request": types.SimpleNamespace(
        ids=[(first + i) % (VOCAB - 2) + 1 for i in range(length)], tools=None, request_type="chat")}
    waiting = responses.requests.qsize()

    def run():
        try:
            ctx, tokens = responses.generate(call["request"], arguments(max_tokens), None,
                                             client=ours, address=("127.0.0.1", port))
            for _ in tokens:
                call["pieces"] += 1
        finally:
            call["done"].set()

    threading.Thread(target=run, daemon=True).start()
    until("the user's call reaches the queue", lambda: responses.requests.qsize() > waiting)
    return call


def status():
    url = f"http://127.0.0.1:{httpd.server_address[1]}/v1/status"
    with urllib.request.urlopen(url, timeout=10) as reply:
        return json.loads(reply.read())


seen, dropped = {}, {}
limit = prompt_cache_limit
reviewers = [post(60, 0), post(60, 20)]
user = {}


def first_step():
    # No room is left: the next arrival is held (#3r req-580).
    global prompt_cache_limit
    prompt_cache_limit = 1
    dropped["held"] = post(40, 40)


def second_step():
    # The held one waits; one more is queued; goose closes every one of them mid-step.
    global prompt_cache_limit
    dropped["queued"] = post(40, 50)
    seen["held_before"] = list(held_ids[0])
    for client, _ in [*reviewers, dropped["held"], dropped["queued"]]:
        client.close()
    # The router's probe, before the chat's own call is sent (a harness bound: the pre-fix
    # wrapper never drops below the four handlers while this step runs).
    deadline = time.monotonic() + 10
    while True:
        seen["during"] = status()
        if seen["during"]["num_running"] == 0 or time.monotonic() > deadline:
            break
        time.sleep(0.01)
    prompt_cache_limit = limit
    user["call"] = ask(30, 7, 3, 50100)


hooks[1] = first_step
hooks[2] = second_step
engine = threading.Thread(target=responses._generate, daemon=True)
engine.start()
until("the engine reads its second prompt step", lambda: "call" in user)
assert user["call"]["done"].wait(60), "the user's call is answered"
until("every dropped handler has ended", lambda: all(
    rid not in handling or handling[rid]["handler_left"]
    for _, rid in [*reviewers, dropped["held"], dropped["queued"]]))
seen.update(
    steps=steps,
    reviewer_ids=[rid for _, rid in reviewers],
    held_id=dropped["held"][1],
    queued_id=dropped["queued"][1],
    user_id=user["call"]["request"].goose_request_id,
    user_pieces=user["call"]["pieces"],
    final=status(),
)
responses._stop = True
engine.join(30)
print("GOOSE_TEST " + json.dumps(seen))
"#;
        let (seen, printed) = run_wrapper_checks_with(&python, checks, |program| {
            if let RankProgram::MlxLmServer {
                prompt_cache_limit_bytes,
                prefill_step_yields,
                ..
            } = program
            {
                *prompt_cache_limit_bytes = Some(17_333_813_248);
                *prefill_step_yields = true;
            }
        });
        let id = |key: &str| seen[key].as_str().unwrap().to_string();
        let reviewers: Vec<String> = seen["reviewer_ids"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        let (held, queued, user) = (id("held_id"), id("queued_id"), id("user_id"));
        assert_eq!(
            seen["held_before"],
            serde_json::json!([held]),
            "the third call was held for room before goose closed it: {seen}"
        );

        let during = &seen["during"];
        assert_eq!(
            during["num_running"], 0,
            "the router's probe, mid-step, sees every slot free: four clients left and nobody \
             waits for their answers (pre-fix: 4, one per do_POST handler): {during}"
        );
        assert_eq!(
            during["status"], "generating",
            "the step still runs: {during}"
        );
        for reviewer in &reviewers {
            let row = during["requests"]
                .as_array()
                .unwrap()
                .iter()
                .find(|r| r["request_id"] == reviewer.as_str())
                .unwrap_or_else(|| panic!("{reviewer}'s batch row is listed: {during}"));
            assert_eq!(row["stopped"]["reason"], "cancelled_by_client", "{row}");
            assert_eq!(row["stopped"]["phase"], "prefill", "{row}");
        }

        let steps: Vec<Vec<String>> = seen["steps"]
            .as_array()
            .unwrap()
            .iter()
            .map(|rows| {
                rows.as_array()
                    .unwrap()
                    .iter()
                    .map(|r| r.as_str().unwrap_or("").to_string())
                    .collect()
            })
            .collect();
        let read_in = |request: &str| -> Vec<usize> {
            (1..=steps.len())
                .filter(|&n| steps[n - 1].iter().any(|r| r == request))
                .collect()
        };
        for reviewer in &reviewers {
            assert_eq!(
                read_in(reviewer),
                [1, 2],
                "{reviewer} leaves at the end of the step it was in when its client left: {steps:?}"
            );
        }
        assert_eq!(
            read_in(&held),
            Vec::<usize>::new(),
            "the held call is never given a prompt step (#3r req-580 read 2,048 tokens): {steps:?}"
        );
        assert_eq!(
            read_in(&queued),
            Vec::<usize>::new(),
            "the queued call is never given a prompt step: {steps:?}"
        );
        assert_eq!(
            read_in(&user).first(),
            Some(&3),
            "the user's call is read at the very next step: {steps:?}"
        );
        assert_eq!(
            seen["user_pieces"], 3,
            "the user's call is answered: {seen}"
        );

        let stops: Vec<serde_json::Value> = printed
            .lines()
            .filter_map(|l| l.strip_prefix("GOOSE_RANK_CANCELLED_BY_CLIENT "))
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        let phase_of = |request: &str| -> Vec<&str> {
            stops
                .iter()
                .filter(|s| s["request_id"] == request)
                .map(|s| s["phase"].as_str().unwrap())
                .collect()
        };
        for reviewer in &reviewers {
            assert_eq!(phase_of(reviewer), ["prefill"], "named once: {printed}");
        }
        assert_eq!(phase_of(&held), ["queued"], "named once: {printed}");
        assert_eq!(phase_of(&queued), ["queued"], "named once: {printed}");
        assert_eq!(stops.len(), 4, "{printed}");
        let dropped: Vec<serde_json::Value> = printed
            .lines()
            .filter_map(|l| l.strip_prefix("GOOSE_RANK_ADMISSION "))
            .map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap())
            .filter(|a| a.get("dropped_departed").is_some())
            .collect();
        assert_eq!(
            dropped,
            [serde_json::json!({"dropped_departed": [held], "still_held": 0})],
            "the rank's log says the held call left the hold unshared: {printed}"
        );

        let last = &seen["final"];
        assert_eq!(last["requests"], serde_json::json!([]), "{last}");
        assert_eq!(last["num_running"], 0, "{last}");
    }

    fn second_state_requests(seen: &serde_json::Value) -> std::collections::BTreeSet<&str> {
        seen["second_step_state"]["requests"]
            .as_object()
            .unwrap()
            .values()
            .map(|v| v.as_str().unwrap())
            .collect()
    }

    /// Q-161: the skeleton guard (rank_xml_guard.py) built from a tokenizer that splits the wire's
    /// markers as the Qwen3.5 tokenizer does, against the real qwen3.8 chat template. At each fixed
    /// position only the template's continuation (or the end of the turn) is left; `</parameter>`
    /// inside a line and anything inside `<think>` are untouched. The measured case: after the
    /// write + mkdir pair the split's model put `!` first (−0.13), `<|im_end|>` −2.6, `\n` −2.9 — the
    /// guard leaves the end of the turn on top. The wrapper hands the guard only to a tool request
    /// of a launch that asks for it, and a template that is not this wire gets none, said.
    #[test]
    fn the_skeleton_guard_leaves_only_the_templates_continuations() {
        let Some(python) = proven_env(&EnvSpec::tensor(), "GOOSE_TEST_TENSOR_PYTHON") else {
            return;
        };
        let checks = r#"
VOCAB = 4096

class PieceTokenizer:
    PIECES = ["<tool_call>", "</tool_call>", "<think>", "</think>", "<|im_end|>", "</", "<",
              "parameter", "function", ">", "=", "\n\n", "\n", "!"]

    def __init__(self, chat_template):
        self.chat_template = chat_template
        self.vocab = {p: i for i, p in enumerate(self.PIECES)}
        self.inverse = dict(enumerate(self.PIECES))
        self.eos_token_ids = {self.vocab["<|im_end|>"]}

    def _id(self, piece):
        if piece not in self.vocab:
            self.vocab[piece] = len(self.vocab)
            self.inverse[self.vocab[piece]] = piece
        return self.vocab[piece]

    def encode(self, text, add_special_tokens=False):
        ids, i = [], 0
        while i < len(text):
            piece = next((p for p in sorted(self.PIECES, key=len, reverse=True) if text.startswith(p, i)), text[i])
            ids.append(self._id(piece))
            i += len(piece)
        return ids

    def decode(self, ids, **_):
        return "".join(self.inverse.get(int(i), "") for i in ids)

    def __len__(self):
        return VOCAB

tok = PieceTokenizer(QWEN38)
guard = XmlSkeletonGuard(skeleton_spec(tok, tok.eos_token_ids))

def left(text, logits=None):
    out = guard(mx.array(tok.encode(text)), mx.zeros((1, VOCAB)) if logits is None else logits)
    kept = [i for i, ok in enumerate((out[0] > -1e30).tolist()) if ok]
    return None if len(kept) == VOCAB else sorted(tok.inverse[i] for i in kept)

CALL = "<tool_call>\n<function=write>\n<parameter=path>\n/w/a.md\n</parameter>\n</function>\n</tool_call>"
cut = lambda marker: CALL[: CALL.index(marker) + len(marker)]
row = [-30.0] * VOCAB
for piece, logprob in {"!": -0.13, "<|im_end|>": -2.6, "\n": -2.9}.items():
    row[tok._id(piece)] = logprob
measured = mx.array([row])
pair = CALL + "\n" + CALL.replace("write", "shell")
picked = lambda logits: tok.inverse[int(mx.argmax(logits, axis=-1).item())]

# The wrapper, on a launch that asks for it (the spec of rank_specs): armed from the tokenizer at
# load, handed to tool requests only.
arm_skeleton_guard(tok)

def processors_for(tools):
    args = types.SimpleNamespace(logits=server.LogitsProcessorArguments(None, 0.0, 20, 0.0, 20, 0.0, 20))
    mark_tool_request((Queue(), types.SimpleNamespace(tools=tools), args))
    return [type(p).__name__ for p in server._make_logits_processors(args)]

tool_request = processors_for([{"type": "function", "function": {"name": "write"}}])
chat_request = processors_for(None)
skeleton["spec"] = None
arm_skeleton_guard(PieceTokenizer("{{ messages }}"))
print("GOOSE_TEST " + json.dumps({
    "after_call": left(CALL),
    "after_call_newline": left(CALL + "\n"),
    "after_open": left("<tool_call>"),
    "after_open_newline": left("<tool_call>\n"),
    "after_value": left(cut("</parameter>")),
    "after_value_newline": left(cut("</parameter>") + "\n"),
    "after_value_newline_close": left(cut("</parameter>") + "\n</"),
    "after_function": left(cut("</function>")),
    "after_function_newline": left(cut("</function>") + "\n"),
    "close_inside_a_line": left("<tool_call>\n<function=shell>\n<parameter=command>\necho </parameter>"),
    "close_outside_a_call": left("text\n</parameter>"),
    "thinking": left("<think>\n" + CALL),
    "measured_unguarded": picked(measured),
    "measured_guarded": picked(guard(mx.array(tok.encode(pair)), measured)),
    "tool_request": tool_request,
    "chat_request": chat_request,
    "unarmed_spec": skeleton["spec"] is None,
}))
"#;
        let (seen, printed) = run_wrapper_checks(&python, checks);
        let pieces = |v: &[&str]| serde_json::json!(v);
        assert_eq!(seen["after_call"], pieces(&["\n", "<|im_end|>"]));
        assert_eq!(
            seen["after_call_newline"],
            pieces(&["<tool_call>", "<|im_end|>"]),
            "after a call's newline the turn may still end: the guard never forces another call"
        );
        assert_eq!(seen["after_open"], pieces(&["\n"]));
        assert_eq!(seen["after_open_newline"], pieces(&["<"]));
        assert_eq!(seen["after_value"], pieces(&["\n"]));
        assert_eq!(seen["after_value_newline"], pieces(&["<", "</"]));
        assert_eq!(seen["after_value_newline_close"], pieces(&["function"]));
        assert_eq!(seen["after_function"], pieces(&["\n"]));
        assert_eq!(seen["after_function_newline"], pieces(&["</tool_call>"]));
        assert_eq!(seen["close_inside_a_line"], serde_json::Value::Null);
        assert_eq!(seen["close_outside_a_call"], serde_json::Value::Null);
        assert_eq!(seen["thinking"], serde_json::Value::Null);
        assert_eq!(seen["measured_unguarded"], "!");
        assert_eq!(
            seen["measured_guarded"], "<|im_end|>",
            "the pair's end of turn outweighs another call once `!` is gone"
        );
        assert_eq!(
            seen["tool_request"],
            serde_json::json!(["XmlSkeletonGuard"])
        );
        assert_eq!(seen["chat_request"], serde_json::json!([]));
        assert_eq!(seen["unarmed_spec"], true);
        let lines: Vec<serde_json::Value> = printed
            .lines()
            .filter_map(|l| l.strip_prefix("GOOSE_RANK_XML_GUARD "))
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(lines.len(), 2, "{printed}");
        assert_eq!(lines[0]["armed"], true);
        assert_eq!(lines[1]["armed"], false);
        assert!(
            lines[1]["why"]
                .as_str()
                .unwrap()
                .contains("does not render the XML tool call"),
            "{printed}"
        );
    }

    /// Q-161 reopened, E2E #3f turn 0 (3.0.57) replayed through the REAL mlx_lm 0.31.3
    /// BatchGenerator as the wrapper runs it (on the CPU, a one-layer model): goose's title request
    /// (no tools, POST 11:52:24) generates and leaves, then the tool request (POST 11:52:27)
    /// joins. The tool request's model WANTS #3f's words — a scripted processor puts them first:
    /// one `write` call, `</tool_call>`, then `!\n\n</parameter>\n</function>\n!…` — and the
    /// wrapper's own processors for a tool request (the XML skeleton guard) follow it. With
    /// `row_processors` the guard runs on every token of the row and masks the `!` after
    /// `</tool_call>`: the answer ends there (`<|im_end|>`). NEGATIVE CONTROL, the 3.0.57 program
    /// (`row_processors` off): mlx_lm's GenerationBatch.filter kept the title row's `[]`, the tool
    /// row reads it, its processors run on its first token only and #3f's words come out. Second
    /// shape: a tool-less row that PREFILLED beside the tool row (its `None` kept) and then left
    /// raises `TypeError` in the generation thread upstream; with `row_processors` it is `[]`.
    #[test]
    fn a_joining_row_runs_its_own_logits_processors() {
        let Some(python) = proven_env(&EnvSpec::tensor(), "GOOSE_TEST_TENSOR_PYTHON") else {
            return;
        };
        let checks = r#"
from mlx_lm.models import llama

# MLX's CPU reports no working-set size; BatchGenerator asks for it only to wire memory.
mx.metal.is_available = lambda: False
VOCAB = 64

class PieceTokenizer:
    PIECES = ["<tool_call>", "</tool_call>", "<think>", "</think>", "<|im_end|>", "</", "<",
              "parameter", "function", ">", "=", "\n\n", "\n", "!"]

    def __init__(self, chat_template):
        self.chat_template = chat_template
        self.vocab = {p: i for i, p in enumerate(self.PIECES)}
        self.inverse = dict(enumerate(self.PIECES))
        self.eos_token_ids = {self.vocab["<|im_end|>"]}

    def encode(self, text, add_special_tokens=False):
        ids, i = [], 0
        while i < len(text):
            piece = next((p for p in sorted(self.PIECES, key=len, reverse=True) if text.startswith(p, i)), text[i])
            if piece not in self.vocab:
                self.vocab[piece] = len(self.vocab)
                self.inverse[self.vocab[piece]] = piece
            ids.append(self.vocab[piece])
            i += len(piece)
        return ids

    def decode(self, ids, **_):
        return "".join(self.inverse.get(int(i), "") for i in ids)

    def __len__(self):
        return VOCAB

tok = PieceTokenizer(QWEN38)
arm_skeleton_guard(tok)
END = tok.vocab["<|im_end|>"]
WORDS = ("<tool_call>\n<function=write>\n<parameter=path>\n/w/notes/kickoff.md\n</parameter>\n</function>\n</tool_call>"
         "!\n\n</parameter>\n</function>\n!\n</parameter>\n</function>\n!\n</parameter>\n!\n</function>\n"
         "!\n</function>\n!\n</function>\n")
SCRIPT = tok.encode(WORDS)
TITLE = tok.encode("title please\n")
TURN0 = tok.encode("turn it into notes\n")
assert max(SCRIPT + TITLE + TURN0) < VOCAB

class Wants:
    # The model #3f's tool request had: SCRIPT's next word on top, everything else far below.
    def __init__(self, prompt):
        self.prompt = len(prompt)

    def __call__(self, tokens, logits):
        written = tokens.shape[0] - self.prompt
        top = SCRIPT[written] if written < len(SCRIPT) else END
        return mx.where(mx.arange(logits.shape[-1]) == top, 0.0, -1e9)[None].astype(logits.dtype)

class Counted:
    def __init__(self, inner):
        self.inner, self.calls = inner, 0

    def __call__(self, tokens, logits):
        self.calls += 1
        return self.inner(tokens, logits)

def processors(tools):
    args = types.SimpleNamespace(logits=server.LogitsProcessorArguments(None, 0.0, 20, 0.0, 20, 0.0, 20))
    mark_tool_request((Queue(), types.SimpleNamespace(tools=tools), args))
    return server._make_logits_processors(args)

TOOLS = [{"type": "function", "function": {"name": "write"}}]
model = llama.Model(llama.ModelArgs(model_type="llama", hidden_size=16, num_hidden_layers=1,
    intermediate_size=32, num_attention_heads=2, rms_norm_eps=1e-5, vocab_size=VOCAB, num_key_value_heads=2))
mx.eval(model.parameters())

def run(generator, uid):
    written, finish = [], None
    for _ in range(4 * len(SCRIPT)):
        for r in generator.next()[1]:
            if r.uid == uid:
                written.append(r.token)
                finish = r.finish_reason or finish
        if finish:
            break
    return written, finish

def turn0(title_first):
    generator = server.BatchGenerator(model, max_tokens=4 * len(SCRIPT), stop_tokens=[[END]])
    guard = Counted(processors(TOOLS)[0])
    if title_first:
        (title,) = generator.insert([TITLE], max_tokens=[3], logits_processors=[processors(None)])
        run(generator, title)
    (tool,) = generator.insert([TURN0], logits_processors=[[Wants(TURN0), guard]])
    written, finish = run(generator, tool)
    return {"text": tok.decode(written), "finish": finish, "guard_calls": guard.calls, "tokens": len(written)}

def beside():
    # The title arrives while the tool request still prefills: PromptProcessingBatch.extend gives
    # the title's row `None`, the title reaches generation first and leaves, the tool row joins.
    generator = server.BatchGenerator(model, max_tokens=4 * len(SCRIPT), stop_tokens=[[END]], prefill_step_size=4)
    long_prompt = TURN0 * 6
    (tool,) = generator.insert([long_prompt], logits_processors=[[Wants(long_prompt), Counted(processors(TOOLS)[0])]])
    generator.next()
    (title,) = generator.insert([TITLE], max_tokens=[2], logits_processors=[processors(None)])
    try:
        written, finish = run(generator, tool)
        return {"text": tok.decode(written), "finish": finish}
    except TypeError as error:
        return {"raised": f"TypeError: {error}"}

print("GOOSE_TEST " + json.dumps({"alone": turn0(False), "after_title": turn0(True), "beside": beside(),
    "script_after_call": WORDS[WORDS.index("</tool_call>") + len("</tool_call>"):]}))
"#;
        let (fixed, _) = run_wrapper_checks(&python, checks);
        let (upstream, _) = run_wrapper_checks_with(&python, checks, |program| {
            if let RankProgram::MlxLmServer { row_processors, .. } = program {
                *row_processors = false;
            }
        });
        let call = "<tool_call>\n<function=write>\n<parameter=path>\n/w/notes/kickoff.md\n\
                    </parameter>\n</function>\n</tool_call>";
        for (arm, seen) in [("row_processors", &fixed), ("3.0.57", &upstream)] {
            assert_eq!(
                seen["alone"]["text"],
                format!("{call}<|im_end|>"),
                "{arm}: a tool request alone in the batch is guarded on every token: {seen}"
            );
        }
        let after_title = &fixed["after_title"];
        assert_eq!(
            after_title["text"],
            format!("{call}<|im_end|>"),
            "the `!` after `</tool_call>` is masked, and the turn ends: {after_title}"
        );
        assert_eq!(after_title["finish"], "stop");
        assert_eq!(
            after_title["guard_calls"].as_u64().unwrap(),
            after_title["tokens"].as_u64().unwrap() + 1,
            "the guard ran on every token the row wrote (and on the one mlx_lm samples a step ahead \
             of the answer's end): {after_title}"
        );
        let unguarded = &upstream["after_title"];
        assert_eq!(
            unguarded["guard_calls"], 1,
            "negative control: upstream ran the row's processors on its first token only: {unguarded}"
        );
        assert!(
            !unguarded["text"]
                .as_str()
                .unwrap()
                .contains(&format!("{call}<|im_end|>")),
            "negative control: #3f's words were not what the unguarded row wrote: {unguarded}"
        );
        assert_eq!(
            fixed["beside"]["text"],
            format!("{call}<|im_end|>"),
            "{}",
            fixed["beside"]
        );
        assert!(
            upstream["beside"]["raised"]
                .as_str()
                .is_some_and(|e| e.contains("'NoneType' object is not iterable")),
            "negative control: {}",
            upstream["beside"]
        );
    }

    /// Q-164 through the REAL mlx_lm 0.31.3 handler, prompt cache and `insert_segments`: an
    /// identical request sent twice gets two normal answers. The stand-in generation thread does
    /// what mlx_lm's `_generate` does with a shared request — the nearest cache entry, the prompt's
    /// segments trimmed by what it holds, `BatchGenerator.insert_segments` — and after answering
    /// stores the entry a one-token answer leaves (key = the prompt: the 08:23:23 crash's shape).
    /// The second, identical request reads only its last token: a plain-KV entry is trimmed by one
    /// (the rest reused whole), a hybrid entry (an ArraysCache layer: not trimmable) gives way to
    /// the nearest shorter one. An empty prompt is refused 400 `empty_prompt`, named. NEGATIVE
    /// CONTROL: mlx_lm's own LRUPromptCache hands the second request back with nothing to read, and
    /// its `insert_segments` raises the IndexError that ended both ranks.
    #[test]
    fn an_identical_request_sent_twice_gets_two_answers() {
        let Some(python) = proven_env(&EnvSpec::tensor(), "GOOSE_TEST_TENSOR_PYTHON") else {
            return;
        };
        let checks = r#"
from collections import deque
from mlx_lm.models.cache import LRUPromptCache as UpstreamPromptCache
from mlx_lm.generate import SequenceStateMachine

KEY = responses.model_provider.model_key

def kv(tokens):
    layer = KVCache()
    layer.update_and_fetch(mx.zeros((1, 1, tokens, 4)), mx.zeros((1, 1, tokens, 4)))
    return layer

def hybrid(tokens):
    state = ArraysCache(1)
    state[0] = mx.zeros((1, 4))
    return [kv(tokens), state]

def insert(generator, prompt, cache, rest):
    segments = [list(prompt)]
    n = len(prompt) - len(rest)
    while n > 0:
        if n >= len(segments[0]):
            n -= len(segments.pop(0))
        else:
            segments[0] = segments[0][n:]
            break
    try:
        generator.insert_segments(segments=[segments], caches=[cache], all_tokens=[prompt[: len(prompt) - len(rest)]], max_tokens=[8])
        return "inserted"
    except IndexError as fatal:
        return f"IndexError: {fatal}"

def batch():
    generator = server.BatchGenerator.__new__(server.BatchGenerator)
    generator.max_tokens, generator.logits_processors, generator._uid_count = 128, [], 0
    generator._default_state_machine = SequenceStateMachine({}, initial="normal")
    generator._unprocessed_sequences = deque()
    generator._make_new_cache = lambda: [KVCache()]
    return generator

answers = []
layers = {"kv": lambda n: [kv(n)], "hybrid": hybrid}

def generation_thread():
    while True:
        request = responses._share_request(responses.requests.get())
        if request is None:
            continue
        rqueue, completion, args = request
        prompt = [ord(c) for c in completion.prompt]
        cache, rest = responses.prompt_cache.fetch_nearest_cache(KEY, prompt)
        answers.append({"prompt": len(prompt), "read": len(rest), "insert": insert(batch(), prompt, cache, rest)})
        rqueue.put(server.GenerationContext(
            has_tool_calling=False, has_thinking=False, tool_parser=None,
            sequences={(3,): "<|im_end|>"}, prompt=prompt, prompt_cache_count=len(prompt) - len(rest),
        ))
        rqueue.put(token("ok", "normal"))
        rqueue.put(token("", None, (3,), "stop"))
        rqueue.put(None)
        responses.prompt_cache.insert_cache(KEY, prompt, layers[completion.prompt.split(":")[0]](len(prompt)))

threading.Thread(target=generation_thread, daemon=True).start()
responses.prompt_cache = server.LRUPromptCache(10)
# The test launch plans a 2-byte cache; these entries must stay.
responses.prompt_cache.max_bytes = 1 << 40
seen = {}
for kind in ("kv", "hybrid"):
    prompt = f"{kind}: the kickoff notes"
    if kind == "hybrid":
        responses.prompt_cache.insert_cache(KEY, [ord(c) for c in prompt[:8]], hybrid(8))
    seen[kind] = [post("/v1/completions", {"model": served, "prompt": prompt, "max_tokens": 1}) for _ in range(2)]
seen["empty"] = post("/v1/completions", {"model": served, "prompt": "", "max_tokens": 1})

upstream = UpstreamPromptCache(10)
prompt = [ord(c) for c in "kv: the kickoff notes"]
upstream.insert_cache(KEY, prompt, [kv(len(prompt))])
cache, rest = upstream.fetch_nearest_cache(KEY, prompt)
print("GOOSE_TEST " + json.dumps({**seen, "answers": answers, "upstream_read": len(rest),
    "upstream_insert": insert(batch(), prompt, cache, rest)}))
"#;
        let (seen, _) = run_wrapper_checks(&python, checks);
        for kind in ["kv", "hybrid"] {
            for reply in seen[kind].as_array().unwrap() {
                assert_eq!(reply[0], 200, "{kind}: {seen}");
            }
        }
        let kv = "kv: the kickoff notes".len() as u64;
        let hybrid = "hybrid: the kickoff notes".len() as u64;
        let answers = seen["answers"].as_array().unwrap();
        let expect = |i: usize, prompt: u64, read: u64| {
            assert_eq!(answers[i]["prompt"], prompt, "{seen}");
            assert_eq!(answers[i]["read"], read, "answer {i}: {seen}");
            assert_eq!(answers[i]["insert"], "inserted", "{seen}");
        };
        expect(0, kv, kv);
        expect(1, kv, 1);
        expect(2, hybrid, hybrid - 8);
        expect(3, hybrid, hybrid - 8);
        assert_eq!(seen["empty"][0], 400, "{seen}");
        assert!(
            seen["empty"][1].as_str().unwrap().contains("empty_prompt"),
            "{seen}"
        );
        assert_eq!(seen["upstream_read"], 0);
        assert_eq!(
            seen["upstream_insert"],
            "IndexError: list index out of range"
        );
    }

    /// Q-177 through the REAL mlx_lm 0.31.3 handler: every request field the split cannot honour
    /// is a 400 naming the field (`param`), its code and — where the engine has one — its limit,
    /// answered by rank 0 before the request reaches `responses.requests` (the queue
    /// `_next_request` shares with every rank); the stand-in generation thread records every
    /// request that reaches it. Before the fix: `top_logprobs` 12, a negative max_tokens, a
    /// missing `messages` closed the connection with no reply (mlx_lm's validator raises a bare
    /// ValueError its do_POST never catches); `n` 2, a JSON `response_format`, a `seed`, a
    /// non-string stop word reached generation (the last one kills every rank's generation thread
    /// in `_make_state_machine`); `stream_options: {}` ended the stream on a KeyError with no
    /// [DONE]. Any other exception escaping the handler before a response began is a 500 naming
    /// it. POSITIVE CONTROLS: `top_logprobs` 11, `response_format` text with `n` 1 and a real stop
    /// word, a chat request, and `stream_options: {}` are served whole.
    #[test]
    fn a_request_the_split_cannot_honour_is_refused_before_any_rank_sees_it() {
        let Some(python) = proven_env(&EnvSpec::tensor(), "GOOSE_TEST_TENSOR_PYTHON") else {
            return;
        };
        let checks = r#"
arrived = []

def generation_thread():
    while True:
        rqueue, request, args = responses.requests.get()
        if request.request_type == "text":
            arrived.append(request.prompt)
        elif isinstance(request.messages, str):
            arrived.append(request.messages)
        else:
            arrived.append(request.messages[-1]["content"])
        rqueue.put(server.GenerationContext(
            has_tool_calling=False, has_thinking=False, tool_parser=None,
            sequences={(3,): "<|im_end|>"}, prompt=[1, 2], prompt_cache_count=0,
        ))
        rqueue.put(token("ok", "normal"))
        rqueue.put(token("", None, (3,), "stop"))
        rqueue.put(None)

threading.Thread(target=generation_thread, daemon=True).start()

def sent(case, path, extra):
    if path == "/v1/completions":
        body = {"model": served, "prompt": case, **extra}
    else:
        body = {"model": served, "messages": [{"role": "user", "content": case}], **extra}
    for key in [k for k, v in extra.items() if v is None]:
        del body[key]
    try:
        status, text = post(path, body)
    except Exception as dropped:
        return {"status": "dropped", "why": f"{type(dropped).__name__}: {dropped}"}
    try:
        return {"status": status, "body": json.loads(text)}
    except ValueError:
        return {"status": status, "text": text}

TEXT, CHAT = "/v1/completions", "/v1/chat/completions"
cases = {
    "top_logprobs_12": (TEXT, {"logprobs": True, "top_logprobs": 12}),
    "top_logprobs_negative": (TEXT, {"logprobs": True, "top_logprobs": -2}),
    "logprobs_integer": (TEXT, {"logprobs": 5}),
    "n_2": (CHAT, {"n": 2}),
    "n_zero": (TEXT, {"n": 0}),
    "json_object": (CHAT, {"response_format": {"type": "json_object"}}),
    "json_schema": (TEXT, {"response_format": {"type": "json_schema", "json_schema": {"name": "x"}}}),
    "max_tokens_negative": (TEXT, {"max_tokens": -1}),
    "max_completion_tokens_negative": (CHAT, {"max_completion_tokens": -5}),
    "stop_not_string": (TEXT, {"stop": ["\n", 7]}),
    "stop_empty": (TEXT, {"stop": [""]}),
    "stop_object": (TEXT, {"stop": {"end": 1}}),
    "seed": (TEXT, {"seed": 7}),
    "stream_options_string": (TEXT, {"stream": True, "stream_options": "usage"}),
    "temperature_negative": (TEXT, {"temperature": -0.5}),
    "no_messages": (CHAT, {"messages": None}),
    "messages_string": (CHAT, {"messages": "messages_string"}),
    "no_prompt": (TEXT, {"prompt": None}),
    "top_logprobs_11": (TEXT, {"logprobs": True, "top_logprobs": 11}),
    "text_format": (TEXT, {"response_format": {"type": "text"}, "n": 1, "stop": ["\n\n"]}),
    "chat": (CHAT, {}),
    "stream_usage_default": (TEXT, {"stream": True, "stream_options": {}}),
}
seen = {case: sent(case, path, extra) for case, (path, extra) in cases.items()}

def unforeseen(self):
    raise RuntimeError("an unforeseen handler failure")

server.APIHandler.handle_text_completions = unforeseen
seen["unforeseen"] = sent("unforeseen", TEXT, {})
seen["arrived"] = arrived
print("GOOSE_TEST " + json.dumps(seen))
"#;
        let (seen, _) = run_wrapper_checks(&python, checks);
        let arrived: Vec<&str> = seen["arrived"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c.as_str().unwrap())
            .collect();
        // (case, param, code, words the message must carry)
        let refused = [
            (
                "top_logprobs_12",
                "top_logprobs",
                "invalid_value",
                "at most 11",
            ),
            (
                "top_logprobs_negative",
                "top_logprobs",
                "invalid_value",
                "at least 0",
            ),
            ("logprobs_integer", "logprobs", "invalid_value", "bool"),
            ("n_2", "n", "unsupported_parameter", "n must be 1"),
            ("n_zero", "n", "invalid_value", "positive integer"),
            (
                "json_object",
                "response_format",
                "unsupported_parameter",
                "json_object",
            ),
            (
                "json_schema",
                "response_format",
                "unsupported_parameter",
                "json_schema",
            ),
            (
                "max_tokens_negative",
                "max_tokens",
                "invalid_value",
                "at least 0",
            ),
            (
                "max_completion_tokens_negative",
                "max_completion_tokens",
                "invalid_value",
                "at least 0",
            ),
            ("stop_not_string", "stop", "invalid_value", "stop[1]"),
            ("stop_empty", "stop", "invalid_value", "stop[0] is empty"),
            ("stop_object", "stop", "invalid_value", "dict"),
            (
                "seed",
                "seed",
                "unsupported_parameter",
                "NotImplementedError",
            ),
            (
                "stream_options_string",
                "stream_options",
                "invalid_value",
                "str",
            ),
            (
                "temperature_negative",
                "temperature",
                "invalid_value",
                "at least 0",
            ),
            (
                "no_messages",
                "messages",
                "missing_required_parameter",
                "required",
            ),
            ("messages_string", "messages", "invalid_value", "str"),
            (
                "no_prompt",
                "prompt",
                "missing_required_parameter",
                "required",
            ),
        ];
        for (case, param, code, words) in refused {
            let reply = &seen[case];
            assert_eq!(reply["status"], 400, "{case}: {reply}");
            let error = &reply["body"]["error"];
            assert_eq!(error["param"], param, "{case}: {reply}");
            assert_eq!(error["code"], code, "{case}: {reply}");
            assert_eq!(error["type"], "invalid_request_error", "{case}: {reply}");
            assert!(
                error["message"].as_str().unwrap().contains(words),
                "{case}: {reply}"
            );
            assert!(
                !arrived.contains(&case),
                "{case} reached generation: {seen}"
            );
        }
        for case in [
            "top_logprobs_11",
            "text_format",
            "chat",
            "stream_usage_default",
        ] {
            assert_eq!(seen[case]["status"], 200, "{case}: {}", seen[case]);
            assert!(arrived.contains(&case), "{case} never reached generation");
        }
        assert!(
            seen["stream_usage_default"]["text"]
                .as_str()
                .unwrap()
                .ends_with("data: [DONE]\n\n"),
            "{}",
            seen["stream_usage_default"]
        );
        assert_eq!(seen["unforeseen"]["status"], 500, "{seen}");
        assert_eq!(
            seen["unforeseen"]["body"]["error"]["message"],
            "RuntimeError: an unforeseen handler failure"
        );
        assert_eq!(arrived.len(), 4, "{seen}");
    }

    /// Q-146 through the REAL mlx_lm 0.31.3 handler: while a streamed chat answer writes a tool
    /// call, rank 0's /v1/status row says what its client has and has not been sent, and a
    /// GOOSE_RANK_WITHHELD line marks each span the handler withholds text in. Four answers, each
    /// paused mid-call while /v1/status is read (the generation is a stand-in feeding mlx_lm's own
    /// Response objects through its own control-token buffer):
    /// - a clean call (a long string `command`, then an integer `timeout`): parser state `tool`,
    ///   the streamer reading `command` as a string, sent characters, nothing withheld beyond the
    ///   string's held tail, the words in the tail; its only withheld span is the typed value,
    ///   entered and left with nothing unsent;
    /// - the same parameter written twice (the streamer's `broken`): mode `tool_broken` naming why,
    ///   the characters since the last frame growing with the call, and the leave line carrying
    ///   the call's words and the characters that were never sent;
    /// - JSON inside `<tool_call>` (a frame the streamer cannot read): mode `tool_unread`;
    /// - NEGATIVE CONTROL: a non-streamed request, whose row carries `stream: null`.
    #[test]
    fn the_status_names_what_a_streamed_answer_withholds() {
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

TOOLS = [{"type": "function", "function": {"name": "shell", "parameters": {"type": "object",
    "properties": {"command": {"type": "string"}, "timeout": {"type": "integer"}}}}}]
COMMAND = "".join(f"echo row {i} ünï \U0001f9a2 >> /tmp/out.txt && " for i in range(40)) + "true"
LEAD = ("I'll", " run", " it.\n")
CLEAN = (f"\n<function=shell>\n<parameter=command>\n{COMMAND}\n</parameter>\n"
         "<parameter=timeout>\n30\n</parameter>\n</function>\n")
BROKEN = (f"\n<function=shell>\n<parameter=command>\necho first\n</parameter>\n"
          f"<parameter=command>\n{COMMAND}\n</parameter>\n</function>\n")
JSON_CALL = "\n" + json.dumps({"name": "shell", "arguments": {"command": COMMAND}}, ensure_ascii=False) + "\n"

def pieces(text):
    return [text[i:i + 3] for i in range(0, len(text), 3)]

def pause_inside(text):
    at = text.rindex(COMMAND) + len(COMMAND) // 2
    return at // 3

responses = server.ResponseGenerator.__new__(server.ResponseGenerator)
responses.model_provider = types.SimpleNamespace(cli_args=cli, tokenizer=types.SimpleNamespace(chat_template=QWEN38))
responses.requests = Queue()
httpd = http.server.ThreadingHTTPServer(
    ("127.0.0.1", 0),
    lambda *args, **kwargs: server.APIHandler(responses, *args, system_fingerprint="test", **kwargs),
)
threading.Thread(target=httpd.serve_forever, daemon=True).start()
base = f"http://127.0.0.1:{httpd.server_address[1]}"
scripts = Queue()
paused = threading.Event()
resume = threading.Event()

def token(text, state, match=None, finish=None):
    return server.Response(text, 7, state, match, 0.0, finish, ())

def generation_thread():
    while True:
        rqueue, request, args = responses.requests.get()
        text, pause_at = scripts.get()
        rqueue.put(server.GenerationContext(
            has_tool_calling=True, has_thinking=False, tool_parser=qwen3_coder.parse_tool_call,
            sequences={(1,): "<tool_call>", (2,): "</tool_call>", (3,): "<|im_end|>"},
            prompt=[0] * 8, prompt_cache_count=0,
        ))
        for piece in LEAD:
            rqueue.put(token(piece, "normal"))
        rqueue.put(token("<tool_call>", "tool", (1,)))
        for piece in pieces(text)[:pause_at]:
            rqueue.put(token(piece, "tool"))
        paused.set()
        resume.wait()
        resume.clear()
        for piece in pieces(text)[pause_at:]:
            rqueue.put(token(piece, "tool"))
        rqueue.put(token("</tool_call>", "normal", (2,)))
        rqueue.put(token("<|im_end|>", None, (3,), "stop"))
        rqueue.put(None)

threading.Thread(target=generation_thread, daemon=True).start()

def get(path):
    with urllib.request.urlopen(base + path, timeout=30) as reply:
        return json.loads(reply.read())

def client(stream):
    body = {"model": served, "stream": stream, "tools": TOOLS,
            "messages": [{"role": "user", "content": "write the rows"}]}
    request = urllib.request.Request(base + "/v1/chat/completions", data=json.dumps(body).encode())
    with urllib.request.urlopen(request, timeout=60) as reply:
        return reply.read().decode()

def run_case(text, stream=True):
    pause_at = pause_inside(text)
    scripts.put((text, pause_at))
    done = []
    reader = threading.Thread(target=lambda: done.append(client(stream)))
    reader.start()
    assert paused.wait(30), "the generation never reached its pause"
    paused.clear()
    generated = sum(len(p) for p in LEAD) + sum(len(p) for p in pieces(text)[:pause_at])
    tokens = len(LEAD) + 1 + pause_at
    for _ in range(3000):
        rows = get("/v1/status")["requests"]
        row = rows[0] if rows else None
        if row is not None and row["completion_tokens"] == tokens and (
            row["stream"] is None or row["stream"]["generated_chars"] == generated
        ):
            break
        time.sleep(0.01)
    else:
        raise AssertionError(f"the status never reached the pause: {rows}")
    resume.set()
    reader.join(60)
    written = text[:pause_at * 3]
    return {"row": row, "generated": generated, "done": bool(done),
            "after": get("/v1/status")["requests"], "last_words": written[-40:],
            "after_last_header": len(written) - written.rfind("<parameter=command>")
                - len("<parameter=command>")}

cases = {
    "clean": run_case(CLEAN),
    "broken": run_case(BROKEN),
    "json": run_case(JSON_CALL),
    "not_streamed": run_case(CLEAN, stream=False),
}
print("GOOSE_TEST " + json.dumps({"cases": cases, "hold": ToolCallStream.HOLD, "piece": 3,
                                   "command": COMMAND, "window": READER_TAIL_CHARS}))
"#;
        let program = format!(
            "{}\
             class _Group:\n    def rank(self): return 0\n    def size(self): return 2\n\
             group = _Group()\n{}QWEN38 = {qwen}\n{checks}",
            tensor_modules(),
            &wrapper[start..end],
            qwen = serde_json::to_string(QWEN38).unwrap(),
        );
        let (seen, printed) = run_against_real_packages_printing(&python, &program, &spec);
        let cases = &seen["cases"];
        let command = seen["command"].as_str().unwrap();
        let window = seen["window"].as_u64().unwrap();
        let withheld = |request: &serde_json::Value| -> Vec<serde_json::Value> {
            printed
                .lines()
                .filter_map(|l| l.strip_prefix("GOOSE_RANK_WITHHELD "))
                .map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap())
                .filter(|l| l["request_id"] == *request)
                .collect()
        };
        for (case, case_seen) in cases.as_object().unwrap() {
            assert_eq!(
                case_seen["done"], true,
                "{case}: the client read the whole answer"
            );
            assert_eq!(
                case_seen["after"],
                serde_json::json!([]),
                "{case}: an answered request leaves the table"
            );
        }

        let clean = &cases["clean"]["row"];
        let stream = &clean["stream"];
        assert_eq!(stream["parser_state"], "tool", "{clean}");
        assert_eq!(stream["withholding"], serde_json::Value::Null, "{clean}");
        assert_eq!(stream["generated_chars"], cases["clean"]["generated"]);
        let call = &stream["tool_call"];
        assert_eq!(call["streamed"], true, "{call}");
        assert_eq!(call["name"], "shell");
        assert_eq!(call["phase"], "value");
        assert_eq!(call["parameter"], "command");
        assert_eq!(call["string_value"], true);
        assert_eq!(call["broken"], serde_json::Value::Null);
        assert!(call["sent_chars"].as_u64().unwrap() > 100, "{call}");
        assert!(
            stream["since_sent_chars"].as_u64().unwrap()
                <= seen["hold"].as_u64().unwrap() + seen["piece"].as_u64().unwrap(),
            "a streaming string holds back only its possible close: {stream}"
        );
        let tail = stream["tail"].as_str().unwrap();
        assert!(
            tail.ends_with(cases["clean"]["last_words"].as_str().unwrap()),
            "the tail ends on the last words written: {tail:?}"
        );
        assert!(
            tail.contains("<tool_call>"),
            "the control sequence the model wrote is shown"
        );
        assert!(tail.chars().count() as u64 <= window);
        assert_eq!(stream["tail_window_chars"], window);
        let lines = withheld(&clean["request_id"]);
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert_eq!(lines[0]["event"], "enter");
        assert_eq!(lines[0]["mode"], "tool_typed_value");
        assert!(lines[0]["reason"].as_str().unwrap().contains("'timeout'"));
        assert_eq!(lines[1]["event"], "leave");
        assert_eq!(lines[1]["mode"], "tool_typed_value");
        assert_eq!(
            lines[1]["since_sent_chars"], 0,
            "the typed value went out when it closed: {:?}",
            lines[1]
        );

        let broken = &cases["broken"]["row"];
        let stream = &broken["stream"];
        assert_eq!(stream["parser_state"], "tool");
        assert_eq!(stream["withholding"]["mode"], "tool_broken", "{broken}");
        assert!(stream["withholding"]["reason"]
            .as_str()
            .unwrap()
            .contains("'command' written twice"));
        assert!(stream["tool_call"]["broken"]
            .as_str()
            .unwrap()
            .contains("written twice"));
        assert!(
            stream["since_sent_chars"].as_u64().unwrap()
                >= cases["broken"]["after_last_header"].as_u64().unwrap(),
            "everything after the second header is unsent: {stream}"
        );
        let lines = withheld(&broken["request_id"]);
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert_eq!(
            (&lines[0]["event"], &lines[0]["mode"]),
            (&"enter".into(), &"tool_broken".into())
        );
        assert_eq!(lines[1]["event"], "leave");
        assert!(
            lines[1]["withheld_chars"].as_u64().unwrap() as usize >= command.chars().count(),
            "{:?}",
            lines[1]
        );
        assert!(
            lines[1]["since_sent_chars"].as_u64().unwrap() as usize >= command.chars().count(),
            "the rest of the call was never sent: {:?}",
            lines[1]
        );
        let words = lines[1]["tail"].as_str().unwrap();
        assert!(
            words.contains("</tool_call>") && words.contains("echo row 39"),
            "the log keeps the words that were withheld: {words:?}"
        );
        assert!(
            printed.contains("GOOSE_RANK_TOOL_CALL_UNPARSED"),
            "the unparsed call is still named"
        );

        let json_call = &cases["json"]["row"];
        let stream = &json_call["stream"];
        assert_eq!(stream["withholding"]["mode"], "tool_unread", "{json_call}");
        assert_eq!(stream["tool_call"]["phase"], "head");
        assert_eq!(stream["tool_call"]["sent_chars"], 0);
        let lines = withheld(&json_call["request_id"]);
        assert_eq!(
            lines.iter().map(|l| l["event"].clone()).collect::<Vec<_>>(),
            vec!["enter", "leave"],
            "{lines:?}"
        );
        assert_eq!(lines[0]["mode"], "tool_unread");

        let not_streamed = &cases["not_streamed"]["row"];
        assert_eq!(
            not_streamed["stream"],
            serde_json::Value::Null,
            "a request answered whole has no stream to watch: {not_streamed}"
        );
        assert!(withheld(&not_streamed["request_id"]).is_empty());
    }

    /// Q-159 through the REAL mlx_lm 0.31.3 handler: what a chat request reaches mlx_lm's own
    /// `_make_sampler` / `_make_logits_processors` with (the functions its generation loop builds
    /// every row's sampler from), recorded at the module functions they call. E2E #3d's split
    /// decoded GREEDY — goose names no sampling field and mlx_lm filled the absence with `--temp`
    /// 0.0 — and one answer held 54 identical tool calls; the single engine samples the checkpoint's
    /// generation_config.json (the 27B's, verbatim here: temperature 1.0, top_k 20, top_p 0.95).
    /// Rank 0 now resolves request > goose's profile (min_p 0.05 here) > the config > the single
    /// engine's fallback; a request's own value wins, its null is no value, its 0 is its greedy.
    /// The request's /v1/status row names each value and its layer. NEGATIVE CONTROL: a checkpoint
    /// with no generation_config.json is named (log line + status field) and samples the single
    /// engine's fallback — never greedy.
    #[test]
    fn a_request_with_no_sampling_field_reaches_the_sampler_with_the_checkpoints_defaults() {
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
        let checkpoint = tempfile::tempdir().unwrap();
        std::fs::write(
            checkpoint.path().join("generation_config.json"),
            r#"{"bos_token_id": 248044, "do_sample": true, "eos_token_id": [248046, 248044],
                "pad_token_id": 248044, "temperature": 1.0, "top_k": 20, "top_p": 0.95}"#,
        )
        .unwrap();
        let bare = tempfile::tempdir().unwrap();
        let spec_for = |model_dir: &std::path::Path| {
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
            spec.model_dir = model_dir.display().to_string();
            spec.set_sampling_defaults(SamplingDefaults {
                min_p: Some(0.05),
                ..SamplingDefaults::default()
            });
            spec
        };
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

samplers, processors = [], []
server.make_sampler = lambda temp, **kw: samplers.append(
    {"temperature": temp, "top_p": kw["top_p"], "top_k": kw["top_k"], "min_p": kw["min_p"]})
server.make_logits_processors = lambda bias, rep, rep_n, pres, pres_n, freq, freq_n: processors.append(
    {"repetition_penalty": rep, "presence_penalty": pres, "frequency_penalty": freq})
tokenizer = types.SimpleNamespace(chat_template=QWEN38, eos_token_id=3, encode=lambda text: [4])

responses = server.ResponseGenerator.__new__(server.ResponseGenerator)
responses.model_provider = types.SimpleNamespace(cli_args=cli, tokenizer=tokenizer)
responses.requests = Queue()
httpd = http.server.ThreadingHTTPServer(
    ("127.0.0.1", 0),
    lambda *args, **kwargs: server.APIHandler(responses, *args, system_fingerprint="test", **kwargs),
)
threading.Thread(target=httpd.serve_forever, daemon=True).start()
base = f"http://127.0.0.1:{httpd.server_address[1]}"
paused = threading.Event()
resume = threading.Event()
pause_next = []

def generation_thread():
    while True:
        rqueue, request, args = responses.requests.get()
        # What mlx_lm's own generation loop hands BatchGenerator.insert_segments for this row.
        server._make_sampler(args, tokenizer)
        server._make_logits_processors(args)
        rqueue.put(server.GenerationContext(
            has_tool_calling=False, has_thinking=False, tool_parser=None,
            sequences={(3,): "<|im_end|>"}, prompt=[0] * 8, prompt_cache_count=0,
        ))
        rqueue.put(server.Response("ok", 7, "normal", None, 0.0, None, ()))
        if pause_next:
            pause_next.clear()
            paused.set()
            resume.wait()
            resume.clear()
        rqueue.put(server.Response("<|im_end|>", 3, None, (3,), 0.0, "stop", ()))
        rqueue.put(None)

threading.Thread(target=generation_thread, daemon=True).start()

def get(path):
    with urllib.request.urlopen(base + path, timeout=30) as reply:
        return json.loads(reply.read())

def chat(**fields):
    body = {"model": served, "messages": [{"role": "user", "content": "go"}], **fields}
    request = urllib.request.Request(base + "/v1/chat/completions", data=json.dumps(body).encode())
    with urllib.request.urlopen(request, timeout=60) as reply:
        reply.read()
    return {"sampler": samplers[-1], "processors": processors[-1]}

# The request goose sends, read on /v1/status while it generates.
pause_next.append(True)
done = []
reader = threading.Thread(target=lambda: done.append(chat()))
reader.start()
assert paused.wait(30), "the generation never reached its pause"
row = get("/v1/status")["requests"][0]
resume.set()
reader.join(60)
print("GOOSE_TEST " + json.dumps({
    "absent": done[0],
    "row": row,
    "explicit": chat(temperature=0.3, top_k=7, presence_penalty=0.5),
    "null": chat(temperature=None, top_p=None),
    "greedy": chat(temperature=0.0),
    "defaults": get("/v1/status")["sampling_defaults"],
}))
"#;
        let program = format!(
            "{}\
             class _Group:\n    def rank(self): return 0\n    def size(self): return 2\n\
             group = _Group()\n{}QWEN38 = {qwen}\n{checks}",
            tensor_modules(),
            &wrapper[start..end],
            qwen = serde_json::to_string(QWEN38).unwrap(),
        );
        let sampled = |t: f64, p: f64, k: u64, m: f64| serde_json::json!({"temperature": t, "top_p": p, "top_k": k, "min_p": m});

        let (seen, printed) =
            run_against_real_packages_printing(&python, &program, &spec_for(checkpoint.path()));
        assert_eq!(
            seen["absent"]["sampler"],
            sampled(1.0, 0.95, 20, 0.05),
            "the checkpoint's own sampling under goose's profile — never greedy (was 0.0, 1.0, 0, 0.0)"
        );
        assert_eq!(
            seen["absent"]["processors"],
            serde_json::json!({"repetition_penalty": 0.0, "presence_penalty": 0.0,
                               "frequency_penalty": 0.0}),
            "no layer sets a penalty: mlx_lm's own off"
        );
        let row = &seen["row"]["sampling"];
        for (key, value, layer) in [
            ("temperature", serde_json::json!(1.0), "generation_config"),
            ("top_p", serde_json::json!(0.95), "generation_config"),
            ("top_k", serde_json::json!(20), "generation_config"),
            ("min_p", serde_json::json!(0.05), "profile"),
            ("repetition_penalty", serde_json::json!(0.0), "unset"),
            ("presence_penalty", serde_json::json!(0.0), "unset"),
            ("frequency_penalty", serde_json::json!(0.0), "unset"),
        ] {
            assert_eq!(
                row[key],
                serde_json::json!({"value": value, "from": layer}),
                "{key}: {row}"
            );
        }
        assert_eq!(
            seen["explicit"]["sampler"],
            sampled(0.3, 0.95, 7, 0.05),
            "the request's own values win"
        );
        assert_eq!(seen["explicit"]["processors"]["presence_penalty"], 0.5);
        assert_eq!(
            seen["null"]["sampler"],
            sampled(1.0, 0.95, 20, 0.05),
            "a null is no value (mlx_lm refused it as not a number)"
        );
        assert_eq!(
            seen["greedy"]["sampler"],
            sampled(0.0, 0.95, 20, 0.05),
            "an explicit 0 is the client's own greedy"
        );
        let defaults = &seen["defaults"];
        assert_eq!(
            defaults["generation_config"],
            serde_json::json!({"temperature": 1.0, "top_k": 20, "top_p": 0.95})
        );
        assert_eq!(defaults["generation_config_error"], serde_json::Value::Null);
        assert_eq!(defaults["profile"], serde_json::json!({"min_p": 0.05}));
        assert!(
            printed.contains("GOOSE_RANK_SAMPLING_DEFAULTS "),
            "{printed}"
        );
        assert!(!printed.contains("GOOSE_RANK_GENERATION_CONFIG_UNREAD"));

        let (seen, printed) =
            run_against_real_packages_printing(&python, &program, &spec_for(bare.path()));
        assert_eq!(
            seen["absent"]["sampler"],
            sampled(0.7, 0.9, 0, 0.05),
            "no config: the single engine's fallback, never greedy"
        );
        assert_eq!(
            seen["row"]["sampling"]["temperature"]["from"],
            "engine_fallback"
        );
        assert_eq!(seen["row"]["sampling"]["top_k"]["from"], "unset");
        let error = seen["defaults"]["generation_config_error"]
            .as_str()
            .unwrap();
        assert!(
            error.ends_with("generation_config.json is absent"),
            "{error}"
        );
        let unread = printed
            .lines()
            .find_map(|l| l.strip_prefix("GOOSE_RANK_GENERATION_CONFIG_UNREAD "))
            .unwrap_or_else(|| panic!("the absence is named in the log: {printed}"));
        let unread: serde_json::Value = serde_json::from_str(unread).unwrap();
        assert_eq!(unread["error"], error);
        assert_eq!(
            unread["in_force"],
            serde_json::json!({"temperature": 0.7, "top_p": 0.9})
        );
    }

    /// The tensor wrapper's own module prelude — its imports, both upstream-attribute checks, the
    /// Q-232 install over qwen3_coder's `_get_arguments_config` and the Q-372 install over its
    /// `_parse_xml_function_call`, exactly as shipped — run
    /// against the REAL mlx_lm 0.31.3. The stubbed tests never executed
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
            "{}{}import types\nfrom mlx_lm.models.cache import ArraysCache, BatchKVCache\n\
             spec = {{\"prefill\": {{}}, \"prompt_cache_limit_bytes\": 1}}\n{}\n\
             assert isinstance(mlx_generate, types.ModuleType), mlx_generate\n\
             assert isinstance(server, types.ModuleType), server\n\
             assert qwen3_coder._get_arguments_config.__wrapped__.__module__ == qwen3_coder.__name__\n\
             assert qwen3_coder._parse_xml_function_call.__qualname__.startswith(\"install_positional_parameters.\")\n\
             print(\"ok\")\n",
            include_str!("rank_tool_schema.py"),
            include_str!("rank_tool_stream.py"),
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
                include_str!("rank_request.py"),
                include_str!("rank_sampling.py"),
                include_str!("rank_budget.py"),
                include_str!("rank_prefill.py"),
                include_str!("rank_batch.py"),
                concat!(
                    include_str!("rank_state.py"),
                    include_str!("rank_prompt_search.py")
                ),
                include_str!("rank_boundary.py"),
                include_str!("rank_tool_schema.py"),
                include_str!("rank_tool_stream.py"),
                include_str!("rank_stream_watch.py"),
                include_str!("rank_xml_guard.py"),
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
        assert_eq!(json["program"], "mlxLmServerStableHead");
        assert_eq!(json["keep_stable_head"], true);
        assert_eq!(json["keep_conversation_prefix"], true);

        // A 3.0.69 requester's spec (the conversation-prefix tag, no keep_stable_head) cuts and
        // keeps no head here: its own ranks cut and keep none either.
        let mut conversation = json.clone();
        conversation["program"] = "mlxLmServerConversationPrefix".into();
        conversation
            .as_object_mut()
            .unwrap()
            .remove("keep_stable_head");
        let read: RankSpec = serde_json::from_value(conversation).unwrap();
        assert!(matches!(
            read.program,
            RankProgram::MlxLmServer {
                keep_stable_head: false,
                keep_conversation_prefix: true,
                ..
            }
        ));

        // A 3.0.69 peer's goosed (its enum knows the conversation-prefix tag, not this one) refuses
        // this spec: its ranks would chunk an agent request's prefill at other steps.
        #[derive(Debug, Deserialize)]
        #[serde(tag = "program", rename_all = "camelCase")]
        #[allow(dead_code)]
        enum ConversationPrefixProgram {
            #[serde(
                rename = "mlxLmServerConversationPrefix",
                alias = "mlxLmServerPrefillYield",
                alias = "mlxLmServerNewestPrefix",
                alias = "mlxLmServerRowProcessors",
                alias = "mlxLmServerSkeletonGuard",
                alias = "mlxLmServerTransientTail",
                alias = "mlxLmServerFormation",
                alias = "mlxLmServerPrefill",
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
        let refused =
            serde_json::from_value::<ConversationPrefixProgram>(json.clone()).unwrap_err();
        let why = link_control_refusal(&refused.to_string());
        assert!(why.is_some_and(|w| w.contains("update goose")), "{refused}");
        assert_eq!(json["prefill_step_yields"], true);
        assert_eq!(json["keep_newest_prefix"], true);
        assert_eq!(json["row_processors"], true);
        assert_eq!(json["xml_skeleton_guard"], true);
        assert_eq!(json["transient_tail_boundary"], true);
        assert_eq!(json["formation"]["rounds"], FORMATION_ROUNDS);

        // A 3.0.66 requester's spec (the prefill-yield tag, no keep_conversation_prefix) keeps the
        // newest "user" entry here: its own ranks keep it too.
        let mut yields = json.clone();
        yields["program"] = "mlxLmServerPrefillYield".into();
        yields
            .as_object_mut()
            .unwrap()
            .remove("keep_conversation_prefix");
        let read: RankSpec = serde_json::from_value(yields).unwrap();
        assert!(matches!(
            read.program,
            RankProgram::MlxLmServer {
                keep_conversation_prefix: false,
                keep_newest_prefix: true,
                prefill_step_yields: true,
                ..
            }
        ));

        // A 3.0.66 peer's goosed (its enum knows the prefill-yield tag, not this one) refuses this
        // spec: its cache would keep a helper's segment where this Mac's keeps the conversation's.
        #[derive(Debug, Deserialize)]
        #[serde(tag = "program", rename_all = "camelCase")]
        #[allow(dead_code)]
        enum PrefillYieldProgram {
            #[serde(
                rename = "mlxLmServerPrefillYield",
                alias = "mlxLmServerNewestPrefix",
                alias = "mlxLmServerRowProcessors",
                alias = "mlxLmServerSkeletonGuard",
                alias = "mlxLmServerTransientTail",
                alias = "mlxLmServerFormation",
                alias = "mlxLmServerPrefill",
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
        let refused = serde_json::from_value::<PrefillYieldProgram>(json.clone()).unwrap_err();
        let why = link_control_refusal(&refused.to_string());
        assert!(why.is_some_and(|w| w.contains("update goose")), "{refused}");

        // A 3.0.60 requester's spec (the newest-prefix tag, no prefill_step_yields) runs mlx_lm's
        // own step loop here: its own ranks run it too.
        let mut newest = json.clone();
        newest["program"] = "mlxLmServerNewestPrefix".into();
        newest
            .as_object_mut()
            .unwrap()
            .remove("prefill_step_yields");
        let read: RankSpec = serde_json::from_value(newest).unwrap();
        assert!(matches!(
            read.program,
            RankProgram::MlxLmServer {
                prefill_step_yields: false,
                keep_newest_prefix: true,
                ..
            }
        ));

        // A 3.0.60 peer's goosed (its enum knows the newest-prefix tag, not this one) refuses this
        // spec: its rank would run on in the model's collectives where this Mac's ends the loop.
        #[derive(Debug, Deserialize)]
        #[serde(tag = "program", rename_all = "camelCase")]
        #[allow(dead_code)]
        enum NewestPrefixProgram {
            #[serde(
                rename = "mlxLmServerNewestPrefix",
                alias = "mlxLmServerRowProcessors",
                alias = "mlxLmServerSkeletonGuard",
                alias = "mlxLmServerTransientTail",
                alias = "mlxLmServerFormation",
                alias = "mlxLmServerPrefill",
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
        let refused = serde_json::from_value::<NewestPrefixProgram>(json.clone()).unwrap_err();
        let why = link_control_refusal(&refused.to_string());
        assert!(why.is_some_and(|w| w.contains("update goose")), "{refused}");

        // A 3.0.59 requester's spec (the row-processors tag, no keep_newest_prefix) evicts by
        // mlx_lm's type counts here: its own ranks evict that way too.
        let mut rows = json.clone();
        rows["program"] = "mlxLmServerRowProcessors".into();
        rows.as_object_mut().unwrap().remove("keep_newest_prefix");
        let read: RankSpec = serde_json::from_value(rows).unwrap();
        assert!(matches!(
            read.program,
            RankProgram::MlxLmServer {
                keep_newest_prefix: false,
                row_processors: true,
                ..
            }
        ));

        // A 3.0.59 peer's goosed (its enum knows the row-processors tag, not this one) refuses this
        // spec: its cache would evict the prefix this Mac's cache keeps.
        #[derive(Debug, Deserialize)]
        #[serde(tag = "program", rename_all = "camelCase")]
        #[allow(dead_code)]
        enum RowProcessorsProgram {
            #[serde(
                rename = "mlxLmServerRowProcessors",
                alias = "mlxLmServerSkeletonGuard",
                alias = "mlxLmServerTransientTail",
                alias = "mlxLmServerFormation",
                alias = "mlxLmServerPrefill",
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
        let refused = serde_json::from_value::<RowProcessorsProgram>(json.clone()).unwrap_err();
        let why = link_control_refusal(&refused.to_string());
        assert!(why.is_some_and(|w| w.contains("update goose")), "{refused}");

        // A 3.0.57 requester's spec (the skeleton-guard tag, no row_processors) keeps upstream's
        // processor lists here: its own ranks keep them too.
        let mut guard = json.clone();
        guard["program"] = "mlxLmServerSkeletonGuard".into();
        guard.as_object_mut().unwrap().remove("row_processors");
        let read: RankSpec = serde_json::from_value(guard).unwrap();
        assert!(matches!(
            read.program,
            RankProgram::MlxLmServer {
                row_processors: false,
                xml_skeleton_guard: true,
                ..
            }
        ));

        // A 3.0.57 peer's goosed (its enum knows the skeleton-guard tag, not this one) refuses this
        // spec: its wrapper would run a departed row's processors where this Mac runs the row's own.
        #[derive(Debug, Deserialize)]
        #[serde(tag = "program", rename_all = "camelCase")]
        #[allow(dead_code)]
        enum SkeletonGuardProgram {
            #[serde(
                rename = "mlxLmServerSkeletonGuard",
                alias = "mlxLmServerTransientTail",
                alias = "mlxLmServerFormation",
                alias = "mlxLmServerPrefill",
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
        let refused = serde_json::from_value::<SkeletonGuardProgram>(json.clone()).unwrap_err();
        let why = link_control_refusal(&refused.to_string());
        assert!(why.is_some_and(|w| w.contains("update goose")), "{refused}");

        // A Q-142 requester's spec (the transient-tail tag, no guard) masks nothing here: its own
        // ranks sample from the unmasked logits too.
        let mut tail = json.clone();
        tail["program"] = "mlxLmServerTransientTail".into();
        tail.as_object_mut().unwrap().remove("xml_skeleton_guard");
        let read: RankSpec = serde_json::from_value(tail).unwrap();
        assert!(matches!(
            read.program,
            RankProgram::MlxLmServer {
                xml_skeleton_guard: false,
                transient_tail_boundary: true,
                ..
            }
        ));

        // A Q-142 peer's goosed (its enum knows the transient-tail tag, not the guard one) refuses
        // this spec: its wrapper would sample a tool call's tokens from logits this Mac masks.
        #[derive(Debug, Deserialize)]
        #[serde(tag = "program", rename_all = "camelCase")]
        #[allow(dead_code)]
        enum TransientTailProgram {
            #[serde(
                rename = "mlxLmServerTransientTail",
                alias = "mlxLmServerFormation",
                alias = "mlxLmServerPrefill",
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
        let refused = serde_json::from_value::<TransientTailProgram>(json.clone()).unwrap_err();
        let why = link_control_refusal(&refused.to_string());
        assert!(why.is_some_and(|w| w.contains("update goose")), "{refused}");

        // A Q-136 requester's spec (the formation tag, no boundary) cuts no prompt here: its own
        // rank 0 never declares the tail, so goose never sends one to that launch.
        let mut formation = json.clone();
        formation["program"] = "mlxLmServerFormation".into();
        formation
            .as_object_mut()
            .unwrap()
            .remove("transient_tail_boundary");
        let read: RankSpec = serde_json::from_value(formation).unwrap();
        assert!(read.formation.is_some());
        assert!(matches!(
            read.program,
            RankProgram::MlxLmServer {
                transient_tail_boundary: false,
                prefill: Some(_),
                ..
            }
        ));

        // A Q-136 peer's goosed (its enum knows the formation tag, not the boundary one) refuses
        // this spec: its wrapper would prefill a tailed request in a different number of chunks
        // than its peers.
        #[derive(Debug, Deserialize)]
        #[serde(tag = "program", rename_all = "camelCase")]
        #[allow(dead_code)]
        enum FormationProgram {
            #[serde(
                rename = "mlxLmServerFormation",
                alias = "mlxLmServerPrefill",
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
        let refused = serde_json::from_value::<FormationProgram>(json.clone()).unwrap_err();
        let why = link_control_refusal(&refused.to_string());
        assert!(why.is_some_and(|w| w.contains("update goose")), "{refused}");

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
             \x20   step = 256\n\
             class PromptTrieResult: pass\n\
             class PromptTrie:\n\
             \x20   def __init__(self): self._trie = {}\n\
             \x20   def search(self, model, tokens): pass\n\
             def can_trim_prompt_cache(cache): return False\n\
             def trim_prompt_cache(cache, num_tokens): return []\n",
        )
        .unwrap();
        std::fs::create_dir_all(site.join("mlx_lm/tool_parsers")).unwrap();
        std::fs::write(site.join("mlx_lm/tool_parsers/__init__.py"), "").unwrap();
        std::fs::write(
            site.join("mlx_lm/tool_parsers/qwen3_coder.py"),
            "def parse_tool_call(model_output, tools=None): pass\n\
             def _parse_xml_function_call(function_call_str, tools): pass\n\
             def _convert_param_value(param_value, param_name, param_config): pass\n\
             def _get_arguments_config(func_name, tools): pass\n",
        )
        .unwrap();
        std::fs::write(
            site.join("mlx_lm/generate.py"),
            "class PromptProcessingBatch:\n\
             \x20   def prompt(self, tokens): pass\n\
             \x20   def split(self, indices): pass\n\
             \x20   def filter(self, keep): pass\n\
             class GenerationBatch:\n\
             \x20   def __init__(self, model, uids, inputs, prompt_cache, tokens, samplers,\n\
             \x20                fallback_sampler, logits_processors, state_machines, max_tokens): pass\n\
             \x20   def filter(self, keep): pass\n",
        )
        .unwrap();
        std::fs::write(
            site.join("mlx_lm/server.py"),
            "import argparse, json, os, sys\n\
             class LRUPromptCache:\n\
             \x20   class CacheOrder:\n\
             \x20       def __init__(self): self._lrus = {'assistant': [], 'user': [], 'system': []}\n\
             \x20       def pop(self): pass\n\
             \x20   def __init__(self, max_size=10, max_bytes=1 << 63):\n\
             \x20       self.max_size, self.max_bytes, self.trims = max_size, max_bytes, []\n\
             \x20       from mlx_lm.models.cache import PromptTrie\n\
             \x20       self._trie = PromptTrie()\n\
             \x20       self._lru = LRUPromptCache.CacheOrder()\n\
             \x20   def insert_cache(self, model, tokens, prompt_cache, *, cache_type='assistant'): pass\n\
             \x20   def fetch_nearest_cache(self, model, tokens): pass\n\
             \x20   def trim_to(self, *, n_sequences=None, n_bytes=None): self.trims.append(n_bytes)\n\
             class _Rows(list):\n\
             \x20   prompt_cache = []\n\
             class BatchGenerator:\n\
             \x20   prompt_cache_nbytes = 7\n\
             \x20   def __init__(self):\n\
             \x20       self._generation_batch, self._prompt_batch = _Rows(), _Rows()\n\
             \x20       self._unprocessed_sequences, self._currently_processing = [], []\n\
             \x20       self._prompt_tokens_counter = 0\n\
             \x20   def close(self): pass\n\
             \x20   def remove(self, uids): pass\n\
             \x20   def insert_segments(self, segments, *a, **k): return []\n\
             \x20   def next(self): return [], []\n\
             class GenerationContext:\n\
             \x20   def stop(self): pass\n\
             class TimeBudget: pass\n\
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
             \x20   def handle_chat_completions(self): pass\n\
             \x20   def generate_response(self, text, finish_reason, **kwargs): pass\n\
             def process_message_content(messages): pass\n\
             class ToolCallFormatter:\n\
             \x20   def __init__(self, tool_parser, tools, streaming=False): pass\n\
             def _make_logits_processors(args): return []\n\
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

    /// Until both drains reach EOF: every line the rank printed is in the log and the live view.
    /// Awaited after the rank exited — its streams close with it (Q-245: this was 600 × 100 ms).
    async fn drained(drains: Vec<tokio::task::JoinHandle<()>>) {
        for drain in drains {
            drain.await.expect("a drain task ended by panic");
        }
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
        let drains = drain_rank_output(&mut child, 1, "studio", &live);
        assert!(child.wait().await.unwrap().success());
        drained(drains).await;
        let live = live.lock().unwrap();
        assert_eq!(live.lines, 4, "{}", live.tail_text());
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
            "{}{prelude}{}{steps}",
            tensor_modules(),
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
        let mut drains = drain_rank_output(&mut child, 1, "Work’s Mac Studio", &live);
        // Until the worker parks at the doorbell, or until it fails — a Traceback in its output or
        // its end, either failing with its tail.
        let parked = loop {
            let (state, tail) = {
                let live = live.lock().unwrap();
                (live.state.clone(), live.tail_text())
            };
            if let Some(state) = state.filter(|state| state["at"] == "doorbell") {
                break state;
            }
            assert!(!tail.contains("Traceback"), "{tail}");
            if let Some(status) = child.try_wait().unwrap() {
                drained(std::mem::take(&mut drains)).await;
                panic!(
                    "the worker ended ({status}) before it parked:\n{}",
                    live.lock().unwrap().tail_text()
                );
            }
            tokio::time::sleep(crate::distributed::supervisor::READY_TICK).await;
        };
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
        drained(drains).await;
        let path = live.lock().unwrap().log.clone().unwrap();
        let log = std::fs::read_to_string(&path).unwrap();
        assert!(log.contains("in _next_request"), "{log}");
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
