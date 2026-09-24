//! THE fit rule — one function for every place goose asks "does this fit on that Mac?": the single
//! engine's mount gate, the placement planner's fit check (single and split), the tensor runner's
//! per-rank budget, the distributed preflight, and the verdict the desktop draws on the picked
//! model (`mlxEngine/status` `mountFit`) — the desktop reads it, never recomputes it.
//!
//! A node's budget is `min(available − RAM × AVAILABLE_MARGIN_RATIO, GPU ceiling)`; a need fits
//! when it is ≤ the budget, and is TIGHT when what is left above it is inside the live re-measure
//! drift (`DERIVED_CONTEXT_MARGIN_RATIO` of RAM). The fork's pipeline planner builds its budget by
//! the same rule in Python (`pipeline_qwen4.py`, its ratio echoed in the plan JSON).
//!
//! It replaced the single engine's own gate — `model + max(8 GiB, 10% of RAM) ≤ available`, a
//! 4 GiB warn band, no ceiling — which refused and admitted on different numbers from the planner:
//! the owner's M4 Max 128 GB read "floor 12.8 GiB" on the mount and "9.3% reserve" on the badge
//! for the same model at the same moment (2026-09-24).

use std::fmt;

pub const GIB: u64 = 1024 * 1024 * 1024;

// measured: the share of each node's RAM that stays available under a rank's full budget —
// budget = min(available − RAM × this, the node's GPU ceiling (Metal's
// max_recommended_working_set_size)) — is the highest kernel-WARN point measured plus the load
// drift. 2026-09-24, goose's own compaction (memory_pressure to WARN) read the kernel's WARN point
// at 9.3 GiB available on the M4 Max 128 GB (7.3% of RAM) and 3.3–4.0 GiB on the M3 Ultra 96 GB;
// + DERIVED_CONTEXT_MARGIN_RATIO (0.02, the ranks' re-measure drift) = 0.093. The first loosening
// (0.07 = watchdog WARN 0.05 + 0.02, from the M3 Ultra alone) sat BELOW the M4 Max's WARN point: a
// live Flash split with rank 0 at 96% of that budget served under kernel WARN (7.1 GiB available)
// and the watchdog closed admission — backed off to this. It replaced the fork's 21% floor and the
// tensor runner's min(available × 0.90, RAM × 0.75) (44.9 / 52.6 GiB budgets on 128 / 96 GB:
// "way too conservative"). The fork carries the same value (pipeline_qwen4.py, echoed in its plan
// JSON); both runners, the placement planner and the single engine's mount share this one rule.
pub const AVAILABLE_MARGIN_RATIO: f64 = 0.093;
// measured: a DERIVED pipeline context is planned against every node's available memory minus
// this share of its RAM. The derivation's own ceiling sits at 100% of budget, and the ranks
// re-check against LIVE memory at load: on 2026-09-24 (Flash, 16 s after a passing preflight) the
// rank-0 budget read 62.55 GiB against preflight's 63.30 (−0.75 GiB = 0.6% of 128 GiB RAM) and
// rank 1's 48.74 against 49.21 (−0.47 GiB = 0.5% of 96), so both ranks refused at 101%. 2% of RAM
// is 3.4x the larger drift; a REQUESTED context is planned as asked, without it. The fit rule's
// TIGHT band is the same drift: a fit with less than this left above it can turn into a refusal
// between the verdict and the load.
pub const DERIVED_CONTEXT_MARGIN_RATIO: f64 = 0.02;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Allow,
    Warn,
    Block,
}

impl Verdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Verdict::Allow => "allow",
            Verdict::Warn => "warn",
            Verdict::Block => "block",
        }
    }
}

/// One Mac's memory as the rule reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NodeMemoryFacts {
    pub available_bytes: u64,
    pub total_bytes: u64,
    /// Metal's `recommendedMaxWorkingSetSize` on that Mac.
    pub ceiling_bytes: u64,
}

impl NodeMemoryFacts {
    pub fn margin_bytes(&self) -> u64 {
        (self.total_bytes as f64 * AVAILABLE_MARGIN_RATIO) as u64
    }

    pub fn budget_bytes(&self) -> u64 {
        budget_bytes(self.available_bytes, self.total_bytes, self.ceiling_bytes)
    }

    /// The budget if every byte of RAM were available: no amount of reclaimed memory lets a need
    /// above it through.
    pub fn best_case_budget_bytes(&self) -> u64 {
        budget_bytes(self.total_bytes, self.total_bytes, self.ceiling_bytes)
    }

    pub fn tight_band_bytes(&self) -> u64 {
        (self.total_bytes as f64 * DERIVED_CONTEXT_MARGIN_RATIO) as u64
    }
}

/// The budget: `min(available − RAM × AVAILABLE_MARGIN_RATIO, GPU ceiling)`.
pub fn budget_bytes(available_bytes: u64, total_bytes: u64, ceiling_bytes: u64) -> u64 {
    available_bytes
        .saturating_sub((total_bytes as f64 * AVAILABLE_MARGIN_RATIO) as u64)
        .min(ceiling_bytes)
}

/// What a single engine asks of its Mac: the weights, plus the KV cache for `context_tokens` when
/// the model's KV can be sized (`kv_gap` says why not, and then only the weights are charged).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Need {
    pub weights_bytes: u64,
    pub kv_bytes: u64,
    pub context_tokens: u64,
    pub kv_gap: Option<String>,
}

impl Need {
    pub fn single_engine(
        weights_bytes: u64,
        kv_bytes_per_token: Result<u64, String>,
        context_tokens: u64,
    ) -> Self {
        match kv_bytes_per_token {
            Ok(per_token) => Need {
                weights_bytes,
                kv_bytes: per_token.saturating_mul(context_tokens),
                context_tokens,
                kv_gap: None,
            },
            Err(gap) => Need {
                weights_bytes,
                kv_bytes: 0,
                context_tokens,
                kv_gap: Some(gap),
            },
        }
    }

    pub fn total_bytes(&self) -> u64 {
        self.weights_bytes.saturating_add(self.kv_bytes)
    }

    fn describe(&self) -> String {
        match &self.kv_gap {
            None => format!(
                "{} ({} of weights + {} of KV for {} tokens)",
                gib(self.total_bytes()),
                gib(self.weights_bytes),
                gib(self.kv_bytes),
                self.context_tokens
            ),
            Some(gap) => format!("{} of weights (KV not sized: {gap})", gib(self.weights_bytes)),
        }
    }
}

/// The rule's answer for one need on one Mac, with every figure it used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FitVerdict {
    pub verdict: Verdict,
    pub need: Need,
    pub facts: NodeMemoryFacts,
    pub budget_bytes: u64,
    /// The rule's arithmetic in words; a refusal carries it verbatim.
    pub message: String,
}

impl FitVerdict {
    pub fn short_bytes(&self) -> Option<u64> {
        (self.verdict == Verdict::Block)
            .then(|| self.need.total_bytes().saturating_sub(self.budget_bytes))
    }

    pub fn spare_bytes(&self) -> Option<u64> {
        (self.verdict != Verdict::Block)
            .then(|| self.budget_bytes.saturating_sub(self.need.total_bytes()))
    }

    /// Whether ANY amount of reclaimed memory could turn this refusal into a fit.
    pub fn could_ever_fit(&self) -> bool {
        self.need.total_bytes() <= self.facts.best_case_budget_bytes()
    }

    pub fn append(&mut self, note: impl fmt::Display) {
        self.message = format!("{} — {note}", self.message);
    }
}

pub fn judge(need: Need, facts: NodeMemoryFacts) -> FitVerdict {
    let budget = facts.budget_bytes();
    let needed = need.total_bytes();
    let rule = format!(
        "budget {} = min(available {} − the {:.1}% margin {}, GPU ceiling {})",
        gib(budget),
        gib(facts.available_bytes),
        AVAILABLE_MARGIN_RATIO * 100.0,
        gib(facts.margin_bytes()),
        gib(facts.ceiling_bytes)
    );
    let (verdict, message) = if needed > budget {
        (
            Verdict::Block,
            format!(
                "needs {} but the {rule} (short {})",
                need.describe(),
                gib(needed - budget)
            ),
        )
    } else if budget - needed < facts.tight_band_bytes() {
        (
            Verdict::Warn,
            format!(
                "fits, but only {} under the {rule} — inside the {:.0}% live-memory drift, expect \
                 pressure under load",
                gib(budget - needed),
                DERIVED_CONTEXT_MARGIN_RATIO * 100.0
            ),
        )
    } else {
        (
            Verdict::Allow,
            format!(
                "needs {} with {} to spare under the {rule}",
                need.describe(),
                gib(budget - needed)
            ),
        )
    };
    FitVerdict {
        verdict,
        need,
        facts,
        budget_bytes: budget,
        message,
    }
}

fn gib(bytes: u64) -> String {
    format!("{:.1} GiB", bytes as f64 / GIB as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The M4 Max 128 GB and its Metal ceiling (2026-09-24).
    const M4_TOTAL: u64 = 128 * GIB;
    const M4_CEILING: u64 = 115_448_725_504;
    /// The M3 Ultra 96 GB.
    const M3_TOTAL: u64 = 96 * GIB;
    const M3_CEILING: u64 = 83_494_174_720;

    fn gib_f(v: f64) -> u64 {
        (v * GIB as f64) as u64
    }

    fn weights_only(bytes: u64) -> Need {
        Need::single_engine(bytes, Ok(0), 0)
    }

    /// The owner's 3.0.25 refusal: Flash-Next-4bit (97.5 GiB on disk) on the M4 Max with 93.0 GiB
    /// available after Make room. The old gate said "floor 12.8 GiB, short 17.3"; the one rule
    /// says budget 81.1 GiB, short 16.4 — and no reclaim could ever fit it (the ceiling is 107.5).
    #[test]
    fn the_recorded_flash_mount_is_refused_by_the_one_rule() {
        let facts = NodeMemoryFacts {
            available_bytes: gib_f(93.0),
            total_bytes: M4_TOTAL,
            ceiling_bytes: M4_CEILING,
        };
        let v = judge(weights_only(gib_f(97.5)), facts);
        assert_eq!(v.verdict, Verdict::Block);
        assert_eq!(v.budget_bytes, gib_f(93.0) - (M4_TOTAL as f64 * 0.093) as u64);
        assert_eq!(v.short_bytes(), Some(gib_f(97.5) - v.budget_bytes));
        assert!(v.message.contains("budget 81.1 GiB"), "{}", v.message);
        assert!(v.message.contains("the 9.3% margin 11.9 GiB"), "{}", v.message);
        assert!(v.message.contains("GPU ceiling 107.5 GiB"), "{}", v.message);
        assert!(v.message.contains("short 16.4 GiB"), "{}", v.message);
        assert!(v.could_ever_fit(), "all 128 GiB free would give a 107.5 GiB budget");
    }

    #[test]
    fn the_gpu_ceiling_binds_a_mount_on_an_idle_mac() {
        let facts = NodeMemoryFacts {
            available_bytes: gib_f(90.0),
            total_bytes: M3_TOTAL,
            ceiling_bytes: M3_CEILING,
        };
        // Available 90 − 8.9 margin = 81.1, but Metal's ceiling is 77.8 GiB.
        assert_eq!(facts.budget_bytes(), M3_CEILING);
        assert_eq!(judge(weights_only(gib_f(79.0)), facts).verdict, Verdict::Block);
        let never = judge(weights_only(gib_f(79.0)), facts);
        assert!(!never.could_ever_fit(), "above the ceiling nothing reclaimed helps");
    }

    #[test]
    fn a_fit_inside_the_live_drift_warns_and_a_roomy_one_allows() {
        let facts = NodeMemoryFacts {
            available_bytes: gib_f(60.0),
            total_bytes: M4_TOTAL,
            ceiling_bytes: M4_CEILING,
        };
        let budget = facts.budget_bytes();
        let tight = judge(weights_only(budget - gib_f(1.0)), facts);
        assert_eq!(tight.verdict, Verdict::Warn, "{}", tight.message);
        assert_eq!(tight.spare_bytes(), Some(gib_f(1.0)));
        let roomy = judge(weights_only(budget - gib_f(10.0)), facts);
        assert_eq!(roomy.verdict, Verdict::Allow, "{}", roomy.message);
        assert_eq!(judge(weights_only(budget), facts).verdict, Verdict::Warn);
        assert_eq!(judge(weights_only(budget + 1), facts).verdict, Verdict::Block);
    }

    #[test]
    fn the_kv_for_the_context_is_charged_and_an_unsized_kv_is_named() {
        let need = Need::single_engine(gib_f(30.0), Ok(65_536), 4_608);
        assert_eq!(need.kv_bytes, 65_536 * 4_608);
        let gap = Need::single_engine(gib_f(30.0), Err("no config.json".into()), 4_608);
        assert_eq!(gap.total_bytes(), gib_f(30.0));
        let facts = NodeMemoryFacts {
            available_bytes: gib_f(20.0),
            total_bytes: M4_TOTAL,
            ceiling_bytes: M4_CEILING,
        };
        let v = judge(gap, facts);
        assert!(v.message.contains("KV not sized: no config.json"), "{}", v.message);
    }
}
