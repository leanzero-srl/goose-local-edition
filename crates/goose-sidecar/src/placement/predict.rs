//! Speed prediction for a placement nobody has measured yet: a formula calibrated on OUR recorded
//! runs, labelled "estimated", with a range as wide as the calibration's own error. Every measured
//! number (the store) wins over it.
//!
//! The model (design §1, rules each rest on a measurement):
//! - decode is memory-bound: one token costs `c + active_bytes / BW_eff` per chip (llama.cpp #4167's
//!   per-chip form); `c` is that fit's per-token overhead, `BW_eff` is least-squares-fitted to our
//!   dense single-Mac runs (the 27B and the 32B on each chip);
//! - an MoE model reaches a smaller share of the bandwidth than a dense one: its class factor is
//!   fitted to our Flash pipeline run, and the range reaches up to the best MoE share published
//!   elsewhere (24–36% of spec bandwidth vs 70–80% for dense);
//! - tensor parallel pays two all-sums per layer per token, paced by the slower rank; the all-sum
//!   cost is fitted to our tensor runs (≈0.3 ms JACCL, ≈0.6 ms ring — 10–15× the raw collective
//!   latency, the STEP1 finding the naive formula missed);
//! - a pipeline runs the stages in turn with one hop per stage;
//! - prefill is compute-bound: `F_eff / (2 × active params)` per chip, `F_eff` fitted to the same
//!   single runs; split prefill carries a factor fitted to the split runs.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::chip::{spec_bandwidth, ChipIdentity};
use super::model::ModelFacts;
use super::store::PlacementKind;

/// Bytes per millisecond in one GB/s.
const BYTES_PER_MS_PER_GBS: f64 = 1e6;

/// One of OUR recorded runs (each one's report named in `source`). Every rank's chip is named with
/// its stage share (a pipeline's layer share; tensor ranks hold `1 / ranks` each).
#[derive(Debug, Clone)]
pub struct SeedRun {
    pub label: &'static str,
    pub source: &'static str,
    pub kind: PlacementKind,
    pub link: Option<&'static str>,
    /// (brand, GPU cores, share of the model's layers).
    pub nodes: &'static [(&'static str, u32, f64)],
    pub active_bytes: u64,
    pub active_params: u64,
    pub layers: u64,
    pub moe: bool,
    pub prefill_tps: f64,
    pub decode_tps: f64,
}

// measured: the owner's Qwen3.8-27B-Atlassian-Q8-mlx, read by `read_model_facts` (2026-09-24).
const QWEN_27B_ACTIVE_BYTES: u64 = 28_420_553_728;
// measured: same read — weights in the active set (U32 packs 32 / 8 bits).
const QWEN_27B_ACTIVE_PARAMS: u64 = 25_624_600_064;
// measured: derived from Qwen3-32B's config (64 layers, hidden 5,120, MLP 25,600, vocab 151,936,
// untied head) at 4-bit g64 — the Hub lists the checkpoint at 18.45 GB, the embedding table
// (0.44 GB) is gathered, not streamed. The checkpoint was deleted after STEP1.
const QWEN3_32B_ACTIVE_BYTES: u64 = 18_010_000_000;
// measured: the same config's weights less the embedding table.
const QWEN3_32B_ACTIVE_PARAMS: u64 = 31_990_000_000;
// measured: Qwen3.8-Flash-Next-4bit, read by `read_model_facts` (2026-09-24): 512 experts, 10 per
// token.
const FLASH_ACTIVE_BYTES: u64 = 3_787_797_760;
// measured: same read.
const FLASH_ACTIVE_PARAMS: u64 = 6_734_332_800;

const M3_ULTRA: (&str, u32) = ("Apple M3 Ultra", 60);
const M4_MAX: (&str, u32) = ("Apple M4 Max", 40);

/// Our recorded runs (design doc table): `mlx_lm.benchmark -p 2048 -g 256 -n 3` for the 27B and
/// the 32B, the fork's pipeline for Flash (2k prompt, decode 21.3–23.7 tok/s → its midpoint).
pub const SEED_RUNS: &[SeedRun] = &[
    SeedRun { label: "27B · M3 Ultra alone", source: "~/goose-builds/jaccl-smoke/STEP1b-soak/REPORT.md", kind: PlacementKind::Single, link: None, nodes: &[(M3_ULTRA.0, M3_ULTRA.1, 1.0)], active_bytes: QWEN_27B_ACTIVE_BYTES, active_params: QWEN_27B_ACTIVE_PARAMS, layers: 64, moe: false, prefill_tps: 335.2, decode_tps: 22.2 },
    SeedRun { label: "27B · M4 Max alone", source: "~/goose-builds/jaccl-smoke/STEP1b-soak/REPORT.md", kind: PlacementKind::Single, link: None, nodes: &[(M4_MAX.0, M4_MAX.1, 1.0)], active_bytes: QWEN_27B_ACTIVE_BYTES, active_params: QWEN_27B_ACTIVE_PARAMS, layers: 64, moe: false, prefill_tps: 223.8, decode_tps: 15.1 },
    SeedRun { label: "27B · tensor JACCL", source: "~/goose-builds/jaccl-smoke/STEP1b-soak/REPORT.md", kind: PlacementKind::Tensor, link: Some("jaccl"), nodes: &[(M4_MAX.0, M4_MAX.1, 0.5), (M3_ULTRA.0, M3_ULTRA.1, 0.5)], active_bytes: QWEN_27B_ACTIVE_BYTES, active_params: QWEN_27B_ACTIVE_PARAMS, layers: 64, moe: false, prefill_tps: 405.7, decode_tps: 14.6 },
    SeedRun { label: "27B · tensor ring", source: "~/goose-builds/jaccl-smoke/STEP1b-soak/REPORT.md", kind: PlacementKind::Tensor, link: Some("ring"), nodes: &[(M4_MAX.0, M4_MAX.1, 0.5), (M3_ULTRA.0, M3_ULTRA.1, 0.5)], active_bytes: QWEN_27B_ACTIVE_BYTES, active_params: QWEN_27B_ACTIVE_PARAMS, layers: 64, moe: false, prefill_tps: 372.9, decode_tps: 10.4 },
    SeedRun { label: "32B 4-bit · M3 Ultra alone", source: "~/goose-builds/jaccl-smoke/STEP1.md", kind: PlacementKind::Single, link: None, nodes: &[(M3_ULTRA.0, M3_ULTRA.1, 1.0)], active_bytes: QWEN3_32B_ACTIVE_BYTES, active_params: QWEN3_32B_ACTIVE_PARAMS, layers: 64, moe: false, prefill_tps: 268.9, decode_tps: 31.9 },
    SeedRun { label: "32B 4-bit · M4 Max alone", source: "~/goose-builds/jaccl-smoke/STEP1.md", kind: PlacementKind::Single, link: None, nodes: &[(M4_MAX.0, M4_MAX.1, 1.0)], active_bytes: QWEN3_32B_ACTIVE_BYTES, active_params: QWEN3_32B_ACTIVE_PARAMS, layers: 64, moe: false, prefill_tps: 179.0, decode_tps: 21.4 },
    SeedRun { label: "32B 4-bit · tensor JACCL", source: "~/goose-builds/jaccl-smoke/STEP1.md", kind: PlacementKind::Tensor, link: Some("jaccl"), nodes: &[(M4_MAX.0, M4_MAX.1, 0.5), (M3_ULTRA.0, M3_ULTRA.1, 0.5)], active_bytes: QWEN3_32B_ACTIVE_BYTES, active_params: QWEN3_32B_ACTIVE_PARAMS, layers: 64, moe: false, prefill_tps: 341.6, decode_tps: 15.7 },
    SeedRun { label: "32B 4-bit · tensor ring", source: "~/goose-builds/jaccl-smoke/STEP1.md", kind: PlacementKind::Tensor, link: Some("ring"), nodes: &[(M4_MAX.0, M4_MAX.1, 0.5), (M3_ULTRA.0, M3_ULTRA.1, 0.5)], active_bytes: QWEN3_32B_ACTIVE_BYTES, active_params: QWEN3_32B_ACTIVE_PARAMS, layers: 64, moe: false, prefill_tps: 281.3, decode_tps: 9.6 },
    SeedRun { label: "Flash · pipeline JACCL [0,20)/[20,48)", source: "mlx-jaccl-cluster skill, Flash on BOTH Macs (2026-09-24)", kind: PlacementKind::Pipeline, link: Some("jaccl"), nodes: &[(M4_MAX.0, M4_MAX.1, 20.0 / 48.0), (M3_ULTRA.0, M3_ULTRA.1, 28.0 / 48.0)], active_bytes: FLASH_ACTIVE_BYTES, active_params: FLASH_ACTIVE_PARAMS, layers: 48, moe: true, prefill_tps: 683.0, decode_tps: 22.5 },
    SeedRun { label: "Flash · pipeline ring [0,20)/[20,48)", source: "mlx-jaccl-cluster skill, Flash on BOTH Macs (2026-09-24)", kind: PlacementKind::Pipeline, link: Some("ring"), nodes: &[(M4_MAX.0, M4_MAX.1, 20.0 / 48.0), (M3_ULTRA.0, M3_ULTRA.1, 28.0 / 48.0)], active_bytes: FLASH_ACTIVE_BYTES, active_params: FLASH_ACTIVE_PARAMS, layers: 48, moe: true, prefill_tps: 716.0, decode_tps: 22.5 },
];

/// llama.cpp #4167's per-token overhead `c` by chip family (the design's research pass). A chip
/// without one takes the mean of these.
const OVERHEAD_PRIOR_MS: &[(&str, f64)] = &[("Apple M3 Ultra", 5.2), ("Apple M4 Max", 4.3)];

/// The best MoE share of the bandwidth relative to dense published elsewhere: MoE runs reach
/// 24–36% of spec bandwidth where dense reaches 70–80% (Qwen3-235B-A22B ≈36%, Kimi K2.5 ≈24%; the
/// design's research pass) → at best 0.36 / 0.70 of a dense model's efficiency.
// ratio: 0.36 / 0.70, the upper end of the published MoE-vs-dense bandwidth share.
const MOE_FACTOR_BEST_PUBLISHED: f64 = 0.36 / 0.70;

/// Concurrent-request gains we measured: aggregate tok/s at `concurrency` ÷ one stream alone.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BatchGain {
    pub gain: f64,
    pub concurrency: u32,
    pub source: &'static str,
}

fn batch_gain(kind: PlacementKind) -> BatchGain {
    match kind {
        // measured: experiments.jsonl swarm-bench 2026-08-30, rapid-mlx 0.13.1, qwen3.5-9b-4bit:
        // n8/n1 aggregate 61.8/24.1, 44.3/25.6, 45.0/26.0, 49.0/26.7 → median 1.785.
        PlacementKind::Single => BatchGain {
            gain: 1.785,
            concurrency: 8,
            source: "rapid-mlx, 8 concurrent vs 1 (experiments.jsonl, 2026-08-30)",
        },
        // measured: STEP1b soak, JACCL — a batched pair streamed 12.1 tok/s each vs 13.7 alone.
        PlacementKind::Tensor => BatchGain {
            gain: 2.0 * 12.1 / 13.7,
            concurrency: 2,
            source: "tensor pair vs one stream (STEP1b soak)",
        },
        // measured: Flash pipeline batch 2 ≈ 47 tok/s aggregate vs 22.5 alone.
        PlacementKind::Pipeline => BatchGain {
            gain: 47.0 / 22.5,
            concurrency: 2,
            source: "Flash pipeline batch 2 vs one stream (2026-09-24)",
        },
    }
}

fn chip_key(brand: &str, cores: Option<u32>) -> String {
    match cores {
        Some(c) => format!("{brand}/{c}"),
        None => brand.to_string(),
    }
}

/// One chip's fitted factors.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChipFactors {
    pub overhead_ms: f64,
    pub bandwidth_gbs: f64,
    /// Effective prefill FLOP/s (`prefill tok/s × 2 × active params`).
    pub prefill_flops: Option<f64>,
    /// "calibrated on N runs" / "from Apple's spec × our efficiency" — shown with the estimate.
    pub basis: String,
    pub calibrated: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Calibration {
    chips: BTreeMap<String, ChipFactors>,
    /// Mean of fitted BW_eff / Apple's spec over calibrated chips, and its spread (max − min).
    spec_efficiency: f64,
    spec_efficiency_spread: f64,
    default_overhead_ms: f64,
    /// Effective all-sum cost per link backend (ms).
    allsum_ms: BTreeMap<String, f64>,
    /// Split prefill ÷ its compute-bound ideal, per (kind, link).
    split_prefill_factor: BTreeMap<(PlacementKind, String), f64>,
    moe_factor: Option<f64>,
    /// Worst relative error of the dense single-Mac fit over its own runs.
    dense_residual: f64,
    tensor_residual: f64,
}

/// A dense single-Mac point (for the least-squares bandwidth fit).
#[derive(Debug, Clone, Copy)]
pub struct SinglePoint {
    pub active_bytes: u64,
    pub active_params: u64,
    pub prefill_tps: Option<f64>,
    pub decode_tps: f64,
}

fn overhead_prior(brand: &str) -> Option<f64> {
    OVERHEAD_PRIOR_MS
        .iter()
        .find(|(b, _)| *b == brand)
        .map(|(_, c)| *c)
}

fn mean(values: &[f64]) -> Option<f64> {
    (!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64)
}

impl Calibration {
    /// Fit on the seed runs plus goose's own dense single-Mac measurements (keyed by chip
    /// `brand/cores`), which join the least-squares bandwidth fit of their chip.
    pub fn fit(extra_single: &BTreeMap<String, Vec<SinglePoint>>) -> Self {
        let default_overhead_ms = mean(
            &OVERHEAD_PRIOR_MS
                .iter()
                .map(|(_, c)| *c)
                .collect::<Vec<_>>(),
        )
        .unwrap_or_default();
        let mut points: BTreeMap<String, (String, Vec<SinglePoint>)> = BTreeMap::new();
        for seed in SEED_RUNS
            .iter()
            .filter(|s| s.kind == PlacementKind::Single && !s.moe)
        {
            let (brand, cores, _) = seed.nodes[0];
            points
                .entry(chip_key(brand, Some(cores)))
                .or_insert_with(|| (brand.to_string(), Vec::new()))
                .1
                .push(SinglePoint {
                    active_bytes: seed.active_bytes,
                    active_params: seed.active_params,
                    prefill_tps: Some(seed.prefill_tps),
                    decode_tps: seed.decode_tps,
                });
        }
        for (key, extra) in extra_single {
            let brand = key.split('/').next().unwrap_or(key).to_string();
            points
                .entry(key.clone())
                .or_insert_with(|| (brand, Vec::new()))
                .1
                .extend(extra.iter().copied());
        }

        let mut chips = BTreeMap::new();
        let mut residuals = Vec::new();
        let mut efficiencies = Vec::new();
        for (key, (brand, pts)) in &points {
            let c = overhead_prior(brand).unwrap_or(default_overhead_ms);
            // least squares through the origin on (bytes, t − c): BW = Σb² / Σ b(t − c).
            let (num, den) = pts.iter().fold((0.0, 0.0), |(n, d), p| {
                let b = p.active_bytes as f64;
                let t = 1000.0 / p.decode_tps;
                (n + b * b, d + b * (t - c))
            });
            if den <= 0.0 {
                continue;
            }
            let bw = num / den / BYTES_PER_MS_PER_GBS;
            for p in pts {
                let predicted = 1000.0 / (c + p.active_bytes as f64 / (bw * BYTES_PER_MS_PER_GBS));
                residuals.push((predicted - p.decode_tps).abs() / p.decode_tps);
            }
            let flops: Vec<f64> = pts
                .iter()
                .filter_map(|p| p.prefill_tps.map(|pp| pp * 2.0 * p.active_params as f64))
                .collect();
            let cores = key.split('/').nth(1).and_then(|c| c.parse().ok());
            if let Ok(spec) = spec_bandwidth(&ChipIdentity {
                hw_model: String::new(),
                brand: brand.clone(),
                gpu_cores: cores,
            }) {
                efficiencies.push(bw / spec.gb_per_s);
            }
            chips.insert(
                key.clone(),
                ChipFactors {
                    overhead_ms: c,
                    bandwidth_gbs: bw,
                    prefill_flops: mean(&flops),
                    basis: format!("calibrated on {} run(s) of this chip", pts.len()),
                    calibrated: true,
                },
            );
        }
        let spec_efficiency = mean(&efficiencies).unwrap_or_default();
        let spec_efficiency_spread = efficiencies.iter().cloned().fold(f64::MIN, f64::max)
            - efficiencies.iter().cloned().fold(f64::MAX, f64::min);

        let mut cal = Calibration {
            chips,
            spec_efficiency,
            spec_efficiency_spread: spec_efficiency_spread.max(0.0),
            default_overhead_ms,
            allsum_ms: BTreeMap::new(),
            split_prefill_factor: BTreeMap::new(),
            moe_factor: None,
            dense_residual: residuals.iter().cloned().fold(0.0, f64::max),
            tensor_residual: 0.0,
        };

        // Tensor: the all-sum cost and the prefill factor, per link, from the dense tensor seeds.
        let mut allsum: BTreeMap<String, Vec<f64>> = BTreeMap::new();
        let mut split_prefill: BTreeMap<(PlacementKind, String), Vec<f64>> = BTreeMap::new();
        for seed in SEED_RUNS.iter().filter(|s| s.kind == PlacementKind::Tensor) {
            let Some(factors) = cal.seed_factors(seed) else {
                continue;
            };
            let link = seed.link.unwrap_or_default().to_string();
            let compute = tensor_compute_ms(&factors, seed.active_bytes as f64, 0.0, 1.0);
            allsum
                .entry(link.clone())
                .or_default()
                .push((1000.0 / seed.decode_tps - compute) / (2.0 * seed.layers as f64));
            if let Some(ideal) = tensor_prefill_ideal(&factors, seed.active_params) {
                split_prefill
                    .entry((PlacementKind::Tensor, link))
                    .or_default()
                    .push(seed.prefill_tps / ideal);
            }
        }
        cal.allsum_ms = allsum
            .into_iter()
            .filter_map(|(k, v)| mean(&v).map(|m| (k, m)))
            .collect();

        // MoE class factor and the pipeline prefill factor, from the MoE pipeline seeds.
        let mut moe = Vec::new();
        for seed in SEED_RUNS
            .iter()
            .filter(|s| s.kind == PlacementKind::Pipeline && s.moe)
        {
            let (Some(factors), Some(hop)) = (
                cal.seed_factors(seed),
                seed.link.and_then(|l| cal.allsum_ms.get(l).copied()),
            ) else {
                continue;
            };
            let a = seed.active_bytes as f64;
            let overhead: f64 = factors.iter().map(|(f, s)| s * f.overhead_ms).sum();
            let bytes_at_full: f64 = factors
                .iter()
                .map(|(f, s)| s * a / (f.bandwidth_gbs * BYTES_PER_MS_PER_GBS))
                .sum();
            let hops = factors.len() as f64 * hop;
            let left = 1000.0 / seed.decode_tps - overhead - hops;
            if left > 0.0 {
                moe.push(bytes_at_full / left);
            }
            if let Some(ideal) = pipeline_prefill_ideal(&factors, seed.active_params) {
                split_prefill
                    .entry((PlacementKind::Pipeline, seed.link.unwrap_or_default().to_string()))
                    .or_default()
                    .push(seed.prefill_tps / ideal);
            }
        }
        cal.moe_factor = mean(&moe);
        cal.split_prefill_factor = split_prefill
            .into_iter()
            .filter_map(|(k, v)| mean(&v).map(|m| (k, m)))
            .collect();

        let mut tensor_residuals = Vec::new();
        for seed in SEED_RUNS.iter().filter(|s| s.kind == PlacementKind::Tensor) {
            let (Some(factors), Some(allsum)) = (
                cal.seed_factors(seed),
                seed.link.and_then(|l| cal.allsum_ms.get(l).copied()),
            ) else {
                continue;
            };
            let t = tensor_compute_ms(&factors, seed.active_bytes as f64, 0.0, 1.0)
                + 2.0 * seed.layers as f64 * allsum;
            tensor_residuals.push((1000.0 / t - seed.decode_tps).abs() / seed.decode_tps);
        }
        cal.tensor_residual = tensor_residuals.iter().cloned().fold(0.0, f64::max);
        cal
    }

    fn seed_factors(&self, seed: &SeedRun) -> Option<Vec<(ChipFactors, f64)>> {
        seed.nodes
            .iter()
            .map(|(brand, cores, share)| {
                self.chips
                    .get(&chip_key(brand, Some(*cores)))
                    .cloned()
                    .map(|f| (f, *share))
            })
            .collect()
    }

    /// This chip's factors: fitted when goose has runs of it; else from Apple's spec bandwidth ×
    /// the efficiency our chips reach (its prefill from a calibrated sibling scaled by GPU cores,
    /// or none); else a named gap.
    pub fn chip(&self, chip: &ChipIdentity) -> Result<ChipFactors, String> {
        if let Some(f) = self.chips.get(&chip_key(&chip.brand, chip.gpu_cores)) {
            return Ok(f.clone());
        }
        let spec = spec_bandwidth(chip)?;
        let sibling = self.chips.iter().find_map(|(key, f)| {
            let (brand, cores) = key.split_once('/')?;
            (brand == chip.brand).then(|| (cores.parse::<f64>().ok(), f))
        });
        let prefill_flops = match (sibling, chip.gpu_cores) {
            (Some((Some(cores), f)), Some(mine)) => {
                f.prefill_flops.map(|flops| flops * mine as f64 / cores)
            }
            _ => None,
        };
        Ok(ChipFactors {
            overhead_ms: overhead_prior(&chip.brand).unwrap_or(self.default_overhead_ms),
            bandwidth_gbs: spec.gb_per_s * self.spec_efficiency,
            prefill_flops,
            basis: format!(
                "not measured on this chip: Apple's {} GB/s ({}) × the {:.0}% our chips reach",
                spec.gb_per_s,
                spec.source,
                self.spec_efficiency * 100.0
            ),
            calibrated: false,
        })
    }

    pub fn moe_factor(&self) -> Option<f64> {
        self.moe_factor
    }

    pub fn allsum_ms(&self, link: &str) -> Option<f64> {
        self.allsum_ms.get(link).copied()
    }

    pub fn dense_residual(&self) -> f64 {
        self.dense_residual
    }
}

/// Tensor decode compute (ms): every rank reads `1 / ranks` of the bytes; the slowest paces.
fn tensor_compute_ms(nodes: &[(ChipFactors, f64)], active: f64, kv: f64, class: f64) -> f64 {
    let ranks = nodes.len() as f64;
    nodes
        .iter()
        .map(|(f, _)| {
            let bw = f.bandwidth_gbs * BYTES_PER_MS_PER_GBS;
            f.overhead_ms + active / ranks / (bw * class) + kv / ranks / bw
        })
        .fold(0.0, f64::max)
}

fn tensor_prefill_ideal(nodes: &[(ChipFactors, f64)], params: u64) -> Option<f64> {
    let slowest = nodes
        .iter()
        .map(|(f, _)| f.prefill_flops)
        .collect::<Option<Vec<f64>>>()?
        .into_iter()
        .fold(f64::MAX, f64::min);
    Some(nodes.len() as f64 * slowest / (2.0 * params as f64))
}

fn pipeline_prefill_ideal(nodes: &[(ChipFactors, f64)], params: u64) -> Option<f64> {
    let per_token_s: f64 = nodes
        .iter()
        .map(|(f, share)| f.prefill_flops.map(|flops| share * 2.0 * params as f64 / flops))
        .sum::<Option<f64>>()?;
    Some(1.0 / per_token_s)
}

/// A value with its range.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Estimate {
    pub value: f64,
    pub low: f64,
    pub high: f64,
}

impl Estimate {
    fn around(value: f64, relative: f64) -> Self {
        Self {
            value,
            low: value * (1.0 - relative),
            high: value * (1.0 + relative),
        }
    }

    fn scaled(self, by: f64) -> Self {
        Self {
            value: self.value * by,
            low: self.low * by,
            high: self.high * by,
        }
    }

    /// The middle and the extremes of measured values.
    pub fn of_measurements(values: &[f64]) -> Option<Self> {
        let mut sorted: Vec<f64> = values.iter().copied().filter(|v| v.is_finite()).collect();
        if sorted.is_empty() {
            return None;
        }
        sorted.sort_by(f64::total_cmp);
        let mid = sorted.len() / 2;
        let value = if sorted.len() % 2 == 1 {
            sorted[mid]
        } else {
            (sorted[mid - 1] + sorted[mid]) / 2.0
        };
        Some(Self {
            value,
            low: sorted[0],
            high: sorted[sorted.len() - 1],
        })
    }
}

/// One node of a placement, as the formula needs it.
#[derive(Debug, Clone)]
pub struct PlacedNode {
    pub chip: ChipIdentity,
    /// Share of the model's layers this node holds (pipeline); tensor/single ignore it.
    pub share: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeedEstimate {
    pub decode: Option<Estimate>,
    pub prefill: Option<Estimate>,
    pub throughput: Option<Estimate>,
    /// Concurrent requests the throughput figure assumes.
    pub concurrency: Option<u32>,
    /// What the estimate rests on, one line per factor; a missing figure's reason is here too.
    pub basis: Vec<String>,
}

/// The formula estimate for a placement at `working_context` tokens already in the conversation.
pub fn estimate(
    cal: &Calibration,
    kind: PlacementKind,
    link: Option<&str>,
    nodes: &[PlacedNode],
    model: &ModelFacts,
    kv_bytes_per_token: u64,
    working_context: u64,
) -> SpeedEstimate {
    let mut basis = Vec::new();
    let factors: Result<Vec<(ChipFactors, f64)>, String> = nodes
        .iter()
        .map(|n| {
            cal.chip(&n.chip)
                .map(|f| (f, n.share))
                .map_err(|e| format!("{}: {e}", n.chip.brand))
        })
        .collect();
    let factors = match factors {
        Ok(f) => f,
        Err(gap) => {
            basis.push(format!("no speed estimate: {gap}"));
            return SpeedEstimate {
                decode: None,
                prefill: None,
                throughput: None,
                concurrency: None,
                basis,
            };
        }
    };
    for (f, _) in &factors {
        if !f.calibrated {
            basis.push(f.basis.clone());
        }
    }
    let uncalibrated = factors.iter().any(|(f, _)| !f.calibrated);
    let chip_spread = if uncalibrated {
        cal.spec_efficiency_spread / cal.spec_efficiency.max(f64::MIN_POSITIVE) / 2.0
    } else {
        0.0
    };
    let active = model.active_bytes_per_token as f64;
    let kv = (kv_bytes_per_token * working_context) as f64;
    let (class, class_high) = if model.is_moe() {
        match cal.moe_factor {
            Some(f) => {
                basis.push(format!(
                    "MoE: {:.0}% of a dense model's bandwidth use, fitted to our Flash run; up to \
                     {:.0}% where other MoE runs reached more",
                    f * 100.0,
                    MOE_FACTOR_BEST_PUBLISHED.max(f) * 100.0
                ));
                (f, MOE_FACTOR_BEST_PUBLISHED.max(f))
            }
            None => {
                basis.push("MoE: no MoE run to fit the class factor".to_string());
                return SpeedEstimate {
                    decode: None,
                    prefill: None,
                    throughput: None,
                    concurrency: None,
                    basis,
                };
            }
        }
    } else {
        (1.0, 1.0)
    };

    let decode_ms = |class: f64| -> Option<f64> {
        match kind {
            PlacementKind::Single => {
                let (f, _) = &factors[0];
                let bw = f.bandwidth_gbs * BYTES_PER_MS_PER_GBS;
                Some(f.overhead_ms + active / (bw * class) + kv / bw)
            }
            PlacementKind::Tensor => {
                let allsum = cal.allsum_ms(link?)?;
                Some(tensor_compute_ms(&factors, active, kv, class) + 2.0 * model.layers as f64 * allsum)
            }
            PlacementKind::Pipeline => {
                let hop = cal.allsum_ms(link?)?;
                let stages: f64 = factors
                    .iter()
                    .map(|(f, share)| {
                        let bw = f.bandwidth_gbs * BYTES_PER_MS_PER_GBS;
                        share * (f.overhead_ms + active / (bw * class) + kv / bw)
                    })
                    .sum();
                Some(stages + factors.len() as f64 * hop)
            }
        }
    };
    let residual = match kind {
        PlacementKind::Single => cal.dense_residual,
        _ => cal.tensor_residual.max(cal.dense_residual),
    } + chip_spread;
    let decode = decode_ms(class).map(|ms| {
        let value = 1000.0 / ms;
        let high_ms = decode_ms(class_high).unwrap_or(ms);
        Estimate {
            value,
            low: value * (1.0 - residual),
            high: (1000.0 / high_ms).max(value) * (1.0 + residual),
        }
    });
    if decode.is_none() {
        basis.push(format!(
            "no decode estimate: no recorded {} run over {} to fit its link cost",
            kind.as_str(),
            link.unwrap_or("an unnamed link")
        ));
    }

    let params = model.active_params_per_token;
    let prefill = match kind {
        PlacementKind::Single => factors[0]
            .0
            .prefill_flops
            .map(|flops| flops / (2.0 * params as f64)),
        PlacementKind::Tensor => tensor_prefill_ideal(&factors, params).and_then(|ideal| {
            cal.split_prefill_factor
                .get(&(kind, link?.to_string()))
                .map(|f| ideal * f)
        }),
        PlacementKind::Pipeline => pipeline_prefill_ideal(&factors, params).and_then(|ideal| {
            cal.split_prefill_factor
                .get(&(kind, link?.to_string()))
                .map(|f| ideal * f)
        }),
    }
    .map(|v| {
        // MoE prefill on one Mac has no run of its own to fit; the pipeline's factor carries it.
        let v = if model.is_moe() && kind == PlacementKind::Single {
            v * cal
                .split_prefill_factor
                .iter()
                .find(|((k, _), _)| *k == PlacementKind::Pipeline)
                .map(|(_, f)| *f)
                .unwrap_or(1.0)
        } else {
            v
        };
        Estimate::around(v, residual)
    });
    if prefill.is_none() {
        basis.push("no prefill estimate: no prefill run on this chip or split".to_string());
    }
    let gain = batch_gain(kind);
    let throughput = decode.map(|d| d.scaled(gain.gain));
    basis.push(format!(
        "many requests: ×{:.2} at {} concurrent — {}",
        gain.gain, gain.concurrency, gain.source
    ));
    SpeedEstimate {
        decode,
        prefill,
        throughput,
        concurrency: Some(gain.concurrency),
        basis,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kv_cache::KvCacheFacts;

    fn chip(brand: &str, cores: u32) -> ChipIdentity {
        ChipIdentity {
            hw_model: String::new(),
            brand: brand.to_string(),
            gpu_cores: Some(cores),
        }
    }

    pub(crate) fn facts(active: u64, params: u64, layers: u64, moe: bool) -> ModelFacts {
        ModelFacts {
            model_type: "t".into(),
            layers,
            max_context: Some(262_144),
            moe: moe.then_some(super::super::model::MoeFacts {
                experts: 512,
                experts_per_token: 10,
            }),
            quant: None,
            resident_bytes: active,
            active_bytes_per_token: active,
            active_params_per_token: params,
            lookup_bytes: 0,
            excluded_bytes: 0,
            largest_layer_bytes: 0,
            kv: Err::<KvCacheFacts, _>("n/a".into()),
        }
    }

    fn within(estimate: Option<Estimate>, measured: f64, relative: f64) {
        let e = estimate.expect("an estimate");
        assert!(
            (e.value - measured).abs() / measured <= relative,
            "estimate {e:?} vs measured {measured}"
        );
        assert!(e.low <= e.value && e.value <= e.high, "{e:?}");
    }

    #[test]
    fn the_fit_reproduces_every_recorded_run_it_was_fitted_on() {
        let cal = Calibration::fit(&BTreeMap::new());
        let m3 = cal.chip(&chip("Apple M3 Ultra", 60)).unwrap();
        let m4 = cal.chip(&chip("Apple M4 Max", 40)).unwrap();
        assert!(m3.calibrated && m4.calibrated);
        // Fitted BW_eff sits below Apple's spec on both chips.
        assert!(m3.bandwidth_gbs < 819.0 && m3.bandwidth_gbs > 600.0, "{m3:?}");
        assert!(m4.bandwidth_gbs < 546.0 && m4.bandwidth_gbs > 400.0, "{m4:?}");
        assert!(cal.dense_residual() < 0.08, "{}", cal.dense_residual());
        // ~0.3 ms per all-sum over JACCL, ~0.5 over ring (STEP1's reading).
        let jaccl = cal.allsum_ms("jaccl").unwrap();
        let ring = cal.allsum_ms("ring").unwrap();
        assert!((0.2..0.4).contains(&jaccl) && ring > jaccl, "{jaccl} {ring}");
        let moe = cal.moe_factor().unwrap();
        assert!((0.1..0.3).contains(&moe), "{moe}");

        let dense27 = facts(QWEN_27B_ACTIVE_BYTES, QWEN_27B_ACTIVE_PARAMS, 64, false);
        let single = |c: ChipIdentity| {
            estimate(&cal, PlacementKind::Single, None, &[PlacedNode { chip: c, share: 1.0 }], &dense27, 0, 0)
        };
        let m3_single = single(chip("Apple M3 Ultra", 60));
        within(m3_single.decode, 22.2, 0.08);
        within(m3_single.prefill, 335.2, 0.08);
        within(single(chip("Apple M4 Max", 40)).decode, 15.1, 0.08);
        let pair = [
            PlacedNode { chip: chip("Apple M4 Max", 40), share: 0.5 },
            PlacedNode { chip: chip("Apple M3 Ultra", 60), share: 0.5 },
        ];
        let tensor = estimate(&cal, PlacementKind::Tensor, Some("jaccl"), &pair, &dense27, 0, 0);
        within(tensor.decode, 14.6, 0.15);
        within(tensor.prefill, 405.7, 0.10);

        let flash = facts(FLASH_ACTIVE_BYTES, FLASH_ACTIVE_PARAMS, 48, true);
        let stages = [
            PlacedNode { chip: chip("Apple M4 Max", 40), share: 20.0 / 48.0 },
            PlacedNode { chip: chip("Apple M3 Ultra", 60), share: 28.0 / 48.0 },
        ];
        let pipe = estimate(&cal, PlacementKind::Pipeline, Some("jaccl"), &stages, &flash, 0, 0);
        within(pipe.decode, 22.5, 0.10);
        within(pipe.prefill, 683.0, 0.10);
        let throughput = pipe.throughput.unwrap();
        assert!((throughput.value / pipe.decode.unwrap().value - 47.0 / 22.5).abs() < 1e-9);
    }

    #[test]
    fn a_chip_without_runs_is_estimated_from_its_spec_and_a_chip_without_a_spec_is_a_gap() {
        let cal = Calibration::fit(&BTreeMap::new());
        let m5 = cal.chip(&chip("Apple M5 Max", 40)).unwrap();
        assert!(!m5.calibrated);
        assert!(m5.bandwidth_gbs < 614.0 && m5.bandwidth_gbs > 450.0, "{m5:?}");
        assert!(m5.prefill_flops.is_none(), "no M5 run: prefill is not guessed");
        let big = cal.chip(&chip("Apple M3 Ultra", 80)).unwrap();
        let small = cal.chip(&chip("Apple M3 Ultra", 60)).unwrap();
        assert!((big.prefill_flops.unwrap() / small.prefill_flops.unwrap() - 80.0 / 60.0).abs() < 1e-9);
        assert!(cal.chip(&chip("Apple M1", 8)).unwrap_err().contains("not in goose's bandwidth table"));

        let dense = facts(QWEN_27B_ACTIVE_BYTES, QWEN_27B_ACTIVE_PARAMS, 64, false);
        let gap = estimate(&cal, PlacementKind::Single, None, &[PlacedNode { chip: chip("Apple M1", 8), share: 1.0 }], &dense, 0, 0);
        assert!(gap.decode.is_none() && gap.basis[0].contains("no speed estimate"), "{gap:?}");
        let m5_est = estimate(&cal, PlacementKind::Single, None, &[PlacedNode { chip: chip("Apple M5 Max", 40), share: 1.0 }], &dense, 0, 0);
        assert!(m5_est.decode.is_some() && m5_est.prefill.is_none());
    }

    #[test]
    fn goose_measurements_move_the_chip_fit() {
        let seeds_only = Calibration::fit(&BTreeMap::new());
        let before = seeds_only.chip(&chip("Apple M4 Max", 40)).unwrap().bandwidth_gbs;
        let faster = BTreeMap::from([(
            "Apple M4 Max/40".to_string(),
            vec![SinglePoint { active_bytes: QWEN_27B_ACTIVE_BYTES, active_params: QWEN_27B_ACTIVE_PARAMS, prefill_tps: None, decode_tps: 21.5 }],
        )]);
        let after = Calibration::fit(&faster).chip(&chip("Apple M4 Max", 40)).unwrap();
        assert!(after.bandwidth_gbs > before, "{} !> {before}", after.bandwidth_gbs);
        assert!(after.basis.contains("3 run(s)"), "{}", after.basis);
    }

    #[test]
    fn a_long_context_costs_decode_speed() {
        let cal = Calibration::fit(&BTreeMap::new());
        let dense = facts(QWEN_27B_ACTIVE_BYTES, QWEN_27B_ACTIVE_PARAMS, 64, false);
        let node = [PlacedNode { chip: chip("Apple M3 Ultra", 60), share: 1.0 }];
        let short = estimate(&cal, PlacementKind::Single, None, &node, &dense, 65_536, 2_048);
        let long = estimate(&cal, PlacementKind::Single, None, &node, &dense, 65_536, 131_072);
        assert!(long.decode.unwrap().value < short.decode.unwrap().value * 0.9);
    }

    #[test]
    fn measurements_summarise_to_their_median_and_extremes() {
        let e = Estimate::of_measurements(&[21.0, 23.0, 22.0]).unwrap();
        assert_eq!((e.value, e.low, e.high), (22.0, 21.0, 23.0));
        assert_eq!(Estimate::of_measurements(&[20.0, 22.0]).unwrap().value, 21.0);
        assert!(Estimate::of_measurements(&[]).is_none());
    }
}
