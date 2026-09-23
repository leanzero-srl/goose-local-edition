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
}

impl Runner {
    pub fn for_model_type(model_type: &str) -> Result<Self> {
        match model_type {
            "qwen3_5" => Ok(Runner::MlxLmTensor),
            "qwen4_exp" => Ok(Runner::PipelineQwen4),
            other => bail!(
                "no distributed runner for model_type '{other}': qwen3_5 splits tensor-parallel \
                 under mlx_lm.server, qwen4_exp splits by layer range under the fork's \
                 pipeline_qwen4; nothing else is wired"
            ),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Runner::MlxLmTensor => "mlxLmTensor",
            Runner::PipelineQwen4 => "pipelineQwen4",
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
    /// The id the engine serves on `/v1/models` and the id chat requests must send.
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
    /// Rank order: `nodes[0]` is this Mac.
    pub nodes: Vec<NodeConfig>,
}

impl DistributedConfig {
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
        let (warn, critical) = self.watchdog_ratios();
        ensure!(
            0.0 < critical && critical < warn && warn < 1.0,
            "watchdog ratios must satisfy 0 < critical ({critical}) < warn ({warn}) < 1"
        );
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
            restart_on_failure: true,
            hang_ratio_only: false,
            watchdog_warn_ratio: None,
            watchdog_critical_ratio: None,
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
        let err = Runner::for_model_type("qwen3_moe").unwrap_err().to_string();
        assert!(err.contains("qwen3_moe"), "{err}");
    }
}
