//! The DISTRIBUTED MLX engine: one model split across several Macs (this Mac as rank 0 and
//! coordinator, peers reached over ssh aliases), serving the same OpenAI API as the single
//! engine on its own port.
//!
//! Isolation (the owner's rule: "all that we add on top needs to be done in isolation so that
//! it doesn't affect our other working stuff"): nothing in the single-node engine
//! (`engine.rs`, `lib.rs`'s `Sidecar`) calls into this module. It READS the single manager's
//! state only to refuse a start while that engine owns this Mac, and it borrows three
//! crate-private helpers (the fixed spawn PATH, the macOS page arithmetic, the port-release
//! grace) so every figure it reports is computed by the SAME code the single engine uses.
//!
//! Shape:
//! - [`config`]: what the operator configures (nodes, backend, per-node model dir + python).
//! - [`exec`]: running a shell script on a node (`/bin/sh` here, `ssh <alias>` there).
//! - [`node_op`], [`link_control`], [`link_host`]: LeanZero Link as the control plane — a node
//!   named `link:<node id>` is driven through its OWN goosed over the mesh (typed ops only, its
//!   rank spawned and stopped there, under a lease); the data plane stays JACCL/ring over TB.
//! - [`probe`]: parsers for what the nodes answer (vm_stat, ps, ifconfig, ibv_devinfo, …).
//! - [`plan`]: the per-rank memory arithmetic (tensor split computed here; the qwen4_exp
//!   pipeline split, bytes, budget and verdict read from the fork's own planner, `plan --json`).
//! - [`preflight`]: every check, per node, with its numbers; the documented TB link repair.
//! - [`local_network`]: macOS local network privacy, named from the ping signature and the
//!   peer's positive control.
//! - [`provision`]: the ranks' Python, a goose-owned uv venv per node (pinned mlx + mlx_lm; the
//!   fork pinned by commit for the pipeline runner).
//! - [`launch`]: the rank processes (goose's own launcher — see its doc for why not mlx.launch)
//!   and the embedded rank programs: `mlx_lm.server` under the tensor wrapper (in-process memory
//!   caps, admission, progress counter), or the fork's `pipeline_qwen4_serve` (which carries the
//!   same HTTP surface itself).
//! - [`supervisor`]: readiness, liveness (the soak's hang rule), the memory watchdog, the
//!   verified stop sequence and the restart policy.

pub mod compaction;
pub mod config;
pub mod exec;
pub mod launch;
pub mod link_control;
pub mod link_host;
pub mod local_network;
pub mod node_op;
pub mod plan;
pub mod preflight;
pub mod probe;
pub mod provision;
pub mod supervisor;

pub use compaction::{CompactionOutcome, CompactionRefusal, CompactionReport};
pub use config::{Backend, DistributedConfig, NodeConfig, Runner};
pub use exec::{ExecOutput, NodeExec, SystemExec};
pub use plan::RankPlan;
pub use preflight::{Check, CheckVerdict, NodePreflight, PreflightReport};
pub use supervisor::{
    global_manager, DistributedManager, DistributedStatus, EngineEvent, EventKind, NodeState,
    NodeStatus, RefusalCode, RunState, StartOutcome, StopReport,
};

// measured: the share of each node's RAM that stays available under a rank's full budget —
// budget = min(available − RAM × this, the node's GPU ceiling (Metal's
// max_recommended_working_set_size)) — is the highest kernel-WARN point measured plus the load
// drift. 2026-09-24, goose's own compaction (memory_pressure to WARN) read the kernel's WARN point
// at 9.3 GiB available on the M4 Max 128 GB (7.3% of RAM) and 3.3–4.0 GiB on the M3 Ultra 96 GB;
// + DERIVED_CONTEXT_MARGIN_RATIO (0.02, the ranks' re-measure drift) = 0.093. The first loosening
// (0.07 = watchdog WARN 0.05 + 0.02, from the M3 Ultra alone) sat BELOW the M4 Max's WARN point: a
// live Flash split with rank 0 at 96% of that budget served under kernel WARN (7.1 GiB available)
// and the watchdog closed admission — backed off to this. It replaced the fork's 21% floor and the
// tensor runner's min(available × 0.90, RAM × 0.75) (44.9 / 52.6 GiB budgets on 128 / 96 GB:
// "way too conservative"). The fork carries the same value (pipeline_qwen4.py, echoed in its plan
// JSON); both runners and the placement planner share this one rule.
pub const AVAILABLE_MARGIN_RATIO: f64 = 0.093;
// measured: TENSOR RUNNER ONLY — 27B tensor bench peak 19.9 GB (18.53 GiB) / 17.07 GiB planned at
// 2,304 tokens = 1.086 (STEP1b); the planned slice is multiplied by this before it is compared
// with a node's budget. The pipeline runner applies NO multiplier: the Flash soak's peaks were
// 1.055 / 1.092 × the fork's OLD plan (0.36 GiB modeled workspace), but the fork's current plan
// for the same shape (split 20, context 8,192, batch 2 — the soak ran 130 two-request batches;
// `plan --json`, 272cb0643) carries MacBook 61.50 / workhorse 42.66 GiB against measured peaks of
// 61.0 / 42.5 — 0.992 / 0.996 — so multiplying again would double count.
pub const RUNTIME_OVERHEAD_RATIO: f64 = 1.10;
// measured: the fork's `serve` proves 2 rows bit-exact against single-process batches
// (pipeline_qwen4_serve.py). The default slot count (`DistributedConfig::slots`): the split is
// planned and KV-budgeted for this many full-context sequences, and one batch carries at most as
// many rows.
pub const PIPELINE_DEFAULT_SLOTS: u32 = 2;
// measured: a DERIVED pipeline context is planned against every node's available memory minus
// this share of its RAM. The derivation's own ceiling sits at 100% of budget, and the ranks
// re-check against LIVE memory at load: on 2026-09-24 (Flash, 16 s after a passing preflight) the
// rank-0 budget read 62.55 GiB against preflight's 63.30 (−0.75 GiB = 0.6% of 128 GiB RAM) and
// rank 1's 48.74 against 49.21 (−0.47 GiB = 0.5% of 96), so both ranks refused at 101%. 2% of RAM
// is 3.4x the larger drift; a REQUESTED context is planned as asked, without it.
pub const DERIVED_CONTEXT_MARGIN_RATIO: f64 = 0.02;
// ratio: the soak's hang rule (STEP1b REPORT: "no progress for 10x the running median of that
// measure"); the worst healthy ratio observed across 326 requests was 2.73x.
pub const HANG_MEDIAN_MULTIPLE: f64 = 10.0;
// ratio: the soak's rule held its verdict until 3 samples of the measure existed.
pub const HANG_MIN_SAMPLES: usize = 3;
// ratio: policy — the prompt cache may hold one full allowed context's worth of KV per rank on
// top of the live request's, so the cache never outgrows what preflight charged it.
pub const PROMPT_CACHE_CONTEXTS: u64 = 1;
// ratio: policy, not yet measured — a node whose available memory falls below 5% of its RAM is
// treated as WARN even before the kernel's own pressure level says so. Overridable per run
// (`DistributedConfig::watchdog_warn_ratio`).
pub const WATCHDOG_WARN_RESERVE_RATIO: f64 = 0.05;
// ratio: policy, not yet measured — below 2% of RAM available the run is stopped as CRITICAL even
// if the kernel has not raised its own level yet (the kernel's CRITICAL always stops it too).
// Overridable per run (`DistributedConfig::watchdog_critical_ratio`).
pub const WATCHDOG_CRITICAL_RESERVE_RATIO: f64 = 0.02;
