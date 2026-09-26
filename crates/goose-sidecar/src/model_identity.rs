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

use crate::engine::EngineSettings;

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
