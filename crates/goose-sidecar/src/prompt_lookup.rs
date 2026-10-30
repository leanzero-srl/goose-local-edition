//! The per-model prompt-lookup (n-gram copy) decoding lever.
//!
//! Rapid-MLX has two n-gram drafters and goose picks the one a model can run:
//!
//! - **MTP lane** — the model is served with its MTP head (`--speculative-config`). Prompt lookup
//!   lives INSIDE the MTP generator as a copy-draft (`spec_decode/mtp/prompt_lookup.py`): when the
//!   last tokens match an n-gram earlier in the context, the following tokens are proposed and the
//!   target verifies them in one forward. It is configured only by process environment
//!   (`RAPID_MLX_MTP_PROMPT_LOOKUP`, `_MIN_NGRAM`, `_MAX_NGRAM`, `_MAX_TOKENS`), read once per
//!   request by `_effective_prompt_lookup_policy`. The family policy decides "auto" (Qwen3.5/3.8:
//!   ON, min 8, max 64, 31 tokens; others: off).
//! - **Suffix lane** — no MTP head (or MTP turned off) on a pure-attention model: the engine's
//!   `--suffix-decoding` drafter (greedy requests only, batch 1, exclusive with MTP). It is OFF unless
//!   the flag is passed, so "auto" passes nothing.
//! - **No lane** — a hybrid linear-attention/SSM model without MTP: the engine refuses suffix
//!   decoding there (chunked verify is not numerically equal to step updates on recurrent layers;
//!   `scheduler._install_suffix_decoding`), so the lever is shown disabled with that reason.
//!
//! "Auto" is the absence of a choice (`mode: None`): a profile that never touched the lever spawns
//! the engine exactly as before the lever existed (no env, no flag).

use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// The explicit choices; auto is `Option::None`, never a variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PromptLookupMode {
    On,
    Off,
}

/// Every field `None` = the engine's own default for the lane.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PromptLookupProfile {
    pub mode: Option<PromptLookupMode>,
    /// MTP lane only: the shortest context n-gram that may trigger a copy (engine floor 2).
    pub min_ngram: Option<u32>,
    /// MTP lane: the longest n-gram matched. Suffix lane: `--suffix-max-suffix-len`.
    pub max_ngram: Option<u32>,
    /// MTP lane: copy-draft ceiling (`_MAX_TOKENS`; 31/63/95 fill whole 32-row verify tiles).
    /// Suffix lane: `--suffix-max-draft`.
    pub max_draft: Option<u32>,
}

impl PromptLookupProfile {
    pub fn is_auto(&self) -> bool {
        *self == Self::default()
    }
}

pub const ENV_ENABLE: &str = "RAPID_MLX_MTP_PROMPT_LOOKUP";
pub const ENV_MIN_NGRAM: &str = "RAPID_MLX_MTP_PROMPT_LOOKUP_MIN_NGRAM";
pub const ENV_MAX_NGRAM: &str = "RAPID_MLX_MTP_PROMPT_LOOKUP_MAX_NGRAM";
pub const ENV_MAX_TOKENS: &str = "RAPID_MLX_MTP_PROMPT_LOOKUP_MAX_TOKENS";

/// Which drafter the lever drives for one model under one serve decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptLookupLane {
    Mtp,
    Suffix,
    Unavailable(String),
}

pub const HYBRID_REASON: &str = "this model mixes linear-attention/SSM layers with attention and \
     is served without an MTP head: the engine refuses suffix decoding on recurrent layers, and \
     prompt lookup only runs inside MTP";

/// The lane for a model whose serve decision is `serves_mtp`, with `hybrid` read from its
/// config.json (`model_is_hybrid`). An unreadable config leaves the suffix lane unproven, so it is
/// unavailable with the read error as the reason (the flag would be refused or silently ignored).
pub fn lane(serves_mtp: bool, hybrid: &Result<bool>) -> PromptLookupLane {
    if serves_mtp {
        return PromptLookupLane::Mtp;
    }
    match hybrid {
        Ok(false) => PromptLookupLane::Suffix,
        Ok(true) => PromptLookupLane::Unavailable(HYBRID_REASON.to_string()),
        Err(e) => PromptLookupLane::Unavailable(format!(
            "goose cannot tell whether this model is pure attention: {e:#}"
        )),
    }
}

/// Layer kinds that keep per-layer recurrent state rather than a KV cache.
const STATE_LAYER_KINDS: &[&str] = &[
    "linear_attention",
    "mamba",
    "mamba2",
    "ssm",
    "conv",
    "recurrent",
];

/// `model_type`s the engine serves as hybrids even when config.json lists no `layer_types`.
const HYBRID_MODEL_TYPES: &[&str] = &[
    "qwen3_next",
    "qwen3_5",
    "qwen3_5_moe",
    "granitemoehybrid",
    "nemotron_h",
    "falcon_h1",
    "jamba",
    "mamba",
    "mamba2",
    "lfm2",
    "lfm2_moe",
];

/// Whether the checkpoint has any recurrent-state layer (GatedDeltaNet, Mamba, ...).
pub fn model_is_hybrid(model_dir: &Path) -> Result<bool> {
    let raw: serde_json::Value = serde_json::from_slice(
        &std::fs::read(model_dir.join("config.json"))
            .with_context(|| format!("reading {}/config.json", model_dir.display()))?,
    )
    .context("parsing config.json")?;
    Ok(config_is_hybrid(&raw))
}

fn config_is_hybrid(raw: &serde_json::Value) -> bool {
    let text = raw.get("text_config").unwrap_or(raw);
    let model_type = |v: &serde_json::Value| {
        v.get("model_type")
            .and_then(|t| t.as_str())
            .map(str::to_ascii_lowercase)
    };
    if [model_type(raw), model_type(text)]
        .iter()
        .flatten()
        .any(|t| HYBRID_MODEL_TYPES.contains(&t.as_str()))
    {
        return true;
    }
    if let Some(types) = text.get("layer_types").and_then(|v| v.as_array()) {
        if types.iter().filter_map(|k| k.as_str()).any(|k| {
            let k = k.to_ascii_lowercase();
            STATE_LAYER_KINDS.iter().any(|s| k.contains(s))
        }) {
            return true;
        }
    }
    text.get("full_attention_interval")
        .and_then(|v| v.as_u64())
        .is_some_and(|interval| interval > 1)
}

/// Process environment for the engine: only on the MTP lane, only for what the profile sets.
pub fn engine_env(profile: &PromptLookupProfile, lane: &PromptLookupLane) -> Vec<(String, String)> {
    if *lane != PromptLookupLane::Mtp {
        return Vec::new();
    }
    let mut env = Vec::new();
    match profile.mode {
        Some(PromptLookupMode::On) => env.push((ENV_ENABLE.to_string(), "1".to_string())),
        Some(PromptLookupMode::Off) => {
            env.push((ENV_ENABLE.to_string(), "0".to_string()));
            // Off is off: tunables would be read by nothing, and sending them would make two
            // identical engines differ in identity.
            return env;
        }
        None => {}
    }
    let (min, max) = ngram_bounds(profile);
    if let Some(min) = min {
        env.push((ENV_MIN_NGRAM.to_string(), min.to_string()));
    }
    if let Some(max) = max {
        env.push((ENV_MAX_NGRAM.to_string(), max.to_string()));
    }
    if let Some(tokens) = profile.max_draft.filter(|t| *t >= 1) {
        env.push((ENV_MAX_TOKENS.to_string(), tokens.to_string()));
    }
    env
}

/// The engine clamps min to >= 2 and max to >= min; goose sends the clamped values so what the
/// app shows is what runs.
fn ngram_bounds(profile: &PromptLookupProfile) -> (Option<u32>, Option<u32>) {
    let min = profile.min_ngram.map(|m| m.max(2));
    let max = profile.max_ngram.map(|m| m.max(min.unwrap_or(1)).max(1));
    (min, max)
}

/// Serve flags: only on the suffix lane, only when the profile says on.
pub fn engine_flags(profile: &PromptLookupProfile, lane: &PromptLookupLane) -> Vec<String> {
    if *lane != PromptLookupLane::Suffix || profile.mode != Some(PromptLookupMode::On) {
        return Vec::new();
    }
    let mut argv = vec!["--suffix-decoding".to_string()];
    if let Some(draft) = profile.max_draft.filter(|d| *d >= 1) {
        argv.extend(["--suffix-max-draft".to_string(), draft.to_string()]);
    }
    if let Some(len) = profile.max_ngram.filter(|l| *l >= 1) {
        argv.extend(["--suffix-max-suffix-len".to_string(), len.to_string()]);
    }
    argv
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn on() -> PromptLookupProfile {
        PromptLookupProfile {
            mode: Some(PromptLookupMode::On),
            min_ngram: Some(6),
            max_ngram: Some(32),
            max_draft: Some(63),
        }
    }

    fn env_of(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn auto_sends_nothing_on_any_lane() {
        let auto = PromptLookupProfile::default();
        assert!(auto.is_auto());
        for lane in [
            PromptLookupLane::Mtp,
            PromptLookupLane::Suffix,
            PromptLookupLane::Unavailable("x".into()),
        ] {
            assert!(engine_env(&auto, &lane).is_empty());
            assert!(engine_flags(&auto, &lane).is_empty());
        }
    }

    #[test]
    fn mtp_lane_on_sets_the_switch_and_every_tunable() {
        assert_eq!(
            engine_env(&on(), &PromptLookupLane::Mtp),
            env_of(&[
                (ENV_ENABLE, "1"),
                (ENV_MIN_NGRAM, "6"),
                (ENV_MAX_NGRAM, "32"),
                (ENV_MAX_TOKENS, "63"),
            ])
        );
        assert!(engine_flags(&on(), &PromptLookupLane::Mtp).is_empty());
    }

    #[test]
    fn mtp_lane_off_sends_only_the_off_switch() {
        let off = PromptLookupProfile {
            mode: Some(PromptLookupMode::Off),
            ..on()
        };
        assert_eq!(
            engine_env(&off, &PromptLookupLane::Mtp),
            env_of(&[(ENV_ENABLE, "0")])
        );
    }

    #[test]
    fn auto_mode_with_tunables_keeps_the_family_switch() {
        let tuned = PromptLookupProfile {
            mode: None,
            max_draft: Some(95),
            ..Default::default()
        };
        assert_eq!(
            engine_env(&tuned, &PromptLookupLane::Mtp),
            env_of(&[(ENV_MAX_TOKENS, "95")])
        );
    }

    #[test]
    fn ngram_bounds_are_clamped_like_the_engine() {
        let p = PromptLookupProfile {
            mode: Some(PromptLookupMode::On),
            min_ngram: Some(1),
            max_ngram: Some(0),
            max_draft: Some(0),
        };
        assert_eq!(
            engine_env(&p, &PromptLookupLane::Mtp),
            env_of(&[
                (ENV_ENABLE, "1"),
                (ENV_MIN_NGRAM, "2"),
                (ENV_MAX_NGRAM, "2")
            ])
        );
    }

    #[test]
    fn suffix_lane_on_passes_the_flag_and_its_tunables_never_env() {
        assert_eq!(
            engine_flags(&on(), &PromptLookupLane::Suffix),
            vec![
                "--suffix-decoding",
                "--suffix-max-draft",
                "63",
                "--suffix-max-suffix-len",
                "32"
            ]
        );
        assert!(engine_env(&on(), &PromptLookupLane::Suffix).is_empty());
        let off = PromptLookupProfile {
            mode: Some(PromptLookupMode::Off),
            ..on()
        };
        assert!(engine_flags(&off, &PromptLookupLane::Suffix).is_empty());
    }

    #[test]
    fn unavailable_lane_never_reaches_the_engine() {
        let lane = PromptLookupLane::Unavailable(HYBRID_REASON.into());
        assert!(engine_env(&on(), &lane).is_empty());
        assert!(engine_flags(&on(), &lane).is_empty());
    }

    #[test]
    fn lane_follows_the_serve_decision_and_the_architecture() {
        assert_eq!(lane(true, &Ok(true)), PromptLookupLane::Mtp);
        assert_eq!(lane(false, &Ok(false)), PromptLookupLane::Suffix);
        assert!(matches!(
            lane(false, &Ok(true)),
            PromptLookupLane::Unavailable(r) if r == HYBRID_REASON
        ));
        assert!(matches!(
            lane(false, &Err(anyhow::anyhow!("no config"))),
            PromptLookupLane::Unavailable(r) if r.contains("no config")
        ));
    }

    #[test]
    fn hybrid_detection_reads_layer_kinds_intervals_and_families() {
        assert!(config_is_hybrid(&json!({
            "text_config": {"model_type": "x", "layer_types": ["linear_attention", "full_attention"]}
        })));
        assert!(config_is_hybrid(
            &json!({"model_type": "x", "full_attention_interval": 4})
        ));
        assert!(config_is_hybrid(&json!({"model_type": "qwen3_5"})));
        assert!(config_is_hybrid(
            &json!({"model_type": "x", "layer_types": ["mamba", "attention"]})
        ));
        assert!(!config_is_hybrid(&json!({
            "model_type": "llama",
            "layer_types": ["full_attention", "sliding_attention"]
        })));
        assert!(!config_is_hybrid(
            &json!({"model_type": "gemma3", "full_attention_interval": 1})
        ));
    }

    #[test]
    fn the_profile_round_trips_with_lowercase_modes_and_tolerates_absence() {
        let back: PromptLookupProfile =
            serde_json::from_value(json!({"mode": "on", "max_draft": 31})).unwrap();
        assert_eq!(back.mode, Some(PromptLookupMode::On));
        assert_eq!(back.max_draft, Some(31));
        assert_eq!(back.min_ngram, None);
        let empty: PromptLookupProfile = serde_json::from_value(json!({})).unwrap();
        assert!(empty.is_auto());
    }
}
