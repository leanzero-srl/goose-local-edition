//! Per-model KV-cache compression: the profile's choice, the engine flag it becomes, and what it
//! saves — computed from the checkpoint's own config.json, never from a table of model names.
//!
//! Rapid-MLX stores a quantized live cache as `QuantizedBatchKVCache`: per element `bits / 8`
//! packed bytes, plus one scale and one bias per group in the activation dtype
//! (`quantized_batch_cache.py`). Only the layers whose KV grows with the context are quantized;
//! linear-attention (GatedDeltaNet) state is fixed-size and stays as it is, and bounded
//! sliding-window buffers stay bf16. The engine is the authority on whether a model's cache
//! layout can take it: an explicit `--kv-cache-dtype int8|int4` it cannot honour fails the mount
//! before ready, with its reason in the stderr tail.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};

/// The engine's default `--kv-cache-quantization-group-size` (cli.py); goose passes none.
const ENGINE_GROUP_SIZE: u64 = 64;
/// `mx.quantize` (affine) accepts only these group sizes; the engine takes the largest one
/// `<= ENGINE_GROUP_SIZE` that divides `head_dim` (`quantized_batch_cache.supported_group_size`).
const SUPPORTED_GROUP_SIZES: [u64; 3] = [128, 64, 32];

/// The measurement record `evals/mlx-engine-bench/kv_quant_compare.py --record` writes into a
/// model directory. goose only reads it.
pub const MEASUREMENT_FILE: &str = "goose-kv-cache.json";

/// A compressed KV cache. Off (bf16, the engine default) is the ABSENCE of a choice
/// (`Option::None`), so a profile that never touched the setting spawns the same argv as before.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KvCacheMode {
    Int8,
    Int4,
}

impl KvCacheMode {
    pub fn bits(self) -> u64 {
        match self {
            KvCacheMode::Int8 => 8,
            KvCacheMode::Int4 => 4,
        }
    }

    /// The `--kv-cache-dtype` value.
    pub fn engine_dtype(self) -> &'static str {
        match self {
            KvCacheMode::Int8 => "int8",
            KvCacheMode::Int4 => "int4",
        }
    }
}

/// What one token of context costs in KV, per cache setting, for one model.
#[derive(Debug, Clone, PartialEq)]
pub struct KvCacheFacts {
    /// Layers whose KV grows with the context (full attention).
    pub attention_layers: u64,
    /// Linear-attention layers: fixed-size recurrent state, not KV, never quantized.
    pub state_layers: u64,
    /// Bounded sliding-window layers: capped at their window, kept bf16 by the engine.
    pub sliding_layers: u64,
    pub kv_heads: u64,
    pub head_dim: u64,
    pub activation_bytes: u64,
    /// The quantization group the engine will use; `None` = no supported group divides
    /// `head_dim`, so the engine cannot quantize this model's KV.
    pub group_size: Option<u64>,
    pub bf16_bytes_per_token: u64,
    pub int8_bytes_per_token: Option<u64>,
    pub int4_bytes_per_token: Option<u64>,
}

impl KvCacheFacts {
    pub fn bytes_per_token(&self, mode: Option<KvCacheMode>) -> Option<u64> {
        match mode {
            None => Some(self.bf16_bytes_per_token),
            Some(KvCacheMode::Int8) => self.int8_bytes_per_token,
            Some(KvCacheMode::Int4) => self.int4_bytes_per_token,
        }
    }
}

fn u64_field(config: &serde_json::Value, key: &str) -> Option<u64> {
    config.get(key).and_then(serde_json::Value::as_u64)
}

fn activation_bytes(dtype: &str) -> Result<u64> {
    Ok(match dtype.trim_start_matches("torch.") {
        "bfloat16" | "float16" => 2,
        "float32" => 4,
        other => bail!("config.json dtype {other:?} has no known KV element width"),
    })
}

fn engine_group_size(head_dim: u64) -> Option<u64> {
    SUPPORTED_GROUP_SIZES
        .into_iter()
        .find(|g| *g <= ENGINE_GROUP_SIZE && head_dim.is_multiple_of(*g))
}

/// Bytes one token of KV costs across the attention layers when stored at `bits` with one scale
/// and one bias per `group` elements (both in the activation dtype).
fn quantized_bytes_per_token(elements_per_token: u64, bits: u64, group: u64, act: u64) -> u64 {
    elements_per_token * bits / 8 + 2 * (elements_per_token / group) * act
}

/// Read the KV facts from a checkpoint's config.json (a multimodal wrapper's `text_config` wins).
pub fn kv_cache_facts(model_dir: &Path) -> Result<KvCacheFacts> {
    let raw: serde_json::Value = serde_json::from_slice(
        &std::fs::read(model_dir.join("config.json"))
            .with_context(|| format!("reading {}/config.json", model_dir.display()))?,
    )
    .context("parsing config.json")?;
    let text = raw.get("text_config").unwrap_or(&raw);
    ensure!(
        text.get("kv_lora_rank").is_none(),
        "multi-head latent attention: the cache holds a compressed latent, so its bytes per \
         token do not follow from head counts"
    );
    let layers = u64_field(text, "num_hidden_layers")
        .context("config.json has no integer `num_hidden_layers`")?;
    let attention_heads = u64_field(text, "num_attention_heads");
    let kv_heads = u64_field(text, "num_key_value_heads")
        .or(attention_heads)
        .context("config.json has neither `num_key_value_heads` nor `num_attention_heads`")?;
    let head_dim = match u64_field(text, "head_dim") {
        Some(dim) => dim,
        None => {
            let hidden = u64_field(text, "hidden_size")
                .context("config.json has neither `head_dim` nor `hidden_size`")?;
            let heads = attention_heads
                .filter(|h| *h > 0)
                .context("config.json has no `num_attention_heads` to derive `head_dim`")?;
            hidden / heads
        }
    };
    let dtype = ["dtype", "torch_dtype"]
        .iter()
        .find_map(|key| {
            text.get(key)
                .or_else(|| raw.get(key))
                .and_then(serde_json::Value::as_str)
        })
        .context("config.json declares no `dtype`/`torch_dtype` for the KV elements")?;
    let act = activation_bytes(dtype)?;

    let (mut attention, mut state, mut sliding) = (0u64, 0u64, 0u64);
    if let Some(types) = text.get("layer_types").and_then(|v| v.as_array()) {
        ensure!(
            types.len() as u64 == layers,
            "config.json declares {layers} layers but {} layer_types",
            types.len()
        );
        for kind in types {
            match kind.as_str() {
                Some("full_attention") => attention += 1,
                Some("linear_attention") => state += 1,
                Some("sliding_attention") => sliding += 1,
                other => {
                    bail!("config.json layer_types holds {other:?}, whose cache goose cannot size")
                }
            }
        }
    } else if let Some(interval) = u64_field(text, "full_attention_interval").filter(|i| *i > 0) {
        attention = layers / interval;
        state = layers - attention;
    } else {
        attention = layers;
    }

    let elements_per_token = attention * 2 * kv_heads * head_dim;
    let group = engine_group_size(head_dim);
    let quantized = |mode: KvCacheMode| {
        group.map(|g| quantized_bytes_per_token(elements_per_token, mode.bits(), g, act))
    };
    Ok(KvCacheFacts {
        attention_layers: attention,
        state_layers: state,
        sliding_layers: sliding,
        kv_heads,
        head_dim,
        activation_bytes: act,
        group_size: group,
        bf16_bytes_per_token: elements_per_token * act,
        int8_bytes_per_token: quantized(KvCacheMode::Int8),
        int4_bytes_per_token: quantized(KvCacheMode::Int4),
    })
}

/// A mount asking for `mode` on a model whose KV the engine provably cannot quantize is refused
/// here, naming why — the engine would otherwise serve a bf16 cache under a quantized flag.
pub fn check_mode_applies(model_dir: &Path, mode: KvCacheMode) -> Result<()> {
    let facts = kv_cache_facts(model_dir).with_context(|| {
        format!(
            "KV cache {} needs the model's KV layout",
            mode.engine_dtype()
        )
    })?;
    ensure!(
        facts.attention_layers > 0,
        "KV cache {}: this model has no full-attention layers, so there is no growing KV to compress",
        mode.engine_dtype()
    );
    ensure!(
        facts.group_size.is_some(),
        "KV cache {}: head_dim {} is divisible by none of the engine's quantization groups (32/64)",
        mode.engine_dtype(),
        facts.head_dim
    );
    Ok(())
}

/// One mode's measured quality against the bf16 reference (`kv_quant_compare.py`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KvModeMeasurement {
    /// Greedy tokens emitted before the first divergence from bf16, over bf16's tokens.
    pub agreement: f64,
    pub identical_answers: u32,
    pub retrieval_found: bool,
    /// Decode tok/s at the longest measured context, over bf16's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decode_tps_ratio: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KvCacheMeasurement {
    pub measured_at: String,
    pub engine: String,
    pub prompts: u32,
    /// bf16 measured against itself: the floor every mode is read against.
    pub noise_floor: KvModeMeasurement,
    pub modes: BTreeMap<KvCacheMode, KvModeMeasurement>,
    pub source: String,
}

/// The model directory's measurement record. `Ok(None)` = never measured on this Mac.
pub fn read_measurement(model_dir: &Path) -> Result<Option<KvCacheMeasurement>> {
    let path = model_dir.join(MEASUREMENT_FILE);
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    serde_json::from_slice(&bytes)
        .with_context(|| format!("parsing {}", path.display()))
        .map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn model_dir(config: serde_json::Value) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("config.json"), config.to_string()).unwrap();
        dir
    }

    /// The owner's Qwen3.8-27B (qwen3_5): 64 layers, every 4th full attention, 4 KV heads x 256.
    fn qwen3_5_27b() -> serde_json::Value {
        let layer_types: Vec<&str> = (0..64)
            .map(|i| {
                if (i + 1) % 4 == 0 {
                    "full_attention"
                } else {
                    "linear_attention"
                }
            })
            .collect();
        json!({
            "model_type": "qwen3_5",
            "text_config": {
                "num_hidden_layers": 64, "num_attention_heads": 24, "num_key_value_heads": 4,
                "head_dim": 256, "hidden_size": 5120, "full_attention_interval": 4,
                "layer_types": layer_types, "dtype": "bfloat16"
            }
        })
    }

    #[test]
    fn the_27b_costs_64_kib_per_token_at_bf16_and_the_packed_layout_when_quantized() {
        let dir = model_dir(qwen3_5_27b());
        let facts = kv_cache_facts(dir.path()).unwrap();
        assert_eq!(facts.attention_layers, 16);
        assert_eq!(facts.state_layers, 48);
        assert_eq!(facts.group_size, Some(64));
        assert_eq!(facts.bf16_bytes_per_token, 65_536);
        // Same figures the fork's admission estimator prices (test_kv_quant_admission_estimate).
        assert_eq!(facts.int8_bytes_per_token, Some(34_816));
        assert_eq!(facts.int4_bytes_per_token, Some(18_432));
    }

    #[test]
    fn interval_only_configs_count_the_same_layers() {
        let dir = model_dir(json!({
            "num_hidden_layers": 48, "num_attention_heads": 16, "num_key_value_heads": 2,
            "head_dim": 256, "full_attention_interval": 4, "torch_dtype": "bfloat16"
        }));
        let facts = kv_cache_facts(dir.path()).unwrap();
        assert_eq!((facts.attention_layers, facts.state_layers), (12, 36));
    }

    #[test]
    fn a_dense_model_derives_head_dim_and_counts_every_layer() {
        let dir = model_dir(json!({
            "num_hidden_layers": 32, "num_attention_heads": 32, "num_key_value_heads": 8,
            "hidden_size": 4096, "torch_dtype": "float16"
        }));
        let facts = kv_cache_facts(dir.path()).unwrap();
        assert_eq!(facts.head_dim, 128);
        assert_eq!(facts.bf16_bytes_per_token, 32 * 2 * 8 * 128 * 2);
        assert_eq!(facts.group_size, Some(64));
    }

    #[test]
    fn a_head_dim_no_group_divides_has_no_quantized_price_and_refuses_the_mode() {
        let dir = model_dir(json!({
            "num_hidden_layers": 2, "num_attention_heads": 4, "num_key_value_heads": 4,
            "head_dim": 80, "dtype": "bfloat16"
        }));
        let facts = kv_cache_facts(dir.path()).unwrap();
        assert_eq!(facts.group_size, None);
        assert_eq!(facts.int8_bytes_per_token, None);
        let err = check_mode_applies(dir.path(), KvCacheMode::Int8).unwrap_err();
        assert!(format!("{err:#}").contains("head_dim 80"), "{err:#}");
    }

    #[test]
    fn a_head_dim_of_96_takes_the_32_group_like_the_engine() {
        let dir = model_dir(json!({
            "num_hidden_layers": 1, "num_attention_heads": 1, "num_key_value_heads": 1,
            "head_dim": 96, "dtype": "bfloat16"
        }));
        assert_eq!(kv_cache_facts(dir.path()).unwrap().group_size, Some(32));
    }

    #[test]
    fn an_unknown_layer_kind_or_mla_is_a_named_error_not_a_guess() {
        let dir = model_dir(json!({
            "num_hidden_layers": 2, "num_key_value_heads": 1, "head_dim": 64, "dtype": "bfloat16",
            "layer_types": ["full_attention", "mamba"]
        }));
        assert!(format!("{:#}", kv_cache_facts(dir.path()).unwrap_err()).contains("mamba"));
        let dir = model_dir(json!({
            "num_hidden_layers": 2, "num_key_value_heads": 1, "head_dim": 64, "dtype": "bfloat16",
            "kv_lora_rank": 512
        }));
        assert!(format!("{:#}", kv_cache_facts(dir.path()).unwrap_err()).contains("latent"));
    }

    #[test]
    fn a_missing_dtype_is_named() {
        let dir = model_dir(json!({
            "num_hidden_layers": 2, "num_key_value_heads": 1, "head_dim": 64
        }));
        assert!(format!("{:#}", kv_cache_facts(dir.path()).unwrap_err()).contains("dtype"));
    }

    #[test]
    fn a_model_without_attention_layers_refuses_the_mode() {
        let dir = model_dir(json!({
            "num_hidden_layers": 2, "num_key_value_heads": 1, "head_dim": 64, "dtype": "bfloat16",
            "layer_types": ["linear_attention", "linear_attention"]
        }));
        assert!(check_mode_applies(dir.path(), KvCacheMode::Int4).is_err());
    }

    #[test]
    fn the_measurement_record_is_optional_and_a_broken_one_is_an_error() {
        let dir = model_dir(qwen3_5_27b());
        assert_eq!(read_measurement(dir.path()).unwrap(), None);
        std::fs::write(dir.path().join(MEASUREMENT_FILE), "{not json").unwrap();
        assert!(read_measurement(dir.path()).is_err());
        let record = json!({
            "measuredAt": "2026-09-24", "engine": "v0.14.3-lz.3", "prompts": 13,
            "noiseFloor": {"agreement": 1.0, "identicalAnswers": 13, "retrievalFound": true},
            "modes": {"int8": {"agreement": 0.98, "identicalAnswers": 11, "retrievalFound": true,
                               "decodeTpsRatio": 0.93}},
            "source": "evals/mlx-engine-bench/results/2026-09-24-kv-quant"
        });
        std::fs::write(dir.path().join(MEASUREMENT_FILE), record.to_string()).unwrap();
        let measured = read_measurement(dir.path()).unwrap().unwrap();
        assert_eq!(measured.modes[&KvCacheMode::Int8].identical_answers, 11);
        assert!(!measured.modes.contains_key(&KvCacheMode::Int4));
    }
}
