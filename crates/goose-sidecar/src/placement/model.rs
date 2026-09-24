//! What a model costs to hold and to run, read from its own files: config.json (architecture,
//! experts, context, quantization) and the safetensors headers (every tensor's bytes — no tensor
//! is read). Decode on Apple Silicon streams the ACTIVE weights once per token, so the planner
//! needs the active bytes, not the file size: a dense model reads all of its matmul weights, an
//! MoE model its shared weights plus `experts_per_token / experts` of the routed ones. Embedding
//! tables are gathered one row per token (a lookup, not a read of the table); the vision tower and
//! the MTP head take no part in a text decode step.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};

use crate::distributed::plan::{dtype_bytes, read_safetensors_header};
use crate::kv_cache::{self, KvCacheFacts};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MoeFacts {
    pub experts: u64,
    pub experts_per_token: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuantFacts {
    pub bits: u64,
    pub group_size: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModelFacts {
    /// Top-level `model_type` (what mlx_lm and goose's runners dispatch on).
    pub model_type: String,
    pub layers: u64,
    /// `max_position_embeddings` of the text model.
    pub max_context: Option<u64>,
    pub moe: Option<MoeFacts>,
    pub quant: Option<QuantFacts>,
    /// Bytes held for text serving: every `model*.safetensors` tensor but the vision tower and MTP.
    pub resident_bytes: u64,
    /// Bytes one decode step streams: dense tensors + the routed experts' active share.
    pub active_bytes_per_token: u64,
    /// Weights (not scales/biases) in that active set — the prefill compute is 2 × this per token.
    pub active_params_per_token: u64,
    /// Embedding tables (gathered, not streamed).
    pub lookup_bytes: u64,
    /// Vision tower + MTP head bytes in `model*.safetensors` (not held for text).
    pub excluded_bytes: u64,
    /// The heaviest single decoder layer's resident bytes (a pipeline stage holds whole layers).
    pub largest_layer_bytes: u64,
    /// KV per context token; `Err` names why it cannot be sized.
    pub kv: Result<KvCacheFacts, String>,
}

impl ModelFacts {
    pub fn is_moe(&self) -> bool {
        self.moe.is_some()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TensorClass {
    Excluded,
    Lookup,
    Routed,
    Dense,
}

fn classify(name: &str, leading_dim: Option<u64>, experts: Option<u64>) -> TensorClass {
    if name.starts_with("mtp.")
        || name.contains(".mtp.")
        || name.contains("visual.")
        || name.contains("vision_tower")
        || name.contains("vision_model")
    {
        TensorClass::Excluded
    } else if name.contains("lm_head") {
        TensorClass::Dense
    } else if name.contains("embed_tokens") || name.contains("embedding") {
        TensorClass::Lookup
    } else if experts.is_some()
        && leading_dim == experts
        && name.contains("experts")
        && !name.contains("shared_expert")
    {
        TensorClass::Routed
    } else {
        TensorClass::Dense
    }
}

fn layer_index(name: &str) -> Option<u64> {
    let rest = &name[name.find("layers.")? + "layers.".len()..];
    rest.split('.').next()?.parse().ok()
}

fn u64_of(config: &serde_json::Value, keys: &[&str]) -> Option<u64> {
    keys.iter()
        .find_map(|k| config.get(k).and_then(serde_json::Value::as_u64))
}

/// Read a model directory's facts. Only `model*.safetensors` count — mlx_lm's loader globs exactly
/// those (plan.rs `read_tensor_facts` holds the same rule).
pub fn read_model_facts(model_dir: &Path) -> Result<ModelFacts> {
    let raw: serde_json::Value = serde_json::from_slice(
        &std::fs::read(model_dir.join("config.json"))
            .with_context(|| format!("reading {}/config.json", model_dir.display()))?,
    )
    .context("parsing config.json")?;
    let text = raw.get("text_config").unwrap_or(&raw);
    let model_type = raw
        .get("model_type")
        .and_then(serde_json::Value::as_str)
        .context("config.json has no `model_type`")?
        .to_string();
    let layers = u64_of(text, &["num_hidden_layers"])
        .context("config.json has no integer `num_hidden_layers`")?;
    let experts = u64_of(
        text,
        &["num_experts", "n_routed_experts", "num_local_experts"],
    );
    let moe = match experts {
        Some(experts) if experts > 1 => Some(MoeFacts {
            experts,
            experts_per_token: u64_of(text, &["num_experts_per_tok", "moe_top_k"]).context(
                "config.json declares experts but no `num_experts_per_tok` — the active share \
                 of the routed weights cannot be derived",
            )?,
        }),
        _ => None,
    };
    let quant_block = raw
        .get("quantization")
        .or_else(|| raw.get("quantization_config"));
    let quant = quant_block.and_then(|q| {
        Some(QuantFacts {
            bits: q.get("bits")?.as_u64()?,
            group_size: q.get("group_size")?.as_u64()?,
        })
    });

    let mut shards: Vec<_> = std::fs::read_dir(model_dir)
        .with_context(|| format!("listing {}", model_dir.display()))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("model") && n.ends_with(".safetensors"))
        })
        .collect();
    shards.sort();
    ensure!(
        !shards.is_empty(),
        "no model*.safetensors in {}",
        model_dir.display()
    );

    let (mut dense, mut routed, mut lookup, mut excluded) = (0u64, 0u64, 0u64, 0u64);
    let (mut dense_params, mut routed_params) = (0u64, 0u64);
    let mut per_layer: BTreeMap<u64, u64> = BTreeMap::new();
    let moe_experts = moe.map(|m| m.experts);
    for shard in &shards {
        for (name, meta) in read_safetensors_header(shard)? {
            if name == "__metadata__" {
                continue;
            }
            let shape: Vec<u64> = meta
                .get("shape")
                .and_then(|s| s.as_array())
                .with_context(|| format!("{name}: no shape"))?
                .iter()
                .map(|d| d.as_u64().with_context(|| format!("{name}: shape entry")))
                .collect::<Result<_>>()?;
            let dtype = meta
                .get("dtype")
                .and_then(|d| d.as_str())
                .with_context(|| format!("{name}: no dtype"))?;
            let elements: u64 = shape.iter().product();
            let bytes = elements * dtype_bytes(dtype)?;
            // A packed quantized weight holds 32 / bits weights per U32; its scales and biases
            // are bookkeeping, not weights.
            let params = if name.ends_with(".scales") || name.ends_with(".biases") {
                0
            } else if dtype == "U32" {
                let bits = quant.map(|q| q.bits).with_context(|| {
                    format!("{name} is packed U32 but config.json declares no quantization bits")
                })?;
                elements * 32 / bits
            } else {
                elements
            };
            let class = classify(&name, shape.first().copied(), moe_experts);
            match class {
                TensorClass::Excluded => excluded += bytes,
                TensorClass::Lookup => lookup += bytes,
                TensorClass::Routed => {
                    routed += bytes;
                    routed_params += params;
                }
                TensorClass::Dense => {
                    dense += bytes;
                    dense_params += params;
                }
            }
            if class != TensorClass::Excluded {
                if let Some(layer) = layer_index(&name) {
                    *per_layer.entry(layer).or_default() += bytes;
                }
            }
        }
    }
    let (active_bytes, active_params) = match moe {
        Some(m) => {
            ensure!(
                routed > 0,
                "config.json declares {} experts but no tensor has a leading dimension of {} — \
                 the routed weights could not be found, so the active bytes are unknown",
                m.experts,
                m.experts
            );
            (
                dense + routed * m.experts_per_token / m.experts,
                dense_params + routed_params * m.experts_per_token / m.experts,
            )
        }
        None => (dense + routed, dense_params + routed_params),
    };
    Ok(ModelFacts {
        model_type,
        layers,
        max_context: u64_of(text, &["max_position_embeddings"]),
        moe,
        quant,
        resident_bytes: dense + routed + lookup,
        active_bytes_per_token: active_bytes,
        active_params_per_token: active_params,
        lookup_bytes: lookup,
        excluded_bytes: excluded,
        largest_layer_bytes: per_layer.values().copied().max().unwrap_or(0),
        kv: kv_cache::kv_cache_facts(model_dir).map_err(|e| format!("{e:#}")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tensors_are_classed_by_role() {
        let e = Some(512);
        assert_eq!(
            classify(
                "model.language_model.layers.3.mlp.experts.gate_up_proj",
                Some(512),
                e
            ),
            TensorClass::Routed
        );
        assert_eq!(
            classify(
                "model.language_model.layers.3.mlp.shared_expert.gate_proj.weight",
                Some(512),
                e
            ),
            TensorClass::Dense
        );
        assert_eq!(
            classify(
                "model.language_model.layers.3.ple.ple_embedding.ngram_embedding.shard_0.weight",
                Some(2_500_012),
                e
            ),
            TensorClass::Lookup
        );
        assert_eq!(
            classify("lm_head.weight", Some(248_320), e),
            TensorClass::Dense
        );
        assert_eq!(
            classify("mtp.layers.0.mlp.experts.down_proj", Some(512), e),
            TensorClass::Excluded
        );
        assert_eq!(
            classify("model.visual.blocks.0.mlp.linear_fc2.weight", Some(1152), e),
            TensorClass::Excluded
        );
        assert_eq!(
            classify("model.layers.0.mlp.experts.w1", Some(64), None),
            TensorClass::Dense
        );
        assert_eq!(
            layer_index("model.language_model.layers.47.mlp.x"),
            Some(47)
        );
        assert_eq!(layer_index("lm_head.weight"), None);
    }

    fn write_checkpoint(dir: &Path, config: serde_json::Value, tensors: &[(&str, &str, &[u64])]) {
        std::fs::write(dir.join("config.json"), config.to_string()).unwrap();
        let mut header = serde_json::Map::new();
        let mut offset = 0u64;
        for (name, dtype, shape) in tensors {
            let bytes = shape.iter().product::<u64>() * dtype_bytes(dtype).unwrap();
            header.insert(
                name.to_string(),
                serde_json::json!({"dtype": dtype, "shape": shape, "data_offsets": [offset, offset + bytes]}),
            );
            offset += bytes;
        }
        let header = serde_json::Value::Object(header).to_string();
        let mut file = (header.len() as u64).to_le_bytes().to_vec();
        file.extend_from_slice(header.as_bytes());
        std::fs::write(dir.join("model-00001-of-00001.safetensors"), file).unwrap();
    }

    #[test]
    fn an_moe_reads_its_shared_weights_and_the_active_share_of_its_experts() {
        let dir = tempfile::tempdir().unwrap();
        write_checkpoint(
            dir.path(),
            serde_json::json!({
                "model_type": "tiny_moe", "num_hidden_layers": 1, "num_attention_heads": 2,
                "num_key_value_heads": 1, "head_dim": 64, "torch_dtype": "bfloat16",
                "num_experts": 8, "num_experts_per_tok": 2, "max_position_embeddings": 4096,
                "quantization": {"bits": 4, "group_size": 64}
            }),
            &[
                ("model.embed_tokens.weight", "U32", &[100, 8]),
                ("model.layers.0.self_attn.q_proj.weight", "U32", &[128, 8]),
                ("model.layers.0.self_attn.q_proj.scales", "BF16", &[128, 1]),
                (
                    "model.layers.0.mlp.experts.gate_up_proj",
                    "U32",
                    &[8, 256, 8],
                ),
                ("lm_head.weight", "U32", &[100, 8]),
                ("mtp.layers.0.mlp.experts.gate_up_proj", "U32", &[8, 256, 8]),
            ],
        );
        let facts = read_model_facts(dir.path()).unwrap();
        let q = 128 * 8 * 4 + 128 * 2;
        let experts = 8 * 256 * 8 * 4;
        let head = 100 * 8 * 4;
        assert_eq!(
            facts.moe,
            Some(MoeFacts {
                experts: 8,
                experts_per_token: 2
            })
        );
        assert_eq!(facts.lookup_bytes, 100 * 8 * 4);
        assert_eq!(facts.excluded_bytes, experts);
        assert_eq!(facts.resident_bytes, q + experts + head + 100 * 8 * 4);
        assert_eq!(facts.active_bytes_per_token, q + head + experts * 2 / 8);
        // U32 at 4 bits packs 8 weights; scales are not weights.
        assert_eq!(
            facts.active_params_per_token,
            (128 * 8 + 100 * 8) * 8 + 8 * 256 * 8 * 8 * 2 / 8
        );
        assert_eq!(facts.largest_layer_bytes, q + experts);
        assert_eq!(facts.max_context, Some(4096));
        assert!(facts.kv.is_ok());
    }

    #[test]
    fn declared_experts_without_expert_tensors_is_a_named_error() {
        let dir = tempfile::tempdir().unwrap();
        write_checkpoint(
            dir.path(),
            serde_json::json!({"model_type": "odd", "num_hidden_layers": 1, "num_experts": 8, "num_experts_per_tok": 2}),
            &[("model.layers.0.mlp.w1.weight", "BF16", &[64, 64])],
        );
        let err = format!("{:#}", read_model_facts(dir.path()).unwrap_err());
        assert!(err.contains("routed weights could not be found"), "{err}");
    }

    /// The owner's two models, read from disk when present (2026-09-24: 27B dense 26.47 GiB active,
    /// Flash 3.53 GiB active of 95.71 GiB resident).
    #[test]
    #[ignore = "reads the owner's real checkpoints from ~/.goose/models"]
    fn the_owners_models_read_whole() {
        let root = crate::engine::expand_tilde("~/.goose/models");
        let gib = |b: u64| b as f64 / crate::GIB as f64;
        let dense =
            read_model_facts(&root.join("Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx")).unwrap();
        eprintln!(
            "27B active {} B {:.3} GiB params {} resident {:.3}",
            dense.active_bytes_per_token,
            gib(dense.active_bytes_per_token),
            dense.active_params_per_token,
            gib(dense.resident_bytes)
        );
        assert!(dense.moe.is_none());
        let flash = read_model_facts(&root.join("rapid-mlx/Qwen3.8-Flash-Next-4bit")).unwrap();
        eprintln!(
            "Flash active {} B {:.3} GiB params {} resident {:.3} largest layer {:.3}",
            flash.active_bytes_per_token,
            gib(flash.active_bytes_per_token),
            flash.active_params_per_token,
            gib(flash.resident_bytes),
            gib(flash.largest_layer_bytes)
        );
        assert_eq!(
            flash.moe,
            Some(MoeFacts {
                experts: 512,
                experts_per_token: 10
            })
        );
    }
}
