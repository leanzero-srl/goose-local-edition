//! One model on disk, three names. A swarm node that means a model may write it as
//!
//! - its HF directory id (`rapid-mlx/Qwen3.8-Flash-Next-4bit`) — what an engine serves any model
//!   under that is not the alias's own;
//! - the alias `mlx_engine.served_model_name` gives the ONE model `mlx_engine.model_id` names
//!   (`engine::served_model_id`) — what the single engine and both splits serve that model under;
//! - the Add-node derivation `<label>-<model tag>-mlx` beside the device id `<label>-mlx`
//!   (ui/desktop `mlxServedAlias` / `mlxDeviceId`) — written into `swarm.devices` whether or not the
//!   alias is still bound to that model (Q-128, 11:57: the node `mihai-flash-mlx` saved
//!   `mihai-flash-qwen3.8-flash-next-4bit-mlx` while the pipeline split served the HF id).
//!
//! [`node_names_model`] is the one comparison; the desktop's `nodeNamesModel` mirrors it and both
//! are pinned to `model_identity.fixture.json`, so the two cannot drift.
//!
//! [`ServedNames`] hands the same rule to the ENGINES (Q-131): every way a model is served — the
//! single engine, the tensor wrapper, the pipeline server — is told every name of the model it
//! serves and answers to each, so which name works no longer depends on how the engine was started,
//! and a client other than goose's router (a user's tool, the harness) may name the model by its HF
//! id. The engines hold the list, never the rule: a name for another model stays a refusal.

use crate::engine::{served_model_id, EngineSettings};
use serde::{Deserialize, Serialize};

/// The Add-node model tag: the repo name, lowercased, with its `mlx` tokens dropped (the alias
/// re-appends `-mlx` as its engine marker). `Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx` →
/// `qwen3.8-27b-atlassian-q8`.
pub fn model_tag(repo_id: &str) -> String {
    let repo = repo_id.rsplit('/').next().unwrap_or(repo_id);
    repo.to_lowercase()
        .split('-')
        .filter(|t| !t.is_empty() && *t != "mlx")
        .collect::<Vec<_>>()
        .join("-")
}

/// The HF directory an engine serving `served` loaded: the model the alias names when `served` IS
/// the alias, else `served` itself (every other model is served under its own id) — the inverse of
/// `engine::served_model_id`.
pub fn served_repo(settings: &EngineSettings, served: &str) -> String {
    match (&settings.served_model_name, &settings.model_id) {
        (Some(alias), Some(model)) if alias == served => model.clone(),
        _ => served.to_string(),
    }
}

/// Whether the swarm device `node_id` whose `model_id` is `node_model_id` means the model an engine
/// serves as `served`, loaded from `repo`. The label of an Add-node alias is the device id's, so a
/// longer model whose tag merely ends in this one's never matches.
pub fn node_names_model(node_id: &str, node_model_id: &str, served: &str, repo: &str) -> bool {
    if node_model_id == served || node_model_id == repo {
        return true;
    }
    match node_id.strip_suffix("-mlx") {
        Some(label) if !label.is_empty() => {
            node_model_id == format!("{label}-{}-mlx", model_tag(repo))
        }
        _ => false,
    }
}

/// A swarm node as the identity reads it: the device id and the model id it names (`swarm.devices`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeModel {
    pub id: String,
    pub model_id: String,
}

/// Every name one engine answers to for the model it serves: `id` is what it advertises first
/// (`engine::served_model_id` — what status reports and chat names), `also` every other name of the
/// SAME model, in order: its HF id, then each swarm node's model id that names it
/// ([`node_names_model`]) — the Add-node alias of a node whose alias is bound elsewhere now (Q-128's
/// 12:1x case) included.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServedNames {
    pub id: String,
    pub also: Vec<String>,
}

impl ServedNames {
    /// The names of `repo` on an engine started under `settings`, with the pool's `nodes`.
    pub fn of(settings: &EngineSettings, repo: &str, nodes: &[NodeModel]) -> Self {
        let id = served_model_id(settings, repo);
        let mut also: Vec<String> = Vec::new();
        let candidates = std::iter::once(repo).chain(
            nodes
                .iter()
                .filter(|node| node_names_model(&node.id, &node.model_id, &id, repo))
                .map(|node| node.model_id.as_str()),
        );
        for name in candidates {
            if name != id && !also.iter().any(|known| known == name) {
                also.push(name.to_string());
            }
        }
        Self { id, also }
    }

    /// An engine that answers to one name only.
    pub fn only(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            also: Vec::new(),
        }
    }

    /// Every name, the advertised id first.
    pub fn all(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.id.as_str()).chain(self.also.iter().map(String::as_str))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Case {
        way: String,
        node_id: String,
        node_model_id: String,
        served: String,
        repo: String,
        names: bool,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Fixture {
        tags: Vec<(String, String)>,
        cases: Vec<Case>,
    }

    fn fixture() -> Fixture {
        serde_json::from_str(include_str!(
            "../../../ui/desktop/src/components/noNodeNotice/model_identity.fixture.json"
        ))
        .unwrap()
    }

    #[test]
    fn the_tag_is_the_add_node_derivation() {
        for (repo, tag) in fixture().tags {
            assert_eq!(model_tag(&repo), tag, "{repo}");
        }
    }

    /// Every way a model is served — single, remote single, the tensor split, the pipeline split —
    /// against every form a node may name it by.
    #[test]
    fn a_node_names_the_model_whatever_form_the_way_serves_it_under() {
        let cases = fixture().cases;
        for way in ["single", "remote", "tensor", "pipeline"] {
            assert!(
                cases.iter().any(|c| c.way == way && c.names),
                "no matching case for {way}"
            );
            assert!(
                cases.iter().any(|c| c.way == way && !c.names),
                "no refusing case for {way}"
            );
        }
        for c in cases {
            assert_eq!(
                node_names_model(&c.node_id, &c.node_model_id, &c.served, &c.repo),
                c.names,
                "{} · {} ({}) vs served {} from {}",
                c.way,
                c.node_id,
                c.node_model_id,
                c.served,
                c.repo
            );
        }
    }

    /// The engine side of the same identity (Q-131): for every fixture case, the engine serving
    /// `repo` as `served` answers to the node's model id exactly when the node names that model —
    /// single, remote, tensor and pipeline alike — and always to the served id and the HF id.
    #[test]
    fn an_engine_answers_to_every_name_a_node_may_call_its_model_and_no_other() {
        for c in fixture().cases {
            let settings = EngineSettings {
                model_id: Some(c.repo.clone()),
                served_model_name: (c.served != c.repo).then(|| c.served.clone()),
                ..EngineSettings::default()
            };
            let node = NodeModel {
                id: c.node_id.clone(),
                model_id: c.node_model_id.clone(),
            };
            let names = ServedNames::of(&settings, &c.repo, std::slice::from_ref(&node));
            assert_eq!(names.id, c.served, "{} · the advertised id", c.way);
            let all: Vec<&str> = names.all().collect();
            assert!(all.contains(&c.repo.as_str()), "{} · {all:?}", c.way);
            assert_eq!(
                all.contains(&c.node_model_id.as_str()),
                c.names,
                "{} · {} ({}) vs {all:?}",
                c.way,
                c.node_id,
                c.node_model_id
            );
            let distinct: std::collections::HashSet<&&str> = all.iter().collect();
            assert_eq!(
                distinct.len(),
                all.len(),
                "{} · no name twice: {all:?}",
                c.way
            );
        }
    }

    const FLASH: &str = "rapid-mlx/Qwen3.8-Flash-Next-4bit";
    const Q8: &str = "Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx";
    const FLASH_ALIAS: &str = "mihai-flash-qwen3.8-flash-next-4bit-mlx";
    const Q8_ALIAS: &str = "mihai-qwen3.8-27b-atlassian-q8-mlx";

    fn pool() -> Vec<NodeModel> {
        [("mihai-mlx", Q8_ALIAS), ("mihai-flash-mlx", FLASH_ALIAS)]
            .into_iter()
            .map(|(id, model_id)| NodeModel {
                id: id.to_string(),
                model_id: model_id.to_string(),
            })
            .collect()
    }

    /// The two live cases: 3.0.49's Flash pipeline (the alias bound to Flash) refused the HF id;
    /// 3.0.48's 27B tensor split (the alias bound to Flash, so the 27B served its HF id) refused
    /// the 27B's own node alias. Each now answers to both forms, never to the other model's name.
    #[test]
    fn the_measured_splits_answer_to_both_forms_and_refuse_the_other_model() {
        let flash_bound = EngineSettings {
            model_id: Some(FLASH.to_string()),
            served_model_name: Some(FLASH_ALIAS.to_string()),
            ..EngineSettings::default()
        };
        assert_eq!(
            ServedNames::of(&flash_bound, FLASH, &pool()),
            ServedNames {
                id: FLASH_ALIAS.to_string(),
                also: vec![FLASH.to_string()],
            }
        );
        assert_eq!(
            ServedNames::of(&flash_bound, Q8, &pool()),
            ServedNames {
                id: Q8.to_string(),
                also: vec![Q8_ALIAS.to_string()],
            }
        );
        assert_eq!(
            ServedNames::of(&flash_bound, FLASH, &[]),
            ServedNames {
                id: FLASH_ALIAS.to_string(),
                also: vec![FLASH.to_string()],
            },
            "with no pool the HF id still answers"
        );
        assert_eq!(
            ServedNames::of(&EngineSettings::default(), FLASH, &[]),
            ServedNames::only(FLASH),
            "no alias and no pool: the HF id alone"
        );
    }

    #[test]
    fn the_served_repo_inverts_the_alias_for_its_own_model_only() {
        let settings = EngineSettings {
            model_id: Some("Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx".to_string()),
            served_model_name: Some("mihai-qwen3.8-27b-atlassian-q8-mlx".to_string()),
            ..EngineSettings::default()
        };
        assert_eq!(
            served_repo(&settings, "mihai-qwen3.8-27b-atlassian-q8-mlx"),
            "Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx"
        );
        assert_eq!(
            served_repo(&settings, "rapid-mlx/Qwen3.8-Flash-Next-4bit"),
            "rapid-mlx/Qwen3.8-Flash-Next-4bit"
        );
        for repo in [
            "Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx",
            "rapid-mlx/Qwen3.8-Flash-Next-4bit",
        ] {
            let served = crate::engine::served_model_id(&settings, repo);
            assert_eq!(served_repo(&settings, &served), repo);
        }
    }
}
