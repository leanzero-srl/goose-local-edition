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
//! - [`probe`]: parsers for what the nodes answer (vm_stat, ps, ifconfig, ibv_devinfo, …).
//! - [`plan`]: the per-rank memory arithmetic (tensor split computed here; the qwen4_exp
//!   pipeline split read from the fork's own planner).
//! - [`preflight`]: every check, per node, with its numbers; the documented TB link repair.
//! - [`provision`]: the ranks' Python, a goose-owned uv venv per node (pinned mlx + mlx_lm).
//! - [`launch`]: the rank processes (goose's own launcher — see its doc for why not mlx.launch)
//!   and the embedded rank wrapper (in-process memory caps, admission, progress counter).
//! - [`supervisor`]: readiness, liveness (the soak's hang rule), the memory watchdog, the
//!   verified stop sequence and the restart policy.

pub mod config;
pub mod exec;
pub mod launch;
pub mod plan;
pub mod preflight;
pub mod probe;
pub mod provision;
pub mod supervisor;

pub use config::{Backend, DistributedConfig, NodeConfig, Runner};
pub use exec::{ExecOutput, NodeExec, SystemExec};
pub use plan::RankPlan;
pub use preflight::{Check, CheckVerdict, NodePreflight, PreflightReport};
pub use supervisor::{
    global_manager, DistributedManager, DistributedStatus, EngineEvent, EventKind, NodeState,
    NodeStatus, RefusalCode, RunState, StartOutcome, StopReport,
};

// ratio: the MTPLX run measured a stable ceiling at 75% of RAM for MLX allocations on 96-128 GB
// Apple silicon (mlx-jaccl-cluster skill, guardrail 3); the fork's pipeline guard uses the same.
pub const MEMORY_LIMIT_RATIO: f64 = 0.75;
// ratio: the same MTPLX receipt measured 60% of RAM as the safe wired ceiling; exo's unbounded
// wiring is what kernel-panicked the 96 GB M3 Ultra.
pub const WIRED_LIMIT_RATIO: f64 = 0.60;
// ratio: policy, not yet measured (parity with the fork's AVAILABLE_HEADROOM_RATIO) — 10% of the
// memory the kernel reports available stays free for the OS and for transients.
pub const AVAILABLE_HEADROOM_RATIO: f64 = 0.90;
// measured: peak MLX memory over planned bytes on the three recorded splits — Flash pipeline
// MacBook 61.0 / 57.81 GiB = 1.055, workhorse 42.5 / 38.92 GiB = 1.092 (2026-09-24), 27B tensor
// bench peak 19.9 GB (18.53 GiB) / 17.07 GiB planned at 2,304 tokens = 1.086 (STEP1b). The planned slice is multiplied by
// this before it is compared with a node's budget.
pub const RUNTIME_OVERHEAD_RATIO: f64 = 1.10;
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
