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
    /// The resident bytes the OTHER MLX engines on that Mac hold (`machine::other_engines`, read
    /// from the kernel), charged against the ceiling: Metal's working set is the Mac's, shared by
    /// every process on the GPU, and each engine wires up to all of it (Q-11, Q-106). The RAM side
    /// needs no charge — what they hold is already not available. 0 where the caller has no census
    /// of the Mac: the placement planner, which recommends and never admits a load (the gates that
    /// admit one — the single engine's mount and the split's preflight — always measure it).
    pub other_engines_bytes: u64,
    /// The resident bytes of the engine a SWITCH stops on this Mac before the load: Run replaces
    /// the way that serves now, it never adds a second one, so what that engine holds is available
    /// to the switch. Measured by the caller (the engine's own footprint); 0 = nothing is replaced
    /// here. Q-120 (2026-09-26): with the 27B served from Work's Mac Studio (~30 GB there), Flash
    /// read "Too big, short 1.6 GB" because only a switch to the SAME model counted that memory.
    pub freed_by_switch_bytes: u64,
}

impl NodeMemoryFacts {
    pub fn margin_bytes(&self) -> u64 {
        (self.total_bytes as f64 * AVAILABLE_MARGIN_RATIO) as u64
    }

    /// What of the GPU ceiling the other engines leave.
    pub fn ceiling_left_bytes(&self) -> u64 {
        self.ceiling_bytes.saturating_sub(self.other_engines_bytes)
    }

    /// Available once the switch has stopped the engine it replaces.
    pub fn available_after_switch_bytes(&self) -> u64 {
        self.available_bytes
            .saturating_add(self.freed_by_switch_bytes)
    }

    pub fn budget_bytes(&self) -> u64 {
        budget_bytes(
            self.available_after_switch_bytes(),
            self.total_bytes,
            self.ceiling_left_bytes(),
        )
    }

    /// The budget while the replaced engine still holds its memory.
    pub fn budget_before_switch_bytes(&self) -> u64 {
        budget_bytes(
            self.available_bytes,
            self.total_bytes,
            self.ceiling_left_bytes(),
        )
    }

    /// The budget if every byte of RAM were available: no amount of reclaimed memory lets a need
    /// above it through (the other engines' share of the ceiling is not reclaimable memory).
    pub fn best_case_budget_bytes(&self) -> u64 {
        budget_bytes(
            self.total_bytes,
            self.total_bytes,
            self.ceiling_left_bytes(),
        )
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

/// What one sequence costs a single engine at a context — ONE model for the mount gate, the
/// placement planner's single-engine fit and its no-fork pipeline estimate.
///
/// At `context` tokens the engine holds the weights, the KV (rounded up to the cache's growth
/// step), the linear-attention state, and — at the last prefill chunk — the step's transient: the
/// attention scores of a `floor_chunk_tokens` chunk against the whole context
/// (`prefill_score_bytes_per_pair` per chunk token × context token), plus the KV buffer being
/// extended, copied once (`kv_growth_bytes_per_token`: one full-attention layer's KV). The floor
/// chunk is the smallest the engine's memory guard halves a prefill step down to, so the largest
/// context is the one whose floor step still fits. Prefix-cache entries and MLX's allocator
/// cache are NOT charged: the engine's guard evicts the one and caps the other before a step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SingleEngineCost {
    pub weights_bytes: u64,
    pub kv_bytes_per_token: u64,
    pub kv_growth_bytes_per_token: u64,
    pub sequence_state_bytes: u64,
    pub prefill_score_bytes_per_pair: u64,
    pub floor_chunk_tokens: u64,
    /// The KV buffer's growth step: a context costs its size rounded up to it.
    pub kv_step_tokens: u64,
    /// The model's own window; no context above it is offered.
    pub max_position: Option<u64>,
    /// What could not be sized and is charged as 0, each naming why.
    pub gaps: Vec<String>,
}

impl SingleEngineCost {
    pub fn kv_bytes(&self, context: u64) -> u64 {
        context.div_ceil(self.kv_step_tokens.max(1)) * self.kv_step_tokens.max(1)
            * self.kv_bytes_per_token
    }

    /// The floor prefill step's transient at `context`: its scores and the KV growth copy.
    pub fn prefill_bytes(&self, context: u64) -> u64 {
        self.floor_chunk_tokens.min(context) * context * self.prefill_score_bytes_per_pair
            + context * self.kv_growth_bytes_per_token
    }

    pub fn bytes_at(&self, context: u64) -> u64 {
        self.weights_bytes
            + self.kv_bytes(context)
            + self.sequence_state_bytes
            + self.prefill_bytes(context)
    }

    /// The largest context (a multiple of the KV step, at most the model's window) whose cost plus
    /// `margin_bytes` stays within `budget_bytes`; 0 when not even one step fits. `None` when
    /// nothing grows with the context (no KV, no scores): every context costs the same.
    pub fn max_context(&self, budget_bytes: u64, margin_bytes: u64) -> Option<u64> {
        let step = self.kv_step_tokens.max(1);
        let per_token =
            self.kv_bytes_per_token + self.kv_growth_bytes_per_token + self.prefill_score_bytes_per_pair;
        if per_token == 0 {
            return None;
        }
        let fits = |context: u64| self.bytes_at(context).saturating_add(margin_bytes) <= budget_bytes;
        let upper = self
            .max_position
            .unwrap_or(budget_bytes / per_token + step)
            .div_ceil(step);
        if upper == 0 || !fits(step) {
            return Some(0);
        }
        let (mut low, mut high) = (1u64, upper);
        while low < high {
            let middle = (low + high).div_ceil(2);
            if fits(middle * step) {
                low = middle;
            } else {
                high = middle - 1;
            }
        }
        Some(match self.max_position {
            Some(max) => (low * step).min(max),
            None => low * step,
        })
    }
}

/// What a single engine asks of its Mac at `context_tokens`: [`SingleEngineCost`]'s terms, or the
/// weights alone when the model's cost cannot be sized (`kv_gap` says why).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Need {
    pub weights_bytes: u64,
    pub kv_bytes: u64,
    /// The linear-attention state and the floor prefill step's transient at the context.
    pub state_bytes: u64,
    pub prefill_bytes: u64,
    pub context_tokens: u64,
    pub kv_gap: Option<String>,
    /// A term of the cost that could not be sized and is charged as 0, each naming why.
    pub gaps: Vec<String>,
}

impl Need {
    pub fn at(cost: &SingleEngineCost, context_tokens: u64) -> Self {
        Need {
            weights_bytes: cost.weights_bytes,
            kv_bytes: cost.kv_bytes(context_tokens),
            state_bytes: cost.sequence_state_bytes,
            prefill_bytes: cost.prefill_bytes(context_tokens),
            context_tokens,
            kv_gap: None,
            gaps: cost.gaps.clone(),
        }
    }

    /// The weights alone, nothing missing: a model whose cost grows with nothing else.
    pub fn weights(weights_bytes: u64) -> Self {
        Need {
            weights_bytes,
            kv_bytes: 0,
            state_bytes: 0,
            prefill_bytes: 0,
            context_tokens: 0,
            kv_gap: None,
            gaps: Vec::new(),
        }
    }

    pub fn weights_only(weights_bytes: u64, gap: String) -> Self {
        Need {
            weights_bytes,
            kv_bytes: 0,
            state_bytes: 0,
            prefill_bytes: 0,
            context_tokens: 0,
            kv_gap: Some(gap),
            gaps: Vec::new(),
        }
    }

    pub fn single_engine(
        weights_bytes: u64,
        cost: Result<SingleEngineCost, String>,
        context_tokens: u64,
    ) -> Self {
        match cost {
            Ok(cost) => Need::at(&cost, context_tokens),
            Err(gap) => Need::weights_only(weights_bytes, gap),
        }
    }

    pub fn total_bytes(&self) -> u64 {
        self.weights_bytes
            .saturating_add(self.kv_bytes)
            .saturating_add(self.state_bytes)
            .saturating_add(self.prefill_bytes)
    }

    fn describe(&self) -> String {
        let gaps: String = self.gaps.iter().map(|g| format!("; {g}")).collect();
        let terms = match &self.kv_gap {
            None if self.state_bytes == 0 && self.prefill_bytes == 0 => format!(
                "{} ({} of weights + {} of KV for {} tokens)",
                gb(self.total_bytes()),
                gb(self.weights_bytes),
                gb(self.kv_bytes),
                self.context_tokens
            ),
            None => format!(
                "{} ({} of weights + {} of KV + {} of recurrent state + {} for a prefill step, for \
                 {} tokens)",
                gb(self.total_bytes()),
                gb(self.weights_bytes),
                gb(self.kv_bytes),
                gb(self.state_bytes),
                gb(self.prefill_bytes),
                self.context_tokens
            ),
            Some(gap) => format!(
                "{} of weights (KV not sized: {gap})",
                gb(self.weights_bytes)
            ),
        };
        format!("{terms}{gaps}")
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

    /// A fit that holds only once the switch has stopped the engine it replaces: "fits once the
    /// 27B stops", never "too big".
    pub fn needs_switch(&self) -> bool {
        self.verdict != Verdict::Block
            && self.need.total_bytes() > self.facts.budget_before_switch_bytes()
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
    let ceiling = match facts.other_engines_bytes {
        0 => format!("GPU ceiling {}", gb(facts.ceiling_bytes)),
        other => format!(
            "GPU ceiling {} − {} other engines hold",
            gb(facts.ceiling_bytes),
            gb(other)
        ),
    };
    let available = match facts.freed_by_switch_bytes {
        0 => format!("available {}", gb(facts.available_bytes)),
        freed => format!(
            "available {} + {} the switch frees",
            gb(facts.available_bytes),
            gb(freed)
        ),
    };
    let rule = format!(
        "budget {} = min({available} − the {:.1}% margin {}, {ceiling})",
        gb(budget),
        AVAILABLE_MARGIN_RATIO * 100.0,
        gb(facts.margin_bytes()),
    );
    let (verdict, message) = if needed > budget {
        (
            Verdict::Block,
            format!(
                "needs {} but the {rule} (short {})",
                need.describe(),
                gb(needed - budget)
            ),
        )
    } else if budget - needed < facts.tight_band_bytes() {
        (
            Verdict::Warn,
            format!(
                "fits, but only {} under the {rule} — inside the {:.0}% live-memory drift, expect \
                 pressure under load",
                gb(budget - needed),
                DERIVED_CONTEXT_MARGIN_RATIO * 100.0
            ),
        )
    } else {
        (
            Verdict::Allow,
            format!(
                "needs {} with {} to spare under the {rule}",
                need.describe(),
                gb(budget - needed)
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

/// A size in the mount gate's words, in the product's unit: GiB labelled GB, as the desktop's
/// picker and memory line write it ("31 GB", "37.0 GB available of 96.0 GB"). One decimal, so the
/// gate's arithmetic (budget = available − margin) still adds up on screen.
pub fn gb(bytes: u64) -> String {
    format!("{:.1} GB", bytes as f64 / GIB as f64)
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
        Need::weights(bytes)
    }

    fn cost(weights: u64, kv_per_token: u64) -> SingleEngineCost {
        SingleEngineCost {
            weights_bytes: weights,
            kv_bytes_per_token: kv_per_token,
            kv_growth_bytes_per_token: 0,
            sequence_state_bytes: 0,
            prefill_score_bytes_per_pair: 0,
            floor_chunk_tokens: 0,
            kv_step_tokens: 1,
            max_position: None,
            gaps: Vec::new(),
        }
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
            other_engines_bytes: 0,
            freed_by_switch_bytes: 0,
        };
        let v = judge(weights_only(gib_f(97.5)), facts);
        assert_eq!(v.verdict, Verdict::Block);
        assert_eq!(
            v.budget_bytes,
            gib_f(93.0) - (M4_TOTAL as f64 * 0.093) as u64
        );
        assert_eq!(v.short_bytes(), Some(gib_f(97.5) - v.budget_bytes));
        assert!(v.message.contains("budget 81.1 GB"), "{}", v.message);
        assert!(
            v.message.contains("the 9.3% margin 11.9 GB"),
            "{}",
            v.message
        );
        assert!(v.message.contains("GPU ceiling 107.5 GB"), "{}", v.message);
        assert!(v.message.contains("short 16.4 GB"), "{}", v.message);
        assert!(!v.message.contains("GiB"), "{}", v.message);
        assert!(
            v.could_ever_fit(),
            "all 128 GiB free would give a 107.5 GiB budget"
        );
    }

    #[test]
    fn the_gpu_ceiling_binds_a_mount_on_an_idle_mac() {
        let facts = NodeMemoryFacts {
            available_bytes: gib_f(90.0),
            total_bytes: M3_TOTAL,
            ceiling_bytes: M3_CEILING,
            other_engines_bytes: 0,
            freed_by_switch_bytes: 0,
        };
        // Available 90 − 8.9 margin = 81.1, but Metal's ceiling is 77.8 GiB.
        assert_eq!(facts.budget_bytes(), M3_CEILING);
        assert_eq!(
            judge(weights_only(gib_f(79.0)), facts).verdict,
            Verdict::Block
        );
        let never = judge(weights_only(gib_f(79.0)), facts);
        assert!(
            !never.could_ever_fit(),
            "above the ceiling nothing reclaimed helps"
        );
    }

    #[test]
    fn a_fit_inside_the_live_drift_warns_and_a_roomy_one_allows() {
        let facts = NodeMemoryFacts {
            available_bytes: gib_f(60.0),
            total_bytes: M4_TOTAL,
            ceiling_bytes: M4_CEILING,
            other_engines_bytes: 0,
            freed_by_switch_bytes: 0,
        };
        let budget = facts.budget_bytes();
        let tight = judge(weights_only(budget - gib_f(1.0)), facts);
        assert_eq!(tight.verdict, Verdict::Warn, "{}", tight.message);
        assert_eq!(tight.spare_bytes(), Some(gib_f(1.0)));
        let roomy = judge(weights_only(budget - gib_f(10.0)), facts);
        assert_eq!(roomy.verdict, Verdict::Allow, "{}", roomy.message);
        assert_eq!(judge(weights_only(budget), facts).verdict, Verdict::Warn);
        assert_eq!(
            judge(weights_only(budget + 1), facts).verdict,
            Verdict::Block
        );
    }

    #[test]
    fn the_kv_for_the_context_is_charged_and_an_unsized_kv_is_named() {
        let need = Need::single_engine(gib_f(30.0), Ok(cost(gib_f(30.0), 65_536)), 4_608);
        assert_eq!(need.kv_bytes, 65_536 * 4_608);
        let gap = Need::single_engine(gib_f(30.0), Err("no config.json".into()), 4_608);
        assert_eq!(gap.total_bytes(), gib_f(30.0));
        let facts = NodeMemoryFacts {
            available_bytes: gib_f(20.0),
            total_bytes: M4_TOTAL,
            ceiling_bytes: M4_CEILING,
            other_engines_bytes: 0,
            freed_by_switch_bytes: 0,
        };
        let v = judge(gap, facts);
        assert!(
            v.message.contains("KV not sized: no config.json"),
            "{}",
            v.message
        );
    }

    /// Q-120 (3.0.47, 2026-09-26): Work's Mac Studio read 44.2 GB available of 96 while the 27B
    /// it served held ~30 GB. A need 1.6 GB over the budget that memory leaves is refused while
    /// nothing is replaced — and fits once the switch counts what stopping the 27B frees, with the
    /// verdict saying it holds only once it stops. The GPU ceiling still binds: freed RAM never
    /// raises Metal's working set.
    #[test]
    fn memory_a_switch_frees_counts_and_the_fit_says_it_needs_the_switch() {
        let held = NodeMemoryFacts {
            available_bytes: gib_f(44.2),
            total_bytes: M3_TOTAL,
            ceiling_bytes: M3_CEILING,
            other_engines_bytes: 0,
            freed_by_switch_bytes: 0,
        };
        let need = weights_only(held.budget_bytes() + gib_f(1.6));
        let refused = judge(need.clone(), held);
        assert_eq!(refused.verdict, Verdict::Block, "{}", refused.message);
        assert!(!refused.needs_switch());

        let switching = NodeMemoryFacts {
            freed_by_switch_bytes: gib_f(30.0),
            ..held
        };
        assert_eq!(switching.budget_before_switch_bytes(), held.budget_bytes());
        assert_eq!(
            switching.budget_bytes(),
            budget_bytes(gib_f(74.2), M3_TOTAL, M3_CEILING)
        );
        let fits = judge(need, switching);
        assert_ne!(fits.verdict, Verdict::Block, "{}", fits.message);
        assert!(fits.needs_switch(), "it fits only once the 27B stops");
        assert!(
            fits.message
                .contains("available 44.2 GB + 30.0 GB the switch frees"),
            "{}",
            fits.message
        );

        let small = judge(weights_only(gib_f(10.0)), switching);
        assert!(
            !small.needs_switch(),
            "a need the Mac holds beside the running engine does not wait on the switch"
        );

        let over_ceiling = NodeMemoryFacts {
            available_bytes: gib_f(60.0),
            freed_by_switch_bytes: gib_f(36.0),
            ..held
        };
        assert_eq!(over_ceiling.budget_bytes(), M3_CEILING);
    }

    /// Q-106: the ceiling is the Mac's, not one process's. On the M3 Ultra with another engine
    /// holding 40 GiB and 50 GiB available (its RAM mostly engines and file cache), the RAM side
    /// alone admits a 38.5 GiB load (budget 41.1); the ceiling that engine leaves (77.8 − 40) does
    /// not. With no other engine the same Mac admits it.
    #[test]
    fn other_engines_are_charged_against_the_gpu_ceiling() {
        let alone = NodeMemoryFacts {
            available_bytes: gib_f(50.0),
            total_bytes: M3_TOTAL,
            ceiling_bytes: M3_CEILING,
            other_engines_bytes: 0,
            freed_by_switch_bytes: 0,
        };
        let need = weights_only(gib_f(38.5));
        assert_ne!(judge(need.clone(), alone).verdict, Verdict::Block);
        let beside = NodeMemoryFacts {
            other_engines_bytes: gib_f(40.0),
            ..alone
        };
        assert_eq!(beside.budget_bytes(), M3_CEILING - gib_f(40.0));
        let v = judge(need, beside);
        assert_eq!(v.verdict, Verdict::Block, "{}", v.message);
        assert!(
            v.message
                .contains("GPU ceiling 77.8 GB − 40.0 GB other engines hold"),
            "{}",
            v.message
        );
        assert!(
            !v.could_ever_fit(),
            "no reclaim frees what another engine holds, so Make room is not offered"
        );
    }
}
