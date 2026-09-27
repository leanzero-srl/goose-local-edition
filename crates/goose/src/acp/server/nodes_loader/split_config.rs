//! Run it's discover-and-configure path for a split set up for another model (design §6.4 step 5,
//! §13 item 5): a port of `splitConfigFor`, `cleanConfig` / `cleanNode`, `missingFields` and
//! `splitPlan` (`ui/desktop/src/components/leanzero-swarm/mlxDistributed.ts`). A split for another
//! model is NOT a step — the loader discovers the candidate's Macs for this model, builds the
//! config from the owner's saved one where it spans the same Macs, and starts. Only a genuinely
//! missing piece refuses: the model missing on a Mac, no uv, a field discovery could not fill, or
//! a Mac whose goose Python must be built first (provisioning stays one click in Run it).
//! `split_config.fixture.json` pins the port: this suite and `splitConfig.fixture.test.ts` (the
//! TS functions themselves) run the same cases.

use goose_sdk_types::custom_requests::{
    MlxDistributedConfigDto, MlxDistributedDiscoveryDto, MlxDistributedNodeConfigDto,
};
use serde::{Deserialize, Serialize};

/// `splitConfigFor`: the config a split of the discovered model starts with. When the saved config
/// spans the SAME Macs (their hosts, in rank order) it is the owner's config with only what belongs
/// to the model replaced from the discovery: the model id, each node's model folder and pipeline
/// Python, the context and slots. Other Macs, or nothing saved: the discovery's own.
pub fn config_for(
    discovery: &MlxDistributedDiscoveryDto,
    saved: Option<&MlxDistributedConfigDto>,
) -> MlxDistributedConfigDto {
    let found = &discovery.config;
    // `nodes.map((n) => n.ssh ?? null).join('\n')`: an absent host joins as "".
    let hosts = |c: &MlxDistributedConfigDto| -> Vec<String> {
        c.nodes
            .iter()
            .map(|n| n.ssh.as_deref().unwrap_or("").to_string())
            .collect()
    };
    let Some(saved) = saved else {
        return found.clone();
    };
    if saved.nodes.len() != found.nodes.len() || hosts(saved) != hosts(found) {
        return found.clone();
    }
    MlxDistributedConfigDto {
        model_id: found.model_id.clone(),
        context: found.context,
        slots: found.slots,
        nodes: saved
            .nodes
            .iter()
            .zip(&found.nodes)
            .map(|(node, found)| MlxDistributedNodeConfigDto {
                model_dir: found.model_dir.clone(),
                pipeline_python: found.pipeline_python.clone(),
                ..node.clone()
            })
            .collect(),
        ..saved.clone()
    }
}

/// `cleanNode`: blank optional fields are omitted, never sent as "".
fn clean_node(node: &MlxDistributedNodeConfigDto, rank: usize) -> MlxDistributedNodeConfigDto {
    let mut next = node.clone();
    let ssh = node.ssh.as_deref().map(str::trim).filter(|s| !s.is_empty());
    next.ssh = match ssh {
        Some(ssh) if rank > 0 => Some(ssh.to_string()),
        _ => None,
    };
    if node
        .pipeline_python
        .as_deref()
        .is_none_or(|p| p.trim().is_empty())
    {
        next.pipeline_python = None;
    }
    next
}

/// `cleanConfig`.
pub fn clean_config(config: &MlxDistributedConfigDto) -> MlxDistributedConfigDto {
    MlxDistributedConfigDto {
        model_id: config.model_id.trim().to_string(),
        nodes: config
            .nodes
            .iter()
            .enumerate()
            .map(|(rank, node)| clean_node(node, rank))
            .collect(),
        ..config.clone()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MissingField {
    /// The node's index in `config.nodes`; `None` = a config-level field.
    node: Option<usize>,
    field: &'static str,
}

/// `missingFields`.
fn missing_fields(config: &MlxDistributedConfigDto) -> Vec<MissingField> {
    let blank = |s: &str| s.trim().is_empty();
    let mut missing = Vec::new();
    let top = |field| MissingField { node: None, field };
    if blank(&config.model_id) {
        missing.push(top("modelId"));
    }
    if config.backend != "jaccl" && config.backend != "ring" {
        missing.push(top("backend"));
    }
    if config.port == 0 {
        missing.push(top("port"));
    }
    if config.coordinator_port == 0 {
        missing.push(top("coordinatorPort"));
    }
    if config.nodes.len() < 2 {
        missing.push(top("nodes"));
    }
    for (i, node) in config.nodes.iter().enumerate() {
        let required = [
            ("name", &node.name),
            ("tbIp", &node.tb_ip),
            ("tbNetmask", &node.tb_netmask),
            ("tbInterface", &node.tb_interface),
            ("tbService", &node.tb_service),
            ("python", &node.python),
            ("modelDir", &node.model_dir),
        ];
        for (field, value) in required {
            if blank(value) {
                missing.push(MissingField {
                    node: Some(i),
                    field,
                });
            }
        }
        if config.backend == "jaccl" && blank(&node.rdma_device) {
            missing.push(MissingField {
                node: Some(i),
                field: "rdmaDevice",
            });
        }
        if i > 0 && node.ssh.as_deref().is_none_or(blank) {
            missing.push(MissingField {
                node: Some(i),
                field: "ssh",
            });
        }
    }
    missing
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotFoundItem {
    pub node: Option<String>,
    pub field: String,
    pub reason: String,
}

/// What stops a split of the model before it starts — each a genuinely missing piece, by Mac.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SplitBlocker {
    NotSplittable { reason: Option<String> },
    ModelMissing { nodes: Vec<String> },
    NoUv { nodes: Vec<String> },
    NotFound { items: Vec<NotFoundItem> },
}

/// `splitPlan`'s answer: a blocker, or the Macs whose goose Python must be built first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SplitPlan {
    Blocker { blocker: SplitBlocker },
    Provision { provision: Vec<String> },
}

/// `splitPlan`.
pub fn plan(
    discovery: &MlxDistributedDiscoveryDto,
    model_id: &str,
    config: &MlxDistributedConfigDto,
) -> SplitPlan {
    let name_of = |rank: u32| {
        discovery
            .nodes
            .iter()
            .find(|n| n.rank == rank)
            .map_or_else(|| rank.to_string(), |n| n.name.clone())
    };
    let model_gap = || discovery.gaps.iter().find(|g| g.field == "modelId");
    let blocked = |blocker| SplitPlan::Blocker { blocker };
    let Some(model) = discovery.models.iter().find(|m| m.id == model_id) else {
        return blocked(SplitBlocker::NotSplittable {
            reason: model_gap().map(|g| g.reason.clone()),
        });
    };
    if !model.on_every_node {
        let nodes = model
            .nodes
            .iter()
            .filter(|n| n.state != "match")
            .map(|n| name_of(n.rank))
            .collect();
        return blocked(SplitBlocker::ModelMissing { nodes });
    }
    if config.model_id != model_id {
        let filled = if config.model_id.is_empty() {
            "no model"
        } else {
            &config.model_id
        };
        return blocked(SplitBlocker::NotFound {
            items: vec![NotFoundItem {
                node: None,
                field: "modelId".to_string(),
                reason: model_gap().map_or_else(
                    || format!("the discovery filled in {filled}"),
                    |g| g.reason.clone(),
                ),
            }],
        });
    }
    let missing = missing_fields(config);
    if !missing.is_empty() {
        let items = missing
            .iter()
            .map(|m| NotFoundItem {
                node: m.node.map(|i| name_of(i as u32)),
                field: m.field.to_string(),
                reason: discovery
                    .gaps
                    .iter()
                    .find(|g| g.node == m.node.map(|i| i as u32) && g.field == m.field)
                    .map_or_else(|| "not found".to_string(), |g| g.reason.clone()),
            })
            .collect();
        return blocked(SplitBlocker::NotFound { items });
    }
    let env_is = |states: &[&str]| -> Vec<String> {
        discovery
            .nodes
            .iter()
            .filter(|n| {
                n.env
                    .as_ref()
                    .is_some_and(|e| states.contains(&e.state.as_str()))
            })
            .map(|n| n.name.clone())
            .collect()
    };
    let no_uv = env_is(&["noUv"]);
    if !no_uv.is_empty() {
        return blocked(SplitBlocker::NoUv { nodes: no_uv });
    }
    SplitPlan::Provision {
        provision: env_is(&["absent", "broken"]),
    }
}

/// The blocker in Run it's own English (`splitBlockerText`).
pub fn blocker_words(blocker: &SplitBlocker, model_id: &str) -> String {
    let model = model_id.rsplit('/').next().unwrap_or(model_id);
    match blocker {
        SplitBlocker::NotSplittable {
            reason: Some(reason),
        } => reason.clone(),
        SplitBlocker::NotSplittable { reason: None } => {
            format!("goose found no way to split {model}.")
        }
        SplitBlocker::ModelMissing { nodes } => format!(
            "{model} is not on {} yet — copy it there, then Run.",
            nodes.join(" and ")
        ),
        SplitBlocker::NoUv { nodes } => format!(
            "goose cannot build its Python on {}: there is no uv there.",
            nodes.join(" and ")
        ),
        SplitBlocker::NotFound { items } => format!(
            "goose could not find what the split needs: {}",
            items
                .iter()
                .map(|item| match &item.node {
                    Some(node) => format!("{node} · {}: {}", item.field, item.reason),
                    None => format!("{}: {}", item.field, item.reason),
                })
                .collect::<Vec<_>>()
                .join("; ")
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Case {
        name: String,
        discovery: MlxDistributedDiscoveryDto,
        saved: Option<MlxDistributedConfigDto>,
        model_id: String,
        expect: Expect,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Expect {
        config: MlxDistributedConfigDto,
        plan: SplitPlan,
    }

    #[derive(Deserialize)]
    struct Fixture {
        cases: Vec<Case>,
    }

    /// The shared fixture: `splitConfig.fixture.test.ts` runs the same cases through the TS
    /// functions, so the loader's split for another model is Run it's, case for case.
    #[test]
    fn the_port_matches_run_its_split_config_for_every_fixture_case() {
        let fixture: Fixture =
            serde_json::from_str(include_str!("split_config.fixture.json")).unwrap();
        assert!(fixture.cases.len() >= 8);
        for case in fixture.cases {
            let config = clean_config(&config_for(&case.discovery, case.saved.as_ref()));
            assert_eq!(config, case.expect.config, "{}: config", case.name);
            assert_eq!(
                plan(&case.discovery, &case.model_id, &config),
                case.expect.plan,
                "{}: plan",
                case.name
            );
        }
    }

    #[test]
    fn a_blocker_reads_in_run_its_words() {
        let words = blocker_words(
            &SplitBlocker::ModelMissing {
                nodes: vec!["Work’s Mac Studio".into()],
            },
            "Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx",
        );
        assert_eq!(
            words,
            "Qwen3.8-27B-Atlassian-Q8-mlx is not on Work’s Mac Studio yet — copy it there, then Run."
        );
        let words = blocker_words(
            &SplitBlocker::NotFound {
                items: vec![NotFoundItem {
                    node: Some("Mihai Macbook".into()),
                    field: "rdmaDevice".into(),
                    reason: "no RDMA device".into(),
                }],
            },
            "m",
        );
        assert_eq!(
            words,
            "goose could not find what the split needs: Mihai Macbook · rdmaDevice: no RDMA device"
        );
    }
}
