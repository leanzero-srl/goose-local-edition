//! Q-18 (2026-09-25): every swarm session saved `"context_limit":128000` — the default for a model
//! name nothing declares — and the swarm answered it as the window until the router's first pick.
//! A window comes from a measurement or a declared value; otherwise it is unknown, said out loud.

use goose::providers::base::{Provider, ProviderDef};
use goose::providers::swarm::SwarmProvider;
use goose_providers::model::ModelConfig;

#[ctor::ctor]
fn hermetic_path_root() {
    goose_test_support::hermetic_path_root();
}

#[tokio::test]
async fn a_swarm_session_saves_no_window_nothing_declared() {
    let entry = goose::providers::get_from_registry("swarm")
        .await
        .expect("swarm is registered");
    for model in &entry.metadata().known_models {
        assert_eq!(
            model.context_limit, 0,
            "{} declares no window: 0 is ModelInfo's unknown",
            model.name
        );
    }
    let saved = entry
        .normalize_model_config(ModelConfig::new("swarm"))
        .unwrap();
    assert_eq!(saved.context_limit, None);
}

#[tokio::test]
async fn swarm_chat_with_no_measured_pool_says_the_window_is_unknown() {
    let provider = SwarmProvider::from_env(vec![], None).await.unwrap();
    let err = provider
        .get_context_limit(&ModelConfig::new("swarm"))
        .await
        .expect_err("no pool is configured under the hermetic root, so nothing was measured");
    let reason = err.to_string();
    assert!(reason.contains("context window is unknown"), "{reason}");
    assert!(reason.contains("swarm"), "{reason}");
}

/// The one derivation the compaction guard, the turn-context line and recall's autoload budget
/// read: unknown stays unknown — unless the model config DECLARES a window.
#[tokio::test]
async fn the_effective_window_is_unknown_not_a_default_and_a_declared_one_holds() {
    let provider = SwarmProvider::from_env(vec![], None).await.unwrap();
    assert_eq!(
        goose::context_mgmt::effective_context_limit(&provider, &ModelConfig::new("swarm")).await,
        None
    );
    let declared = ModelConfig::new("swarm").with_context_limit(Some(65_536));
    assert_eq!(
        goose::context_mgmt::effective_context_limit(&provider, &declared).await,
        Some(65_536)
    );
}
