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

use super::{
    BATCH_KV_TRANSIENT_RATIO, PROMPT_CACHE_CONTEXTS, RUNTIME_OVERHEAD_RATIO, TENSOR_PREFILL_STEP,
};

/// mlx_lm `KVCache.step`: the KV buffer grows in 256-token blocks (models/cache.py), so a context
/// costs its size rounded up to the step. An algorithm constant of the engine, not a policy.
pub const KV_CACHE_STEP: u64 = 256;
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
    /// What one prefill chunk materializes beyond the plan's resident bytes. Pipeline: the fork's
    /// workspace (49× the chunk's stream bytes per layer, measured) plus the attention scores the
    /// fork's model leaves out (`PipelineAttention::scores_bytes`). Tensor: the attention scores and the
    /// batch mask ONE row's chunk materializes at the planned context
    /// (`TensorModelFacts::prefill_workspace_bytes`); the rank keeps every step inside it
    /// (rank_prefill.py). The chunk's other transients ride `RUNTIME_OVERHEAD_RATIO`.
    pub workspace_bytes: u64,
    /// Tensor: what the rank's prefill and admission are sized from (the same on every rank).
    /// `None` for the pipeline runner and for a plan made before Q-104.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefill: Option<TensorPrefill>,
    /// The prefill chunk (tokens) the launch hands the engine: tensor mlx_lm's
    /// `--prefill-step-size` (the rank shrinks it per step to the workspace); pipeline the fork
    /// serve's `--prefill-step`, the chunk whose scores fit every stage. 0 = the fork's plan read
    /// alone, or a plan made before Q-104.
    #[serde(default)]
    pub prefill_step: u64,
    /// What the plan charges this rank for CACHED prompts, on top of the live request's
    /// `state_bytes` (tensor: one allowed context; the launch hands mlx_lm the sum — see
    /// [`RankPlan::prompt_cache_limit_bytes`]).
    pub prompt_cache_bytes: u64,
    /// The prompt cache's entry bound (tensor: `--prompt-cache-size`): as many of the smallest
    /// entries a request can leave as the byte bound holds, so the count never evicts before the
    /// bytes do. 0 for pipeline (the fork's server keeps no prompt cache).
    #[serde(default)]
    pub prompt_cache_entries: u64,
    /// Tensor: weights + state + prompt cache (the resident bytes; the workspace is apart).
    /// Pipeline: the fork's total, workspace included.
    pub planned_bytes: u64,
    /// What is compared with the budget: tensor `planned × RUNTIME_OVERHEAD_RATIO + workspace`;
    /// pipeline the fork's total as planned plus goose's scores term (no multiplier — see
    /// `RUNTIME_OVERHEAD_RATIO`).
    pub with_overhead_bytes: u64,
    pub budget_bytes: u64,
    pub fits: bool,
    /// Pipeline: the prefill attention scores in `workspace_bytes` that the fork's own plan leaves
    /// out (`PipelineAttention::scores_bytes` at `prefill_step`). The rank's serve is handed them
    /// (`--attention-scores-bytes`) so MLX's buffer cache holds the fork's measured budget less
    /// the plan less these — the transient room the chunk was sized into (Q-127). `None` for the
    /// tensor runner and for a pipeline plan made before Q-127.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attention_scores_bytes: Option<u64>,
}

impl RankPlan {
    /// The bound handed to mlx_lm (`--prompt-cache-bytes`, and the prompt cache's own
    /// `max_bytes`): the plan's whole KV charge, live and cached. mlx_lm 0.31.3 reads its flag as
    /// cached + LIVE — each admission trims the cache to `flag − the batch's live KV`
    /// (server.py:795-798) — so handing it `prompt_cache_bytes` alone left the cache one context
    /// MINUS the live request, and a ~56k-token turn on the 27B split then held room for one
    /// ~1.8 GB prefix: E2E #1 (2026-09-25) re-read 55,977 tokens cold at 21:11:43. The cache
    /// itself never exceeds the same sum between admissions (the wrapper's `max_bytes`).
    pub fn prompt_cache_limit_bytes(&self) -> u64 {
        self.state_bytes + self.prompt_cache_bytes
    }

    /// MLX's free-buffer cache limit on a tensor rank (`mx.set_cache_limit`): the plan's
    /// transient allowance beside the workspace, planned × (RUNTIME_OVERHEAD_RATIO − 1). The
    /// rank used to set it to its GPU ceiling less the planned bytes — 48,135,889,408 B on the
    /// Studio in E2E #2 — which let freed buffers stay resident up to MLX's own reclaim point (95%
    /// of that ceiling), past what the node could give beside its OS and goose. The workspace is
    /// not in it: mlx_lm hands every prefill chunk's buffers back (`mx.clear_cache()` per chunk).
    pub fn mlx_cache_limit_bytes(&self) -> u64 {
        self.with_overhead_bytes
            .saturating_sub(self.planned_bytes + self.workspace_bytes)
    }

    /// The prefill chunk this plan's workspace affords one row at the full `context` (tensor).
    pub fn full_context_chunk(&self, context: u64) -> Option<u64> {
        self.prefill.map(|p| p.chunk(1, context))
    }

    /// A tensor plan charged `workspace` instead of its own (every rank of a launch runs the
    /// smallest workspace any rank affords: the chunk a step takes must agree across ranks).
    pub fn sharing_workspace(mut self, workspace: u64) -> RankPlan {
        self.with_overhead_bytes = self.with_overhead_bytes - self.workspace_bytes + workspace;
        self.workspace_bytes = workspace;
        if let Some(prefill) = self.prefill.as_mut() {
            prefill.workspace_bytes = workspace;
        }
        self.fits = self.with_overhead_bytes <= self.budget_bytes;
        self
    }
}

/// A tensor rank's prefill figures: the chunk mlx_lm is launched with, the workspace every step
/// stays inside, and the per-token costs the rank projects a batch's memory with. Every rank of a
/// launch gets the same figures (`TensorLaunch::for_ranks`): the chunk a step takes, and the
/// requests a batch admits, must agree across ranks or the collectives no longer pair up.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TensorPrefill {
    /// `--prefill-step-size`: the largest chunk any step takes.
    pub step: u64,
    /// The attention scores + mask one step may materialize (bytes).
    pub workspace_bytes: u64,
    /// Bytes per row × chunk token × context token (`TensorModelFacts::prefill_pair_bytes`).
    pub pair_bytes: u64,
    /// KV bytes one token costs on the rank.
    pub kv_bytes_per_token: u64,
    /// The recurrent state one row holds whatever its length.
    pub sequence_state_bytes: u64,
    /// What mlx_lm's batch operations transiently hold of a batch's padded KV
    /// (`BATCH_KV_TRANSIENT_RATIO`), carried so every rank projects with the requester's figure.
    pub batch_transient_ratio: f64,
}

impl TensorPrefill {
    /// The chunk (tokens) `rows` rows reaching `width` may take in one step: the workspace over
    /// what one chunk token costs them, rounded down to the KV cache step, at most `step`. 0 when
    /// not even one step of the KV block fits.
    pub fn chunk(&self, rows: u64, width: u64) -> u64 {
        let per_token = rows * width * self.pair_bytes;
        let chunk = (self.workspace_bytes / per_token.max(1)).min(self.step);
        chunk / KV_CACHE_STEP * KV_CACHE_STEP
    }
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
    /// Query heads (`num_attention_heads`). MLX 0.32.2 has no fused prefill attention for
    /// head_dim 256, so a full-attention layer's chunk materializes one score per query head ×
    /// chunk token × context token (Q-104, measured 0.96-1.06× that product).
    pub attention_heads: u64,
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
        attention_heads: config_u64(text, "num_attention_heads")?,
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
            ("num_attention_heads", self.attention_heads),
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

    /// Bytes one row's prefill chunk token costs per context token on one rank, in the full-
    /// attention layer being computed (one at a time — each needs the previous one's output): the
    /// rank's query heads' scores at the activation width, and the batch's boolean mask
    /// (BatchKVCache's left-padded causal mask, one byte). Measured on 2 localhost ranks of the
    /// 27B's layers (Q-104, 2026-09-26): one row, chunk 2,048, context 12,288 → 135,168: peak −
    /// the row's KV − the chunk's other transients = 0.604 → 6.644 GB of scores, the product
    /// exactly.
    pub fn prefill_pair_bytes(&self, ranks: u64) -> u64 {
        self.attention_heads / ranks * self.act_bytes + 1
    }

    /// The workspace one row's `chunk`-token prefill step needs at `context` on one rank.
    pub fn prefill_workspace_bytes(&self, ranks: u64, chunk: u64, context: u64) -> u64 {
        chunk * context * self.prefill_pair_bytes(ranks)
    }

    /// The prefill figures of a plan whose workspace is `workspace_bytes`.
    pub fn prefill(&self, ranks: u64, workspace_bytes: u64) -> TensorPrefill {
        TensorPrefill {
            step: TENSOR_PREFILL_STEP,
            workspace_bytes,
            pair_bytes: self.prefill_pair_bytes(ranks),
            kv_bytes_per_token: self.kv_bytes_per_token(ranks),
            sequence_state_bytes: self.linear_state_bytes(ranks),
            batch_transient_ratio: BATCH_KV_TRANSIENT_RATIO,
        }
    }

    /// What the plan charges for cached prompts on top of the live request (`RankPlan::
    /// prompt_cache_bytes`).
    pub fn prompt_cache_bytes(&self, ranks: u64, context: u64) -> u64 {
        PROMPT_CACHE_CONTEXTS * self.sequence_bytes(ranks, context)
    }

    /// The smallest entry any request leaves in mlx_lm's prompt cache on one rank: one KV step
    /// and, on a hybrid model, the whole recurrent state (the 27B: 8 MiB + 73.4 MiB).
    pub fn smallest_cache_entry_bytes(&self, ranks: u64) -> u64 {
        self.sequence_bytes(ranks, 1)
    }

    /// One rank's plan at `context`. The workspace is what the rank's budget leaves above the
    /// resident bytes (× the overhead ratio), as a whole number of KV-step chunks for one row at
    /// the full context: at most `TENSOR_PREFILL_STEP` tokens, at least one KV step — charged even
    /// when it does not fit, so the verdict names it. `TensorLaunch::for_ranks` hands every rank
    /// the smallest of the ranks' workspaces.
    pub fn rank_plan(&self, ranks: u64, rank: u64, context: u64, budget: u64) -> RankPlan {
        let weights = self.weights_per_rank(ranks);
        let state = self.sequence_bytes(ranks, context);
        let prompt_cache = self.prompt_cache_bytes(ranks, context);
        let planned = weights + state + prompt_cache;
        let resident = with_overhead(planned);
        let headroom = budget.saturating_sub(resident);
        let chunk = self
            .prefill(ranks, headroom)
            .chunk(1, context)
            .max(KV_CACHE_STEP.min(TENSOR_PREFILL_STEP));
        let workspace = self.prefill_workspace_bytes(ranks, chunk, context);
        let with_overhead = resident + workspace;
        let mut plan = RankPlan {
            layer_start: 0,
            layer_end: self.num_layers as u32,
            shard_index: Some(rank as u32),
            shard_count: Some(ranks as u32),
            weights_bytes: weights,
            state_bytes: state,
            workspace_bytes: workspace,
            prefill: Some(self.prefill(ranks, workspace)),
            prefill_step: TENSOR_PREFILL_STEP,
            prompt_cache_bytes: prompt_cache,
            prompt_cache_entries: 0,
            planned_bytes: planned,
            with_overhead_bytes: with_overhead,
            budget_bytes: budget,
            fits: with_overhead <= budget,
            attention_scores_bytes: None,
        };
        plan.prompt_cache_entries =
            plan.prompt_cache_limit_bytes() / self.smallest_cache_entry_bytes(ranks);
        plan
    }

    /// The largest context (a multiple of the cache step, at most `max_position`) whose plan fits
    /// `budget` on every rank — its resident bytes and one row's prefill in KV-step chunks at that
    /// context; 0 when not even the weights fit.
    pub fn max_context(&self, ranks: u64, budget: u64) -> u64 {
        let fits = |context: u64| {
            with_overhead(self.planned_at(ranks, context))
                + self.prefill_workspace_bytes(ranks, KV_CACHE_STEP, context)
                <= budget
        };
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
    /// `with_overhead_bytes` is the fork's total. [`PipelineStage::rank_plan_with_scores`] adds
    /// the attention scores that workspace leaves out.
    pub fn rank_plan(&self) -> RankPlan {
        self.rank_plan_with_scores(0, 0)
    }

    /// The fork's plan plus `scores` bytes of prefill workspace (`PipelineAttention::scores_bytes`
    /// at `chunk`), the verdict re-drawn on the fork's own budget.
    pub fn rank_plan_with_scores(&self, scores: u64, chunk: u64) -> RankPlan {
        let total = self.total_bytes + scores;
        RankPlan {
            layer_start: self.layer_start,
            layer_end: self.layer_end,
            shard_index: None,
            shard_count: None,
            weights_bytes: self.weight_bytes,
            state_bytes: self.state_bytes,
            workspace_bytes: self.workspace_bytes + scores,
            prefill: None,
            prefill_step: chunk,
            prompt_cache_bytes: 0,
            prompt_cache_entries: 0,
            planned_bytes: total,
            with_overhead_bytes: total,
            budget_bytes: self.budget_bytes,
            fits: self.fits && total <= self.budget_bytes,
            attention_scores_bytes: Some(scores),
        }
    }
}

/// What the fork's workspace model leaves out of a qwen4_exp stage: its prefill takes the DENSE
/// attention path (both QSA sparse routes are opt-in: `block_sparse_decline_reason` and
/// `indexed_splitk_decline_reason` answer "disabled" without their env), and MLX 0.32.2's SDPA
/// has no fused prefill kernel for head_dim 256, so each full-attention layer materializes
/// rows × query heads × chunk × context scores at the activation width. The fork charges that
/// layer `batch × tokens × context × (1 + act)` (its selection and additive masks) — measured on
/// the Flash checkpoint's layer 3 (fork b7bd1afc2, 2026-09-26): one row, chunk 2,048 → 3.50 /
/// 5.03 GB above the layer's weights at widths 16,384 / 32,768, the slope 93 KB per context
/// token = 24 heads × 2,048 × 2 B (0.95×); the fork's term for the second is 0.20 GB.
#[derive(Debug, Clone, PartialEq)]
pub struct PipelineAttention {
    /// Per decoder layer: does it run full attention?
    pub full_attention: Vec<bool>,
    pub attention_heads: u64,
    pub act_bytes: u64,
}

/// Read a qwen4_exp checkpoint's attention layout: `layer_types`, `num_attention_heads`, and the
/// activation width (the embedding's scales dtype, as the fork's planner reads it).
pub fn read_pipeline_attention(model_dir: &Path) -> Result<PipelineAttention> {
    let raw: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(model_dir.join("config.json"))
            .with_context(|| format!("reading {}/config.json", model_dir.display()))?,
    )?;
    let text = raw.get("text_config").unwrap_or(&raw);
    let full_attention = text
        .get("layer_types")
        .and_then(|v| v.as_array())
        .context("config.json has no `layer_types`")?
        .iter()
        .map(|t| {
            t.as_str()
                .map(|kind| kind != "linear_attention")
                .context("layer_types entry")
        })
        .collect::<Result<Vec<bool>>>()?;
    Ok(PipelineAttention {
        full_attention,
        attention_heads: config_u64(text, "num_attention_heads")?,
        act_bytes: activation_bytes(model_dir)?,
    })
}

/// The embedding's scales dtype (its weight's when unquantized): the stream's activation width.
fn activation_bytes(model_dir: &Path) -> Result<u64> {
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
    let mut weight = None;
    for shard in &shards {
        for (key, meta) in read_safetensors_header(shard)? {
            if !key.contains("embed_tokens") {
                continue;
            }
            let dtype = meta
                .get("dtype")
                .and_then(|d| d.as_str())
                .with_context(|| format!("{key}: no dtype"))?;
            if key.ends_with(".scales") {
                return dtype_bytes(dtype);
            }
            if weight.is_none() {
                weight = Some(dtype_bytes(dtype)?);
            }
        }
    }
    weight.with_context(|| format!("no embed_tokens tensor in {}", model_dir.display()))
}

impl PipelineAttention {
    /// The scores one prefill chunk of `slots` rows at `context` materializes on a stage holding
    /// `[layer_start, layer_end)` (one layer at a time: each needs the previous one's output); 0
    /// for a stage without a full-attention layer.
    pub fn scores_bytes(&self, stage: &PipelineStage, slots: u64, chunk: u64, context: u64) -> u64 {
        let holds_attention = self
            .full_attention
            .get(stage.layer_start as usize..stage.layer_end as usize)
            .is_some_and(|kinds| kinds.iter().any(|full| *full));
        if holds_attention {
            slots * self.attention_heads * self.act_bytes * chunk * context
        } else {
            0
        }
    }

    /// The largest prefill chunk (a whole number of KV steps, at most `step`, the fork's own)
    /// whose scores fit what every stage's budget leaves above the fork's total; 0 when not even
    /// one KV step does.
    pub fn chunk(&self, plan: &PipelinePlan, step: u64) -> u64 {
        let slots = u64::from(plan.slots);
        plan.stages
            .iter()
            .filter_map(|stage| {
                let per_token = self.scores_bytes(stage, slots, 1, plan.context);
                (per_token > 0)
                    .then(|| stage.budget_bytes.saturating_sub(stage.total_bytes) / per_token)
            })
            .fold(step, u64::min)
            / KV_CACHE_STEP
            * KV_CACHE_STEP
    }

    /// The largest context (a multiple of the KV step, below the planned one) whose scores at one
    /// KV-step chunk fit every stage's room above the fork's total at `plan`'s context — the
    /// fork's own bytes shrink with the context, so the room there is at least this.
    pub fn context_ceiling(&self, plan: &PipelinePlan) -> u64 {
        let slots = u64::from(plan.slots);
        plan.stages
            .iter()
            .filter_map(|stage| {
                let per_context_token = self.scores_bytes(stage, slots, KV_CACHE_STEP, 1);
                (per_context_token > 0).then(|| {
                    stage.budget_bytes.saturating_sub(stage.total_bytes) / per_context_token
                })
            })
            .fold(plan.context.saturating_sub(KV_CACHE_STEP), u64::min)
            / KV_CACHE_STEP
            * KV_CACHE_STEP
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
            attention_heads: 24,
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

    /// E2E #1's live split (2026-09-25, rank 0's spec): the 27B over 2 ranks at a derived 141,568
    /// tokens, `prompt_cache_bytes` 4,715,872,256 — which mlx_lm reads as cached + LIVE. The launch
    /// now hands it the plan's whole KV charge, and the entry count holds as many of the smallest
    /// entries.
    #[test]
    fn the_e2e_split_hands_mlx_lm_the_plans_whole_kv_charge() {
        let facts = qwen_27b();
        let plan = facts.rank_plan(2, 0, 141_568, u64::MAX);
        assert_eq!(plan.state_bytes, 4_715_872_256);
        assert_eq!(
            plan.prompt_cache_bytes, 4_715_872_256,
            "the charge is unchanged"
        );
        assert_eq!(plan.prompt_cache_limit_bytes(), 9_431_744_512);
        // 256 tokens × 32 KiB + the recurrent state (76,972,032 B): a helper's 170-token prompt.
        assert_eq!(facts.smallest_cache_entry_bytes(2), 85_360_640);
        assert_eq!(plan.prompt_cache_entries, 110);

        // What the turn needed at the agent's admission: its own live copy of the 48,647-token
        // system prefix plus that prefix and the previous call's 55,749-token entry cached.
        let live = facts.sequence_bytes(2, 48_647);
        let needed = facts.sequence_bytes(2, 48_647) + facts.sequence_bytes(2, 55_749);
        assert!(
            plan.prompt_cache_bytes - live < needed,
            "the old flag's room"
        );
        assert!(plan.prompt_cache_limit_bytes() - live >= needed);
    }

    /// E2E #2 (2026-09-25, the Studio's GOOSE_RANK_CAPS): the 27B over 2 ranks at 262,144 tokens
    /// planned 35,358,285,312 B, and the rank set MLX's free-buffer cache limit to its ceiling
    /// less that — 48,135,889,408 B. The plan's own transient allowance is a tenth of the plan.
    #[test]
    fn the_mlx_buffer_cache_holds_the_plans_transient_allowance_only() {
        let facts = qwen_27b();
        let mut plan = facts.rank_plan(2, 1, 262_144, u64::MAX);
        assert_eq!(plan.prompt_cache_limit_bytes(), 17_333_813_248);
        // The fixture's weights sit 104,936 B under the owner's checkpoint; the rank's own report
        // is the figure.
        plan.planned_bytes = 35_358_285_312;
        plan.with_overhead_bytes = with_overhead(plan.planned_bytes) + plan.workspace_bytes;
        assert_eq!(plan.mlx_cache_limit_bytes(), 3_535_828_532);
        assert_eq!(
            plan.planned_bytes + plan.mlx_cache_limit_bytes() + plan.workspace_bytes,
            plan.with_overhead_bytes
        );
        let e2e_2_ceiling = 83_494_174_720u64;
        assert_eq!(e2e_2_ceiling - plan.planned_bytes, 48_135_889_408);
    }

    /// The ranks' budgets differ (the MacBook and the Studio), their cache bounds may not.
    #[test]
    fn every_tensor_rank_gets_the_same_prompt_cache_bounds() {
        use crate::distributed::launch::TensorLaunch;
        let facts = qwen_27b();
        let macbook = budget_bytes(gib(92.7), gib(128.0), M4_MAX_CEILING);
        let workhorse = budget_bytes(gib(61.6), gib(96.0), M3_ULTRA_CEILING);
        let context = facts.max_context(2, macbook.min(workhorse));
        let plans = [
            facts.rank_plan(2, 0, context, macbook),
            facts.rank_plan(2, 1, context, workhorse),
        ];
        let launches = TensorLaunch::for_ranks(&[&plans[0], &plans[1]]).unwrap();
        assert_eq!(
            launches[0].prompt_cache_limit_bytes,
            launches[1].prompt_cache_limit_bytes
        );
        let mut skewed = plans[1].clone();
        skewed.prompt_cache_entries += 1;
        let refusal = TensorLaunch::for_ranks(&[&plans[0], &skewed])
            .unwrap_err()
            .to_string();
        assert!(
            refusal.contains("rank 1") && refusal.contains("evict identically"),
            "{refusal}"
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

    /// Q-104's measurement (2026-09-26, 2 localhost ranks of the owner's 27B layers under
    /// mlx_lm 0.31.3's own BatchGenerator): one row, chunk 2,048, a 131,072-token prefix read to
    /// 135,168 peaked 8.049 GB above the weights on its 8-layer view — the row's KV (4,096 B a
    /// token on that view), the chunk's other transients (0.59 GB, measured on the 4-layer view
    /// whose last attention layer never runs), and this arithmetic's scores + mask.
    #[test]
    fn the_prefill_workspace_is_the_measured_step_of_one_row() {
        let facts = qwen_27b();
        assert_eq!(facts.prefill_pair_bytes(2), 12 * 2 + 1);
        let scores_and_mask = facts.prefill_workspace_bytes(2, 2_048, 135_168);
        let predicted = scores_and_mask + 135_168 * 4_096 + 590_000_000;
        let measured = 8_049_000_000u64;
        assert!(
            predicted.abs_diff(measured) * 100 < measured,
            "within 1%: {predicted} vs {measured}"
        );
        // The same rule at the planned window is what E2E #2's plan (262,144 tokens) never
        // charged: 13.42 GB for one row's chunk against the 3.54 GB its overhead ratio granted.
        let plan = facts.rank_plan(2, 1, 262_144, u64::MAX);
        assert_eq!(plan.workspace_bytes, 2_048 * 262_144 * 25);
        assert_eq!(plan.prefill_step, 2_048);
        assert!(plan.workspace_bytes > 3 * plan.mlx_cache_limit_bytes());
        assert_eq!(
            plan.with_overhead_bytes,
            with_overhead(plan.planned_bytes) + plan.workspace_bytes
        );
    }

    /// The workspace is what the rank's budget leaves above the resident plan, in KV-step chunks
    /// of one row at the full context — never more than mlx_lm's own step, never less than one KV
    /// step (charged even when it does not fit, so the verdict names it).
    #[test]
    fn the_prefill_chunk_is_a_ratio_of_the_ranks_headroom() {
        let facts = qwen_27b();
        let workhorse = budget_bytes(gib(61.6), gib(96.0), M3_ULTRA_CEILING);
        let context = 131_072;
        let roomy = facts.rank_plan(2, 1, context, workhorse);
        assert_eq!(roomy.prefill_step, TENSOR_PREFILL_STEP);
        assert_eq!(roomy.full_context_chunk(context), Some(2_048));
        assert!(roomy.fits);

        // A budget that leaves 1/4 of the default chunk's scores above the resident bytes.
        let resident = with_overhead(roomy.planned_bytes);
        let quarter = resident + facts.prefill_workspace_bytes(2, 512, context);
        let tight = facts.rank_plan(2, 1, context, quarter);
        assert_eq!(tight.full_context_chunk(context), Some(512));
        assert_eq!(tight.with_overhead_bytes, quarter);
        assert!(tight.fits);

        // Below one KV step's scores the plan does not fit, and says by how much.
        let short = facts.rank_plan(2, 1, context, resident + 1);
        assert_eq!(
            short.workspace_bytes,
            facts.prefill_workspace_bytes(2, 256, context)
        );
        assert!(!short.fits);

        // The context ceiling holds the smallest chunk: at it a 256-token step fits, one KV step
        // wider does not.
        let ceiling = facts.max_context(2, workhorse);
        let at = facts.rank_plan(2, 1, ceiling, workhorse);
        assert!(at.fits && at.full_context_chunk(ceiling).unwrap() >= 256);
    }

    /// A step's chunk shrinks as the batch widens so its scores stay inside the workspace.
    #[test]
    fn a_wider_batch_takes_a_smaller_chunk_inside_the_same_workspace() {
        let prefill = qwen_27b().prefill(2, 2_048 * 262_144 * 25);
        assert_eq!(prefill.chunk(1, 262_144), 2_048);
        assert_eq!(
            prefill.chunk(5, 51_200),
            2_048,
            "E2E #2's five rows at ~51k fit whole"
        );
        assert_eq!(prefill.chunk(5, 100_000), 1_024);
        assert_eq!(prefill.chunk(8, 262_144), 256);
        assert_eq!(
            prefill.chunk(9, 262_144),
            0,
            "not one KV step: admission keeps it away"
        );
        for (rows, width) in [(1, 262_144), (5, 51_200), (5, 100_000), (8, 262_144)] {
            let chunk = prefill.chunk(rows, width);
            assert!(rows * width * chunk * prefill.pair_bytes <= prefill.workspace_bytes);
        }
    }

    /// Every rank of a launch runs the smallest workspace any rank affords (the MacBook's
    /// headroom is larger than the Studio's; the chunk a step takes must agree).
    #[test]
    fn every_tensor_rank_runs_the_smallest_workspace() {
        use crate::distributed::launch::TensorLaunch;
        let facts = qwen_27b();
        let context = 262_144;
        let small = with_overhead(facts.planned_at(2, context))
            + facts.prefill_workspace_bytes(2, 768, context);
        let plans = [
            facts.rank_plan(2, 0, context, u64::MAX),
            facts.rank_plan(2, 1, context, small),
        ];
        assert_ne!(plans[0].workspace_bytes, plans[1].workspace_bytes);
        let launches = TensorLaunch::for_ranks(&[&plans[0], &plans[1]]).unwrap();
        assert_eq!(launches[0].prefill, launches[1].prefill);
        assert_eq!(
            launches[0].prefill.workspace_bytes,
            facts.prefill_workspace_bytes(2, 768, context)
        );
        let shared = plans[0].clone().sharing_workspace(plans[1].workspace_bytes);
        assert_eq!(shared.workspace_bytes, plans[1].workspace_bytes);
        assert_eq!(
            shared.with_overhead_bytes,
            with_overhead(shared.planned_bytes) + shared.workspace_bytes
        );

        // A plan made before Q-104 carries no prefill figures: named, never launched on guesses.
        let mut older = plans[0].clone();
        older.prefill = None;
        let refusal = TensorLaunch::for_ranks(&[&older, &plans[1]])
            .unwrap_err()
            .to_string();
        assert!(refusal.contains("no prefill figures"), "{refusal}");
    }

    /// The fork's plan for Flash at 32,768 tokens, batch 2 (FLASH_PLAN_32K) leaves rank 1
    /// 13.93 GB above its total; a 2,048-token chunk of 2 rows materializes 24 heads × 2 B ×
    /// 2,048 × 32,768 × 2 = 6.44 GB of scores its workspace (4.52 GB) does not model.
    #[test]
    fn the_pipeline_chunk_fits_the_scores_the_forks_workspace_leaves_out() {
        let dir = flash_attention_dir();
        let attention = read_pipeline_attention(dir.path()).unwrap();
        assert_eq!(attention.attention_heads, 24);
        assert_eq!(attention.act_bytes, 2);
        assert_eq!(attention.full_attention.iter().filter(|f| **f).count(), 12);
        let plan = parse_pipeline_plan(FLASH_PLAN_32K).unwrap();
        let rank1 = &plan.stages[1];
        assert_eq!(
            attention.scores_bytes(rank1, 2, 2_048, 32_768),
            6_442_450_944
        );
        assert_eq!(attention.chunk(&plan, plan.prefill_step), 2_048);
        let charged = rank1.rank_plan_with_scores(6_442_450_944, 2_048);
        assert_eq!(charged.with_overhead_bytes, 48_424_487_136 + 6_442_450_944);
        assert_eq!(charged.workspace_bytes, 4_515_057_664 + 6_442_450_944);
        assert_eq!(charged.attention_scores_bytes, Some(6_442_450_944));
        assert!(charged.fits);

        // The same room at the model's full 262,144 tokens: a 2,048-token chunk would need 51.5
        // GB; the largest multiple of 256 inside rank 1's 13.93 GB is 512 (12.9 GB).
        let full = FLASH_PLAN_32K.replacen("\"context\": 32768", "\"context\": 262144", 1);
        let full = parse_pipeline_plan(&full).unwrap();
        assert_eq!(
            attention.scores_bytes(&full.stages[1], 2, 2_048, 262_144),
            51_539_607_552
        );
        assert_eq!(attention.chunk(&full, full.prefill_step), 512);

        // A stage with no full-attention layer carries no scores: [0, 3) is linear only.
        let mut linear = plan.stages[0].clone();
        linear.layer_end = 3;
        assert_eq!(attention.scores_bytes(&linear, 2, 2_048, 32_768), 0);

        // Room for less than one KV step: chunk 0, and the ceiling names the context that fits.
        let squeezed = FLASH_PLAN_32K.replacen(
            "\"budget_bytes\": 62354335204",
            "\"budget_bytes\": 49000000000",
            1,
        );
        let squeezed = parse_pipeline_plan(&squeezed).unwrap();
        assert_eq!(attention.chunk(&squeezed, squeezed.prefill_step), 0);
        let ceiling = attention.context_ceiling(&squeezed);
        // (49,000,000,000 − 48,424,487,136) / (2 × 24 × 2 × 256) = 23,417 → 23,296.
        assert_eq!(ceiling, 23_296);
    }

    /// The owner's Flash checkpoint, when present on this Mac: its attention layout reads as the
    /// fixture above says.
    #[test]
    fn the_owners_flash_attention_reads_as_recorded_when_present() {
        let dir = dirs::home_dir()
            .unwrap()
            .join(".goose/models/rapid-mlx/Qwen3.8-Flash-Next-4bit");
        if !dir.join("config.json").exists() {
            eprintln!("skipped: {} absent", dir.display());
            return;
        }
        let attention = read_pipeline_attention(&dir).unwrap();
        let fixture = read_pipeline_attention(flash_attention_dir().path()).unwrap();
        assert_eq!(attention, fixture);
    }

    /// The fork planner's real JSON for Flash (2f02ac645, `plan --json --model <Flash> --node
    /// m:128:90:107.52 --node w:96:67:77.76 --context 32768 --batch 2`, 2026-09-24 — the GPU
    /// ceilings are the two Macs' measured max_recommended_working_set_size): exit 0.
    pub(crate) const FLASH_PLAN_32K: &str = r#"{"context": 32768, "batch": 2, "prefill_step": 2048, "max_context": 262144, "starts": [0, 19], "slots": 2, "fits": true, "vision": {"rank": 0, "weight_bytes": 448092512, "workspace_bytes": 1229312000}, "checkpoint": {"text_bytes": 102766153240, "head_bytes": 357580800, "tail_bytes": 361287680, "excluded_bytes": {"mtp": 1467242656, "vision": 448092512}, "layer_bytes": [1459858528, 33478587768, 1459858528, 1456137376, 1459858528, 1459858528, 1459858528, 1456137376, 1459858528, 1459858528, 1459858528, 1456137376, 1459858528, 1459858528, 1459858528, 1456137376, 1459858528, 1459858528, 1459858528, 1456137376, 1459858528, 1459858528, 1459858528, 1456137376, 1459858528, 1459858528, 1459858528, 1456137376, 1459858528, 1459858528, 1459858528, 1456137376, 1459858528, 1459858528, 1459858528, 1456137376, 1459858528, 1459858528, 1459858528, 1456137376, 1459858528, 1459858528, 1459858528, 1456137376, 1459858528, 1459858528, 1459858528, 1456137376]}, "ratios": {"available_margin": 0.093, "layer_transient_stream_multiple": 49}, "wire": {"stream_per_hop": 20480, "hops": 1, "token_broadcast": 4, "total_per_token": 20484}, "stages": [{"rank": 0, "node": "m", "layer_start": 0, "layer_end": 19, "weight_bytes": 60546829976, "state_bytes": 650240032, "workspace_bytes": 4513071104, "total_bytes": 65710141112, "budget_bytes": 83854941488, "available_bytes": 96636764160, "ceiling_bytes": 115448720916, "ram_bytes": 137438953472, "budget_source": "free + GPU ceiling given", "fits": true}, {"rank": 1, "node": "w", "layer_start": 19, "layer_end": 48, "weight_bytes": 42667415776, "state_bytes": 1242013696, "workspace_bytes": 4515057664, "total_bytes": 48424487136, "budget_bytes": 62354335204, "available_bytes": 71940702208, "ceiling_bytes": 83494164234, "ram_bytes": 103079215104, "budget_source": "free + GPU ceiling given", "fits": true}]}"#;

    /// A directory holding what `read_pipeline_attention` reads of the Flash checkpoint: its
    /// config's 48 layer kinds (every fourth full attention) and 24 query heads, and a
    /// safetensors header whose embedding scales are BF16.
    pub(crate) fn flash_attention_dir() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let kinds: Vec<&str> = (0..48)
            .map(|i| {
                if (i + 1) % 4 == 0 {
                    "full_attention"
                } else {
                    "linear_attention"
                }
            })
            .collect();
        let config = serde_json::json!({
            "model_type": "qwen4_exp",
            "text_config": {"layer_types": kinds, "num_attention_heads": 24}
        });
        std::fs::write(dir.path().join("config.json"), config.to_string()).unwrap();
        let header = serde_json::json!({
            "language_model.model.embed_tokens.scales":
                {"dtype": "BF16", "shape": [1], "data_offsets": [0, 2]}
        })
        .to_string();
        let mut shard = (header.len() as u64).to_le_bytes().to_vec();
        shard.extend_from_slice(header.as_bytes());
        shard.extend_from_slice(&[0, 0]);
        std::fs::write(dir.path().join("model-00001-of-00001.safetensors"), shard).unwrap();
        dir
    }

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
