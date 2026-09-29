//! The swarm config's load and save: the `swarm:` block of goose's config.yaml read OVER the struct
//! default (`load_config`, `merge_config_over_defaults`, `merge_json`), the one durable note of a
//! parse failure (`CONFIG_PARSE_ERROR`), and the CLI's save (`save_config`).
//!
//! Sibling module under the incremental-split law
//! (development_gates::swarm_rs_line_count_only_decreases). Extracted verbatim from swarm.rs with
//! its two merge tests; only visibility changed.

use anyhow::{anyhow, Result};
use goose::config::{Config, ConfigError};

use super::{SwarmConfig, SWARM_CONFIG_KEY};

/// Deep-merge `over` INTO `base`: for objects, recurse key-by-key; a NULL in `over` means "leave the
/// base default" (never overwrite a real default with null); any non-object leaf in `over` replaces base.
fn merge_json(base: &mut serde_json::Value, over: serde_json::Value) {
    match (base, over) {
        (serde_json::Value::Object(b), serde_json::Value::Object(o)) => {
            for (k, v) in o {
                if v.is_null() {
                    continue;
                }
                merge_json(b.entry(k).or_insert(serde_json::Value::Null), v);
            }
        }
        (b, o) => *b = o,
    }
}

/// Load the swarm config with the STRUCT DEFAULT as the base, so a key OMITTED from config.yaml keeps the
/// intended default rather than serde's TYPE default (None/false).
///
/// `get_param::<SwarmConfig>()` deserializes the `swarm:` block directly, and serde fills a missing key with
/// the FIELD TYPE's default (None/false), NOT the `Default for SwarmConfig` value — so every non-type
/// default (e.g. `sink_max_turns: Some(120)`, the whole baked golden formula) was silently reverted whenever
/// a swarm block existed, which is always. Merging the raw config OVER `SwarmConfig::default()` fixes the
/// entire class at once, and makes the resolvers correct too (a `cfg.unwrap_or(false)` now sees the merged
/// `Some(golden)`). Falls back to the old typed read on any serialization hiccup, so it can only be safer.
/// GEN-6a #2 (fallback rule): the one durable note of a swarm-config parse failure. load_config
/// runs on hot paths with no event sink, so a broken config block used to silently run the
/// DEFAULTS while levers_resolved echoed those defaults as if chosen — the operator's yaml was
/// ignored and nothing said so. levers_resolved reads this note into the event.
pub(super) static CONFIG_PARSE_ERROR: std::sync::OnceLock<String> = std::sync::OnceLock::new();

pub(super) fn load_config() -> SwarmConfig {
    let cfg = Config::global();
    let raw = match cfg.get(SWARM_CONFIG_KEY, false) {
        Ok(v) => Some(v),
        // A missing key is a fresh install — defaults are the honest answer, silently.
        Err(ConfigError::NotFound(_)) => None,
        Err(e) => {
            // Any OTHER failure means the operator's config EXISTS but never reached the run.
            // MEASURED 2026-08-30 (run swarm-20260830-222740116): a duplicate top-level
            // `mlx_engine:` key failed the whole config-file parse, `.ok()` erased the evidence,
            // and the run silently used defaults — allow_model_load fell to off, the configured
            // sidecar device and planner vanished, and the red run was misdiagnosed as an engine
            // defect. Same OnceLock as the block-level arm in merge_config_over_defaults, so the
            // levers echo carries `config_parse_error` and the red banner names it.
            let _ = CONFIG_PARSE_ERROR.set(format!(
                "the goose config FILE failed to load ({e}) — the operator's swarm block never \
                 reached the run"
            ));
            None
        }
    };
    merge_config_over_defaults(raw, || {
        cfg.get_param::<SwarmConfig>(SWARM_CONFIG_KEY)
            .unwrap_or_default()
    })
}

/// The pure half of `load_config`, split out so the merge composition is PINNED by a test
/// (`an_omitted_key_keeps_the_baked_golden_through_the_real_merge`): repeat_break/omni_judge
/// stay armed for a yaml that lacks those keys ONLY because the operator's raw block is merged
/// OVER `serde_json::to_value(SwarmConfig::default())` here. A de-merge refactor — reading the
/// typed block directly, or basing the merge on serde's type defaults — silently reverts the
/// entire baked golden formula (every Some(...) default) the moment a swarm block exists, which
/// is always; the test fails it loudly instead.
fn merge_config_over_defaults(
    raw: Option<serde_json::Value>,
    typed_fallback: impl Fn() -> SwarmConfig,
) -> SwarmConfig {
    let Ok(mut base) = serde_json::to_value(SwarmConfig::default()) else {
        return typed_fallback();
    };
    if let Some(raw) = raw {
        merge_json(&mut base, raw);
    }
    serde_json::from_value(base).unwrap_or_else(|e| {
        let _ = CONFIG_PARSE_ERROR.set(format!(
            "swarm config block failed to deserialize ({e}) — the run is on DEFAULTS, not the \
             operator's config"
        ));
        typed_fallback()
    })
}

pub(super) fn save_config(cfg: &SwarmConfig) -> Result<()> {
    Config::global()
        .set_param(SWARM_CONFIG_KEY, cfg)
        .map_err(|e| anyhow!("failed to save swarm config: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// THE MERGE PIN (refactor hazard, works-prover). repeat_break/omni_judge stay armed for a
    /// yaml that lacks those keys ONLY because load_config merges the operator's raw block OVER
    /// SwarmConfig::default() — serde's type default for both is None, which every resolver
    /// reads as off. This traverses the REAL path (merge_config_over_defaults is load_config's
    /// body minus the global-config read), so a future de-merge refactor fails here loudly
    /// instead of silently disarming the golden formula on every configured machine.
    #[test]
    fn an_omitted_key_keeps_the_baked_golden_through_the_real_merge() {
        let never = || panic!("a parseable block must never fall back to the typed read");
        // An empty swarm block — the minimal configured machine.
        let merged = merge_config_over_defaults(Some(serde_json::json!({})), never);
        assert_eq!(merged.repeat_break, Some(true), "golden survives {{}}");
        assert_eq!(merged.omni_judge, Some(true), "golden survives {{}}");
        // A minimal operator yaml: one real key set, everything omitted stays golden.
        let merged = merge_config_over_defaults(
            Some(serde_json::json!({"planner_model": "operator-model"})),
            never,
        );
        assert_eq!(merged.planner_model, "operator-model");
        assert_eq!(merged.repeat_break, Some(true));
        assert_eq!(merged.omni_judge, Some(true));
        // merge_json's null rule: an explicit null leaves the baked default, never disarms it.
        let merged =
            merge_config_over_defaults(Some(serde_json::json!({"repeat_break": null})), never);
        assert_eq!(merged.repeat_break, Some(true));
        // No block at all (a fresh machine) is the struct default outright.
        let merged = merge_config_over_defaults(None, never);
        assert_eq!(merged.repeat_break, Some(true));
        assert_eq!(merged.omni_judge, Some(true));
    }

    /// load_config MERGES the config over the struct Default, so the baked golden formula survives an
    /// omitted key even though a bare #[serde(default)] on the field alone would not. This exercises the
    /// exact merge path load_config uses (to_value(default) -> merge partial -> from_value).
    #[test]
    fn the_merge_path_keeps_baked_golden_defaults_for_omitted_keys() {
        let mut base = serde_json::to_value(SwarmConfig::default()).unwrap();
        // A user config that sets ONE unrelated key and omits the whole golden bundle.
        let partial: serde_json::Value = serde_json::json!({ "persona": false });
        merge_json(&mut base, partial);
        let cfg: SwarmConfig = serde_json::from_value(base).unwrap();
        // The omitted golden levers keep their baked ON default (this is what a bare serde default lost).
        assert!(cfg.require_tests && cfg.author_pitfalls);
        assert_eq!(cfg.spec_wins, Some(true));
        assert_eq!(
            cfg.straggler_stop, None,
            "retired (r6e): no baked value to keep"
        );
        assert_eq!(cfg.parallel_tests, Some(true));
        assert_eq!(cfg.spiral_break_chars, Some(12000));
        assert_eq!(cfg.struct_stop, 80);
        assert_eq!(cfg.sink_max_turns, Some(120));
        // The explicitly-set key wins over the default, and a NULL would keep the default.
        assert!(
            !cfg.persona,
            "an explicit value overrides the baked default"
        );
    }
}
