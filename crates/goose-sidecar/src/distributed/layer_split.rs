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
    /// KV one token costs one full-attention layer: kv_heads × (head_dim + v_head_dim) × act.
    pub full_kv_bytes_per_token: u64,
    /// What one sliding-window layer holds per row whatever the context:
    /// window × swa_kv_heads × (swa_head_dim + swa_v_head_dim) × act.
    pub swa_state_bytes: u64,
    /// Bytes per chunk token × context token of one full-attention layer's prefill scores:
    /// attention heads × act + 1 (the mask).
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
        for (name, bytes) in tensors {
            match layer_index(&name) {
                Some(i) => {
                    ensure!(
                        i < layers,
                        "tensor {name} names layer {i}; config.json declares {layers}"
                    );
                    layer_bytes[i] += bytes;
                }
                None => shared_bytes += bytes,
            }
        }
        if let Some(empty) = layer_bytes.iter().position(|b| *b == 0) {
            bail!("the checkpoint's headers hold no tensor for layer {empty}");
        }
        Ok(Self {
            layer_bytes,
            full_attention,
            shared_bytes,
            full_kv_bytes_per_token: cfg(config, "num_key_value_heads")?
                * (cfg(config, "head_dim")? + cfg(config, "v_head_dim")?)
                * act,
            swa_state_bytes: cfg(config, "sliding_window_size")?
                * cfg(config, "swa_num_key_value_heads")?
                * (cfg(config, "swa_head_dim")? + cfg(config, "swa_v_head_dim")?)
                * act,
            pair_bytes: cfg(config, "num_attention_heads")? * act + 1,
            max_position: cfg(config, "max_position_embeddings")?,
        })
    }

    pub fn num_layers(&self) -> usize {
        self.layer_bytes.len()
    }

    /// The need of the stage holding `[start, end)` at `context` with prefill chunk `chunk`.
    pub fn stage_need(&self, start: usize, end: usize, context: u64, chunk: u64) -> StageNeed {
        let width = context.div_ceil(KV_CACHE_STEP) * KV_CACHE_STEP;
        let full = self.full_attention[start..end]
            .iter()
            .filter(|f| **f)
            .count() as u64;
        let swa = (end - start) as u64 - full;
        let weights = self.shared_bytes + self.layer_bytes[start..end].iter().sum::<u64>();
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
            prompt_cache: PROMPT_CACHE_CONTEXTS * state,
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
) -> (Vec<usize>, i128, Vec<i128>) {
    let spares_of = |split: &[usize]| -> Vec<i128> {
        ranges(split)
            .iter()
            .zip(nodes)
            .map(|((start, end), node)| {
                node.budget(reserve_ratio) as i128
                    - facts.stage_need(*start, *end, context, chunk).total() as i128
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
) -> Option<(f64, Vec<usize>, Vec<i128>)> {
    // ratio: the search's resolution, 0.1% of a node's RAM.
    let step = 0.001;
    let mut ratio = (from / step).floor() * step;
    while ratio >= 0.0 {
        let (split, min, spares) = best_split(facts, nodes, ratio, context, KV_CACHE_STEP);
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
    let (split, min_spare, spares) =
        best_split(facts, nodes, reserve_ratio, select_context, KV_CACHE_STEP);
    if min_spare < 0 {
        let per_node =
            nodes
                .iter()
                .zip(ranges(&split))
                .zip(&spares)
                .map(|((node, (start, end)), spare)| {
                    format!(
                    "{} layers {start}-{} need {} against budget {} (available {} − {:.1}% of {} \
                     RAM, GPU ceiling {}), spare {}",
                    node.name,
                    end - 1,
                    gb(facts.stage_need(start, end, select_context, KV_CACHE_STEP).total() as i128),
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
        let fit = match fitting_reserve(facts, nodes, reserve_ratio, select_context) {
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
        bail!(
            "no layer split fits at reserve {:.1}% of RAM: the best ({} layers, rank 0 = the last) \
             leaves {per_node}. {fit}",
            reserve_ratio * 100.0,
            split.iter().map(usize::to_string).collect::<Vec<_>>().join("/"),
        );
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
            let need = facts.stage_need(*start, *end, planned_context, 0);
            (need.full_layers > 0).then(|| {
                node.budget(reserve_ratio).saturating_sub(need.resident())
                    / (width * facts.pair_bytes)
            })
        })
        .min()
        .unwrap_or(TENSOR_PREFILL_STEP)
        .min(TENSOR_PREFILL_STEP)
        / KV_CACHE_STEP
        * KV_CACHE_STEP;
    ensure!(
        chunk >= KV_CACHE_STEP,
        "context {planned_context} leaves a rank no room for a {KV_CACHE_STEP}-token prefill chunk"
    );

    let needs: Vec<StageNeed> = stages
        .iter()
        .map(|(start, end)| facts.stage_need(*start, *end, planned_context, chunk))
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
    let prefill = TensorPrefill {
        step: chunk,
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
                prefill_step: chunk,
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
         prefill chunk, reserve {:.1}% of RAM",
        layers.iter().map(u32::to_string).collect::<Vec<_>>().join("/"),
        compositions(facts.num_layers(), nodes.len()).len(),
        reserve_ratio * 100.0,
    );
    Ok(LayerSplit {
        layers,
        plans,
        context: planned_context,
        chunk,
        reserve_ratio,
        reason,
    })
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
mod tests {
    use super::*;

    const GIB: u64 = 1 << 30;

    /// The MiMo-V2.6-Flash-RL-mxfp4-q8 checkpoint's header bytes (2026-10-26, all 45 files).
    fn mimo_facts() -> LayerSplitFacts {
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
    fn nodes() -> Vec<SplitNode> {
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
        assert_eq!(f.pair_bytes, 129);
        assert_eq!(f.full_attention.iter().filter(|x| **x).count(), 9);
        assert_eq!(f.shared_bytes, 1_328_029_696);
    }

    #[test]
    fn the_shared_reserve_refuses_naming_the_numbers_and_the_reserve_that_fits() {
        let err = plan_layer_split(
            &mimo_facts(),
            &nodes(),
            crate::fit::AVAILABLE_MARGIN_RATIO,
            super::super::WATCHDOG_WARN_RESERVE_RATIO,
            None,
        )
        .unwrap_err()
        .to_string();
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
        assert_eq!(split.context, 60_672);
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
        assert_eq!(config.plan_reserve_ratio(), 0.05);
    }
}
