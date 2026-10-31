//! What the operator configures for the distributed engine. Nothing here is defaulted from a
//! guess: every path, address and name is supplied, and `validate` refuses a config that cannot
//! describe a real launch (rank 0 must be this Mac, every peer needs an ssh alias, …).

use std::path::Path;

use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Backend {
    /// RDMA over Thunderbolt 5 (needs the IPv4-mapped GID on the TB interface).
    Jaccl,
    /// TCP over the Thunderbolt /30.
    Ring,
}

impl Backend {
    pub fn as_str(self) -> &'static str {
        match self {
            Backend::Jaccl => "jaccl",
            Backend::Ring => "ring",
        }
    }
}

/// Which program serves the split, chosen by the checkpoint's `model_type` — never by the
/// operator, so a model is always run the way its architecture can be split.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Runner {
    /// `mlx_lm.server` on every rank, tensor-parallel (`qwen3_5.py` implements `shard()`).
    MlxLmTensor,
    /// The fork's `rapid_mlx.distributed.pipeline_qwen4` (contiguous layer ranges per rank).
    PipelineQwen4,
    /// `mlx_lm.server --pipeline` on every rank under goose's rank wrapper, each rank holding the
    /// contiguous layer range goose's planner derived from the checkpoint's per-layer bytes
    /// (`plan::plan_layer_split`; rank 0 = the LAST layers). The fork's env carries the model
    /// code: `import rapid_mlx.mlx_lm_models` makes `mlx_lm.models.<model_type>` resolvable.
    MlxLmPipeline,
}

/// THE one table of which checkpoint `model_type` splits under which runner. Every other place
/// asks [`Runner`] (`for_model_type`, `placement_kind`, `layout_words`, `uses_pipeline_env`); a
/// test (`no_other_file_maps_a_model_type_to_a_runner`) fails the build on a second copy.
pub const RUNNERS: &[(&str, Runner)] = &[
    ("qwen3_5", Runner::MlxLmTensor),
    ("qwen4_exp", Runner::PipelineQwen4),
    ("mimo_v2", Runner::MlxLmPipeline),
];

/// Per-model plan reserves, used when the config sets no `plan_reserve_ratio`: a model whose
/// split measured fine inside the shared reserve plans at its own thinner one instead of being
/// refused. Floored at the watchdog WARN reserve in force (`DistributedConfig::plan_reserve`).
/// The shared default (`AVAILABLE_MARGIN_RATIO`) is never lowered for every model.
pub const MODEL_PLAN_RESERVES: &[(&str, f64)] = &[
    // ratio: of each node's RAM; mimo_v2's 19/29 run served at 68.7 / 100.7 GB footprints with a ~1 GB swap peak and no watchdog trip (2026-10-26), the owner budgets ~80 GB workhorse / ~110 GB mule; the shared 9.3% refuses it, 7.7% is the highest that fits.
    ("mimo_v2", 0.05),
];

/// Where the plan reserve in force came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ReserveSource {
    /// The shared `AVAILABLE_MARGIN_RATIO` every runner plans with.
    Default,
    /// The model's own entry in [`MODEL_PLAN_RESERVES`].
    Model,
    /// The config's `plan_reserve_ratio`.
    User,
}

impl ReserveSource {
    pub fn as_str(self) -> &'static str {
        match self {
            ReserveSource::Default => "default",
            ReserveSource::Model => "model",
            ReserveSource::User => "user",
        }
    }
}

impl Runner {
    pub fn for_model_type(model_type: &str) -> Result<Self> {
        match RUNNERS.iter().find(|(t, _)| *t == model_type) {
            Some((_, runner)) => Ok(*runner),
            None => bail!(
                "no distributed runner for model_type '{model_type}': {}; nothing else is wired",
                Self::wired_summary()
            ),
        }
    }

    /// Every wired model_type with how it splits, from [`RUNNERS`]
    /// ("qwen3_5 tensor-parallel, qwen4_exp by layer range (fork pipeline), …").
    pub fn wired_summary() -> String {
        RUNNERS
            .iter()
            .map(|(t, r)| format!("{t} {}", r.layout_words()))
            .collect::<Vec<_>>()
            .join(", ")
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Runner::MlxLmTensor => "mlxLmTensor",
            Runner::PipelineQwen4 => "pipelineQwen4",
            Runner::MlxLmPipeline => "mlxLmPipeline",
        }
    }

    /// How the placement store and planner key this runner's splits.
    pub fn placement_kind(self) -> crate::placement::store::PlacementKind {
        match self {
            Runner::MlxLmTensor => crate::placement::store::PlacementKind::Tensor,
            Runner::PipelineQwen4 | Runner::MlxLmPipeline => {
                crate::placement::store::PlacementKind::Pipeline
            }
        }
    }

    /// How the split divides the model, in words.
    pub fn layout_words(self) -> &'static str {
        match self {
            Runner::MlxLmTensor => "tensor-parallel",
            Runner::PipelineQwen4 => "by layer range (the fork's pipeline_qwen4)",
            Runner::MlxLmPipeline => "by layer range (mlx_lm pipeline, goose's split)",
        }
    }

    /// The ranks run the fork's interpreter (`NodeConfig::pipeline_python`): the qwen4_exp
    /// pipeline is the fork's program; the mlx_lm pipeline imports the fork's vendored model code.
    pub fn uses_pipeline_env(self) -> bool {
        match self {
            Runner::MlxLmTensor => false,
            Runner::PipelineQwen4 | Runner::MlxLmPipeline => true,
        }
    }

    /// Each rank runs `mlx_lm.server` under goose's rank wrapper (`TENSOR_PROGRAM`): the wrapper's
    /// serving loop, its `/v1/status` shape and its phases, whichever way the layers are split.
    pub fn runs_rank_wrapper(self) -> bool {
        match self {
            Runner::MlxLmTensor | Runner::MlxLmPipeline => true,
            Runner::PipelineQwen4 => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NodeConfig {
    /// Display name ("MacBook Pro", "workhorse").
    pub name: String,
    /// The ssh alias for a peer (`workhorse`); `None` exactly for this Mac, which is rank 0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssh: Option<String>,
    /// This node's IPv4 on the Thunderbolt /30 (192.168.0.1 / 192.168.0.2).
    pub tb_ip: String,
    /// The /30's mask, re-applied by the link repair (255.255.255.252).
    pub tb_netmask: String,
    /// The BSD interface carrying the TB link (en3).
    pub tb_interface: String,
    /// The macOS network service on that interface ("EXO Thunderbolt 3"). The link repair
    /// toggles THIS service only, and only after proving its hardware port is a Thunderbolt one.
    pub tb_service: String,
    /// The RDMA device JACCL uses on this node (rdma_en3). Empty under ring, which uses none.
    #[serde(default)]
    pub rdma_device: String,
    /// The interpreter carrying mlx + mlx_lm on this node.
    pub python: String,
    /// The interpreter carrying the fork (`rapid_mlx`) — only the qwen4_exp pipeline runner uses it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pipeline_python: Option<String>,
    /// The model directory ON THIS NODE (paths differ per node; each rank loads its own).
    pub model_dir: String,
    /// "Free memory automatically": when a preflight finds this node short, compact it (ask macOS
    /// to reclaim — `compaction`) and preflight again. ON unless the owner turned it off, so a
    /// config saved before the switch existed reads as ON.
    #[serde(default = "free_memory_default")]
    pub free_memory_automatically: bool,
}

fn free_memory_default() -> bool {
    true
}

impl NodeConfig {
    pub fn is_local(&self) -> bool {
        self.ssh.is_none()
    }

    /// The host an exec targets: `None` = this Mac.
    pub fn host(&self) -> Option<&str> {
        self.ssh.as_deref()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DistributedConfig {
    /// The model every rank loads (its HF id; each node's `model_dir` holds it). The id served on
    /// `/v1/models` is `engine::served_model_id(settings, model_id)` — this model's own HF id,
    /// unless it is the model the single engine's alias names — the one rule both engines serve
    /// under.
    pub model_id: String,
    pub backend: Backend,
    /// The OpenAI API port on this Mac (loopback). Never the single engine's port.
    pub port: u16,
    /// JACCL's coordinator port on rank 0's TB IP; ring uses it + rank on each node.
    pub coordinator_port: u16,
    /// The context to allow. `None` = the largest context every rank fits (capped at the model's
    /// `max_position_embeddings`), reported as derived.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<u64>,
    /// Pipeline runner only: the full-context sequences the split is planned and KV-budgeted for
    /// (the fork planner's `--batch`, the server's `--slots`). It sizes the budget, not the rows:
    /// the server admits each request by what it needs of that budget, so short requests run
    /// beside `slots` full-context ones (Q-160). `None` = `PIPELINE_DEFAULT_SLOTS`. The tensor
    /// runner (`mlx_lm.server`) has no slots.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slots: Option<u32>,
    /// Restart after a rank death or a hang (the breaker still applies). A watchdog CRITICAL
    /// stop never restarts.
    #[serde(default)]
    pub restart_on_failure: bool,
    /// Diagnostic: skip the `ps` stat-T fast path, so a stopped rank is caught only by the
    /// progress-ratio hang rule (how that rule is proven live on a real freeze).
    #[serde(default)]
    pub hang_ratio_only: bool,
    /// The watchdog's WARN reserve as a fraction of each node's RAM (available below it → stop
    /// admitting). `None` = `WATCHDOG_WARN_RESERVE_RATIO`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub watchdog_warn_ratio: Option<f64>,
    /// The watchdog's CRITICAL reserve (available below it → verified stop, never restarted).
    /// `None` = `WATCHDOG_CRITICAL_RESERVE_RATIO`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub watchdog_critical_ratio: Option<f64>,
    /// The plan's reserve as a fraction of each node's RAM: budget = min(available − RAM × this,
    /// GPU ceiling). `None` = the shared `AVAILABLE_MARGIN_RATIO` every runner plans with. An
    /// explicit per-config override for a split that measured fine inside the shared reserve
    /// (mimo_v2, 2026-10-26: the manual 19/29 run served at 68.7 / 100.7 GB footprints, swap
    /// peak ~1 GB, no watchdog trip, where the shared reserve leaves 67.11 + 98.22 GB for 168.6 GB
    /// of weights). Validated: at least the WARN reserve in force (below it the watchdog closes
    /// admission on a plan that fits) and below the shared default (it only ever loosens).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_reserve_ratio: Option<f64>,
    /// GB the layer split's plan leaves free under each Mac's GPU ceiling: the ceiling the budget
    /// is capped at is the measured one less this. `None` = the measured ceiling. Not an owner
    /// rule (corrected 2026-10-28): the 8 GB used in the 2026-10-27 MiMo runs was a test margin,
    /// and stacked on the reserve (`plan_reserve_ratio` / the shared one) it counted the same
    /// safety twice; the 256k MiMo split runs with 0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu_headroom_gb: Option<f64>,
    /// Per-node `gpu_headroom_gb` by node name, over the config-wide one. Why (2026-10-27): the
    /// workhorse is also the NFS server of the model and a desktop; at 8 GB under its ceiling
    /// rank 0's 71 GB plan (19 layers) still pushed it to swap +6 GB and memory pressure WARN
    /// during its load, twice, where 18 layers (65.8 GB) loaded clean.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_gpu_headroom_gb: Option<std::collections::BTreeMap<String, f64>>,
    /// Rank order: `nodes[0]` is this Mac.
    pub nodes: Vec<NodeConfig>,
}

impl DistributedConfig {
    /// A node's GPU ceiling less its headroom: `node_gpu_headroom_gb[node]`, else
    /// `gpu_headroom_gb`, else none.
    pub fn planned_ceiling(&self, node: &str, ceiling: u64) -> u64 {
        let headroom = self
            .node_gpu_headroom_gb
            .as_ref()
            .and_then(|by_node| by_node.get(node).copied())
            .or(self.gpu_headroom_gb)
            .unwrap_or(0.0)
            .max(0.0)
            * 1e9;
        ceiling.saturating_sub(headroom as u64)
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(!self.model_id.trim().is_empty(), "model_id is empty");
        ensure!(
            self.nodes.len() >= 2,
            "a distributed engine needs at least 2 nodes, {} configured",
            self.nodes.len()
        );
        ensure!(
            self.nodes[0].is_local(),
            "rank 0 must be this Mac (node '{}' carries ssh alias '{}')",
            self.nodes[0].name,
            self.nodes[0].ssh.as_deref().unwrap_or_default()
        );
        for (rank, node) in self.nodes.iter().enumerate().skip(1) {
            ensure!(
                !node.is_local(),
                "node '{}' (rank {rank}) has no ssh alias; only rank 0 may be this Mac",
                node.name
            );
        }
        ensure!(self.port != 0, "port 0 is not a serving port");
        ensure!(
            self.coordinator_port != 0,
            "coordinator_port 0 is not a port"
        );
        for node in &self.nodes {
            for (field, value) in [
                ("name", &node.name),
                ("tb_ip", &node.tb_ip),
                ("tb_netmask", &node.tb_netmask),
                ("tb_interface", &node.tb_interface),
                ("tb_service", &node.tb_service),
                ("python", &node.python),
                ("model_dir", &node.model_dir),
            ] {
                ensure!(
                    !value.trim().is_empty(),
                    "node '{}': {field} is empty",
                    node.name
                );
            }
            ensure!(
                self.backend != Backend::Jaccl || !node.rdma_device.trim().is_empty(),
                "node '{}': rdma_device is empty (JACCL needs the node's RDMA device)",
                node.name
            );
            node.tb_ip
                .parse::<std::net::Ipv4Addr>()
                .with_context(|| format!("node '{}': tb_ip '{}'", node.name, node.tb_ip))?;
            node.tb_netmask
                .parse::<std::net::Ipv4Addr>()
                .with_context(|| {
                    format!("node '{}': tb_netmask '{}'", node.name, node.tb_netmask)
                })?;
            ensure!(
                Path::new(&node.python).is_absolute() && Path::new(&node.model_dir).is_absolute(),
                "node '{}': python and model_dir must be absolute paths (a rank runs without a shell PATH)",
                node.name
            );
        }
        ensure!(self.slots() >= 1, "slots must be at least 1");
        let (warn, critical) = self.watchdog_ratios();
        ensure!(
            0.0 < critical && critical < warn && warn < 1.0,
            "watchdog ratios must satisfy 0 < critical ({critical}) < warn ({warn}) < 1"
        );
        if let Some(reserve) = self.plan_reserve_ratio {
            ensure!(
                reserve >= warn && reserve < super::AVAILABLE_MARGIN_RATIO,
                "plan_reserve_ratio {reserve} must be at least the watchdog WARN reserve ({warn}: \
                 below it the watchdog closes admission on a plan that fits) and below the shared \
                 reserve ({}); remove it to plan with the shared reserve",
                super::AVAILABLE_MARGIN_RATIO
            );
        }
        let mut names: Vec<&str> = self.nodes.iter().map(|n| n.name.as_str()).collect();
        names.sort_unstable();
        names.dedup();
        ensure!(names.len() == self.nodes.len(), "node names must be unique");
        Ok(())
    }

    /// (warn, critical) reserve ratios in force for this run.
    pub fn watchdog_ratios(&self) -> (f64, f64) {
        (
            self.watchdog_warn_ratio
                .unwrap_or(super::WATCHDOG_WARN_RESERVE_RATIO),
            self.watchdog_critical_ratio
                .unwrap_or(super::WATCHDOG_CRITICAL_RESERVE_RATIO),
        )
    }

    /// The plan reserve in force for a checkpoint of `model_type`, with where it came from: the
    /// config's override, else the model's entry in [`MODEL_PLAN_RESERVES`] (never below the WARN
    /// reserve in force), else the shared `AVAILABLE_MARGIN_RATIO`.
    pub fn plan_reserve(&self, model_type: Option<&str>) -> (f64, ReserveSource) {
        if let Some(reserve) = self.plan_reserve_ratio {
            return (reserve, ReserveSource::User);
        }
        let model = model_type
            .and_then(|t| MODEL_PLAN_RESERVES.iter().find(|(m, _)| *m == t))
            .map(|(_, r)| *r);
        match model {
            Some(reserve) => (
                reserve
                    .max(self.watchdog_ratios().0)
                    .min(super::AVAILABLE_MARGIN_RATIO),
                ReserveSource::Model,
            ),
            None => (super::AVAILABLE_MARGIN_RATIO, ReserveSource::Default),
        }
    }

    /// The slot count in force for a pipeline run.
    pub fn slots(&self) -> u32 {
        self.slots.unwrap_or(super::PIPELINE_DEFAULT_SLOTS)
    }

    pub fn size(&self) -> usize {
        self.nodes.len()
    }

    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn two_mac_config() -> DistributedConfig {
        DistributedConfig {
            model_id: "Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx".to_string(),
            backend: Backend::Jaccl,
            port: 8190,
            coordinator_port: 32323,
            context: None,
            slots: None,
            restart_on_failure: true,
            hang_ratio_only: false,
            watchdog_warn_ratio: None,
            watchdog_critical_ratio: None,
            plan_reserve_ratio: None,
            gpu_headroom_gb: None,
            node_gpu_headroom_gb: None,
            nodes: vec![
                NodeConfig {
                    name: "MacBook Pro".to_string(),
                    ssh: None,
                    tb_ip: "192.168.0.1".to_string(),
                    tb_netmask: "255.255.255.252".to_string(),
                    tb_interface: "en3".to_string(),
                    tb_service: "EXO Thunderbolt 3".to_string(),
                    rdma_device: "rdma_en3".to_string(),
                    python: "/tmp/jaccl-smoke/.venv/bin/python".to_string(),
                    pipeline_python: None,
                    model_dir:
                        "/Users/me/.goose/models/Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx"
                            .to_string(),
                    free_memory_automatically: true,
                },
                NodeConfig {
                    name: "workhorse".to_string(),
                    ssh: Some("workhorse".to_string()),
                    tb_ip: "192.168.0.2".to_string(),
                    tb_netmask: "255.255.255.252".to_string(),
                    tb_interface: "en3".to_string(),
                    tb_service: "EXO Thunderbolt 2".to_string(),
                    rdma_device: "rdma_en3".to_string(),
                    python: "/tmp/jaccl-smoke/.venv/bin/python".to_string(),
                    pipeline_python: None,
                    model_dir: "/Users/workhorse/jaccl-smoke/models/Qwen3.8-27B-Atlassian-Q8-mlx"
                        .to_string(),
                    free_memory_automatically: true,
                },
            ],
        }
    }

    #[test]
    fn the_recorded_two_mac_setup_validates() {
        two_mac_config().validate().unwrap();
    }

    #[test]
    fn rank_zero_must_be_this_mac_and_peers_need_an_alias() {
        let mut config = two_mac_config();
        config.nodes.swap(0, 1);
        let err = config.validate().unwrap_err().to_string();
        assert!(err.contains("rank 0 must be this Mac"), "{err}");

        let mut config = two_mac_config();
        config.nodes[1].ssh = None;
        let err = config.validate().unwrap_err().to_string();
        assert!(err.contains("only rank 0 may be this Mac"), "{err}");
    }

    #[test]
    fn relative_paths_and_bad_addresses_are_refused() {
        let mut config = two_mac_config();
        config.nodes[1].python = "python3".to_string();
        assert!(config.validate().is_err());
        let mut config = two_mac_config();
        config.nodes[1].tb_ip = "workhorse.lan".to_string();
        assert!(config.validate().is_err());
    }

    #[test]
    fn only_jaccl_needs_an_rdma_device() {
        let mut config = two_mac_config();
        config.nodes[1].rdma_device = String::new();
        let err = config.validate().unwrap_err().to_string();
        assert!(err.contains("rdma_device"), "{err}");
        config.backend = Backend::Ring;
        config.validate().unwrap();
    }

    #[test]
    fn watchdog_ratio_overrides_must_keep_critical_below_warn() {
        let mut config = two_mac_config();
        assert_eq!(config.watchdog_ratios(), (0.05, 0.02));
        config.watchdog_warn_ratio = Some(0.45);
        config.watchdog_critical_ratio = Some(0.35);
        config.validate().unwrap();
        config.watchdog_critical_ratio = Some(0.5);
        assert!(config.validate().is_err());
    }

    #[test]
    fn the_runner_follows_the_model_type_and_refuses_the_rest() {
        assert_eq!(
            Runner::for_model_type("qwen3_5").unwrap(),
            Runner::MlxLmTensor
        );
        assert_eq!(
            Runner::for_model_type("qwen4_exp").unwrap(),
            Runner::PipelineQwen4
        );
        assert_eq!(
            Runner::for_model_type("mimo_v2").unwrap(),
            Runner::MlxLmPipeline
        );
        let err = Runner::for_model_type("qwen3_moe").unwrap_err().to_string();
        assert!(err.contains("qwen3_moe"), "{err}");
        for (model_type, _) in RUNNERS {
            assert!(err.contains(model_type), "{err}");
        }
    }

    /// The runner rule lives in [`RUNNERS`] only: no other non-test source maps a model_type
    /// string to a runner, or compares a model_type with a literal. A second copy is how the
    /// arch list drifted into prose and placement code before.
    #[test]
    fn no_other_file_maps_a_model_type_to_a_runner() {
        let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let mut offenders = Vec::new();
        let mut stack = vec![crates.join("goose-sidecar/src"), crates.join("goose/src")];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                if path.extension().and_then(|e| e.to_str()) != Some("rs")
                    || path.ends_with("distributed/config.rs")
                {
                    continue;
                }
                let text = std::fs::read_to_string(&path).unwrap();
                // Production code only: everything before the file's test module.
                let code = text.split("#[cfg(test)]").next().unwrap_or_default();
                for (n, line) in code.lines().enumerate() {
                    let mapped = RUNNERS
                        .iter()
                        .any(|(t, _)| line.contains(&format!("\"{t}\" =>")));
                    if mapped || line.contains("model_type == \"") {
                        offenders.push(format!("{}:{}: {}", path.display(), n + 1, line.trim()));
                    }
                }
            }
        }
        assert!(offenders.is_empty(), "{offenders:#?}");
    }
}
