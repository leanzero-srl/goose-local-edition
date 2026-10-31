//! goose's own layer split for the mlx_lm pipeline runner (`Runner::MlxLmPipeline`, mimo_v2).
//!
//! Each rank holds a contiguous layer range; rank 0 holds the LAST layers (mlx_lm's
//! `PipelineMixin`: rank `r` runs `[sum(s[r+1..]), sum(s[r..]))`). Every stage also loads the
//! shared tensors (embed, final norm, lm_head): mlx_lm keeps the whole outer model on every rank.
//! Per-layer bytes come from the checkpoint's safetensors headers (no tensor is read), grouped
//! by `model.layers.N.`.
//!
//! A stage's need: weights + its KV at the planned context (full-attention layers grow with the
//! context, sliding-window layers hold a fixed window) + one context's prompt cache
//! (`PROMPT_CACHE_CONTEXTS`) + one prefill chunk's attention scores when the stage holds a
//! full-attention layer (no fused SDPA for this head_dim: heads × chunk × context × act + the
//! mask). No `RUNTIME_OVERHEAD_RATIO` multiplier: measured 2026-10-26 on the manual 19/29 MiMo
//! run, the footprints were 68.7 / 100.7 GB against 68.28 / 100.31 GB of weights (1.006 /
//! 1.004), so a 1.10 multiplier would charge ~17 GB that the run never used.
//!
//! The split chosen maximises the minimum spare (budget − need) across ranks. The budget is the
//! ONE fit rule (`crate::fit`) at the config's reserve (`DistributedConfig::plan_reserve_ratio`).

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{bail, ensure, Context, Result};

use super::plan::{read_safetensors_header, RankPlan, TensorPrefill, KV_CACHE_STEP};
use super::{BATCH_KV_TRANSIENT_RATIO, PROMPT_CACHE_CONTEXTS, TENSOR_PREFILL_STEP};
use crate::fit::budget_bytes_with_reserve;

/// What the split arithmetic reads from a mimo_v2 checkpoint.
#[derive(Debug, Clone, PartialEq)]
pub struct LayerSplitFacts {
    /// Bytes of each layer's tensors, in layer order.
    pub layer_bytes: Vec<u64>,
    /// Whether each layer is full attention (else sliding window).
    pub full_attention: Vec<bool>,
    /// Tensors outside `model.layers.N.` (embed, norm, lm_head): every stage loads them.
    pub shared_bytes: u64,
    /// An omni checkpoint's vision tower (`vision_tower.*`): loaded by the stage holding the LAST
    /// layers (rank 0, which serves HTTP and encodes images: rank_media.py). The audio encoder,
    /// the speech embeddings and the MTP heads are never loaded by the split (the vendored
    /// mimo_v2 `sanitize` drops them), so they are charged nowhere.
    pub encoder_bytes: u64,
    /// KV one token costs one full-attention layer: kv_heads × (head_dim + v_head_dim) × act.
    pub full_kv_bytes_per_token: u64,
    /// What one sliding-window layer holds per row whatever the context:
    /// window × swa_kv_heads × (swa_head_dim + swa_v_head_dim) × act.
    pub swa_state_bytes: u64,
    /// Bytes per chunk token × context token of one full-attention layer's prefill scores:
    /// 2 × attention heads × act (the scores and their softmax) + 1 (the mask).
    pub pair_bytes: u64,
    pub max_position: u64,
}

fn cfg(config: &serde_json::Value, key: &str) -> Result<u64> {
    config
        .get(key)
        .and_then(serde_json::Value::as_u64)
        .with_context(|| format!("config.json has no integer `{key}`"))
}

/// Activation bytes from config.json's dtype (the KV and the scores are held in it).
fn act_bytes(config: &serde_json::Value) -> Result<u64> {
    let dtype = config
        .get("dtype")
        .or_else(|| config.get("torch_dtype"))
        .and_then(serde_json::Value::as_str)
        .context("config.json names no `dtype` / `torch_dtype`")?;
    Ok(match dtype {
        "bfloat16" | "float16" => 2,
        "float32" => 4,
        other => bail!("config.json dtype '{other}' is not bfloat16, float16 or float32"),
    })
}

/// What a checkpoint tensor costs the split. mlx-vlm-layout omni builds (EigenLabs
/// MiMo-V2.6-Flash-MOPD-MLX-4bit-mtp) name the text model `language_model.*`.
enum TensorRole {
    Layer(usize),
    Shared,
    VisionTower,
    NotLoaded,
}

fn tensor_role(name: &str) -> TensorRole {
    if name.starts_with("vision_tower.") || name.starts_with("visual.") {
        return TensorRole::VisionTower;
    }
    if name.starts_with("audio_encoder.") || name.starts_with("speech_embeddings.") {
        return TensorRole::NotLoaded;
    }
    let text = name.strip_prefix("language_model.").unwrap_or(name);
    if text.starts_with("model.mtp.") {
        return TensorRole::NotLoaded;
    }
    match layer_index(text) {
        Some(i) => TensorRole::Layer(i),
        None => TensorRole::Shared,
    }
}

/// `model.layers.N.` → N.
fn layer_index(name: &str) -> Option<usize> {
    let rest = name.strip_prefix("model.layers.")?;
    rest.split('.').next()?.parse().ok()
}

impl LayerSplitFacts {
    /// Builds the facts from config.json and per-tensor `(name, bytes)` pairs.
    pub fn from_parts(
        config: &serde_json::Value,
        tensors: impl IntoIterator<Item = (String, u64)>,
    ) -> Result<Self> {
        let layers = cfg(config, "num_hidden_layers")? as usize;
        let pattern = config
            .get("hybrid_layer_pattern")
            .and_then(serde_json::Value::as_array)
            .context("config.json has no `hybrid_layer_pattern`")?;
        ensure!(
            pattern.len() == layers,
            "config.json declares {layers} layers but hybrid_layer_pattern has {}",
            pattern.len()
        );
        // mimo_v2: 0 = full attention, 1 = sliding window.
        let full_attention = pattern
            .iter()
            .map(|v| match v.as_u64() {
                Some(0) => Ok(true),
                Some(1) => Ok(false),
                _ => bail!("hybrid_layer_pattern entry {v} is not 0 (full) or 1 (sliding window)"),
            })
            .collect::<Result<Vec<_>>>()?;
        let act = act_bytes(config)?;
        let mut layer_bytes = vec![0u64; layers];
        let mut shared_bytes = 0u64;
        let mut encoder_bytes = 0u64;
        for (name, bytes) in tensors {
            match tensor_role(&name) {
                TensorRole::Layer(i) => {
                    ensure!(
                        i < layers,
                        "tensor {name} names layer {i}; config.json declares {layers}"
                    );
                    layer_bytes[i] += bytes;
                }
                TensorRole::Shared => shared_bytes += bytes,
                TensorRole::VisionTower => encoder_bytes += bytes,
                TensorRole::NotLoaded => {}
            }
        }
        if let Some(empty) = layer_bytes.iter().position(|b| *b == 0) {
            bail!("the checkpoint's headers hold no tensor for layer {empty}");
        }
        Ok(Self {
            layer_bytes,
            full_attention,
            shared_bytes,
            encoder_bytes,
            full_kv_bytes_per_token: cfg(config, "num_key_value_heads")?
                * (cfg(config, "head_dim")? + cfg(config, "v_head_dim")?)
                * act,
            swa_state_bytes: cfg(config, "sliding_window_size")?
                * cfg(config, "swa_num_key_value_heads")?
                * (cfg(config, "swa_head_dim")? + cfg(config, "swa_v_head_dim")?)
                * act,
            // The scores AND their softmax are live at once in MLX 0.32's SDPA fallback (no fused
            // kernel for head_dim 192): measured 2026-10-27 on the 64k MiMo prompt, both ranks
            // held 9.5-10 GB past their resident bytes against a 5.1 GB one-copy workspace.
            pair_bytes: 2 * cfg(config, "num_attention_heads")? * act + 1,
            max_position: cfg(config, "max_position_embeddings")?,
        })
    }

    pub fn num_layers(&self) -> usize {
        self.layer_bytes.len()
    }

    /// The need of the stage holding `[start, end)` at `context` with prefill chunk `chunk`.
    pub fn stage_need(&self, start: usize, end: usize, context: u64, chunk: u64) -> StageNeed {
        self.stage_need_cached(start, end, context, chunk, PROMPT_CACHE_CONTEXTS)
    }

    /// `stage_need` with `cached` prompt-cache contexts charged beside the live one.
    pub fn stage_need_cached(
        &self,
        start: usize,
        end: usize,
        context: u64,
        chunk: u64,
        cached: u64,
    ) -> StageNeed {
        let width = context.div_ceil(KV_CACHE_STEP) * KV_CACHE_STEP;
        let full = self.full_attention[start..end]
            .iter()
            .filter(|f| **f)
            .count() as u64;
        let swa = (end - start) as u64 - full;
        let encoder = if end == self.layer_bytes.len() {
            self.encoder_bytes
        } else {
            0
        };
        let weights =
            self.shared_bytes + encoder + self.layer_bytes[start..end].iter().sum::<u64>();
        let state = width * full * self.full_kv_bytes_per_token + swa * self.swa_state_bytes;
        let workspace = if full > 0 {
            chunk * width * self.pair_bytes
        } else {
            0
        };
        StageNeed {
            full_layers: full,
            weights,
            state,
            // With no cached context charged, the row's own entry is still extracted (a copy of
            // its KV) when it ends, beside the live batch — measured 2026-10-27, MiMo 250k: one
            // KV more on both ranks at the end of the request. It is not live with the prefill
            // workspace, so only the part of it the workspace does not already cover is charged.
            prompt_cache: if cached == 0 {
                state.saturating_sub(workspace)
            } else {
                cached * state
            },
            workspace,
        }
    }
}

/// Reads the facts from `model_dir`: config.json plus every `model*.safetensors` header.
pub fn read_layer_split_facts(model_dir: &Path) -> Result<LayerSplitFacts> {
    let config: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(model_dir.join("config.json"))
            .with_context(|| format!("reading {}/config.json", model_dir.display()))?,
    )?;
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
    let mut tensors = Vec::new();
    for shard in &shards {
        for (name, entry) in read_safetensors_header(shard)? {
            if name == "__metadata__" {
                continue;
            }
            let offsets = entry
                .get("data_offsets")
                .and_then(serde_json::Value::as_array)
                .and_then(|o| Some((o.first()?.as_u64()?, o.get(1)?.as_u64()?)))
                .with_context(|| format!("{}: {name} has no data_offsets", shard.display()))?;
            tensors.push((name, offsets.1 - offsets.0));
        }
    }
    LayerSplitFacts::from_parts(&config, tensors)
}

/// One stage's charge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StageNeed {
    pub full_layers: u64,
    pub weights: u64,
    pub state: u64,
    pub prompt_cache: u64,
    pub workspace: u64,
}

impl StageNeed {
    pub fn resident(&self) -> u64 {
        self.weights + self.state + self.prompt_cache
    }

    pub fn total(&self) -> u64 {
        self.resident() + self.workspace
    }
}

/// One node's measured memory, in rank order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SplitNode {
    pub name: String,
    pub available: u64,
    pub total: u64,
    pub ceiling: u64,
}

impl SplitNode {
    pub fn budget(&self, reserve_ratio: f64) -> u64 {
        budget_bytes_with_reserve(self.available, self.total, self.ceiling, reserve_ratio)
    }
}

/// The split the planner chose.
#[derive(Debug, Clone, PartialEq)]
pub struct LayerSplit {
    /// Layers per rank, rank 0 first (rank 0 holds the LAST layers): mlx_lm's `split`.
    pub layers: Vec<u32>,
    /// Full-attention layers per rank, rank 0 first (the rest are sliding window).
    pub full_layers: Vec<u32>,
    /// Each rank's plan (`layer_start..layer_end`, bytes, budget).
    pub plans: Vec<RankPlan>,
    pub context: u64,
    /// The prefill chunk every rank takes (`--prefill-step-size`): the hidden states pass rank to
    /// rank each step, so it is one figure.
    pub chunk: u64,
    pub reserve_ratio: f64,
    /// Why this split, in words (the max-min-spare sentence the UI shows).
    pub reason: String,
}

/// Every contiguous split of `layers` into `ranks` parts of at least one layer.
fn compositions(layers: usize, ranks: usize) -> Vec<Vec<usize>> {
    if ranks == 1 {
        return vec![vec![layers]];
    }
    let mut out = Vec::new();
    for first in 1..=layers.saturating_sub(ranks - 1) {
        for mut rest in compositions(layers - first, ranks - 1) {
            rest.insert(0, first);
            out.push(rest);
        }
    }
    out
}

/// `[start, end)` of each rank for `split` (rank 0 = the last layers).
fn ranges(split: &[usize]) -> Vec<(usize, usize)> {
    (0..split.len())
        .map(|r| {
            let start: usize = split[r + 1..].iter().sum();
            (start, start + split[r])
        })
        .collect()
}

/// The best split at `context` and `chunk` under `reserve_ratio`: (split, min spare, spares).
fn best_split(
    facts: &LayerSplitFacts,
    nodes: &[SplitNode],
    reserve_ratio: f64,
    context: u64,
    chunk: u64,
    cached: u64,
) -> (Vec<usize>, i128, Vec<i128>) {
    let spares_of = |split: &[usize]| -> Vec<i128> {
        ranges(split)
            .iter()
            .zip(nodes)
            .map(|((start, end), node)| {
                node.budget(reserve_ratio) as i128
                    - facts
                        .stage_need_cached(*start, *end, context, chunk, cached)
                        .total() as i128
            })
            .collect()
    };
    let mut best: Option<(Vec<usize>, i128, Vec<i128>)> = None;
    for split in compositions(facts.num_layers(), nodes.len()) {
        let spares = spares_of(&split);
        let min = *spares.iter().min().unwrap_or(&i128::MIN);
        if best.as_ref().is_none_or(|(_, m, _)| min > *m) {
            best = Some((split, min, spares));
        }
    }
    best.unwrap_or_default()
}

/// The highest reserve (to 0.1% of RAM) at which some split fits, searched below `from`:
/// `None` when none fits even with no reserve.
fn fitting_reserve(
    facts: &LayerSplitFacts,
    nodes: &[SplitNode],
    from: f64,
    context: u64,
    chunk: u64,
    cached: u64,
) -> Option<(f64, Vec<usize>, Vec<i128>)> {
    // ratio: the search's resolution, 0.1% of a node's RAM.
    let step = 0.001;
    let mut ratio = (from / step).floor() * step;
    while ratio >= 0.0 {
        let (split, min, spares) = best_split(facts, nodes, ratio, context, chunk, cached);
        if min >= 0 {
            return Some((ratio, split, spares));
        }
        ratio -= step;
    }
    None
}

fn gb(bytes: i128) -> String {
    format!("{:.2} GB", bytes as f64 / 1e9)
}

/// The planner's refusal when no split fits at the reserve in force: the sentence with every
/// node's numbers, and the highest reserve a config may set (at least the WARN floor) at which a
/// split fits, so the app can offer it without parsing the sentence.
#[derive(Debug, Clone, PartialEq)]
pub struct SplitRefusal {
    pub message: String,
    pub fitting_reserve_ratio: Option<f64>,
}

impl std::fmt::Display for SplitRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for SplitRefusal {}

/// Plans the split. `context`: the requested context, `None` = the largest the chosen split
/// fits. `warn_floor`: the lowest reserve a config may choose (the watchdog WARN reserve), named
/// in the refusal so the owner sees whether an override can make it fit.
pub fn plan_layer_split(
    facts: &LayerSplitFacts,
    nodes: &[SplitNode],
    reserve_ratio: f64,
    warn_floor: f64,
    context: Option<u64>,
) -> Result<LayerSplit> {
    ensure!(
        nodes.len() >= 2 && nodes.len() <= facts.num_layers(),
        "{} nodes cannot split {} layers (each rank holds at least one)",
        nodes.len(),
        facts.num_layers()
    );
    if let Some(requested) = context {
        ensure!(
            requested <= facts.max_position,
            "requested context {requested} exceeds the model's max_position_embeddings {}",
            facts.max_position
        );
    }
    let select_context = context.unwrap_or(KV_CACHE_STEP).max(1);
    // A requested context that no split fits with a KV-step chunk may still fit with a smaller
    // one: the scores workspace is chunk x width x pair, so halving the chunk halves the largest
    // term at a long context (MiMo at 360k: ~23.7 GB at 256 tokens, ~5.9 GB at 64).
    // When no chunk fits with a cached prompt context beside the live one, the plan charges
    // none: the prompt cache then holds the live request's own entry only, and the reason says
    // a follow-up reads its prompt again (MiMo at 300k-350k on 96 + 128 GB, 2026-10-27).
    let (select_chunk, cached) = [PROMPT_CACHE_CONTEXTS, 0]
        .into_iter()
        .flat_map(|cached| prefill_chunk_candidates().map(move |chunk| (chunk, cached)))
        .find(|(chunk, cached)| {
            best_split(facts, nodes, reserve_ratio, select_context, *chunk, *cached).1 >= 0
        })
        // Nothing fits: the refusal names the smallest plan (smallest chunk, no cached context).
        .unwrap_or((MIN_PREFILL_CHUNK, 0));
    let (split, min_spare, spares) = best_split(
        facts,
        nodes,
        reserve_ratio,
        select_context,
        select_chunk,
        cached,
    );
    if min_spare < 0 {
        let per_node = nodes
            .iter()
            .zip(ranges(&split))
            .zip(&spares)
            .map(|((node, (start, end)), spare)| {
                format!(
                    "{} layers {start}-{} need {} against budget {} (available {} − {:.1}% of {} \
                     RAM, GPU ceiling {}), spare {}",
                    node.name,
                    end - 1,
                    gb(facts
                        .stage_need_cached(start, end, select_context, select_chunk, cached)
                        .total() as i128),
                    gb(node.budget(reserve_ratio) as i128),
                    gb(node.available as i128),
                    reserve_ratio * 100.0,
                    gb(node.total as i128),
                    gb(node.ceiling as i128),
                    gb(*spare),
                )
            })
            .collect::<Vec<_>>()
            .join("; ");
        let found = fitting_reserve(
            facts,
            nodes,
            reserve_ratio,
            select_context,
            select_chunk,
            cached,
        );
        // ratio: the search steps by 0.1% of RAM, so the named reserve is rounded to that step.
        let settable = found
            .as_ref()
            .map(|(ratio, _, _)| (ratio * 1000.0).round() / 1000.0)
            .filter(|ratio| *ratio >= warn_floor);
        let fit = match found {
            Some((ratio, fit_split, fit_spares)) => format!(
                "a split fits at reserve {:.1}% ({} layers, spare {}){}",
                ratio * 100.0,
                fit_split
                    .iter()
                    .map(usize::to_string)
                    .collect::<Vec<_>>()
                    .join("/"),
                fit_spares
                    .iter()
                    .map(|s| gb(*s))
                    .collect::<Vec<_>>()
                    .join(" / "),
                if ratio >= warn_floor {
                    format!(
                        ": set plan_reserve_ratio between {warn_floor} and {ratio:.3} to plan there"
                    )
                } else {
                    format!(
                        ", below the watchdog WARN reserve {:.1}%: no override can make it fit",
                        warn_floor * 100.0
                    )
                }
            ),
            None => "no split fits even with no reserve".to_string(),
        };
        return Err(anyhow::Error::new(SplitRefusal {
            message: format!(
                "no layer split fits at reserve {:.1}% of RAM: the best ({} layers, rank 0 = the \
                 last) leaves {per_node}. {fit}",
                reserve_ratio * 100.0,
                split
                    .iter()
                    .map(usize::to_string)
                    .collect::<Vec<_>>()
                    .join("/"),
            ),
            fitting_reserve_ratio: settable,
        }));
    }

    let stages = ranges(&split);
    let planned_context = match context {
        Some(requested) => requested,
        None => stages
            .iter()
            .zip(nodes)
            .map(|((start, end), node)| {
                max_context(facts, *start, *end, node.budget(reserve_ratio))
            })
            .min()
            .unwrap_or(0),
    };
    ensure!(
        planned_context >= KV_CACHE_STEP,
        "the split fits no context of at least {KV_CACHE_STEP} tokens"
    );
    let width = planned_context.div_ceil(KV_CACHE_STEP) * KV_CACHE_STEP;
    let chunk = stages
        .iter()
        .zip(nodes)
        .filter_map(|((start, end), node)| {
            let need = facts.stage_need_cached(*start, *end, planned_context, 0, cached);
            // With no cached context the row's end-of-request copy is not live with the
            // workspace: the chunk may use the room beside weights + live KV (the copy's own
            // fit is the `fits` check below).
            let resident = if cached == 0 {
                need.resident() - need.prompt_cache
            } else {
                need.resident()
            };
            (need.full_layers > 0).then(|| {
                node.budget(reserve_ratio).saturating_sub(resident) / (width * facts.pair_bytes)
            })
        })
        .min()
        .unwrap_or(TENSOR_PREFILL_STEP)
        .min(TENSOR_PREFILL_STEP);
    let chunk = prefill_chunk_candidates()
        .find(|c| *c <= chunk)
        .map(|c| {
            if c == KV_CACHE_STEP {
                chunk / KV_CACHE_STEP * KV_CACHE_STEP
            } else {
                c
            }
        })
        .unwrap_or(0);
    ensure!(
        chunk >= MIN_PREFILL_CHUNK,
        "context {planned_context} leaves a rank no room for a {MIN_PREFILL_CHUNK}-token prefill chunk"
    );

    let needs: Vec<StageNeed> = stages
        .iter()
        .map(|(start, end)| facts.stage_need_cached(*start, *end, planned_context, chunk, cached))
        .collect();
    let kv_per_token = needs
        .iter()
        .map(|n| n.full_layers * facts.full_kv_bytes_per_token)
        .max()
        .unwrap_or(0);
    let sequence_state = stages
        .iter()
        .zip(&needs)
        .map(|((start, end), n)| ((end - start) as u64 - n.full_layers) * facts.swa_state_bytes)
        .max()
        .unwrap_or(0);
    let workspace = needs
        .iter()
        .filter(|n| n.full_layers > 0)
        .map(|n| n.workspace)
        .min()
        .unwrap_or(0);
    // The largest chunk a step takes is the tensor runner's: the rank shrinks each step to the
    // workspace at the width it reaches (rank_prefill.py `prefill_chunk`), so a prompt shorter
    // than the planned context reads in larger chunks inside the same scores bound. Measured
    // 2026-10-27 on MiMo's first 6 layers (scratchpad mimo10/mini.py, 32k tokens): 256-token
    // chunks 16.2 s, 1,024-token 11.7 s, 64-token 19.8 s.
    let prefill = TensorPrefill {
        step: TENSOR_PREFILL_STEP.max(chunk),
        workspace_bytes: workspace,
        pair_bytes: facts.pair_bytes,
        kv_bytes_per_token: kv_per_token,
        sequence_state_bytes: sequence_state,
        batch_transient_ratio: BATCH_KV_TRANSIENT_RATIO,
    };
    let smallest_entry = (KV_CACHE_STEP * kv_per_token + sequence_state).max(1);
    let plans: Vec<RankPlan> = stages
        .iter()
        .zip(&needs)
        .zip(nodes)
        .map(|(((start, end), need), node)| {
            let budget = node.budget(reserve_ratio);
            RankPlan {
                layer_start: *start as u32,
                layer_end: *end as u32,
                shard_index: None,
                shard_count: None,
                weights_bytes: need.weights,
                state_bytes: need.state,
                workspace_bytes: need.workspace,
                prefill: Some(prefill),
                prefill_step: prefill.step,
                prompt_cache_bytes: need.prompt_cache,
                prompt_cache_entries: (need.state + need.prompt_cache) / smallest_entry,
                planned_bytes: need.resident(),
                with_overhead_bytes: need.total(),
                budget_bytes: budget,
                fits: need.total() <= budget,
                attention_scores_bytes: None,
            }
        })
        .collect();
    let spares = plans
        .iter()
        .map(|p| gb(p.budget_bytes as i128 - p.with_overhead_bytes as i128))
        .collect::<Vec<_>>()
        .join(" / ");
    let layers: Vec<u32> = split.iter().map(|n| *n as u32).collect();
    let reason =
        format!(
        "split {} (rank 0 = the last layers) leaves the most memory on the tightest Mac of all {} \
         contiguous splits: spare {spares} at context {planned_context} with a {chunk}-token \
         prefill chunk, reserve {:.1}% of RAM{}",
        layers.iter().map(u32::to_string).collect::<Vec<_>>().join("/"),
        compositions(facts.num_layers(), nodes.len()).len(),
        reserve_ratio * 100.0,
        if cached == 0 {
            "; no cached prompt context fits beside the live one, so a follow-up request reads \
             its whole prompt again"
        } else {
            ""
        },
    );
    Ok(LayerSplit {
        layers,
        full_layers: needs.iter().map(|n| n.full_layers as u32).collect(),
        plans,
        context: planned_context,
        chunk,
        reserve_ratio,
        reason,
    })
}

/// The smallest prefill chunk the planner hands a rank at the full context: a sixteenth KV
/// step. The rank takes larger chunks while the prompt is shorter (rank_prefill.py), so the
/// floor only paces the last stretch of a very long prompt; 64-token chunks read MiMo's first 6
/// layers at 0.83x the 256-token rate (measured 2026-10-27), and MiMo at 350k on 96 + 128 GB
/// with 8 GB left under each GPU ceiling fits only at 16.
pub const MIN_PREFILL_CHUNK: u64 = KV_CACHE_STEP / 16;

/// The chunks the planner tries, largest first: the KV step, then halvings to
/// `MIN_PREFILL_CHUNK`.
fn prefill_chunk_candidates() -> impl Iterator<Item = u64> {
    std::iter::successors(Some(KV_CACHE_STEP), |c| Some(c / 2))
        .take_while(|c| *c >= MIN_PREFILL_CHUNK)
}

/// The largest context (a multiple of the KV step, at most `max_position`) the stage fits in
/// `budget` with a KV-step prefill chunk; 0 when not even its weights fit.
fn max_context(facts: &LayerSplitFacts, start: usize, end: usize, budget: u64) -> u64 {
    let base = facts.stage_need(start, end, 0, 0);
    let fixed = base.resident();
    let one_step = facts.stage_need(start, end, KV_CACHE_STEP, KV_CACHE_STEP);
    // Per KV step of width: the full layers' KV (live + cached) and one chunk's scores.
    let per_step = one_step.total() - fixed;
    let cap = facts.max_position / KV_CACHE_STEP * KV_CACHE_STEP;
    if budget < fixed {
        return 0;
    }
    if per_step == 0 {
        return cap;
    }
    ((budget - fixed) / per_step * KV_CACHE_STEP).min(cap)
}

/// `(name, bytes)` pairs for a byte table: `layers[i]` bytes under `model.layers.i.`, `shared`
/// under one outer tensor. For tests and fixtures.
pub fn tensors_from_table(layers: &[u64], shared: u64) -> BTreeMap<String, u64> {
    let mut out: BTreeMap<String, u64> = layers
        .iter()
        .enumerate()
        .map(|(i, b)| (format!("model.layers.{i}.weight"), *b))
        .collect();
    out.insert("model.embed_tokens.weight".to_string(), shared);
    out
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    const GIB: u64 = 1 << 30;

    /// The MiMo-V2.6-Flash-RL-mxfp4-q8 checkpoint's header bytes (2026-10-26, all 45 files).
    pub(crate) fn mimo_facts() -> LayerSplitFacts {
        let full = [0, 5, 11, 17, 23, 29, 35, 41, 47];
        let pattern: Vec<u64> = (0..48).map(|i| u64::from(!full.contains(&i))).collect();
        let layers: Vec<u64> = (0..48)
            .map(|i| match i {
                0 => 308_625_408,
                i if full.contains(&i) => 3_519_366_144,
                _ => 3_524_936_832,
            })
            .collect();
        let config = serde_json::json!({
            "num_hidden_layers": 48, "hybrid_layer_pattern": pattern,
            "num_attention_heads": 64, "num_key_value_heads": 4, "head_dim": 192,
            "v_head_dim": 128, "swa_num_key_value_heads": 8, "swa_head_dim": 192,
            "swa_v_head_dim": 128, "sliding_window_size": 128,
            "max_position_embeddings": 1_048_576, "dtype": "bfloat16",
        });
        LayerSplitFacts::from_parts(&config, tensors_from_table(&layers, 1_328_029_696)).unwrap()
    }

    /// workhorse (M3 Ultra 96 GiB) as rank 0, the mule (128 GiB) as rank 1: measured available
    /// and GPU ceilings from the manual run.
    pub(crate) fn nodes() -> Vec<SplitNode> {
        vec![
            SplitNode {
                name: "workhorse".to_string(),
                available: 76_700_000_000,
                total: 96 * GIB,
                ceiling: 83_490_000_000,
            },
            SplitNode {
                name: "mule".to_string(),
                available: 111_000_000_000,
                total: 128 * GIB,
                ceiling: 115_450_000_000,
            },
        ]
    }

    #[test]
    fn facts_read_the_byte_table() {
        let f = mimo_facts();
        assert_eq!(f.full_kv_bytes_per_token, 2560);
        assert_eq!(f.swa_state_bytes, 655_360);
        assert_eq!(f.pair_bytes, 257);
        assert_eq!(f.full_attention.iter().filter(|x| **x).count(), 9);
        assert_eq!(f.shared_bytes, 1_328_029_696);
    }

    #[test]
    fn the_shared_reserve_refuses_naming_the_numbers_and_the_reserve_that_fits() {
        let refusal = plan_layer_split(
            &mimo_facts(),
            &nodes(),
            crate::fit::AVAILABLE_MARGIN_RATIO,
            super::super::WATCHDOG_WARN_RESERVE_RATIO,
            None,
        )
        .unwrap_err();
        assert_eq!(
            refusal
                .downcast_ref::<SplitRefusal>()
                .and_then(|r| r.fitting_reserve_ratio),
            Some(0.077)
        );
        let err = refusal.to_string();
        assert!(err.contains("no layer split fits at reserve 9.3%"), "{err}");
        assert!(err.contains("workhorse layers 29-47"), "{err}");
        assert!(err.contains("budget 67.11 GB"), "{err}");
        assert!(err.contains("budget 98.22 GB"), "{err}");
        assert!(
            err.contains("a split fits at reserve 7.7% (19/29 layers"),
            "{err}"
        );
        assert!(
            err.contains("set plan_reserve_ratio between 0.05 and 0.077"),
            "{err}"
        );
    }

    /// EigenLabs/MiMo-V2.6-Flash-MOPD-MLX-4bit-mtp (header bytes 2026-10-27): the same text
    /// tensors as the RL mxfp4-q8 build under `language_model.`, plus vision_tower 1.457 GB,
    /// audio_encoder + speech_embeddings 0.277 GB and the MTP heads 1.976 GB.
    pub(crate) fn omni_tensors() -> BTreeMap<String, u64> {
        let text = tensors_from_table(&mimo_facts().layer_bytes, mimo_facts().shared_bytes);
        let mut out: BTreeMap<String, u64> = text
            .into_iter()
            .map(|(k, v)| (format!("language_model.{k}"), v))
            .collect();
        out.insert(
            "vision_tower.blocks.0.attn.qkv.weight".to_string(),
            1_457_000_000,
        );
        out.insert(
            "audio_encoder.projection.mlp.0.weight".to_string(),
            225_000_000,
        );
        out.insert("speech_embeddings.0.weight".to_string(), 52_000_000);
        out.insert(
            "language_model.model.mtp.layers.0.mlp.weight".to_string(),
            1_976_000_000,
        );
        out
    }

    fn mimo_config() -> serde_json::Value {
        let full = [0, 5, 11, 17, 23, 29, 35, 41, 47];
        let pattern: Vec<u64> = (0..48).map(|i| u64::from(!full.contains(&i))).collect();
        serde_json::json!({
            "num_hidden_layers": 48, "hybrid_layer_pattern": pattern,
            "num_attention_heads": 64, "num_key_value_heads": 4, "head_dim": 192,
            "v_head_dim": 128, "swa_num_key_value_heads": 8, "swa_head_dim": 192,
            "swa_v_head_dim": 128, "sliding_window_size": 128,
            "max_position_embeddings": 1_048_576, "dtype": "bfloat16",
        })
    }

    /// The largest context any split fits with the smallest prefill chunk, under the 8 GB GPU
    /// headroom the 275k live run used (ceilings 83.49 / 115.45 GB less 8).
    fn max_split_context(facts: &LayerSplitFacts) -> (u64, Vec<usize>) {
        let nodes: Vec<SplitNode> = [("w", 83_494_174_720u64, 96), ("m", 115_450_000_000, 128)]
            .iter()
            .map(|(n, c, t)| SplitNode {
                name: n.to_string(),
                available: c - 8_000_000_000,
                total: t * GIB,
                ceiling: c - 8_000_000_000,
            })
            .collect();
        let mut best = (0, vec![]);
        let mut c = KV_CACHE_STEP;
        loop {
            let (split, min, _) = best_split(facts, &nodes, 0.0, c, MIN_PREFILL_CHUNK, 1);
            if min < 0 {
                return best;
            }
            best = (c, split);
            c += KV_CACHE_STEP;
        }
    }

    #[test]
    fn an_omni_checkpoint_charges_the_vision_tower_to_rank_0_only_and_loads_no_audio_or_mtp() {
        let text = mimo_facts();
        let omni = LayerSplitFacts::from_parts(&mimo_config(), omni_tensors()).unwrap();
        assert_eq!(omni.layer_bytes, text.layer_bytes);
        assert_eq!(omni.shared_bytes, text.shared_bytes);
        assert_eq!(omni.encoder_bytes, 1_457_000_000);
        let last = omni.stage_need(29, 48, 0, 0).weights - text.stage_need(29, 48, 0, 0).weights;
        let first = omni.stage_need(0, 29, 0, 0).weights - text.stage_need(0, 29, 0, 0).weights;
        assert_eq!((last, first), (1_457_000_000, 0));

        // The context budget (Session 12): text-only, vision on rank 0, everything loaded.
        let (text_ctx, text_split) = max_split_context(&text);
        let (omni_ctx, omni_split) = max_split_context(&omni);
        let mut all = omni.clone();
        all.encoder_bytes += 225_000_000 + 52_000_000 + 1_976_000_000;
        let (all_ctx, all_split) = max_split_context(&all);
        println!(
            "max context under 8 GB headroom: text {text_ctx} {text_split:?}, vision on rank 0 \
             {omni_ctx} {omni_split:?}, vision+audio+MTP {all_ctx} {all_split:?}"
        );
        assert!(text_ctx >= omni_ctx && omni_ctx >= all_ctx);
    }

    #[test]
    fn the_warn_floor_reserve_splits_19_on_rank0_and_29_on_rank1() {
        let warn = super::super::WATCHDOG_WARN_RESERVE_RATIO;
        let split = plan_layer_split(&mimo_facts(), &nodes(), warn, warn, None).unwrap();
        assert_eq!(split.layers, vec![19, 29]);
        assert_eq!(
            (split.plans[0].layer_start, split.plans[0].layer_end),
            (29, 48)
        );
        assert_eq!(
            (split.plans[1].layer_start, split.plans[1].layer_end),
            (0, 29)
        );
        assert_eq!(split.plans[0].weights_bytes, 68_279_546_752);
        assert_eq!(split.plans[1].weights_bytes, 100_312_603_648);
        assert!(split.plans.iter().all(|p| p.fits));
        assert_eq!(split.context, 37_632);
        assert_eq!(split.chunk, 256);
        assert!(split.reason.contains("split 19/29"), "{}", split.reason);
    }

    #[test]
    fn a_requested_context_plans_at_it_and_shares_the_prefill_figures() {
        let warn = super::super::WATCHDOG_WARN_RESERVE_RATIO;
        let split = plan_layer_split(&mimo_facts(), &nodes(), warn, warn, Some(32_768)).unwrap();
        assert_eq!(split.layers, vec![19, 29]);
        assert_eq!(split.context, 32_768);
        assert!(split.chunk >= KV_CACHE_STEP && split.chunk <= TENSOR_PREFILL_STEP);
        assert_eq!(split.plans[0].prefill, split.plans[1].prefill);
        // rank 1 holds 5 full-attention layers (0, 5, 11, 17, 23), rank 0 four.
        assert_eq!(split.plans[0].prefill.unwrap().kv_bytes_per_token, 5 * 2560);
    }

    #[test]
    fn a_context_past_the_kv_step_chunk_plans_with_a_smaller_chunk() {
        let warn = super::super::WATCHDOG_WARN_RESERVE_RATIO;
        let facts = mimo_facts();
        let nodes = nodes();
        let past = (37_632..)
            .step_by(KV_CACHE_STEP as usize)
            .find(|c| best_split(&facts, &nodes, warn, *c, KV_CACHE_STEP, 1).1 < 0)
            .unwrap();
        assert!(best_split(&facts, &nodes, warn, past, MIN_PREFILL_CHUNK, 1).1 >= 0);
        let split = plan_layer_split(&facts, &nodes, warn, warn, Some(past)).unwrap();
        assert_eq!(split.context, past);
        assert!(
            split.chunk >= MIN_PREFILL_CHUNK && split.chunk < KV_CACHE_STEP,
            "{}",
            split.chunk
        );
        assert!(split.chunk.is_power_of_two());
        assert!(split.plans.iter().all(|p| p.fits));
        let prefill = split.plans[0].prefill.unwrap();
        // The largest step stays the tensor runner's; the workspace bounds it at each width.
        assert_eq!(prefill.step, TENSOR_PREFILL_STEP);
        assert_eq!(split.plans[0].prefill_step, TENSOR_PREFILL_STEP);
        let width = past.div_ceil(KV_CACHE_STEP) * KV_CACHE_STEP;
        assert_eq!(
            prefill.workspace_bytes,
            split.chunk * width * facts.pair_bytes
        );
        assert!(
            split.reason.contains(&format!("{}-token", split.chunk)),
            "{}",
            split.reason
        );
    }

    #[test]
    fn a_context_past_every_chunk_with_a_cached_context_plans_without_one() {
        let warn = super::super::WATCHDOG_WARN_RESERVE_RATIO;
        let facts = mimo_facts();
        let nodes = nodes();
        let past = (37_632..)
            .step_by(KV_CACHE_STEP as usize)
            .find(|c| {
                best_split(&facts, &nodes, warn, *c, MIN_PREFILL_CHUNK, 1).1 < 0
                    && best_split(&facts, &nodes, warn, *c, MIN_PREFILL_CHUNK, 0).1 >= 0
            })
            .unwrap();
        let split = plan_layer_split(&facts, &nodes, warn, warn, Some(past)).unwrap();
        assert!(split
            .plans
            .iter()
            .all(|p| p.prompt_cache_bytes < p.state_bytes && p.fits));
        assert!(
            split.reason.contains("reads its whole prompt again"),
            "{}",
            split.reason
        );
    }

    #[test]
    fn the_gpu_headroom_lowers_the_ceiling_the_budget_is_capped_at() {
        let mut config = super::super::config::tests::two_mac_config();
        assert_eq!(config.planned_ceiling("w", 83_494_174_720), 83_494_174_720);
        config.gpu_headroom_gb = Some(8.0);
        assert_eq!(config.planned_ceiling("w", 83_494_174_720), 75_494_174_720);
        config.node_gpu_headroom_gb = Some([("w".to_string(), 16.0)].into());
        assert_eq!(config.planned_ceiling("w", 83_494_174_720), 67_494_174_720);
        assert_eq!(
            config.planned_ceiling("m", 115_000_000_000),
            107_000_000_000
        );
        config.node_gpu_headroom_gb = None;
        let node = SplitNode {
            name: "workhorse".to_string(),
            available: 90_000_000_000,
            total: 96 * GIB,
            ceiling: config.planned_ceiling("w", 83_494_174_720),
        };
        assert_eq!(node.budget(0.05), 75_494_174_720);
    }

    /// The workhorse's vm_stat page counts at 2026-10-31 ~23:30 (96 GiB, nothing loaded, kernel
    /// NORMAL, `memory_pressure` 85% free): free 66.6 GB, inactive 12.2 GB. Lines in vm_stat's own
    /// order and wording.
    fn workhorse_vm_stat(free: u64, inactive: u64) -> String {
        format!(
            "Mach Virtual Memory Statistics: (page size of 16384 bytes)
Pages free:                               {free}.
Pages active:                                 380827.
Pages inactive:                           {inactive}.
Pages speculative:                             29203.
Pages throttled:                                   0.
Pages wired down:                             865903.
Pages purgeable:                               38700.
File-backed pages:                            392401.
Anonymous pages:                              770769.
"
        )
    }

    /// Q-530 (2026-10-31): the MiMo split 18/30 at 262,144 could not mount because the plan read
    /// the workhorse through free + file cache only (69.9 GiB) and so offered 17/31, which puts
    /// rank 1 over its Metal limit. Read through the one `available` rule (inactive counted), the
    /// same node plans 18/30 at the requested 262,144; a genuinely tight node still refuses.
    #[test]
    fn tonight_s_workhorse_plans_mimo_18_30_at_262144_and_a_tight_node_still_refuses() {
        let omni = LayerSplitFacts::from_parts(&mimo_config(), omni_tensors()).unwrap();
        let warn = super::super::WATCHDOG_WARN_RESERVE_RATIO;
        let workhorse = |vm: &str| {
            let reading = super::super::probe::parse_vm_stat(vm, 96 * GIB).unwrap();
            SplitNode {
                name: "workhorse".to_string(),
                available: reading.available_bytes,
                total: 96 * GIB,
                ceiling: 83_490_000_000,
            }
        };
        let tonight = workhorse(&workhorse_vm_stat(4_151_703, 742_603));
        assert_eq!(
            tonight.available,
            (4_151_703 + 29_203 + 742_603 + 38_700) * 16_384,
            "free + speculative + inactive + purgeable"
        );
        let mut nodes = nodes();
        nodes[0] = tonight;
        let split = plan_layer_split(&omni, &nodes, warn, warn, Some(262_144)).unwrap();
        let spare: Vec<i128> = split
            .plans
            .iter()
            .map(|p| p.budget_bytes as i128 - p.with_overhead_bytes as i128)
            .collect();
        println!(
            "tonight: split {:?} context {} chunk {} rank0 {} of {} rank1 {} of {} spare {spare:?}",
            split.layers,
            split.context,
            split.chunk,
            split.plans[0].with_overhead_bytes,
            split.plans[0].budget_bytes,
            split.plans[1].with_overhead_bytes,
            split.plans[1].budget_bytes,
        );
        assert_eq!(split.layers, vec![18, 30], "{}", split.reason);
        assert_eq!(split.context, 262_144);
        assert!(split.plans.iter().all(|p| p.fits));

        // Little free and little inactive: the same 18/30 at 262,144 is refused, not squeezed in.
        nodes[0] = workhorse(&workhorse_vm_stat(3_600_000, 60_000));
        let refusal = plan_layer_split(&omni, &nodes, warn, warn, Some(262_144));
        assert!(refusal.is_err(), "{:?}", refusal.map(|s| s.layers));
    }

    #[test]
    fn a_context_no_chunk_fits_refuses_at_the_smallest_chunk() {
        let warn = super::super::WATCHDOG_WARN_RESERVE_RATIO;
        let err = plan_layer_split(&mimo_facts(), &nodes(), warn, warn, Some(1_000_000))
            .unwrap_err()
            .to_string();
        assert!(err.contains("no layer split fits"), "{err}");
    }

    #[test]
    fn halved_budgets_refuse_and_say_no_override_fits() {
        let half: Vec<SplitNode> = nodes()
            .into_iter()
            .map(|n| SplitNode {
                available: n.available / 2,
                ..n
            })
            .collect();
        let warn = super::super::WATCHDOG_WARN_RESERVE_RATIO;
        let err = plan_layer_split(&mimo_facts(), &half, warn, warn, None)
            .unwrap_err()
            .to_string();
        assert!(err.contains("no split fits even with no reserve"), "{err}");
        assert!(err.contains("workhorse layers"), "{err}");
    }

    #[test]
    fn the_mule_as_rank0_still_holds_the_last_layers() {
        let mut swapped = nodes();
        swapped.reverse();
        let warn = super::super::WATCHDOG_WARN_RESERVE_RATIO;
        let split = plan_layer_split(&mimo_facts(), &swapped, warn, warn, None).unwrap();
        assert_eq!(split.plans[0].layer_end, 48);
        assert_eq!(split.plans[1].layer_start, 0);
        assert_eq!(split.plans[0].layer_start, split.plans[1].layer_end);
        assert_eq!(split.layers.iter().sum::<u32>(), 48);
        assert!(split.layers[0] > split.layers[1], "{:?}", split.layers);
    }

    #[test]
    fn a_plan_reserve_below_the_warn_floor_or_above_the_default_is_a_config_error() {
        let mut config = super::super::config::tests::two_mac_config();
        config.plan_reserve_ratio = Some(0.04);
        let err = config.validate().unwrap_err().to_string();
        assert!(
            err.contains("plan_reserve_ratio 0.04 must be at least the watchdog WARN reserve"),
            "{err}"
        );
        config.plan_reserve_ratio = Some(0.093);
        assert!(config.validate().is_err());
        config.plan_reserve_ratio = Some(0.05);
        config.validate().unwrap();
        assert_eq!(
            config.plan_reserve(Some("mimo_v2")),
            (0.05, super::super::config::ReserveSource::User)
        );
    }

    #[test]
    fn mimo_v2_plans_at_its_own_reserve_unless_the_config_sets_one() {
        use super::super::config::ReserveSource;
        let mut config = super::super::config::tests::two_mac_config();
        assert_eq!(
            config.plan_reserve(Some("mimo_v2")),
            (0.05, ReserveSource::Model)
        );
        assert_eq!(
            config.plan_reserve(Some("qwen3_5")),
            (super::super::AVAILABLE_MARGIN_RATIO, ReserveSource::Default)
        );
        assert_eq!(
            config.plan_reserve(None),
            (super::super::AVAILABLE_MARGIN_RATIO, ReserveSource::Default)
        );
        // The model's reserve never goes under the WARN reserve in force.
        config.watchdog_warn_ratio = Some(0.06);
        config.watchdog_critical_ratio = Some(0.03);
        assert_eq!(
            config.plan_reserve(Some("mimo_v2")),
            (0.06, ReserveSource::Model)
        );
        config.plan_reserve_ratio = Some(0.07);
        assert_eq!(
            config.plan_reserve(Some("mimo_v2")),
            (0.07, ReserveSource::User)
        );
    }
}
