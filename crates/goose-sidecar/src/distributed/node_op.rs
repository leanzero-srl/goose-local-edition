//! The CLOSED set of scripts goose runs on a distributed node, as typed operations.
//!
//! Over ssh a node runs whatever script the caller composes (`NodeExec::run`). Over LeanZero Link
//! it never does: the requester sends a [`NodeOp`] — data only — and the PEER's own goosed builds
//! the script from it with the same builders the ssh path uses ([`NodeOp::script`]), after
//! [`NodeOp::authorize`] has checked the op against the peer's own facts. So "Allow this Mac to
//! serve as a distributed node" lets a same-account Mac run goose's distributed engine here —
//! never an arbitrary command:
//! - an interpreter a probe executes must be one of goose's managed envs under the peer's own
//!   `$HOME/.goose/distributed/` (an operator's own interpreter is an ssh-path feature);
//! - a signal may only reach a process whose command line carries the goose rank marker;
//! - the TB link repair runs only after the peer re-proves, from its own service listing, that the
//!   named service is a Thunderbolt one on the named device (`preflight::repair_licence`).

use anyhow::{anyhow, bail, ensure, Result};
use serde::{Deserialize, Serialize};

use super::config::{Backend, DistributedConfig, NodeConfig, Runner};
use super::launch::RANK_MARKER;
use super::provision::{EnvSpec, ENVS_DIR};
use super::{compaction, preflight, supervisor};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Signal {
    Term,
    Kill,
}

impl Signal {
    pub fn name(self) -> &'static str {
        match self {
            Signal::Term => "TERM",
            Signal::Kill => "KILL",
        }
    }
}

/// One operation on one node. The wire shape (`{"kind": "...", ...}`, camelCase) is the LeanZero
/// Link contract (`POST /v1/swarm/distributed/exec` `{"op": <NodeOp>}`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum NodeOp {
    /// Preflight's per-node probe for rank `rank` of `config`.
    #[serde(rename_all = "camelCase")]
    Probe {
        config: DistributedConfig,
        runner: Option<Runner>,
        rank: usize,
    },
    /// The TB link's state (interface IPv4, RDMA GID table), re-read after a repair.
    #[serde(rename_all = "camelCase")]
    Link { node: NodeConfig, backend: Backend },
    /// The documented TB link repair: toggle the node's TB service, re-apply its /30.
    #[serde(rename_all = "camelCase")]
    Repair { node: NodeConfig },
    /// The supervisor's poll of a node: memory, kernel pressure, and the rank's `ps` row.
    #[serde(rename_all = "camelCase")]
    Sample { pid: Option<u32> },
    /// One pid's `ps` row (pid, stat, cpu time).
    #[serde(rename_all = "camelCase")]
    PidRow { pid: u32 },
    /// One signal to one pid — never a process group.
    #[serde(rename_all = "camelCase")]
    Signal { pid: u32, signal: Signal },
    /// Every process with its command line (the rank-marker sweep).
    ProcessList,
    /// Memory compaction (`compaction::compaction_script`): raise pressure to the kernel's WARN
    /// with Apple's `memory_pressure`, release it at once, report before/peak/after. A Link peer
    /// refuses it while any MLX engine runs there.
    Compact,
}

impl NodeOp {
    pub fn kind(&self) -> &'static str {
        match self {
            NodeOp::Probe { .. } => "probe",
            NodeOp::Link { .. } => "link",
            NodeOp::Repair { .. } => "repair",
            NodeOp::Sample { .. } => "sample",
            NodeOp::PidRow { .. } => "pidRow",
            NodeOp::Signal { .. } => "signal",
            NodeOp::ProcessList => "processList",
            NodeOp::Compact => "compact",
        }
    }

    /// The script, built by the same functions every path uses (ssh sends it; a Link peer builds
    /// it from the op itself).
    pub fn script(&self) -> Result<String> {
        Ok(match self {
            NodeOp::Probe {
                config,
                runner,
                rank,
            } => {
                ensure!(
                    *rank < config.nodes.len(),
                    "probe of rank {rank} in a {}-node config",
                    config.nodes.len()
                );
                preflight::node_probe_script(config, *runner, *rank)
            }
            NodeOp::Link { node, backend } => preflight::link_script(node, *backend),
            NodeOp::Repair { node } => preflight::repair_script(node),
            NodeOp::Sample { pid } => supervisor::sample_script(*pid),
            NodeOp::PidRow { pid } => pid_row_script(*pid),
            NodeOp::Signal { pid, signal } => format!("/bin/kill -{} {pid}", signal.name()),
            NodeOp::ProcessList => PROCESS_LIST_SCRIPT.to_string(),
            NodeOp::Compact => compaction::compaction_script(),
        })
    }

    /// The peer's own verdict on an op a same-account requester sent: `Ok` or the named reason it
    /// is refused. `home` is the peer's `$HOME`; `command_of` answers a pid's command line on the
    /// peer (`None` = no such process); `services` answers the peer's
    /// `networksetup -listnetworkserviceorder`.
    pub fn authorize(
        &self,
        home: &str,
        command_of: impl FnOnce(u32) -> Result<Option<String>>,
        services: impl FnOnce() -> Result<String>,
    ) -> Result<()> {
        match self {
            NodeOp::Probe { config, rank, .. } => {
                let node = config
                    .nodes
                    .get(*rank)
                    .ok_or_else(|| anyhow!("probe of rank {rank}: the config has no such node"))?;
                managed_interpreter(home, &node.python)?;
                if let Some(python) = &node.pipeline_python {
                    managed_interpreter(home, python)?;
                }
                Ok(())
            }
            NodeOp::Repair { node } => {
                let listing = services()?;
                preflight::repair_licence(node, &listing)
                    .map_err(|why| anyhow!("the TB link repair is refused on this Mac: {why}"))
            }
            NodeOp::Signal { pid, .. } => match command_of(*pid)? {
                None => Ok(()),
                Some(command) if command.contains(RANK_MARKER) => Ok(()),
                Some(command) => bail!(
                    "pid {pid} is not a goose rank (its command line carries no \
                     '{RANK_MARKER}': {}); a Link requester may signal goose ranks only",
                    command.chars().take(160).collect::<String>()
                ),
            },
            // A compaction's own guard (no engine on the node) is the peer's, checked against its
            // live process list right before it runs (`link_host`).
            NodeOp::Link { .. }
            | NodeOp::Sample { .. }
            | NodeOp::PidRow { .. }
            | NodeOp::ProcessList
            | NodeOp::Compact => Ok(()),
        }
    }
}

pub(crate) const PROCESS_LIST_SCRIPT: &str = "/bin/ps -axo pid=,command=";

pub(crate) fn pid_row_script(pid: u32) -> String {
    format!("/bin/ps -o pid=,stat=,time= -p {pid}")
}

/// `python` is one of goose's managed interpreters under THIS node's `home`, or the named reason
/// it is not.
pub fn managed_interpreter(home: &str, python: &str) -> Result<EnvSpec> {
    let root = format!("{}/{ENVS_DIR}/", home.trim_end_matches('/'));
    match EnvSpec::managed_by(python) {
        Some(spec) if python == spec.python(home) => Ok(spec),
        _ => bail!(
            "interpreterNotManaged: '{python}' is not a goose-managed interpreter under {root} — \
             a LeanZero Link node runs only the envs goose provisions there (Set up › Save and \
             provision builds them); an interpreter of your own is an ssh-path setting"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::distributed::config::tests::two_mac_config;

    const HOME: &str = "/Users/workhorse";

    fn link_config() -> DistributedConfig {
        let mut config = two_mac_config();
        config.nodes[1].python = EnvSpec::tensor().python(HOME);
        config
    }

    fn no_process(_: u32) -> Result<Option<String>> {
        Ok(None)
    }

    fn no_services() -> Result<String> {
        panic!("only a repair reads the service listing")
    }

    #[test]
    fn every_op_builds_the_script_the_ssh_path_runs() {
        let config = link_config();
        let probe = NodeOp::Probe {
            config: config.clone(),
            runner: Some(Runner::MlxLmTensor),
            rank: 1,
        };
        assert_eq!(
            probe.script().unwrap(),
            preflight::node_probe_script(&config, Some(Runner::MlxLmTensor), 1)
        );
        assert_eq!(
            NodeOp::Sample { pid: Some(42) }.script().unwrap(),
            supervisor::sample_script(Some(42))
        );
        assert_eq!(
            NodeOp::Signal {
                pid: 42,
                signal: Signal::Kill
            }
            .script()
            .unwrap(),
            "/bin/kill -KILL 42"
        );
        assert_eq!(
            NodeOp::PidRow { pid: 7 }.script().unwrap(),
            "/bin/ps -o pid=,stat=,time= -p 7"
        );
        let out_of_range = NodeOp::Probe {
            config,
            runner: None,
            rank: 5,
        };
        assert!(out_of_range.script().is_err());
    }

    #[test]
    fn the_wire_shape_is_tagged_camel_case() {
        let op = NodeOp::Signal {
            pid: 9,
            signal: Signal::Term,
        };
        assert_eq!(
            serde_json::to_value(&op).unwrap(),
            serde_json::json!({"kind": "signal", "pid": 9, "signal": "TERM"})
        );
        let back: NodeOp =
            serde_json::from_value(serde_json::json!({"kind": "processList"})).unwrap();
        assert_eq!(back, NodeOp::ProcessList);
        let probe = NodeOp::Probe {
            config: link_config(),
            runner: None,
            rank: 1,
        };
        let json = serde_json::to_value(&probe).unwrap();
        assert_eq!(json["kind"], "probe");
        assert_eq!(serde_json::from_value::<NodeOp>(json).unwrap(), probe);
    }

    #[test]
    fn a_probe_runs_only_managed_interpreters_of_the_peer_itself() {
        let config = link_config();
        let probe = |config: DistributedConfig| NodeOp::Probe {
            config,
            runner: None,
            rank: 1,
        };
        probe(config.clone())
            .authorize(HOME, no_process, no_services)
            .unwrap();

        let mut foreign = config.clone();
        foreign.nodes[1].python = "/bin/sh".to_string();
        let err = probe(foreign)
            .authorize(HOME, no_process, no_services)
            .unwrap_err()
            .to_string();
        assert!(err.starts_with("interpreterNotManaged"), "{err}");

        // A managed env's NAME under somebody else's home is not this node's env.
        let mut elsewhere = config.clone();
        elsewhere.nodes[1].python = EnvSpec::tensor().python("/Users/other");
        assert!(probe(elsewhere)
            .authorize(HOME, no_process, no_services)
            .is_err());

        let mut pipeline = config;
        pipeline.nodes[1].pipeline_python = Some("/tmp/evil/bin/python".to_string());
        assert!(probe(pipeline)
            .authorize(HOME, no_process, no_services)
            .is_err());
    }

    #[test]
    fn a_signal_reaches_only_a_goose_rank() {
        let term = |pid| NodeOp::Signal {
            pid,
            signal: Signal::Term,
        };
        term(10)
            .authorize(
                HOME,
                |_| Ok(Some(format!("/x/python -c boot a b {RANK_MARKER}"))),
                no_services,
            )
            .unwrap();
        let err = term(11)
            .authorize(
                HOME,
                |_| {
                    Ok(Some(
                        "/Applications/Safari.app/Contents/MacOS/Safari".into(),
                    ))
                },
                no_services,
            )
            .unwrap_err()
            .to_string();
        assert!(err.contains("not a goose rank"), "{err}");
        // A pid that is already gone: the kill is harmless and the caller observes it gone.
        term(12).authorize(HOME, no_process, no_services).unwrap();
    }

    #[test]
    fn a_repair_is_licensed_by_the_peers_own_service_listing() {
        let node = link_config().nodes[1].clone();
        let tb = "(1) EXO Thunderbolt 2\n(Hardware Port: Thunderbolt 2, Device: en3)\n";
        NodeOp::Repair { node: node.clone() }
            .authorize(HOME, no_process, || Ok(tb.to_string()))
            .unwrap();
        let wifi = "(1) EXO Thunderbolt 2\n(Hardware Port: Wi-Fi, Device: en3)\n";
        let err = NodeOp::Repair { node }
            .authorize(HOME, no_process, || Ok(wifi.to_string()))
            .unwrap_err()
            .to_string();
        assert!(err.contains("not a Thunderbolt port"), "{err}");
    }
}
