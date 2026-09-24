//! The rule (design §2), deterministic: enumerate every placement the model's architecture allows
//! AND goose can run, drop what does not fit (weights + KV for the context ≤ each Mac's budget —
//! `crate::fit`, the ONE fit rule the single engine's mount, both split runners and the preflight
//! share), predict each survivor
//! (measured if goose has measured it, else the calibrated formula, labelled), pick by the goal,
//! and say why every other placement lost. The planner is pure: the ACP layer measures the Macs,
//! reads the store and runs the fork's planner, then hands the facts in.

use serde::{Deserialize, Serialize};

use super::bench::Workload;
use super::chip::ChipIdentity;
use super::model::ModelFacts;
use super::predict::{self, Calibration, Estimate, PlacedNode};
use super::store::{PlacementKey, PlacementKind, SpeedRecord};
use crate::distributed::plan::TensorModelFacts;
use crate::distributed::Runner;
use crate::fit::{self, Need, NodeMemoryFacts, Verdict};
use crate::kv_cache::KvCacheMode;

/// The serving stack behind each placement kind.
pub const BACKEND_SINGLE: &str = "rapid-mlx";
pub const BACKEND_TENSOR: &str = "mlx_lm";
pub const BACKEND_PIPELINE: &str = "pipeline_qwen4";

pub fn backend_of(kind: PlacementKind) -> &'static str {
    match kind {
        PlacementKind::Single => BACKEND_SINGLE,
        PlacementKind::Tensor => BACKEND_TENSOR,
        PlacementKind::Pipeline => BACKEND_PIPELINE,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Goal {
    /// The fastest single conversation: decode tok/s.
    #[default]
    Chat,
    /// Reading long prompts: prefill tok/s.
    LongDocuments,
    /// Many requests at once: total tok/s.
    ManyRequests,
}

impl Goal {
    /// The benchmark workload whose shape the goal's figure is measured at.
    pub fn workload(self) -> Workload {
        match self {
            Goal::LongDocuments => Workload::LongDocument,
            Goal::Chat | Goal::ManyRequests => Workload::Chat,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct NodeMemory {
    pub total_bytes: u64,
    pub available_bytes: u64,
    /// Metal's working-set ceiling; `Err` = it could not be read on that node.
    pub ceiling_bytes: Result<u64, String>,
}

/// One Mac as measured just now. `nodes[0]` of a plan input is this Mac.
#[derive(Debug, Clone, PartialEq)]
pub struct NodeInput {
    /// `local` for this Mac, else the configured host (ssh alias or `link:<id>`).
    pub id: String,
    pub name: String,
    pub chip: Result<ChipIdentity, String>,
    pub memory: Result<NodeMemory, String>,
    /// Whether this node holds the model (same directory name, same loaded files and sizes).
    pub has_model: Result<bool, String>,
    /// A peer: whether goose can start its single engine and chat with it (remote single over
    /// LeanZero Link), or why not. Unused for this Mac.
    pub remote_single: Result<(), String>,
    /// Why this Mac cannot serve a rank of a split right now, in the step that fixes it ("allow
    /// this Mac to serve as a distributed node on <name>"); `None` = nothing known against it.
    pub split_refusal: Option<String>,
}

impl NodeInput {
    pub fn is_local(&self) -> bool {
        self.id == "local"
    }
}

/// The Macs a split would run on: this Mac's saved distributed setup, or — when this Mac has none
/// — the Macs it reaches over LeanZero Link (a split is still what the model needs, and the badge
/// says so from either Mac; Set up is the step before it can start).
#[derive(Debug, Clone, PartialEq)]
pub struct ClusterInput {
    /// "jaccl" | "ring"; `None` = no setup names the link yet.
    pub link: Option<String>,
    /// Pipeline slots (full-context sequences the split is planned for).
    pub slots: u32,
    /// The model the saved setup names; a split of another model starts after Set up picks it.
    /// `None` = no split is set up on this Mac.
    pub config_model_id: Option<String>,
}

/// What the fork's planner (`pipeline_qwen4 plan --json`) said for this model on these Macs.
#[derive(Debug, Clone, PartialEq)]
pub struct PipelineFitInput {
    pub fits: bool,
    pub context: Option<u64>,
    /// Per stage, in rank order: (bytes planned, budget).
    pub stages: Vec<(u64, u64)>,
    /// Each stage's share of the layers.
    pub layer_shares: Vec<f64>,
}

pub struct PlanInput<'a> {
    pub model_id: &'a str,
    pub model: &'a ModelFacts,
    /// What the single engine's mount gate charges: the model directory's size on disk.
    pub bytes_on_disk: u64,
    /// The single engine's KV cache setting for this model (its profile).
    pub kv_mode: Option<KvCacheMode>,
    pub nodes: &'a [NodeInput],
    pub cluster: Option<&'a ClusterInput>,
    /// The tensor runner's arithmetic (qwen3_5), when the architecture has it.
    pub tensor: Option<&'a Result<TensorModelFacts, String>>,
    /// The fork planner's answer; `None` = not asked (its environment is not on this Mac).
    pub pipeline: Option<&'a Result<PipelineFitInput, String>>,
    pub goal: Goal,
    /// The context wanted; `None` = the largest that fits, capped at the model's own maximum.
    pub context: Option<u64>,
    pub records: &'a [SpeedRecord],
    pub calibration: &'a Calibration,
    /// The placement serving THIS model right now (by candidate id) and its context window: it
    /// fits by construction — its memory is already in use, so this Mac's available figure cannot
    /// judge it.
    pub running: Option<(String, Option<u64>)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FitStatus {
    Fits,
    /// Fits, but only at a smaller context than wanted (shown).
    SmallerContext,
    Short,
    Unknown,
}

impl FitStatus {
    pub fn fits(self) -> bool {
        matches!(self, FitStatus::Fits | FitStatus::SmallerContext)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeFit {
    pub name: String,
    pub need_bytes: u64,
    pub budget_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Fit {
    pub status: FitStatus,
    /// The context this placement runs at (requested, or the largest that fits).
    pub context: Option<u64>,
    /// How much more memory the worst node needs for the smallest useful context.
    pub short_bytes: Option<u64>,
    /// The node that is short (by name).
    pub short_node: Option<String>,
    pub nodes: Vec<NodeFit>,
    /// The arithmetic in words, with its source; a refusal's own message verbatim.
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Figure {
    pub estimate: Estimate,
    pub measured: bool,
    /// Measured runs behind it (0 for an estimate).
    pub runs: u32,
    pub last_measured_ms: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Speed {
    pub decode: Option<Figure>,
    pub prefill: Option<Figure>,
    pub throughput: Option<Figure>,
    pub concurrency: Option<u32>,
    pub basis: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum Action {
    /// Mount it on this Mac's single engine.
    MountHere,
    /// Start the distributed engine; `setupMatches` = the saved setup already names this model.
    StartSplit { setup_matches: bool },
    /// Start the single engine on the peer and chat through Link.
    RemoteSingle,
    /// Nothing goose can start for it today — why.
    Unavailable { reason: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "code"
)]
pub enum Outcome {
    Best,
    /// The fastest placement cannot be started yet; this one is the best that can.
    BestAvailableNow,
    NotSupported {
        reason: String,
    },
    DoesNotFit,
    FitUnknown {
        reason: String,
    },
    NoFigure {
        reason: String,
    },
    Slower {
        mine: f64,
        best: f64,
    },
    /// Within the error of the best, which needs fewer Macs.
    TiedNeedsMoreMacs {
        mine: f64,
        best: f64,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub id: String,
    pub key: PlacementKey,
    pub node_names: Vec<String>,
    pub chips: Vec<Option<ChipIdentity>>,
    pub backend: String,
    pub supported: bool,
    pub fit: Fit,
    pub speed: Speed,
    pub action: Action,
    pub outcome: Outcome,
}

impl Candidate {
    fn metric(&self, goal: Goal) -> Option<&Figure> {
        match goal {
            Goal::Chat => self.speed.decode.as_ref(),
            Goal::LongDocuments => self.speed.prefill.as_ref(),
            Goal::ManyRequests => self.speed.throughput.as_ref(),
        }
    }

    /// Startable by [Use this] as things stand: not an unavailable one, and not a split whose saved
    /// setup names another model (Set up must pick this one first).
    fn runnable(&self) -> bool {
        !matches!(
            self.action,
            Action::Unavailable { .. }
                | Action::StartSplit {
                    setup_matches: false
                }
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum Badge {
    FitsThisMac,
    FitsPeer {
        name: String,
    },
    /// Only a split fits. `needs` = the step before it can start ("allow this Mac to serve as a
    /// distributed node on <name>", "set up the split with this model"); absent = startable now.
    NeedsBothMacs {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        needs: Option<String>,
    },
    TooBig {
        short_bytes: u64,
    },
    Unknown {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub model_id: String,
    pub goal: Goal,
    /// Best first, then every other candidate in the order it lost.
    pub candidates: Vec<Candidate>,
    /// The fastest placement for the goal (by id), runnable or not.
    pub best: Option<String>,
    /// The fastest placement goose can start today (differs from `best` while the best needs a
    /// piece that has not landed).
    pub best_available: Option<String>,
    pub badge: Badge,
    /// Things the plan could not consider, in words ("no second Mac is set up").
    pub notes: Vec<String>,
}

fn gib(bytes: u64) -> String {
    format!("{:.1} GiB", bytes as f64 / crate::GIB as f64)
}

struct Budget {
    budget: u64,
    facts: NodeMemoryFacts,
}

fn node_budget(node: &NodeInput) -> Result<Budget, String> {
    let memory = node
        .memory
        .as_ref()
        .map_err(|e| format!("{}: {e}", node.name))?;
    let ceiling = memory
        .ceiling_bytes
        .as_ref()
        .map_err(|e| format!("{}: GPU ceiling unknown — {e}", node.name))?;
    let facts = NodeMemoryFacts {
        available_bytes: memory.available_bytes,
        total_bytes: memory.total_bytes,
        ceiling_bytes: *ceiling,
    };
    Ok(Budget {
        budget: facts.budget_bytes(),
        facts,
    })
}

/// The smallest context a placement must hold to be useful: the chat benchmark's shape.
pub fn min_useful_context() -> u64 {
    Workload::Chat.context_needed()
}

/// KV bytes per token at the model's KV cache setting, or why it cannot be sized.
pub fn kv_bytes_per_token(model: &ModelFacts, kv_mode: Option<KvCacheMode>) -> Result<u64, String> {
    model.kv.as_ref().map_err(Clone::clone).and_then(|kv| {
        kv.bytes_per_token(kv_mode)
            .ok_or_else(|| "the KV cache setting cannot be applied to this model".to_string())
    })
}

/// What the single engine needs to serve the smallest useful context: the need the mount gate
/// judges and the planner's single-engine fit — one need, so both answer alike.
pub fn single_engine_need(
    weights_bytes: u64,
    model: Result<&ModelFacts, String>,
    kv_mode: Option<KvCacheMode>,
) -> Need {
    Need::single_engine(
        weights_bytes,
        model.and_then(|m| kv_bytes_per_token(m, kv_mode)),
        min_useful_context(),
    )
}

fn unknown_fit(reason: String) -> Fit {
    Fit {
        status: FitStatus::Unknown,
        context: None,
        short_bytes: None,
        short_node: None,
        nodes: Vec::new(),
        detail: reason,
    }
}

/// Status from the largest context that fits, the context wanted and the smallest useful one.
fn status_for(max_fit: u64, wanted: u64, min_useful: u64) -> (FitStatus, Option<u64>) {
    if max_fit >= wanted {
        (FitStatus::Fits, Some(wanted))
    } else if max_fit >= min_useful {
        (FitStatus::SmallerContext, Some(max_fit))
    } else {
        (FitStatus::Short, None)
    }
}

/// A single engine on one Mac: the one fit rule (`crate::fit::judge`) on the need at the smallest
/// useful context decides whether it fits at all — the SAME verdict the mount gate reaches on
/// that Mac — and the budget left above the weights sizes the context it can run at.
fn single_fit(input: &PlanInput, node: &NodeInput, min_useful: u64) -> Fit {
    let b = match node_budget(node) {
        Ok(b) => b,
        Err(reason) => return unknown_fit(reason),
    };
    let weights = input.bytes_on_disk;
    let verdict = fit::judge(
        single_engine_need(weights, Ok(input.model), input.kv_mode),
        b.facts,
    );
    let node_fit = |need_bytes| {
        vec![NodeFit {
            name: node.name.clone(),
            need_bytes,
            budget_bytes: b.budget,
        }]
    };
    if verdict.verdict == Verdict::Block {
        return Fit {
            status: FitStatus::Short,
            context: None,
            short_bytes: verdict.short_bytes(),
            short_node: Some(node.name.clone()),
            nodes: node_fit(verdict.need.total_bytes()),
            detail: format!("{}: {}", node.name, verdict.message),
        };
    }
    let model_max = input.model.max_context;
    match kv_bytes_per_token(input.model, input.kv_mode) {
        Ok(per_token) if per_token > 0 => {
            let room = b.budget.saturating_sub(weights);
            let max_fit = (room / per_token).min(model_max.unwrap_or(u64::MAX));
            let wanted = input.context.or(model_max).unwrap_or(max_fit);
            let (status, context) = status_for(max_fit, wanted, min_useful);
            Fit {
                status,
                context,
                short_bytes: None,
                short_node: None,
                nodes: node_fit(weights + per_token * context.unwrap_or(min_useful)),
                detail: format!(
                    "{}: {} of weights + {} of KV per 1k tokens; {}",
                    node.name,
                    gib(weights),
                    gib(per_token * 1024),
                    verdict.message
                ),
            }
        }
        other => {
            let reason = match other {
                Err(e) => e,
                Ok(_) => "the model has no growing KV cache".to_string(),
            };
            Fit {
                status: FitStatus::Fits,
                context: None,
                short_bytes: None,
                short_node: None,
                nodes: node_fit(weights),
                detail: format!(
                    "{}: context not sized ({reason}); {}",
                    node.name, verdict.message
                ),
            }
        }
    }
}

fn tensor_fit(input: &PlanInput, facts: &TensorModelFacts, min_useful: u64) -> Fit {
    let ranks = input.nodes.len() as u64;
    let budgets: Result<Vec<Budget>, String> = input.nodes.iter().map(node_budget).collect();
    let budgets = match budgets {
        Ok(b) => b,
        Err(reason) => return unknown_fit(reason),
    };
    let max_fit = budgets
        .iter()
        .map(|b| facts.max_context(ranks, b.budget))
        .min()
        .unwrap_or(0);
    let wanted = input.context.unwrap_or(facts.max_position);
    let (status, context) = status_for(max_fit, wanted, min_useful);
    let at = context.unwrap_or(min_useful);
    let nodes: Vec<NodeFit> = input
        .nodes
        .iter()
        .zip(&budgets)
        .enumerate()
        .map(|(rank, (node, b))| {
            let plan = facts.rank_plan(ranks, rank as u64, at, b.budget);
            NodeFit {
                name: node.name.clone(),
                need_bytes: plan.with_overhead_bytes,
                budget_bytes: b.budget,
            }
        })
        .collect();
    let worst = nodes
        .iter()
        .max_by_key(|n| n.need_bytes.saturating_sub(n.budget_bytes))
        .cloned();
    Fit {
        status,
        context,
        short_bytes: (status == FitStatus::Short)
            .then(|| {
                worst
                    .as_ref()
                    .map(|w| w.need_bytes.saturating_sub(w.budget_bytes))
            })
            .flatten(),
        short_node: (status == FitStatus::Short)
            .then(|| worst.map(|w| w.name))
            .flatten(),
        detail: format!(
            "the tensor runner's arithmetic: every rank holds 1/{ranks} of each layer plus the \
             unsharded embeddings and head, KV and a prompt cache for the context, × the measured \
             runtime overhead; largest context every rank fits: {max_fit}"
        ),
        nodes,
    }
}

fn pipeline_fit(
    input: &PlanInput,
    cluster: &ClusterInput,
    min_useful: u64,
) -> (Fit, Option<Vec<f64>>) {
    match input.pipeline {
        Some(Err(reason)) => (unknown_fit(format!("the fork's planner: {reason}")), None),
        Some(Ok(plan)) => {
            let status = match (plan.fits, plan.context) {
                (true, Some(ctx)) if input.context.is_none_or(|w| ctx >= w) => FitStatus::Fits,
                (true, Some(ctx)) if ctx >= min_useful => FitStatus::SmallerContext,
                (true, None) => FitStatus::Fits,
                _ => FitStatus::Short,
            };
            let nodes: Vec<NodeFit> = input
                .nodes
                .iter()
                .zip(&plan.stages)
                .map(|(n, (need, budget))| NodeFit {
                    name: n.name.clone(),
                    need_bytes: *need,
                    budget_bytes: *budget,
                })
                .collect();
            let worst = nodes
                .iter()
                .max_by_key(|n| n.need_bytes.saturating_sub(n.budget_bytes))
                .cloned();
            let fit = Fit {
                status,
                context: status.fits().then_some(plan.context).flatten(),
                short_bytes: (status == FitStatus::Short)
                    .then(|| {
                        worst
                            .as_ref()
                            .map(|w| w.need_bytes.saturating_sub(w.budget_bytes))
                    })
                    .flatten(),
                short_node: (status == FitStatus::Short)
                    .then(|| worst.map(|w| w.name))
                    .flatten(),
                nodes,
                detail: format!(
                    "the fork's planner (pipeline_qwen4 plan) at {} slots: {}",
                    cluster.slots,
                    if plan.fits { "fits" } else { "does not fit" }
                ),
            };
            (fit, Some(plan.layer_shares.clone()))
        }
        None => {
            // No fork planner on this Mac: the aggregate arithmetic, labelled as such.
            let budgets: Result<Vec<Budget>, String> =
                input.nodes.iter().map(node_budget).collect();
            let budgets = match budgets {
                Ok(b) => b,
                Err(reason) => return (unknown_fit(reason), None),
            };
            let total_budget: u64 = budgets.iter().map(|b| b.budget).sum();
            let resident = input.model.resident_bytes;
            let per_token = input
                .model
                .kv
                .as_ref()
                .map(|kv| kv.bf16_bytes_per_token * cluster.slots as u64)
                .unwrap_or(0);
            let largest_ok = budgets
                .iter()
                .any(|b| b.budget >= input.model.largest_layer_bytes);
            let room = total_budget.saturating_sub(resident);
            let max_fit = if !largest_ok {
                0
            } else if per_token == 0 {
                input.model.max_context.unwrap_or(0)
            } else {
                (room / per_token).min(input.model.max_context.unwrap_or(u64::MAX))
            };
            let wanted = input.context.or(input.model.max_context).unwrap_or(max_fit);
            let (status, context) = status_for(max_fit, wanted, min_useful);
            let need = resident + per_token * context.unwrap_or(min_useful);
            let shares: Vec<f64> = budgets
                .iter()
                .map(|b| b.budget as f64 / total_budget.max(1) as f64)
                .collect();
            let fit = Fit {
                status,
                context,
                short_bytes: (status == FitStatus::Short).then(|| need.saturating_sub(total_budget)),
                short_node: None,
                nodes: input
                    .nodes
                    .iter()
                    .zip(&budgets)
                    .zip(&shares)
                    .map(|((n, b), share)| NodeFit {
                        name: n.name.clone(),
                        need_bytes: (need as f64 * share) as u64,
                        budget_bytes: b.budget,
                    })
                    .collect(),
                detail: format!(
                    "estimate — the fork's planner is not provisioned on this Mac, so the split is \
                     sized in aggregate: {} of weights + KV for {} slots against {} across the Macs \
                     (its largest layer, {}, must fit one Mac); the planner decides at Start",
                    gib(resident),
                    cluster.slots,
                    gib(total_budget),
                    gib(input.model.largest_layer_bytes)
                ),
            };
            (fit, Some(shares))
        }
    }
}

/// Goose's measurements for this placement at the goal's shape.
fn measured(
    records: &[&SpeedRecord],
    bucket: u64,
    pick: impl Fn(&SpeedRecord) -> Option<f64>,
) -> Option<Figure> {
    let hits: Vec<&&SpeedRecord> = records
        .iter()
        .filter(|r| r.context_bucket == bucket)
        .collect();
    let values: Vec<f64> = hits.iter().filter_map(|r| pick(r)).collect();
    Estimate::of_measurements(&values).map(|estimate| Figure {
        estimate,
        measured: true,
        runs: values.len() as u32,
        last_measured_ms: hits.iter().map(|r| r.recorded_at_ms).max(),
    })
}

fn estimated(estimate: Option<Estimate>) -> Option<Figure> {
    estimate.map(|estimate| Figure {
        estimate,
        measured: false,
        runs: 0,
        last_measured_ms: None,
    })
}

/// The single engine's gain over the mlx_lm-calibrated formula for THIS model: its measured
/// decode ÷ the formula on the same chip (Rapid-MLX's MTP drafting and scheduler are not in the
/// seed runs). `None` until goose has measured this model on the single engine.
fn single_engine_factor(
    input: &PlanInput,
    pick: fn(&SpeedRecord) -> Option<f64>,
    figure: fn(&predict::SpeedEstimate) -> Option<Estimate>,
) -> Option<(f64, usize)> {
    let bucket = Workload::Chat.bucket();
    let ratios: Vec<f64> = input
        .records
        .iter()
        .filter(|r| {
            r.model_id == input.model_id
                && r.backend == BACKEND_SINGLE
                && r.placement.kind == PlacementKind::Single
                && r.context_bucket == bucket
        })
        .filter_map(|r| {
            let chip = r.chips.first()?.clone()?;
            let formula = predict::estimate(
                input.calibration,
                PlacementKind::Single,
                None,
                &[PlacedNode { chip, share: 1.0 }],
                input.model,
                kv_per_token(input),
                bucket,
            );
            Some(pick(r)? / figure(&formula)?.value)
        })
        .collect();
    let n = ratios.len();
    Estimate::of_measurements(&ratios).map(|e| (e.value, n))
}

fn kv_per_token(input: &PlanInput) -> u64 {
    input
        .model
        .kv
        .as_ref()
        .ok()
        .and_then(|kv| kv.bytes_per_token(input.kv_mode))
        .unwrap_or(0)
}

fn speed_for(input: &PlanInput, key: &PlacementKey, nodes: &[&NodeInput], shares: &[f64]) -> Speed {
    let backend = backend_of(key.kind);
    let mine: Vec<&SpeedRecord> = input
        .records
        .iter()
        .filter(|r| r.model_id == input.model_id && &r.placement == key && r.backend == backend)
        .collect();
    let chat_bucket = Workload::Chat.bucket();
    let goal_bucket = input.goal.workload().bucket();
    let mut speed = Speed::default();
    let chips: Result<Vec<PlacedNode>, String> = nodes
        .iter()
        .zip(shares)
        .map(|(n, share)| {
            n.chip
                .clone()
                .map(|chip| PlacedNode {
                    chip,
                    share: *share,
                })
                .map_err(|e| format!("{}'s chip is unknown: {e}", n.name))
        })
        .collect();
    let formula = match &chips {
        Ok(placed) => Some(predict::estimate(
            input.calibration,
            key.kind,
            key.link.as_deref(),
            placed,
            input.model,
            if key.kind == PlacementKind::Single {
                kv_per_token(input)
            } else {
                input
                    .model
                    .kv
                    .as_ref()
                    .map(|k| k.bf16_bytes_per_token)
                    .unwrap_or(0)
            },
            chat_bucket,
        )),
        Err(gap) => {
            speed.basis.push(format!("no speed estimate: {gap}"));
            None
        }
    };
    let mut decode_estimate = formula.as_ref().and_then(|f| f.decode);
    let mut prefill_estimate = formula.as_ref().and_then(|f| f.prefill);
    if key.kind == PlacementKind::Single {
        match single_engine_factor(input, |r| r.decode_tps, |f| f.decode) {
            Some((factor, runs)) => {
                decode_estimate = decode_estimate.map(|e| Estimate { value: e.value * factor, low: e.low * factor, high: e.high * factor });
                speed.basis.push(format!(
                    "decode ×{factor:.2}: the single engine measured against the formula for this model ({runs} run(s))"
                ));
            }
            None => speed.basis.push(
                "estimated from mlx_lm runs; the single engine (Rapid-MLX, MTP drafting) has not been measured with this model yet".to_string(),
            ),
        }
        if let Some((factor, _)) = single_engine_factor(input, |r| r.prefill_tps, |f| f.prefill) {
            prefill_estimate = prefill_estimate.map(|e| Estimate {
                value: e.value * factor,
                low: e.low * factor,
                high: e.high * factor,
            });
        }
    }
    if let Some(f) = &formula {
        speed.basis.extend(f.basis.iter().cloned());
        speed.concurrency = f.concurrency;
    }
    speed.decode =
        measured(&mine, chat_bucket, |r| r.decode_tps).or_else(|| estimated(decode_estimate));
    speed.prefill = measured(&mine, goal_bucket, |r| r.prefill_tps)
        .or_else(|| {
            measured(&mine, chat_bucket, |r| r.prefill_tps).filter(|_| goal_bucket == chat_bucket)
        })
        .or_else(|| estimated(prefill_estimate));
    // Many requests: one stream's decode × the concurrency gain we measured for this stack.
    let gain = match (&formula, &speed.decode) {
        (Some(f), Some(_)) => f.throughput.zip(f.decode).map(|(t, d)| t.value / d.value),
        _ => None,
    };
    speed.throughput = speed.decode.as_ref().zip(gain).map(|(d, g)| Figure {
        estimate: Estimate {
            value: d.estimate.value * g,
            low: d.estimate.low * g,
            high: d.estimate.high * g,
        },
        measured: false,
        runs: 0,
        last_measured_ms: None,
    });
    speed
}

fn candidate_base(
    key: PlacementKey,
    nodes: &[&NodeInput],
) -> (String, Vec<String>, Vec<Option<ChipIdentity>>, String) {
    (
        key.id(),
        nodes.iter().map(|n| n.name.clone()).collect(),
        nodes.iter().map(|n| n.chip.clone().ok()).collect(),
        backend_of(key.kind).to_string(),
    )
}

fn missing_model(nodes: &[&NodeInput]) -> Option<String> {
    let missing: Vec<String> = nodes
        .iter()
        .filter_map(|n| match &n.has_model {
            Ok(true) => None,
            Ok(false) => Some(format!(
                "the model is not on {} yet — copy it there first",
                n.name
            )),
            Err(e) => Some(format!(
                "could not tell whether {} holds the model: {e}",
                n.name
            )),
        })
        .collect();
    (!missing.is_empty()).then(|| missing.join("; "))
}

/// Plan one model.
pub fn plan(input: &PlanInput) -> Plan {
    let min_useful = Workload::Chat.context_needed();
    let mut notes = Vec::new();
    let mut candidates: Vec<Candidate> = Vec::new();

    for node in input.nodes {
        let key = PlacementKey::single(&node.id);
        let (id, node_names, chips, backend) = candidate_base(key.clone(), &[node]);
        let fit = single_fit(input, node, min_useful);
        let action = if node.is_local() {
            match missing_model(&[node]) {
                Some(reason) => Action::Unavailable { reason },
                None => Action::MountHere,
            }
        } else if let Err(reason) = &node.remote_single {
            Action::Unavailable {
                reason: reason.clone(),
            }
        } else {
            match missing_model(&[node]) {
                Some(reason) => Action::Unavailable { reason },
                None => Action::RemoteSingle,
            }
        };
        let speed = speed_for(input, &key, &[node], &[1.0]);
        candidates.push(Candidate {
            id,
            key,
            node_names,
            chips,
            backend,
            supported: true,
            fit,
            speed,
            action,
            outcome: Outcome::Best,
        });
    }

    let all: Vec<&NodeInput> = input.nodes.iter().collect();
    match input.cluster {
        None => notes.push(
            "no second Mac is set up or connected over LeanZero Link — connect one to plan across Macs"
                .to_string(),
        ),
        Some(_) if input.nodes.len() < 2 => {
            notes.push("the distributed setup names one Mac only".to_string())
        }
        Some(cluster) => {
            let runner = Runner::for_model_type(&input.model.model_type);
            for kind in [PlacementKind::Tensor, PlacementKind::Pipeline] {
                let key = PlacementKey {
                    kind,
                    nodes: input.nodes.iter().map(|n| n.id.clone()).collect(),
                    link: cluster.link.clone(),
                };
                let (id, node_names, chips, backend) = candidate_base(key.clone(), &all);
                let wired = matches!(
                    (&runner, kind),
                    (Ok(Runner::MlxLmTensor), PlacementKind::Tensor)
                        | (Ok(Runner::PipelineQwen4), PlacementKind::Pipeline)
                );
                let unsupported = if !wired {
                    Some(match &runner {
                        Err(e) => format!("not supported yet: {e:#}"),
                        Ok(r) => format!(
                            "not supported yet: goose splits {} {} only",
                            input.model.model_type,
                            match r {
                                Runner::MlxLmTensor => "tensor-parallel",
                                Runner::PipelineQwen4 => "by layer range (pipeline)",
                            }
                        ),
                    })
                } else if kind == PlacementKind::Tensor && cluster.link.as_deref() != Some("jaccl") {
                    Some(match &cluster.link {
                        Some(link) => format!(
                            "not offered: tensor parallel needs JACCL (two all-sums per layer per token); these Macs are linked over {link}"
                        ),
                        None => "not offered: tensor parallel needs JACCL, and no distributed setup on this Mac names the link yet".to_string(),
                    })
                } else if kind == PlacementKind::Tensor {
                    match input.tensor {
                        Some(Ok(facts)) => facts
                            .check_divisible(input.nodes.len() as u64)
                            .err()
                            .map(|e| format!("not supported: {e:#}")),
                        _ => None,
                    }
                } else {
                    None
                };
                if let Some(reason) = unsupported {
                    candidates.push(Candidate {
                        id,
                        key,
                        node_names,
                        chips,
                        backend,
                        supported: false,
                        fit: unknown_fit(reason.clone()),
                        speed: Speed::default(),
                        action: Action::Unavailable {
                            reason: reason.clone(),
                        },
                        outcome: Outcome::NotSupported { reason },
                    });
                    continue;
                }
                let (fit, shares) = match kind {
                    PlacementKind::Tensor => match input.tensor {
                        Some(Ok(facts)) => (tensor_fit(input, facts, min_useful), None),
                        Some(Err(e)) => (
                            unknown_fit(format!(
                                "the tensor arithmetic could not read the model: {e}"
                            )),
                            None,
                        ),
                        None => (
                            unknown_fit("the tensor arithmetic was not run".to_string()),
                            None,
                        ),
                    },
                    _ => pipeline_fit(input, cluster, min_useful),
                };
                let shares = shares
                    .unwrap_or_else(|| vec![1.0 / input.nodes.len() as f64; input.nodes.len()]);
                let speed = speed_for(input, &key, &all, &shares);
                let refusals: Vec<&str> = all
                    .iter()
                    .filter_map(|n| n.split_refusal.as_deref())
                    .collect();
                let action = match missing_model(&all) {
                    _ if !refusals.is_empty() => Action::Unavailable {
                        reason: refusals.join("; "),
                    },
                    Some(reason) => Action::Unavailable { reason },
                    None => Action::StartSplit {
                        setup_matches: cluster.config_model_id.as_deref() == Some(input.model_id),
                    },
                };
                candidates.push(Candidate {
                    id,
                    key,
                    node_names,
                    chips,
                    backend,
                    supported: true,
                    fit,
                    speed,
                    action,
                    outcome: Outcome::Best,
                });
            }
        }
    }

    if let Some((id, context)) = &input.running {
        if let Some(c) = candidates.iter_mut().find(|c| &c.id == id) {
            c.fit = Fit {
                status: FitStatus::Fits,
                context: context.or(c.fit.context),
                short_bytes: None,
                short_node: None,
                nodes: c.fit.nodes.clone(),
                detail: "running now — its memory is already in use".to_string(),
            };
        }
    }
    let (best, best_available) = pick(&mut candidates, input.goal);
    let badge = badge(&candidates);
    Plan {
        model_id: input.model_id.to_string(),
        goal: input.goal,
        candidates,
        best,
        best_available,
        badge,
        notes,
    }
}

fn overlaps(a: &Estimate, b: &Estimate) -> bool {
    a.low <= b.high && b.low <= a.high
}

/// The winner among `eligible` (indices): the highest figure, except that a placement within the
/// error of it on fewer Macs wins ("tie → fewer Macs").
fn winner(candidates: &[Candidate], eligible: &[usize], goal: Goal) -> Option<usize> {
    let top = eligible.iter().copied().max_by(|a, b| {
        let va = candidates[*a]
            .metric(goal)
            .map(|f| f.estimate.value)
            .unwrap_or_default();
        let vb = candidates[*b]
            .metric(goal)
            .map(|f| f.estimate.value)
            .unwrap_or_default();
        va.total_cmp(&vb)
    })?;
    let top_estimate = candidates[top].metric(goal)?.estimate;
    eligible
        .iter()
        .copied()
        .filter(|i| {
            candidates[*i]
                .metric(goal)
                .is_some_and(|f| overlaps(&f.estimate, &top_estimate))
        })
        .min_by(|a, b| {
            let (ca, cb) = (&candidates[*a], &candidates[*b]);
            ca.key.nodes.len().cmp(&cb.key.nodes.len()).then_with(|| {
                let va = ca
                    .metric(goal)
                    .map(|f| f.estimate.value)
                    .unwrap_or_default();
                let vb = cb
                    .metric(goal)
                    .map(|f| f.estimate.value)
                    .unwrap_or_default();
                vb.total_cmp(&va)
            })
        })
}

fn pick(candidates: &mut Vec<Candidate>, goal: Goal) -> (Option<String>, Option<String>) {
    let eligible: Vec<usize> = (0..candidates.len())
        .filter(|i| {
            let c = &candidates[*i];
            c.supported && c.fit.status.fits() && c.metric(goal).is_some()
        })
        .collect();
    let best = winner(candidates, &eligible, goal);
    let runnable: Vec<usize> = eligible
        .iter()
        .copied()
        .filter(|i| candidates[*i].runnable())
        .collect();
    let best_available = winner(candidates, &runnable, goal);
    let best_value = best.and_then(|b| candidates[b].metric(goal).map(|f| f.estimate));
    let outcomes: Vec<Outcome> = candidates
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let outcome = if Some(i) == best {
                Outcome::Best
            } else if Some(i) == best_available {
                Outcome::BestAvailableNow
            } else if !c.supported {
                c.outcome.clone()
            } else if c.fit.status == FitStatus::Unknown {
                Outcome::FitUnknown {
                    reason: c.fit.detail.clone(),
                }
            } else if !c.fit.status.fits() {
                Outcome::DoesNotFit
            } else if let (Some(mine), Some(best)) = (c.metric(goal), best_value) {
                if overlaps(&mine.estimate, &best) {
                    Outcome::TiedNeedsMoreMacs {
                        mine: mine.estimate.value,
                        best: best.value,
                    }
                } else {
                    Outcome::Slower {
                        mine: mine.estimate.value,
                        best: best.value,
                    }
                }
            } else {
                Outcome::NoFigure {
                    reason: c
                        .speed
                        .basis
                        .iter()
                        .find(|b| b.starts_with("no "))
                        .cloned()
                        .unwrap_or_else(|| "no measurement and no estimate".to_string()),
                }
            };
            outcome
        })
        .collect();
    for (c, outcome) in candidates.iter_mut().zip(outcomes) {
        c.outcome = outcome;
    }
    let rank = |c: &Candidate| -> (u8, i64) {
        let class = match c.outcome {
            Outcome::Best => 0,
            Outcome::BestAvailableNow => 1,
            Outcome::TiedNeedsMoreMacs { .. } | Outcome::Slower { .. } => 2,
            Outcome::NoFigure { .. } => 3,
            Outcome::DoesNotFit | Outcome::FitUnknown { .. } => 4,
            Outcome::NotSupported { .. } => 5,
        };
        let value = c
            .metric(goal)
            .map(|f| (f.estimate.value * 1000.0) as i64)
            .unwrap_or_default();
        (class, -value)
    };
    candidates.sort_by_key(rank);
    let id =
        |i: Option<usize>, cs: &Vec<Candidate>, outcome: fn(&Outcome) -> bool| -> Option<String> {
            i.and_then(|_| {
                cs.iter()
                    .find(|c| outcome(&c.outcome))
                    .map(|c| c.id.clone())
            })
        };
    let best_id = id(best, candidates, |o| matches!(o, Outcome::Best));
    let available_id = if best == best_available {
        best_id.clone()
    } else {
        id(best_available, candidates, |o| {
            matches!(o, Outcome::BestAvailableNow)
        })
    };
    (best_id, available_id)
}

/// The split a model that fits no single Mac should run as: the fitting split goose can start,
/// else the first fitting split (its action says what must happen first). The badge and the
/// single engine's mount refusal both name THIS one.
pub fn split_that_fits(candidates: &[Candidate]) -> Option<&Candidate> {
    let fitting = || {
        candidates
            .iter()
            .filter(|c| c.supported && c.fit.status.fits() && c.key.kind != PlacementKind::Single)
    };
    fitting()
        .find(|c| c.runnable())
        .or_else(|| fitting().next())
}

/// Where a model this Mac's single engine cannot hold should run instead — what the mount
/// refusal offers: the best placement goose can start that is not this Mac alone, else the fitting
/// split (its action names the step first), else any other placement that fits.
pub fn alternative_to_this_mac(candidates: &[Candidate]) -> Option<&Candidate> {
    let elsewhere = |c: &&Candidate| {
        c.supported
            && c.fit.status.fits()
            && !(c.key.kind == PlacementKind::Single && c.key.nodes[0] == "local")
    };
    candidates
        .iter()
        .filter(elsewhere)
        .find(|c| c.runnable())
        .or_else(|| split_that_fits(candidates))
        .or_else(|| candidates.iter().find(elsewhere))
}

fn badge(candidates: &[Candidate]) -> Badge {
    let fits = |c: &&Candidate| c.supported && c.fit.status.fits();
    if candidates
        .iter()
        .filter(fits)
        .any(|c| c.key.kind == PlacementKind::Single && c.key.nodes[0] == "local")
    {
        return Badge::FitsThisMac;
    }
    if let Some(peer) = candidates
        .iter()
        .filter(fits)
        .find(|c| c.key.kind == PlacementKind::Single)
    {
        return Badge::FitsPeer {
            name: peer.node_names[0].clone(),
        };
    }
    if let Some(split) = split_that_fits(candidates) {
        return Badge::NeedsBothMacs {
            needs: match &split.action {
                Action::Unavailable { reason } => Some(reason.clone()),
                Action::StartSplit {
                    setup_matches: false,
                } => Some(
                    "set up the distributed engine with this model (Set up → the model)"
                        .to_string(),
                ),
                _ => None,
            },
        };
    }
    let supported: Vec<&Candidate> = candidates.iter().filter(|c| c.supported).collect();
    if let Some(unknown) = supported
        .iter()
        .find(|c| c.fit.status == FitStatus::Unknown)
    {
        return Badge::Unknown {
            reason: unknown.fit.detail.clone(),
        };
    }
    match supported.iter().filter_map(|c| c.fit.short_bytes).min() {
        Some(short_bytes) => Badge::TooBig { short_bytes },
        None => Badge::Unknown {
            reason: "no placement could be sized".to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kv_cache::KvCacheFacts;
    use crate::placement::store::RecordSource;
    use crate::GIB;
    use std::collections::BTreeMap;

    const M4_CEILING: u64 = 115_448_725_504;
    const M3_CEILING: u64 = 83_494_174_720;

    fn gib(v: f64) -> u64 {
        (v * GIB as f64) as u64
    }

    fn chip(brand: &str, cores: u32) -> ChipIdentity {
        ChipIdentity {
            hw_model: String::new(),
            brand: brand.into(),
            gpu_cores: Some(cores),
        }
    }

    /// The two Macs with the available memory a normal working day leaves (the Flash planner runs'
    /// recorded figures, 128:90 / 96:67 GiB).
    fn macs(macbook_avail: f64, studio_avail: f64) -> Vec<NodeInput> {
        vec![
            NodeInput {
                id: "local".into(),
                name: "Mihai Macbook".into(),
                chip: Ok(chip("Apple M4 Max", 40)),
                memory: Ok(NodeMemory {
                    total_bytes: gib(128.0),
                    available_bytes: gib(macbook_avail),
                    ceiling_bytes: Ok(M4_CEILING),
                }),
                has_model: Ok(true),
                remote_single: Ok(()),
                split_refusal: None,
            },
            NodeInput {
                id: "link:worksmacstudio".into(),
                name: "Work’s Mac Studio".into(),
                chip: Ok(chip("Apple M3 Ultra", 60)),
                memory: Ok(NodeMemory {
                    total_bytes: gib(96.0),
                    available_bytes: gib(studio_avail),
                    ceiling_bytes: Ok(M3_CEILING),
                }),
                has_model: Ok(true),
                remote_single: Ok(()),
                split_refusal: None,
            },
        ]
    }

    fn kv(attention: u64, kv_heads: u64) -> Result<KvCacheFacts, String> {
        let bf16 = attention * 2 * kv_heads * 256 * 2;
        Ok(KvCacheFacts {
            attention_layers: attention,
            state_layers: 0,
            sliding_layers: 0,
            kv_heads,
            head_dim: 256,
            activation_bytes: 2,
            group_size: Some(64),
            bf16_bytes_per_token: bf16,
            int8_bytes_per_token: Some(bf16 / 2),
            int4_bytes_per_token: Some(bf16 / 4),
        })
    }

    /// The 27B as read from disk (2026-09-24).
    fn qwen27b() -> ModelFacts {
        ModelFacts {
            model_type: "qwen3_5".into(),
            layers: 64,
            max_context: Some(262_144),
            moe: None,
            quant: None,
            resident_bytes: 30_963_000_000,
            active_bytes_per_token: 28_420_553_728,
            active_params_per_token: 25_624_600_064,
            lookup_bytes: 2_542_000_000,
            excluded_bytes: 921_000_000,
            largest_layer_bytes: 444_000_000,
            kv: kv(16, 4),
        }
    }

    /// The 27B's tensor facts (plan.rs's own fixture values).
    fn qwen27b_tensor() -> TensorModelFacts {
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
            replicated_bytes: gib(2.0 * 2.368),
            excluded_bytes: 0,
        }
    }

    fn flash() -> ModelFacts {
        ModelFacts {
            model_type: "qwen4_exp".into(),
            layers: 48,
            max_context: Some(262_144),
            moe: Some(super::super::model::MoeFacts {
                experts: 512,
                experts_per_token: 10,
            }),
            quant: None,
            resident_bytes: gib(95.708),
            active_bytes_per_token: 3_787_797_760,
            active_params_per_token: 6_734_332_800,
            lookup_bytes: gib(30.135),
            excluded_bytes: gib(1.784),
            largest_layer_bytes: gib(31.179),
            kv: kv(12, 2),
        }
    }

    fn cluster(model: &str) -> ClusterInput {
        ClusterInput {
            link: Some("jaccl".into()),
            slots: 2,
            config_model_id: Some(model.into()),
        }
    }

    struct Fixture {
        model: ModelFacts,
        nodes: Vec<NodeInput>,
        cluster: Option<ClusterInput>,
        tensor: Option<Result<TensorModelFacts, String>>,
        pipeline: Option<Result<PipelineFitInput, String>>,
        records: Vec<SpeedRecord>,
        bytes_on_disk: u64,
        cal: Calibration,
        running: Option<(String, Option<u64>)>,
    }

    impl Fixture {
        fn plan(&self, goal: Goal) -> Plan {
            plan(&PlanInput {
                model_id: "m",
                model: &self.model,
                bytes_on_disk: self.bytes_on_disk,
                kv_mode: None,
                nodes: &self.nodes,
                cluster: self.cluster.as_ref(),
                tensor: self.tensor.as_ref(),
                pipeline: self.pipeline.as_ref(),
                goal,
                context: None,
                records: &self.records,
                calibration: &self.cal,
                running: self.running.clone(),
            })
        }
    }

    fn the_27b() -> Fixture {
        Fixture {
            model: qwen27b(),
            nodes: macs(60.0, 74.0),
            cluster: Some(cluster("other")),
            tensor: Some(Ok(qwen27b_tensor())),
            pipeline: None,
            records: Vec::new(),
            bytes_on_disk: 32_800_000_000,
            cal: Calibration::fit(&BTreeMap::new()),
            running: None,
        }
    }

    fn by_id<'a>(plan: &'a Plan, id: &str) -> &'a Candidate {
        plan.candidates
            .iter()
            .find(|c| c.id == id)
            .unwrap_or_else(|| panic!("no {id} in {:#?}", plan.candidates))
    }

    #[test]
    fn the_27b_picks_the_m3_ultra_alone_for_chat() {
        let plan = the_27b().plan(Goal::Chat);
        assert_eq!(
            plan.best.as_deref(),
            Some("single:link:worksmacstudio"),
            "{plan:#?}"
        );
        assert_eq!(plan.best_available, plan.best);
        let studio = by_id(&plan, "single:link:worksmacstudio");
        assert_eq!(studio.action, Action::RemoteSingle);
        assert_eq!(studio.fit.status, FitStatus::Fits);
        assert_eq!(studio.fit.context, Some(262_144));
        let decode = studio.speed.decode.as_ref().unwrap();
        assert!(
            !decode.measured && (decode.estimate.value - 22.2).abs() < 1.5,
            "{decode:?}"
        );
        let tensor = by_id(&plan, "tensor:jaccl:local+link:worksmacstudio");
        assert!(
            matches!(tensor.outcome, Outcome::Slower { .. }),
            "{:?}",
            tensor.outcome
        );
        assert!(matches!(
            tensor.action,
            Action::StartSplit {
                setup_matches: false
            }
        ));
        let pipeline = by_id(&plan, "pipeline:jaccl:local+link:worksmacstudio");
        assert!(
            matches!(&pipeline.outcome, Outcome::NotSupported { reason } if reason.contains("tensor-parallel"))
        );
        assert_eq!(plan.badge, Badge::FitsThisMac);

        // A peer goose cannot start a single engine on stays the fastest, and is named as such,
        // while the best a person can start now is this Mac.
        let mut ssh_peer = the_27b();
        ssh_peer.nodes[1].remote_single =
            Err("remote single runs over LeanZero Link; this Mac is set up over ssh".into());
        let plan = ssh_peer.plan(Goal::Chat);
        assert_eq!(plan.best.as_deref(), Some("single:link:worksmacstudio"));
        assert_eq!(plan.best_available.as_deref(), Some("single:local"));
        let studio = by_id(&plan, "single:link:worksmacstudio");
        assert!(
            matches!(&studio.action, Action::Unavailable { reason } if reason.contains("over ssh"))
        );
    }

    #[test]
    fn the_27b_picks_tensor_for_long_documents() {
        let plan = the_27b().plan(Goal::LongDocuments);
        assert_eq!(
            plan.best.as_deref(),
            Some("tensor:jaccl:local+link:worksmacstudio"),
            "{plan:#?}"
        );
        // The saved setup names another model, so the split is not startable as things stand.
        assert_eq!(
            plan.best_available.as_deref(),
            Some("single:link:worksmacstudio")
        );
        let prefill = by_id(&plan, &plan.best.clone().unwrap())
            .speed
            .prefill
            .clone()
            .unwrap();
        assert!(
            (prefill.estimate.value - 405.7).abs() / 405.7 < 0.1,
            "{prefill:?}"
        );
    }

    #[test]
    fn a_measured_speed_wins_over_the_formula_and_recalibrates_the_single_engine() {
        let mut f = the_27b();
        let mut record = SpeedRecord {
            model_id: "m".into(),
            placement: PlacementKey::single("local"),
            node_names: vec!["Mihai Macbook".into()],
            chips: vec![Some(chip("Apple M4 Max", 40))],
            backend: BACKEND_SINGLE.into(),
            context_bucket: 2048,
            prompt_tokens: 1900,
            completion_tokens: 256,
            prefill_tps: Some(238.0),
            decode_tps: Some(21.5),
            ttft_ms: Some(8000.0),
            recorded_at_ms: 7,
            source: RecordSource::Benchmark,
            workload: Some("chat".into()),
            kv_cache: None,
        };
        f.records.push(record.clone());
        record.decode_tps = Some(21.9);
        record.recorded_at_ms = 9;
        f.records.push(record);
        let plan = f.plan(Goal::Chat);
        let here = by_id(&plan, "single:local").speed.decode.clone().unwrap();
        assert!(
            here.measured && here.runs == 2 && here.last_measured_ms == Some(9),
            "{here:?}"
        );
        assert!((here.estimate.value - 21.7).abs() < 1e-9);
        // Rapid-MLX beat the mlx_lm formula on this Mac; the Studio's estimate carries the same gain.
        let studio = by_id(&plan, "single:link:worksmacstudio").speed.clone();
        assert!(
            studio.decode.as_ref().unwrap().estimate.value > 28.0,
            "{studio:?}"
        );
        assert!(studio
            .basis
            .iter()
            .any(|b| b.contains("measured against the formula")));
        assert_eq!(plan.best.as_deref(), Some("single:link:worksmacstudio"));
    }

    #[test]
    fn flash_needs_the_pipeline_on_a_normal_day() {
        let f = Fixture {
            model: flash(),
            nodes: macs(90.0, 67.0),
            cluster: Some(cluster("m")),
            tensor: None,
            pipeline: None,
            records: Vec::new(),
            bytes_on_disk: 104_700_000_000,
            cal: Calibration::fit(&BTreeMap::new()),
            running: None,
        };
        let plan = f.plan(Goal::Chat);
        assert_eq!(
            plan.best.as_deref(),
            Some("pipeline:jaccl:local+link:worksmacstudio"),
            "{plan:#?}"
        );
        assert_eq!(plan.best_available, plan.best);
        assert_eq!(plan.badge, Badge::NeedsBothMacs { needs: None });
        let here = by_id(&plan, "single:local");
        assert_eq!(here.outcome, Outcome::DoesNotFit);
        assert!(
            here.fit.detail.contains("needs") && here.fit.detail.contains("budget"),
            "{}",
            here.fit.detail
        );
        let best = by_id(&plan, plan.best.as_deref().unwrap());
        assert!(
            best.fit.detail.starts_with("estimate"),
            "{}",
            best.fit.detail
        );
        assert!(matches!(
            best.action,
            Action::StartSplit {
                setup_matches: true
            }
        ));
        let tensor = by_id(&plan, "tensor:jaccl:local+link:worksmacstudio");
        assert!(!tensor.supported);

        // The fork's planner, when it ran, decides the fit instead of the aggregate estimate.
        let with_fork = Fixture {
            pipeline: Some(Ok(PipelineFitInput {
                fits: true,
                context: Some(73_216),
                stages: vec![(gib(61.5), gib(81.0)), (gib(42.7), gib(60.3))],
                layer_shares: vec![20.0 / 48.0, 28.0 / 48.0],
            })),
            ..f
        };
        let plan = with_fork.plan(Goal::Chat);
        let best = by_id(&plan, plan.best.as_deref().unwrap());
        assert_eq!(best.fit.context, Some(73_216));
        assert!(best.fit.detail.starts_with("the fork's planner"));
        let decode = best.speed.decode.as_ref().unwrap();
        assert!((decode.estimate.value - 22.5).abs() < 2.5, "{decode:?}");
    }

    #[test]
    fn a_model_too_big_for_both_macs_names_its_shortfall() {
        let mut big = flash();
        big.resident_bytes = gib(400.0);
        big.largest_layer_bytes = gib(9.0);
        let f = Fixture {
            model: big,
            nodes: macs(110.0, 80.0),
            cluster: Some(cluster("m")),
            tensor: None,
            pipeline: None,
            records: Vec::new(),
            bytes_on_disk: gib(420.0),
            cal: Calibration::fit(&BTreeMap::new()),
            running: None,
        };
        let plan = f.plan(Goal::Chat);
        assert_eq!(plan.best, None);
        assert_eq!(plan.best_available, None);
        let Badge::TooBig { short_bytes } = plan.badge.clone() else {
            panic!("{:?}", plan.badge)
        };
        // Both Macs' budgets together (~101 + ~73 GiB) fall short of 400 GiB by ~226 GiB.
        assert!(
            short_bytes > gib(200.0) && short_bytes < gib(250.0),
            "{}",
            short_bytes as f64 / GIB as f64
        );
        assert!(plan.candidates.iter().all(|c| c.outcome != Outcome::Best));
    }

    #[test]
    fn a_peer_that_cannot_name_its_chip_is_a_named_gap_not_a_guess() {
        let mut f = the_27b();
        f.nodes[1].chip =
            Err("its goose did not report a chip (older than the placement planner)".into());
        let plan = f.plan(Goal::Chat);
        let studio = by_id(&plan, "single:link:worksmacstudio");
        assert!(studio.speed.decode.is_none());
        assert!(
            matches!(&studio.outcome, Outcome::NoFigure { reason } if reason.contains("chip is unknown")),
            "{:?}",
            studio.outcome
        );
        assert_eq!(plan.best.as_deref(), Some("single:local"));
    }

    #[test]
    fn a_single_mac_setup_has_no_split_and_says_so() {
        let mut f = the_27b();
        f.nodes.truncate(1);
        f.cluster = None;
        let plan = f.plan(Goal::ManyRequests);
        assert_eq!(plan.candidates.len(), 1);
        assert_eq!(plan.best.as_deref(), Some("single:local"));
        assert!(plan.notes[0].contains("no second Mac"));
        let throughput = plan.candidates[0].speed.throughput.as_ref().unwrap();
        let decode = plan.candidates[0].speed.decode.as_ref().unwrap();
        assert!(throughput.estimate.value > decode.estimate.value);
    }

    #[test]
    fn the_running_placement_fits_although_its_memory_is_in_use() {
        let mut f = the_27b();
        // Measured 2026-09-24 in the packaged app: with the 27B mounted here the MacBook read
        // ~39 GiB available and the planner called the RUNNING model "short 10.5 GB".
        f.nodes[0].memory = Ok(NodeMemory {
            total_bytes: gib(128.0),
            available_bytes: gib(39.0),
            ceiling_bytes: Ok(M4_CEILING),
        });
        let before = f.plan(Goal::Chat);
        assert_eq!(by_id(&before, "single:local").fit.status, FitStatus::Short);
        f.running = Some(("single:local".into(), Some(262_144)));
        let plan = f.plan(Goal::Chat);
        let here = by_id(&plan, "single:local");
        assert_eq!(here.fit.status, FitStatus::Fits);
        assert_eq!(here.fit.context, Some(262_144));
        assert!(here.fit.detail.starts_with("running now"));
        assert_eq!(plan.badge, Badge::FitsThisMac);
    }

    #[test]
    fn a_smaller_context_is_kept_and_shown() {
        let mut f = the_27b();
        f.nodes.truncate(1);
        f.cluster = None;
        // 50 GiB available on the MacBook: a budget of ~41 GiB leaves room for ~150k tokens of KV.
        f.nodes[0].memory = Ok(NodeMemory {
            total_bytes: gib(128.0),
            available_bytes: gib(55.0),
            ceiling_bytes: Ok(M4_CEILING),
        });
        let plan = f.plan(Goal::Chat);
        let here = &plan.candidates[0];
        assert_eq!(here.fit.status, FitStatus::SmallerContext, "{:?}", here.fit);
        let ctx = here.fit.context.unwrap();
        assert!(ctx > 100_000 && ctx < 262_144, "{ctx}");
        assert_eq!(plan.best.as_deref(), Some("single:local"));
    }

    /// The owner's 3.0.25 refusal (2026-09-24): Flash-Next-4bit, 97.5 GiB on disk, Mount on the
    /// M4 Max with 93.0 GiB available after Make room. The planner's single fit and the mount gate
    /// are ONE verdict (the same `fit::judge` on the same need), and the refusal names the split
    /// that would work.
    #[test]
    fn the_recorded_flash_refusal_is_the_mount_gates_verdict_and_names_the_split() {
        let f = Fixture {
            model: flash(),
            nodes: macs(93.0, 67.0),
            cluster: Some(cluster("m")),
            tensor: None,
            pipeline: None,
            records: Vec::new(),
            bytes_on_disk: gib(97.5),
            cal: Calibration::fit(&BTreeMap::new()),
            running: None,
        };
        let plan = f.plan(Goal::Chat);
        let here = by_id(&plan, "single:local");
        assert_eq!(here.fit.status, FitStatus::Short);
        let gate = fit::judge(
            single_engine_need(gib(97.5), Ok(&flash()), None),
            NodeMemoryFacts {
                available_bytes: gib(93.0),
                total_bytes: gib(128.0),
                ceiling_bytes: M4_CEILING,
            },
        );
        assert_eq!(gate.verdict, Verdict::Block);
        assert_eq!(here.fit.detail, format!("Mihai Macbook: {}", gate.message));
        assert_eq!(here.fit.short_bytes, gate.short_bytes());
        assert!(gate.message.contains("budget 81.1 GiB"), "{}", gate.message);
        let split = split_that_fits(&plan.candidates).expect("a split fits");
        assert_eq!(split.id, "pipeline:jaccl:local+link:worksmacstudio");
        assert_eq!(
            alternative_to_this_mac(&plan.candidates).map(|c| c.id.as_str()),
            Some(split.id.as_str()),
            "the Studio alone is short too (its GPU ceiling is 77.8 GiB), so the split is offered"
        );
        assert!(matches!(
            split.action,
            Action::StartSplit {
                setup_matches: true
            }
        ));
        assert_eq!(plan.badge, Badge::NeedsBothMacs { needs: None });
    }

    /// The workhorse's view of the same model: this Mac (the M3 Ultra) has no distributed setup —
    /// the MacBook coordinates — and its 3.0.25 app badged Flash "Too big" because the planner
    /// enumerated peers from the saved setup only. With the Link peer measured, the badge is the
    /// MacBook's answer from the other side, and it names the step still missing.
    #[test]
    fn the_badge_is_the_same_truth_from_either_mac() {
        let macbook_view = Fixture {
            model: flash(),
            nodes: macs(93.0, 72.0),
            cluster: Some(cluster("m")),
            tensor: None,
            pipeline: None,
            records: Vec::new(),
            bytes_on_disk: gib(97.5),
            cal: Calibration::fit(&BTreeMap::new()),
            running: None,
        };
        assert_eq!(
            macbook_view.plan(Goal::Chat).badge,
            Badge::NeedsBothMacs { needs: None }
        );

        let [macbook, studio]: [NodeInput; 2] = macs(93.0, 72.0).try_into().unwrap();
        let studio_here = NodeInput {
            id: "local".into(),
            ..studio
        };
        let macbook_peer = NodeInput {
            id: "link:mihaimacbook".into(),
            split_refusal: Some(
                "allow this Mac to serve as a distributed node on Mihai Macbook (its LeanZero \
                 Link setting is off)"
                    .into(),
            ),
            ..macbook
        };
        let link_peers = ClusterInput {
            link: None,
            slots: 2,
            config_model_id: None,
        };
        let workhorse_view = Fixture {
            nodes: vec![studio_here, macbook_peer],
            cluster: Some(link_peers),
            ..macbook_view
        };
        let plan = workhorse_view.plan(Goal::Chat);
        assert_eq!(by_id(&plan, "single:local").fit.status, FitStatus::Short);
        let Badge::NeedsBothMacs { needs: Some(needs) } = &plan.badge else {
            panic!("{:?}", plan.badge)
        };
        assert!(needs.contains("on Mihai Macbook"), "{needs}");

        let mut allowed = Fixture {
            nodes: workhorse_view.nodes.clone(),
            cluster: workhorse_view.cluster.clone(),
            model: flash(),
            tensor: None,
            pipeline: None,
            records: Vec::new(),
            bytes_on_disk: gib(97.5),
            cal: Calibration::fit(&BTreeMap::new()),
            running: None,
        };
        allowed.nodes[1].split_refusal = None;
        let Badge::NeedsBothMacs { needs: Some(needs) } = allowed.plan(Goal::Chat).badge else {
            panic!("the split still needs a setup on this Mac")
        };
        assert!(needs.contains("set up the distributed engine"), "{needs}");

        // Negative control — 3.0.25's input on the workhorse: no saved setup, so no peer at all.
        allowed.nodes.truncate(1);
        allowed.cluster = None;
        assert!(matches!(
            allowed.plan(Goal::Chat).badge,
            Badge::TooBig { .. }
        ));
    }
}
