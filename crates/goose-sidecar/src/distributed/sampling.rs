//! goose's per-model sampling profile on a split (Q-159). The single engine gets the model's
//! profile as `--default-*` flags (engine.rs, the argv builder) — the layer Rapid-MLX resolves
//! between a request's own field and the checkpoint's `generation_config.json`
//! (`service/helpers.py` `_resolve_*`: request > `--default-*` > alias > generation_config >
//! fallback). A split gets the same layer: the tensor wrapper reads it from rank 0's spec
//! (`rank_sampling.py`), the pipeline fork from the same `--default-*` flags
//! (lz-pipeline-qwen4.9). Both read the checkpoint's `generation_config.json` beneath it
//! themselves, on rank 0, and name a missing or unreadable file on `/v1/status`.

use serde::{Deserialize, Serialize};

use crate::engine::{EngineSettings, ModelProfile};

/// The sampling subset of the split model's [`ModelProfile`]; `None` = the profile leaves that
/// field to the checkpoint.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct SamplingDefaults {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_p: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repetition_penalty: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presence_penalty: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frequency_penalty: Option<f64>,
}

impl SamplingDefaults {
    /// The profile the single engine would mount `model_id` with — the same lookup its argv
    /// builder makes. A model with no profile has no profile layer (the single engine then
    /// passes no `--default-*` flag either).
    pub fn of(settings: &EngineSettings, model_id: &str) -> Self {
        settings
            .model_profiles
            .get(model_id)
            .map(Self::from_profile)
            .unwrap_or_default()
    }

    pub fn from_profile(profile: &ModelProfile) -> Self {
        Self {
            temperature: profile.temperature,
            top_p: profile.top_p,
            top_k: profile.top_k,
            min_p: profile.min_p,
            repetition_penalty: profile.repetition_penalty,
            presence_penalty: profile.presence_penalty,
            frequency_penalty: profile.frequency_penalty,
        }
    }

    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// The fork's `serve --default-*` flags (the single engine's names) for every field set.
    pub fn serve_flags(&self) -> Vec<String> {
        let floats = [
            ("--default-temperature", self.temperature),
            ("--default-top-p", self.top_p),
            ("--default-min-p", self.min_p),
            ("--default-repetition-penalty", self.repetition_penalty),
            ("--default-presence-penalty", self.presence_penalty),
            ("--default-frequency-penalty", self.frequency_penalty),
        ];
        let mut flags = Vec::new();
        for (flag, value) in floats {
            if let Some(value) = value {
                flags.extend([flag.to_string(), value.to_string()]);
            }
        }
        if let Some(top_k) = self.top_k {
            flags.extend(["--default-top-k".to_string(), top_k.to_string()]);
        }
        flags
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_split_reads_the_profile_the_single_engine_mounts_the_model_with() {
        let profile = ModelProfile {
            temperature: Some(0.6),
            top_k: Some(20),
            presence_penalty: Some(1.5),
            context_limit: Some(65_536),
            ..ModelProfile::default()
        };
        let settings = EngineSettings {
            model_profiles: [("Org/Model".to_string(), profile)].into(),
            ..EngineSettings::default()
        };
        let sampling = SamplingDefaults::of(&settings, "Org/Model");
        assert_eq!(
            sampling,
            SamplingDefaults {
                temperature: Some(0.6),
                top_k: Some(20),
                presence_penalty: Some(1.5),
                ..SamplingDefaults::default()
            }
        );
        assert_eq!(
            sampling.serve_flags(),
            [
                "--default-temperature",
                "0.6",
                "--default-presence-penalty",
                "1.5",
                "--default-top-k",
                "20"
            ]
        );
        assert_eq!(
            serde_json::to_value(sampling).unwrap(),
            serde_json::json!({"temperature": 0.6, "top_k": 20, "presence_penalty": 1.5})
        );
        let other = SamplingDefaults::of(&settings, "Org/Other");
        assert!(other.is_empty() && other.serve_flags().is_empty());
    }
}
