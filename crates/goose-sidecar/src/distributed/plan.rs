//! Per-rank memory arithmetic. The tensor split (qwen3_5 under `mlx_lm.server`) is computed here
//! from the checkpoint's own config and safetensors headers — no tensor is read — and its verdict
//! is `planned × RUNTIME_OVERHEAD_RATIO ≤ min(available − RAM × AVAILABLE_MARGIN_RATIO, GPU
//! ceiling)`, the ceiling being the node's own Metal `max_recommended_working_set_size`.
//!
//! The qwen4_exp pipeline split is the fork planner's (`pipeline_qwen4 plan --json`), read, never
//! re-derived: one planner decides the layer ranges, the bytes, the budget (the same rule,
//! `min(available − RAM × available_margin, ceiling)`, its ratio in the JSON) and the verdict —
//! the same planner the fork's loader re-runs on every rank before it loads.

use std::io::Read;
use std::path::Path;

use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};

use super::{PROMPT_CACHE_CONTEXTS, RUNTIME_OVERHEAD_RATIO};

/// mlx_lm `KVCache.step`: the KV buffer grows in 256-token blocks (models/cache.py), so a context
/// costs its size rounded up to the step. An algorithm constant of the engine, not a policy.
const KV_CACHE_STEP: u64 = 256;
/// gated_delta.py allocates the recurrent state in float32.
const RECURRENT_STATE_BYTES: u64 = 4;

/// What one rank will hold. For the tensor split every rank holds every layer's shard
/// (`shard_index` of `shard_count`); for the pipeline split a rank holds `[layer_start, layer_end)`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RankPlan {
    pub layer_start: u32,
    pub layer_end: u32,
    pub shard_index: Option<u32>,
    pub shard_count: Option<u32>,
    pub weights_bytes: u64,
    /// KV + recurrent state for the allowed context (pipeline: the fork's "state").
    pub state_bytes: u64,
    /// The fork's prefill workspace (pipeline only: 49× the chunk's stream bytes per layer,
    /// measured; 0 for tensor, where the measured overhead ratio carries the transients).
    pub workspace_bytes: u64,
    /// The prompt cache's byte bound on this rank (tensor: `--prompt-cache-bytes`).
    pub prompt_cache_bytes: u64,
    pub planned_bytes: u64,
    /// What is compared with the budget: tensor `planned × RUNTIME_OVERHEAD_RATIO`; pipeline the
    /// fork's total as planned (no multiplier — see `RUNTIME_OVERHEAD_RATIO`).
    pub with_overhead_bytes: u64,
    pub budget_bytes: u64,
    pub fits: bool,
}

/// The tensor runner's per-rank budget is the ONE fit rule's (`crate::fit`); the pipeline
/// runner reads the fork's, built by the same rule.
pub use crate::fit::budget_bytes;

pub fn with_overhead(planned_bytes: u64) -> u64 {
    (planned_bytes as f64 * RUNTIME_OVERHEAD_RATIO).ceil() as u64
}

/// What the tensor arithmetic needs from a qwen3_5 checkpoint.
#[derive(Debug, Clone, PartialEq)]
pub struct TensorModelFacts {
    pub num_layers: u64,
    pub full_attention_layers: u64,
    pub linear_layers: u64,
    pub kv_heads: u64,
    pub head_dim: u64,
    pub linear_key_heads: u64,
    pub linear_value_heads: u64,
    pub linear_key_head_dim: u64,
    pub linear_value_head_dim: u64,
    pub conv_kernel: u64,
    pub max_position: u64,
    /// Bytes of one activation element (the embedding's scales dtype — the stream's dtype).
    pub act_bytes: u64,
    /// Bytes split across ranks (every decoder layer).
    pub sharded_bytes: u64,
    /// Bytes every rank holds whole (embed_tokens, lm_head — measured unsharded in STEP1b).
    pub replicated_bytes: u64,
    /// Bytes the loader drops (vision tower, MTP head).
    pub excluded_bytes: u64,
}

fn config_u64(config: &serde_json::Value, key: &str) -> Result<u64> {
    config
        .get(key)
        .and_then(serde_json::Value::as_u64)
        .with_context(|| format!("config.json has no integer `{key}`"))
}

/// The checkpoint's `model_type` (top level, as mlx_lm dispatches on it).
pub fn read_model_type(model_dir: &Path) -> Result<String> {
    let config: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(model_dir.join("config.json"))
            .with_context(|| format!("reading {}/config.json", model_dir.display()))?,
    )
    .context("parsing config.json")?;
    config
        .get("model_type")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .context("config.json has no `model_type`")
}

pub(crate) fn read_safetensors_header(
    path: &Path,
) -> Result<serde_json::Map<String, serde_json::Value>> {
    let mut file =
        std::fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut len = [0u8; 8];
    file.read_exact(&mut len)
        .with_context(|| format!("reading the header length of {}", path.display()))?;
    let len = u64::from_le_bytes(len);
    ensure!(
        len < 256 * 1024 * 1024,
        "{}: header length {len} is not a safetensors header",
        path.display()
    );
    let mut header = vec![0u8; len as usize];
    file.read_exact(&mut header)?;
    serde_json::from_slice(&header).with_context(|| format!("parsing {}'s header", path.display()))
}

pub(crate) fn dtype_bytes(dtype: &str) -> Result<u64> {
    Ok(match dtype {
        "BF16" | "F16" => 2,
        "F32" | "U32" | "I32" => 4,
        "F64" | "I64" | "U64" => 8,
        "U8" | "I8" => 1,
        other => bail!("safetensors dtype {other} has no known width"),
    })
}

/// Read a qwen3_5 checkpoint's facts. Only `model*.safetensors` are read — mlx_lm's loader globs
/// exactly those, so `mtp.safetensors` beside them is never loaded (STEP1b) and is not charged.
pub fn read_tensor_facts(model_dir: &Path) -> Result<TensorModelFacts> {
    let raw: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(model_dir.join("config.json"))
            .with_context(|| format!("reading {}/config.json", model_dir.display()))?,
    )?;
    let text = raw.get("text_config").unwrap_or(&raw);
    let num_layers = config_u64(text, "num_hidden_layers")?;
    let layer_types: Vec<String> = match text.get("layer_types").and_then(|v| v.as_array()) {
        Some(types) => types
            .iter()
            .map(|t| t.as_str().map(str::to_string).context("layer_types entry"))
            .collect::<Result<_>>()?,
        None => {
            let interval = config_u64(text, "full_attention_interval")?;
            (0..num_layers)
                .map(|i| {
                    if (i + 1) % interval == 0 {
                        "full_attention".to_string()
                    } else {
                        "linear_attention".to_string()
                    }
                })
                .collect()
        }
    };
    ensure!(
        layer_types.len() as u64 == num_layers,
        "config.json declares {num_layers} layers but {} layer_types",
        layer_types.len()
    );
    let full = layer_types
        .iter()
        .filter(|t| *t == "full_attention")
        .count() as u64;
    let linear = layer_types
        .iter()
        .filter(|t| *t == "linear_attention")
        .count() as u64;
    ensure!(
        full + linear == num_layers,
        "layer_types holds kinds other than full_attention/linear_attention"
    );

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
    let (mut sharded, mut replicated, mut excluded) = (0u64, 0u64, 0u64);
    let mut embed_dtype: Option<(bool, String)> = None;
    for shard in &shards {
        for (key, meta) in read_safetensors_header(shard)? {
            if key == "__metadata__" {
                continue;
            }
            let offsets = meta
                .get("data_offsets")
                .and_then(|o| o.as_array())
                .with_context(|| format!("{key}: no data_offsets"))?;
            let (start, end) = (
                offsets.first().and_then(|v| v.as_u64()).unwrap_or(0),
                offsets.get(1).and_then(|v| v.as_u64()).unwrap_or(0),
            );
            ensure!(end >= start, "{key}: data_offsets run backwards");
            let bytes = end - start;
            if key.starts_with("vision_tower")
                || key.starts_with("model.visual")
                || key.starts_with("mtp.")
            {
                excluded += bytes;
            } else if key.contains("embed_tokens") || key.contains("lm_head") {
                replicated += bytes;
                if key.contains("embed_tokens") {
                    let dtype = meta
                        .get("dtype")
                        .and_then(|d| d.as_str())
                        .with_context(|| format!("{key}: no dtype"))?
                        .to_string();
                    let is_scales = key.ends_with(".scales");
                    if is_scales || embed_dtype.is_none() {
                        embed_dtype = Some((is_scales, dtype));
                    }
                }
            } else {
                sharded += bytes;
            }
        }
    }
    let (_, act_dtype) = embed_dtype.context("no embed_tokens tensor in the checkpoint")?;
    Ok(TensorModelFacts {
        num_layers,
        full_attention_layers: full,
        linear_layers: linear,
        kv_heads: config_u64(text, "num_key_value_heads")?,
        head_dim: config_u64(text, "head_dim")?,
        linear_key_heads: config_u64(text, "linear_num_key_heads")?,
        linear_value_heads: config_u64(text, "linear_num_value_heads")?,
        linear_key_head_dim: config_u64(text, "linear_key_head_dim")?,
        linear_value_head_dim: config_u64(text, "linear_value_head_dim")?,
        conv_kernel: config_u64(text, "linear_conv_kernel_dim")?,
        max_position: config_u64(text, "max_position_embeddings")?,
        act_bytes: dtype_bytes(&act_dtype)?,
        sharded_bytes: sharded,
        replicated_bytes: replicated,
        excluded_bytes: excluded,
    })
}

impl TensorModelFacts {
    /// qwen3_5.py `shard()` divides linear heads by N and splits KV heads (repeating them when N
    /// exceeds their count). A count it cannot divide is refused here, not at load.
    pub fn check_divisible(&self, ranks: u64) -> Result<()> {
        for (name, heads) in [
            ("linear_num_key_heads", self.linear_key_heads),
            ("linear_num_value_heads", self.linear_value_heads),
        ] {
            ensure!(
                heads.is_multiple_of(ranks),
                "{name} = {heads} does not split over {ranks} ranks"
            );
        }
        ensure!(
            self.kv_heads.is_multiple_of(ranks) || ranks.is_multiple_of(self.kv_heads),
            "num_key_value_heads = {} neither splits over nor repeats onto {ranks} ranks",
            self.kv_heads
        );
        Ok(())
    }

    fn kv_heads_per_rank(&self, ranks: u64) -> u64 {
        (self.kv_heads / ranks).max(1)
    }

    /// KV bytes one token costs on one rank.
    pub fn kv_bytes_per_token(&self, ranks: u64) -> u64 {
        self.full_attention_layers
            * 2
            * self.kv_heads_per_rank(ranks)
            * self.head_dim
            * self.act_bytes
    }

    /// Conv + recurrent state one sequence costs on one rank (context-independent).
    pub fn linear_state_bytes(&self, ranks: u64) -> u64 {
        let key_dim = self.linear_key_heads * self.linear_key_head_dim;
        let value_dim = self.linear_value_heads * self.linear_value_head_dim;
        let conv = (self.conv_kernel - 1) * (2 * key_dim + value_dim) / ranks * self.act_bytes;
        let recurrent = self.linear_value_heads / ranks
            * self.linear_value_head_dim
            * self.linear_key_head_dim
            * RECURRENT_STATE_BYTES;
        self.linear_layers * (conv + recurrent)
    }

    pub fn weights_per_rank(&self, ranks: u64) -> u64 {
        self.sharded_bytes.div_ceil(ranks) + self.replicated_bytes
    }

    /// One sequence at `context` tokens on one rank: KV (rounded to the cache step) + state.
    pub fn sequence_bytes(&self, ranks: u64, context: u64) -> u64 {
        context.div_ceil(KV_CACHE_STEP) * KV_CACHE_STEP * self.kv_bytes_per_token(ranks)
            + self.linear_state_bytes(ranks)
    }

    /// The prompt cache's byte bound, handed to `mlx_lm.server --prompt-cache-bytes`.
    pub fn prompt_cache_bytes(&self, ranks: u64, context: u64) -> u64 {
        PROMPT_CACHE_CONTEXTS * self.sequence_bytes(ranks, context)
    }

    pub fn rank_plan(&self, ranks: u64, rank: u64, context: u64, budget: u64) -> RankPlan {
        let weights = self.weights_per_rank(ranks);
        let state = self.sequence_bytes(ranks, context);
        let prompt_cache = self.prompt_cache_bytes(ranks, context);
        let planned = weights + state + prompt_cache;
        let with_overhead = with_overhead(planned);
        RankPlan {
            layer_start: 0,
            layer_end: self.num_layers as u32,
            shard_index: Some(rank as u32),
            shard_count: Some(ranks as u32),
            weights_bytes: weights,
            state_bytes: state,
            workspace_bytes: 0,
            prompt_cache_bytes: prompt_cache,
            planned_bytes: planned,
            with_overhead_bytes: with_overhead,
            budget_bytes: budget,
            fits: with_overhead <= budget,
        }
    }

    /// The largest context (a multiple of the cache step, at most `max_position`) whose plan fits
    /// `budget` on every rank; 0 when not even the weights fit.
    pub fn max_context(&self, ranks: u64, budget: u64) -> u64 {
        let fits = |context: u64| with_overhead(self.planned_at(ranks, context)) <= budget;
        if !fits(KV_CACHE_STEP.min(self.max_position)) {
            return 0;
        }
        let (mut low, mut high) = (1u64, self.max_position.div_ceil(KV_CACHE_STEP));
        while low < high {
            let middle = (low + high).div_ceil(2);
            if fits(middle * KV_CACHE_STEP) {
                low = middle;
            } else {
                high = middle - 1;
            }
        }
        (low * KV_CACHE_STEP).min(self.max_position)
    }

    fn planned_at(&self, ranks: u64, context: u64) -> u64 {
        self.weights_per_rank(ranks)
            + self.sequence_bytes(ranks, context)
            + self.prompt_cache_bytes(ranks, context)
    }
}

/// One stage of the fork planner's JSON (`pipeline_qwen4 plan --json`, `plan_json`'s `stages`).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct PipelineStage {
    pub rank: u32,
    pub node: String,
    pub layer_start: u32,
    pub layer_end: u32,
    pub weight_bytes: u64,
    pub state_bytes: u64,
    pub workspace_bytes: u64,
    pub total_bytes: u64,
    /// The fork's rule over the figures goose gave it: min(free − RAM × available_margin,
    /// ceiling).
    pub budget_bytes: u64,
    /// The figures that budget was built from (goose's `--node NAME:RAM:FREE:CEILING`).
    pub available_bytes: u64,
    pub ceiling_bytes: u64,
    pub ram_bytes: u64,
    pub budget_source: String,
    pub fits: bool,
}

/// The ratio the fork's budget is built from — its home for the pipeline runner, read, never
/// re-typed (its caps sit at each node's GPU ceiling, no ratio).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct PipelineRatios {
    pub available_margin: f64,
}

/// The fork planner's whole answer. Exit 0 = fits, 2 = does not fit (the JSON is printed either
/// way).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct PipelinePlan {
    pub context: u64,
    pub batch: u32,
    pub prefill_step: u64,
    /// The largest context every rank fits on THIS split; `None` when none does.
    pub max_context: Option<u64>,
    pub starts: Vec<u32>,
    /// The full-context sequences every stage's state and workspace were planned for (the
    /// planner's `--batch`).
    pub slots: u32,
    pub fits: bool,
    pub ratios: PipelineRatios,
    pub stages: Vec<PipelineStage>,
}

/// Parse the planner's stdout: one JSON line. A shape it cannot vouch for (stages out of order,
/// ranges that do not tile, a `fits` that disagrees with its stages) is an error — a changed
/// planner must fail loudly, never drop or reorder a rank.
pub fn parse_pipeline_plan(stdout: &str) -> Result<PipelinePlan> {
    let line = stdout
        .lines()
        .map(str::trim)
        .rfind(|l| l.starts_with('{'))
        .with_context(|| format!("the planner printed no JSON line:\n{stdout}"))?;
    let plan: PipelinePlan =
        serde_json::from_str(line).context("the planner's JSON does not match its contract")?;
    ensure!(!plan.stages.is_empty(), "the planner returned no stages");
    for (index, stage) in plan.stages.iter().enumerate() {
        ensure!(
            stage.rank as usize == index,
            "planner stages out of order at rank {}",
            stage.rank
        );
        ensure!(
            plan.starts.get(index) == Some(&stage.layer_start),
            "starts {:?} disagree with rank {index}'s layer_start {}",
            plan.starts,
            stage.layer_start
        );
        ensure!(
            stage.layer_start < stage.layer_end,
            "rank {index} holds no layers ([{}, {}))",
            stage.layer_start,
            stage.layer_end
        );
        if let Some(next) = plan.stages.get(index + 1) {
            ensure!(
                stage.layer_end == next.layer_start,
                "rank {index} ends at layer {} but rank {} starts at {}",
                stage.layer_end,
                index + 1,
                next.layer_start
            );
        }
    }
    ensure!(
        plan.starts.len() == plan.stages.len() && plan.starts.first() == Some(&0),
        "starts {:?} do not describe {} stages from layer 0",
        plan.starts,
        plan.stages.len()
    );
    ensure!(
        plan.slots == plan.batch,
        "the planner planned batch {} but reports {} slots",
        plan.batch,
        plan.slots
    );
    ensure!(
        plan.fits == plan.stages.iter().all(|s| s.fits),
        "the planner's fits ({}) disagrees with its stages",
        plan.fits
    );
    Ok(plan)
}

/// The `--split` argument for approved starts: ranks 1..N-1's first layers, comma-joined.
pub fn split_arg(starts: &[u32]) -> String {
    starts
        .iter()
        .skip(1)
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

impl PipelinePlan {
    pub fn split_arg(&self) -> String {
        split_arg(&self.starts)
    }
}

impl PipelineStage {
    /// The rank's plan in goose's terms — the fork's bytes, budget and verdict, verbatim. No
    /// overhead multiplier: the fork's workspace term (49× the chunk's stream bytes per layer)
    /// already covers the measured peaks (see `RUNTIME_OVERHEAD_RATIO`'s receipt), so
    /// `with_overhead_bytes` is the fork's total.
    pub fn rank_plan(&self) -> RankPlan {
        RankPlan {
            layer_start: self.layer_start,
            layer_end: self.layer_end,
            shard_index: None,
            shard_count: None,
            weights_bytes: self.weight_bytes,
            state_bytes: self.state_bytes,
            workspace_bytes: self.workspace_bytes,
            prompt_cache_bytes: 0,
            planned_bytes: self.total_bytes,
            with_overhead_bytes: self.total_bytes,
            budget_bytes: self.budget_bytes,
            fits: self.fits,
        }
    }
}

/// The `--node NAME:RAM_GIB:FREE_GIB:CEILING_GIB` argument for the fork planner: the node's RAM,
/// goose's own measured available memory (host_statistics64 — the same measure the fork's loader
/// takes on every rank at load time), unscaled, and the node's GPU ceiling (Metal's
/// max_recommended_working_set_size, read on the node). The name is reduced to `[A-Za-z0-9-]`
/// (the fork splits the argument on `:`).
pub fn planner_node_arg(
    name: &str,
    total_bytes: u64,
    available_bytes: u64,
    ceiling_bytes: u64,
) -> String {
    let gib = |b: u64| b as f64 / crate::GIB as f64;
    let safe_name: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    format!(
        "{safe_name}:{:.4}:{:.4}:{:.4}",
        gib(total_bytes),
        gib(available_bytes),
        gib(ceiling_bytes)
    )
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::GIB;

    fn gib(value: f64) -> u64 {
        (value * GIB as f64) as u64
    }

    /// Measured 2026-09-24, `mx.device_info()["max_recommended_working_set_size"]`.
    pub(crate) const M4_MAX_CEILING: u64 = 115_448_725_504;
    pub(crate) const M3_ULTRA_CEILING: u64 = 83_494_174_720;

    /// The owner's Qwen3.8-27B-Atlassian-Q8-mlx, from its config and safetensors headers
    /// (2026-09-24): 64 layers (16 full attention), 4 KV heads × 256, Q8 embed scales in BF16;
    /// decoder layers 24.101 GiB, embed_tokens 2.368 GiB + lm_head 2.368 GiB unsharded.
    fn qwen_27b() -> TensorModelFacts {
        TensorModelFacts {
            num_layers: 64,
            full_attention_layers: 16,
            linear_layers: 48,
            kv_heads: 4,
            head_dim: 256,
            linear_key_heads: 16,
            linear_value_heads: 48,
            linear_key_head_dim: 128,
            linear_value_head_dim: 128,
            conv_kernel: 4,
            max_position: 262_144,
            act_bytes: 2,
            sharded_bytes: gib(24.101),
            replicated_bytes: gib(2.368) * 2,
            excluded_bytes: gib(0.9),
        }
    }

    #[test]
    fn the_27b_tensor_split_covers_the_recorded_rank_footprints() {
        let facts = qwen_27b();
        facts.check_divisible(2).unwrap();
        // 16 full-attention layers × K,V × 2 heads per rank × 256 × 2 B.
        assert_eq!(facts.kv_bytes_per_token(2), 32_768);
        let weights = facts.weights_per_rank(2);
        assert!(
            (weights as f64 / GIB as f64 - 16.786).abs() < 0.01,
            "{weights}"
        );

        // STEP1b bench (2048 prompt + 256 generated): measured peak 19.9 GB per rank.
        let bench = facts.rank_plan(2, 0, 2_304, u64::MAX);
        assert!(
            bench.with_overhead_bytes >= 19_900_000_000,
            "the plan must cover the measured bench peak: {}",
            bench.with_overhead_bytes
        );
        // STEP1b soak: 8.7k-token prompts, mlx_lm's unbounded LRU prompt cache (10 sequences);
        // measured 20.4-23.5 GB per rank. The same arithmetic charging those 10 cached sequences
        // lands above the recorded maximum.
        let soak_cache = 10 * facts.sequence_bytes(2, 8_704);
        let soak =
            with_overhead(facts.weights_per_rank(2) + facts.sequence_bytes(2, 8_704) + soak_cache);
        assert!(soak >= 23_500_000_000, "{soak}");
        // With goose's bound (one context's worth of cache) the 8.7k plan sits inside the range.
        let bounded = facts.rank_plan(2, 0, 8_704, u64::MAX).with_overhead_bytes;
        assert!(
            (20_000_000_000..23_500_000_000).contains(&bounded),
            "{bounded}"
        );
    }

    #[test]
    fn the_tensor_budget_on_the_recorded_nodes() {
        // Node figures of 2026-09-24: MacBook 92.7 GiB available of 128, workhorse 61.6 of 96;
        // GPU ceilings (mx.device_info max_recommended_working_set_size) 107.52 / 77.76 GiB.
        let macbook = budget_bytes(gib(92.7), gib(128.0), M4_MAX_CEILING);
        let workhorse = budget_bytes(gib(61.6), gib(96.0), M3_ULTRA_CEILING);
        // available − 9.3% of RAM: 92.7 − 11.90, 61.6 − 8.93 (the old rule gave 83.43 / 55.44).
        assert!((macbook as f64 / GIB as f64 - 80.80).abs() < 0.01);
        assert!((workhorse as f64 / GIB as f64 - 52.67).abs() < 0.01);
        // The GPU ceiling binds when the node is idle (the old rule: RAM × 0.75 = 96 / 72 GiB).
        assert_eq!(
            budget_bytes(gib(127.0), gib(128.0), M4_MAX_CEILING),
            M4_MAX_CEILING
        );
        assert_eq!(
            budget_bytes(gib(95.0), gib(96.0), M3_ULTRA_CEILING),
            M3_ULTRA_CEILING
        );
        // Below the margin there is no budget at all, never an underflow.
        assert_eq!(budget_bytes(gib(5.0), gib(96.0), M3_ULTRA_CEILING), 0);
    }

    #[test]
    fn preflight_budgets_before_and_after_the_recorded_compaction() {
        // The workhorse, 2026-09-24, nothing loaded: 72.4 GiB available → memory_pressure to
        // WARN → 78.0 GiB. Flash's rank 1 on the fork's split [19, 48) plans 45.10 GiB.
        let before = budget_bytes(gib(72.4), gib(96.0), M3_ULTRA_CEILING);
        let after = budget_bytes(gib(78.0), gib(96.0), M3_ULTRA_CEILING);
        let old_rule = |available: f64| gib(available - 96.0 * 0.21);
        assert!((before as f64 / GIB as f64 - 63.47).abs() < 0.01);
        assert!((after as f64 / GIB as f64 - 69.07).abs() < 0.01);
        assert_eq!(
            after - before,
            gib(78.0) - gib(72.4),
            "compaction's gain is all budget"
        );
        // The old 21% floor: 52.24 → 57.84 GiB for the same two readings.
        assert!(old_rule(78.0) < before);
        // The ceiling (77.76 GiB) binds only above 86.69 GiB available.
        assert_eq!(
            budget_bytes(gib(87.0), gib(96.0), M3_ULTRA_CEILING),
            M3_ULTRA_CEILING
        );
    }

    #[test]
    fn a_node_that_cannot_hold_its_slice_is_refused_with_the_numbers() {
        let facts = qwen_27b();
        // The workhorse with only 20 GiB available: budget 11.07 GiB < 16.79 GiB weights × 1.10.
        let budget = budget_bytes(gib(20.0), gib(96.0), M3_ULTRA_CEILING);
        let plan = facts.rank_plan(2, 1, 8_192, budget);
        assert!(!plan.fits);
        assert_eq!(facts.max_context(2, budget), 0);
    }

    #[test]
    fn the_context_ceiling_is_the_largest_that_fits_and_stops_at_the_model() {
        let facts = qwen_27b();
        let roomy = budget_bytes(gib(90.0), gib(128.0), M4_MAX_CEILING);
        assert_eq!(facts.max_context(2, roomy), 262_144);
        let tight = budget_bytes(gib(29.0), gib(96.0), M3_ULTRA_CEILING);
        let ceiling = facts.max_context(2, tight);
        assert!(ceiling > 0 && ceiling < 262_144 && ceiling.is_multiple_of(KV_CACHE_STEP));
        assert!(facts.rank_plan(2, 0, ceiling, tight).fits);
        assert!(!facts.rank_plan(2, 0, ceiling + KV_CACHE_STEP, tight).fits);
    }

    #[test]
    fn heads_that_do_not_split_are_refused() {
        let facts = qwen_27b();
        assert!(facts.check_divisible(3).is_err());
        facts.check_divisible(4).unwrap();
    }

    /// The fork planner's real JSON for Flash (2f02ac645, `plan --json --model <Flash> --node
    /// m:128:90:107.52 --node w:96:67:77.76 --context 32768 --batch 2`, 2026-09-24 — the GPU
    /// ceilings are the two Macs' measured max_recommended_working_set_size): exit 0.
    pub(crate) const FLASH_PLAN_32K: &str = r#"{"context": 32768, "batch": 2, "prefill_step": 2048, "max_context": 262144, "starts": [0, 19], "slots": 2, "fits": true, "vision": {"rank": 0, "weight_bytes": 448092512, "workspace_bytes": 1229312000}, "checkpoint": {"text_bytes": 102766153240, "head_bytes": 357580800, "tail_bytes": 361287680, "excluded_bytes": {"mtp": 1467242656, "vision": 448092512}, "layer_bytes": [1459858528, 33478587768, 1459858528, 1456137376, 1459858528, 1459858528, 1459858528, 1456137376, 1459858528, 1459858528, 1459858528, 1456137376, 1459858528, 1459858528, 1459858528, 1456137376, 1459858528, 1459858528, 1459858528, 1456137376, 1459858528, 1459858528, 1459858528, 1456137376, 1459858528, 1459858528, 1459858528, 1456137376, 1459858528, 1459858528, 1459858528, 1456137376, 1459858528, 1459858528, 1459858528, 1456137376, 1459858528, 1459858528, 1459858528, 1456137376, 1459858528, 1459858528, 1459858528, 1456137376, 1459858528, 1459858528, 1459858528, 1456137376]}, "ratios": {"available_margin": 0.093, "layer_transient_stream_multiple": 49}, "wire": {"stream_per_hop": 20480, "hops": 1, "token_broadcast": 4, "total_per_token": 20484}, "stages": [{"rank": 0, "node": "m", "layer_start": 0, "layer_end": 19, "weight_bytes": 60546829976, "state_bytes": 650240032, "workspace_bytes": 4513071104, "total_bytes": 65710141112, "budget_bytes": 83854941488, "available_bytes": 96636764160, "ceiling_bytes": 115448720916, "ram_bytes": 137438953472, "budget_source": "free + GPU ceiling given", "fits": true}, {"rank": 1, "node": "w", "layer_start": 19, "layer_end": 48, "weight_bytes": 42667415776, "state_bytes": 1242013696, "workspace_bytes": 4515057664, "total_bytes": 48424487136, "budget_bytes": 62354335204, "available_bytes": 71940702208, "ceiling_bytes": 83494164234, "ram_bytes": 103079215104, "budget_source": "free + GPU ceiling given", "fits": true}]}"#;

    /// The measured soak's shape re-planned by the same planner (2f02ac645): split 20, context
    /// 8,192, batch 2, the node figures recorded at that run (MacBook 92.7 of 128 GiB available,
    /// workhorse 61.6 of 96) with each Mac's GPU ceiling: exit 0.
    const FLASH_SOAK_SHAPE: &str = r#"{"context": 8192, "batch": 2, "prefill_step": 2048, "max_context": 262144, "starts": [0, 20], "slots": 2, "fits": true, "vision": {"rank": 0, "weight_bytes": 448092512, "workspace_bytes": 1229312000}, "checkpoint": {"text_bytes": 102766153240, "head_bytes": 357580800, "tail_bytes": 361287680, "excluded_bytes": {"mtp": 1467242656, "vision": 448092512}, "layer_bytes": [1459858528, 33478587768, 1459858528, 1456137376, 1459858528, 1459858528, 1459858528, 1456137376, 1459858528, 1459858528, 1459858528, 1456137376, 1459858528, 1459858528, 1459858528, 1456137376, 1459858528, 1459858528, 1459858528, 1456137376, 1459858528, 1459858528, 1459858528, 1456137376, 1459858528, 1459858528, 1459858528, 1456137376, 1459858528, 1459858528, 1459858528, 1456137376, 1459858528, 1459858528, 1459858528, 1456137376, 1459858528, 1459858528, 1459858528, 1456137376, 1459858528, 1459858528, 1459858528, 1456137376, 1459858528, 1459858528, 1459858528, 1456137376]}, "ratios": {"available_margin": 0.093, "layer_transient_stream_multiple": 49}, "wire": {"stream_per_hop": 20480, "hops": 1, "token_broadcast": 4, "total_per_token": 20484}, "stages": [{"rank": 0, "node": "macbook", "layer_start": 0, "layer_end": 20, "weight_bytes": 62002967352, "state_bytes": 269608992, "workspace_bytes": 4211081216, "total_bytes": 66483657560, "budget_bytes": 86754044412, "available_bytes": 99535867084, "ceiling_bytes": 115448720916, "ram_bytes": 137438953472, "budget_source": "free + GPU ceiling given", "fits": true}, {"rank": 1, "node": "workhorse", "layer_start": 20, "layer_end": 48, "weight_bytes": 41211278400, "state_bytes": 376936448, "workspace_bytes": 4213067776, "total_bytes": 45801282624, "budget_bytes": 56556129354, "available_bytes": 66142496358, "ceiling_bytes": 83494164234, "ram_bytes": 103079215104, "budget_source": "free + GPU ceiling given", "fits": true}]}"#;

    #[test]
    fn the_plan_json_is_read_verbatim() {
        let plan = parse_pipeline_plan(FLASH_PLAN_32K).unwrap();
        assert_eq!(
            (plan.context, plan.batch, plan.prefill_step),
            (32_768, 2, 2_048)
        );
        assert_eq!(plan.max_context, Some(262_144));
        assert_eq!(plan.starts, vec![0, 19]);
        assert_eq!(plan.slots, 2, "planned for 2 full-context sequences");
        assert_eq!(plan.split_arg(), "19");
        assert!(plan.fits);
        assert_eq!(
            plan.ratios.available_margin,
            crate::distributed::AVAILABLE_MARGIN_RATIO
        );
        let rank1 = plan.stages[1].rank_plan();
        assert_eq!((rank1.layer_start, rank1.layer_end), (19, 48));
        assert_eq!(rank1.weights_bytes, 42_667_415_776);
        assert_eq!(rank1.workspace_bytes, 4_515_057_664);
        assert_eq!(rank1.planned_bytes, 48_424_487_136);
        assert_eq!(
            rank1.with_overhead_bytes, rank1.planned_bytes,
            "no multiplier on the fork's plan"
        );
        assert_eq!(
            rank1.budget_bytes, 62_354_335_204,
            "the fork's budget, not re-derived"
        );
        assert!(rank1.fits);
    }

    #[test]
    fn the_forks_plan_covers_the_measured_soak_peaks_and_the_ceiling_rule_admits_it() {
        // Flash JACCL soak (131 single + 130 two-request batches, context 8,192, split 20):
        // MLX peak MacBook 61.0 GiB, workhorse 42.5 GiB.
        let plan = parse_pipeline_plan(FLASH_SOAK_SHAPE).unwrap();
        let (macbook, workhorse) = (plan.stages[0].rank_plan(), plan.stages[1].rank_plan());
        assert!(macbook.planned_bytes >= gib(61.0) && workhorse.planned_bytes >= gib(42.5));
        // The soak ran this shape without a WARN. The old 21% floor refused it (61.6 − 96 × 0.21
        // = 41.44 GiB < 42.66 planned); available − 9.3% of RAM gives 61.6 − 8.93 = 52.67 GiB.
        assert!(macbook.fits && workhorse.fits && plan.fits);
        assert!((workhorse.budget_bytes as f64 / GIB as f64 - 52.67).abs() < 0.01);
        assert!((plan.stages[1].ceiling_bytes as f64 / GIB as f64 - 77.76).abs() < 0.01);
    }

    #[test]
    fn a_changed_planner_contract_fails_loudly() {
        let broken = FLASH_PLAN_32K.replace("\"total_bytes\"", "\"total\"");
        assert!(parse_pipeline_plan(&broken).is_err());
        let gap = FLASH_PLAN_32K.replace("\"layer_start\": 19", "\"layer_start\": 20");
        let err = parse_pipeline_plan(&gap).unwrap_err().to_string();
        assert!(err.contains("starts"), "{err}");
        let lying = FLASH_PLAN_32K.replacen(
            "\"fits\": true, \"vision\"",
            "\"fits\": false, \"vision\"",
            1,
        );
        assert!(parse_pipeline_plan(&lying).is_err());
        assert!(parse_pipeline_plan("Traceback (most recent call last):").is_err());
        // A planner without `slots` (before ea6f8dee1), or one disagreeing with its batch.
        assert!(parse_pipeline_plan(&FLASH_PLAN_32K.replace("\"slots\": 2, ", "")).is_err());
        let err = parse_pipeline_plan(&FLASH_PLAN_32K.replace("\"slots\": 2", "\"slots\": 3"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("3 slots"), "{err}");
        // Anything the planner printed before its JSON line is not the plan.
        let noisy = format!("warning: something\n{FLASH_PLAN_32K}\n");
        assert!(parse_pipeline_plan(&noisy).is_ok());
    }

    #[test]
    fn planner_node_figures_are_goose_measurements_unscaled() {
        let arg = planner_node_arg("MacBook Pro", gib(128.0), gib(92.7), M4_MAX_CEILING);
        assert_eq!(arg, "MacBook-Pro:128.0000:92.7000:107.5200");
    }

    /// The real checkpoint, when present on this Mac: the facts read from its headers match the
    /// figures the tests above use.
    #[test]
    fn the_owners_27b_headers_read_as_recorded_when_present() {
        let dir = dirs::home_dir()
            .unwrap()
            .join(".goose/models/Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx");
        if !dir.join("config.json").exists() {
            eprintln!("skipped: {} absent", dir.display());
            return;
        }
        assert_eq!(read_model_type(&dir).unwrap(), "qwen3_5");
        let facts = read_tensor_facts(&dir).unwrap();
        let recorded = qwen_27b();
        assert_eq!(facts.full_attention_layers, recorded.full_attention_layers);
        assert_eq!(facts.kv_heads, recorded.kv_heads);
        assert_eq!(facts.act_bytes, 2);
        assert!((facts.sharded_bytes as f64 / GIB as f64 - 24.101).abs() < 0.01);
        assert!((facts.replicated_bytes as f64 / GIB as f64 - 4.736).abs() < 0.01);
    }
}
