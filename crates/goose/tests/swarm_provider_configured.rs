#[ctor::ctor]
fn hermetic_path_root() {
    goose_test_support::hermetic_path_root();
}

/// A fresh install must be able to pick Goose Swarm at onboarding: the provider needs no credential,
/// so it is configured before anything is written to the profile (the defaults save refuses an
/// unconfigured provider — that refusal broke "Use Goose Swarm" on every new install, 2026-09-22).
#[tokio::test]
async fn goose_swarm_is_configured_on_a_fresh_profile() {
    std::env::set_var("GOOSE_DISABLE_KEYRING", "1");
    let entry = goose::providers::get_from_registry("swarm")
        .await
        .expect("swarm is registered");
    assert!(entry.inventory_configured());
}
