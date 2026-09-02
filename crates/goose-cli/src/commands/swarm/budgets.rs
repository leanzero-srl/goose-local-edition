//! VA-126: every character budget that bounds what a model is SHOWN, derived from the fleet's
//! PROBED context window instead of typed for one fleet. Mihai, 2026-09-02: "we need to avoid
//! hard coded bits because this is an agent and that makes it useless outside of the scope of
//! what we are doing now; the benchmark is the cause not the goal."
//!
//! Every reference value below was hand-sized against a 27B model loaded at
//! `loaded_context_length` = 262,144 (r6h: mihai/workhorse 262,144, gabee 180,224 — VA-112's
//! probe, `openai.rs::probe_context_window`). The derivation keeps each value as the RATIO
//! `reference_chars / REFERENCE_WINDOW_TOKENS` — dimensionless, no chars-per-token assumed — so a
//! 1M-window model is briefed proportionally and a 262,144 window reproduces today's bytes.
//!
//! ONE budget set per run, from the fleet's MAXIMUM window (Mihai 2026-09-02 20:4x: "ensure
//! that our work here does not affect or has least affects on impairing our solution"). A
//! per-host derivation would shrink gabee's briefs by 180,224 / 262,144 = 0.6875 — an unmeasured
//! change to the golden path — so on this fleet the maximum is 262,144 and every number is
//! byte-identical on all three hosts (`this_fleets_windows_reproduce_todays_numbers_on_every_host`
//! is the regression proof). `// later: per-host` — a measured step, not this one.
//!
//! Sibling module under the incremental-split law (development_gates::
//! swarm_rs_line_count_only_decreases); the dispatcher resolves the set once at construction
//! (`GooseAgentDispatcher::resolve_fleet_budgets`) and every consumer reads the plain field.
//!
//! What is NOT derived here, and why (each read whole under gate 8 before the call):
//! * `ANSWER_WINDOW_CHARS` (swarm.rs, 24,000): the digest's rolling answer-channel window — an
//!   I/O bound on a file the panel and tick.py read, never shown to a model.
//! * `OMNI_JUDGE_MIN_CHARS` / `OMNI_JUDGE_GROWTH_CHARS` (ladder.rs): production floors that decide
//!   WHEN the judge looks — supervision cadence on evidence, not a budget of shown text.
//! * `THIN_BRIEF_MIN_CHARS` (briefs.rs, 240): the specificity gate's warning floor on a brief's
//!   length — a property of the brief, not of the reader's window.
//! * `SPEC_ORIENTATION_MIN_CHARS` (orientation.rs, 12,000): whether the SPEC has enough document
//!   structure to index — a property of the input; a 1M model still gets the index on a 54k spec.
//! * the digest's `last_thinking` view (`tail_chars(_, LOOK_TAIL_CHARS)`, swarm.rs): a VIEW the
//!   panel reads; the two model-facing tails (the judge's look tail and the re-stream seed's
//!   carried tail) are derived — `look_tail_chars`.
//! * the split rule's 2.0× median and the recurrence meter's 48-char shingle WIDTH: different
//!   classes (a statistic of the plan; the unit of verbatim repetition). The meter's REACH — how
//!   far back it can see a period — IS a window budget and is derived here (`recurrence_reach`).

use super::dep_sources::{DEP_SOURCES_BUDGET_CHARS, DEP_SOURCE_FILE_CHARS};
use super::judge_context::{OWNED_EXCERPT_PER_FILE, OWNED_EXCERPT_TOTAL};
use super::ladder::LOOK_TAIL_CHARS;
use super::ledger_block::REPAIR_HISTORY_CHARS;
use super::user_notes::USER_NOTES_BUDGET_CHARS;
use super::vendor_probe::VENDOR_PROBE_BODY_CHARS;

/// The window every reference value was measured on: r6h's fleet, qwen3.8-27b at LM Studio's
/// `loaded_context_length` = 262,144. Not a cap and not a model property — the denominator of a
/// dimensionless ratio, so a window of exactly this size is the fixed point of `for_window`.
pub(super) const REFERENCE_WINDOW_TOKENS: usize = 262_144;

/// A spec-named document's fetched body (`doc_fetched`, per URL). 24,000 was `DOC_MAX_BYTES` in
/// `run_swarm`'s doc-fetch block — chars, despite the old name — on the 262,144 window.
pub(super) const DOC_FETCH_CHARS: usize = 24_000;

/// The sink's ledger block (`render_ledger_block_measured`'s budget in
/// `sink_semantic_description`). 7,000 was the literal at that one live site on the 262,144 window.
pub(super) const LEDGER_BLOCK_CHARS: usize = 7_000;

/// The recurrence meter's shingle reach (`desk::RecurrenceMeter`): how many 48-char shingle
/// fingerprints it holds, i.e. how far back a repetition period stays visible. 65,536 was
/// `RECURRENCE_REACH` in desk.rs — ~65k characters of memory at ~3 MB per live call, 16x the
/// longest repetition period measured (~4,000 chars) and 27x the tail window that was blind to
/// it — on the 262,144 window: exactly one quarter of it, so a model that can hold four times the
/// reasoning is watched four times as far back (VA-137).
pub(super) const RECURRENCE_REACH_SHINGLES: usize = 65_536;

/// The budgets a run shows its models under, all scaled from one window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ShownBudgets {
    /// The window the set was derived FROM — the fleet maximum (`budgets_resolved` names the
    /// model it came from and every probed window beside it).
    pub(super) window_tokens: usize,
    /// `DOC_FETCH_CHARS` scaled: a spec-named document's fetched body.
    pub(super) doc_fetch_chars: usize,
    /// `USER_NOTES_BUDGET_CHARS` scaled: the queued user-notes block on every dispatch.
    pub(super) user_notes_chars: usize,
    /// `DEP_SOURCES_BUDGET_CHARS` scaled: the worker's whole "API of" dependency block.
    pub(super) dep_sources_chars: usize,
    /// `DEP_SOURCE_FILE_CHARS` scaled: one dependency source inside that block.
    pub(super) dep_source_file_chars: usize,
    /// `VENDOR_PROBE_BODY_CHARS` scaled: one vendor endpoint's sample body.
    pub(super) vendor_body_chars: usize,
    /// `LEDGER_BLOCK_CHARS` scaled: the sink's ledger block.
    pub(super) ledger_block_chars: usize,
    /// `REPAIR_HISTORY_CHARS` scaled: a repair shard's prior-rounds splice.
    pub(super) repair_history_chars: usize,
    /// `OWNED_EXCERPT_TOTAL` scaled: the judge's owned-file excerpts, all files.
    pub(super) owned_excerpt_total_chars: usize,
    /// `OWNED_EXCERPT_PER_FILE` scaled: the judge's owned-file excerpt, one file.
    pub(super) owned_excerpt_per_file_chars: usize,
    /// `LOOK_TAIL_CHARS` scaled: the judge's look tail and the re-stream seed's carried tail.
    pub(super) look_tail_chars: usize,
    /// `RECURRENCE_REACH_SHINGLES` scaled: the recurrence meter's shingle reach, live and replayed.
    pub(super) recurrence_reach: usize,
}

/// `reference_chars × window / REFERENCE_WINDOW_TOKENS`, floored. u128 so no product overflows;
/// exact (no rounding at all) when `window == REFERENCE_WINDOW_TOKENS`.
fn scaled(reference_chars: usize, window_tokens: usize) -> usize {
    ((reference_chars as u128 * window_tokens as u128) / REFERENCE_WINDOW_TOKENS as u128) as usize
}

impl ShownBudgets {
    /// The set for one window. Pure; the fleet's window comes from `fleet_window`.
    pub(super) fn for_window(window_tokens: usize) -> Self {
        Self {
            window_tokens,
            doc_fetch_chars: scaled(DOC_FETCH_CHARS, window_tokens),
            user_notes_chars: scaled(USER_NOTES_BUDGET_CHARS, window_tokens),
            dep_sources_chars: scaled(DEP_SOURCES_BUDGET_CHARS, window_tokens),
            dep_source_file_chars: scaled(DEP_SOURCE_FILE_CHARS, window_tokens),
            vendor_body_chars: scaled(VENDOR_PROBE_BODY_CHARS, window_tokens),
            ledger_block_chars: scaled(LEDGER_BLOCK_CHARS, window_tokens),
            repair_history_chars: scaled(REPAIR_HISTORY_CHARS, window_tokens),
            owned_excerpt_total_chars: scaled(OWNED_EXCERPT_TOTAL, window_tokens),
            owned_excerpt_per_file_chars: scaled(OWNED_EXCERPT_PER_FILE, window_tokens),
            look_tail_chars: scaled(LOOK_TAIL_CHARS, window_tokens),
            recurrence_reach: scaled(RECURRENCE_REACH_SHINGLES, window_tokens),
        }
    }

    /// The set on the reference window — the fixed point of `for_window`, every field its
    /// reference constant. The dispatcher's pre-resolve placeholder and the tests use it.
    pub(super) fn reference() -> Self {
        Self::for_window(REFERENCE_WINDOW_TOKENS)
    }

    /// The set as JSON for the `budgets_resolved` event — every field, so tick.py and a reader
    /// of the archive see the numbers the run briefed under without re-deriving them.
    pub(super) fn to_json(self) -> serde_json::Value {
        serde_json::json!({
            "window_tokens": self.window_tokens,
            "doc_fetch_chars": self.doc_fetch_chars,
            "user_notes_chars": self.user_notes_chars,
            "dep_sources_chars": self.dep_sources_chars,
            "dep_source_file_chars": self.dep_source_file_chars,
            "vendor_body_chars": self.vendor_body_chars,
            "ledger_block_chars": self.ledger_block_chars,
            "repair_history_chars": self.repair_history_chars,
            "owned_excerpt_total_chars": self.owned_excerpt_total_chars,
            "owned_excerpt_per_file_chars": self.owned_excerpt_per_file_chars,
            "look_tail_chars": self.look_tail_chars,
            "recurrence_reach": self.recurrence_reach,
        })
    }
}

/// The run's ONE window from the fleet's probed windows: the MAXIMUM (see the module doc — the
/// minimum would shrink every brief on a mixed fleet, the golden path this must not move).
/// `None` only for an empty fleet, which the caller reports by name rather than defaulting.
pub(super) fn fleet_window(windows: &[(String, usize)]) -> Option<(String, usize)> {
    let mut best: Option<(String, usize)> = None;
    for (model, w) in windows {
        match &best {
            Some((_, bw)) if *w <= *bw => {}
            _ => best = Some((model.clone(), *w)),
        }
    }
    best
}

impl super::GooseAgentDispatcher {
    /// Resolve the run's shown budgets from the fleet's probed windows — called ONCE from the
    /// constructor, before the dispatcher is handed out, so no consumer can read an unresolved
    /// set. Every model's window is the SAME derivation the compaction guard and the lane's
    /// digest use (`effective_context_limit`: the provider's probe, else the model config's
    /// limit, under the optional local cap). Gate 1: a model whose provider could not probe a
    /// window runs on the model config's default, and that is SAID — `budgets_window_unprobed`
    /// per model, once per run — never quietly folded into the maximum. The set itself rides
    /// `budgets_resolved` with every window beside it.
    pub(super) async fn resolve_fleet_budgets(
        &self,
        fleet_models: &[String],
    ) -> anyhow::Result<ShownBudgets> {
        let mut windows: Vec<(String, usize)> = Vec::new();
        let mut unprobed: Vec<String> = Vec::new();
        for model_id in fleet_models {
            if windows.iter().any(|(m, _)| m == model_id) {
                continue;
            }
            let provider = self.provider_for(model_id).await?;
            let model_config = goose::model_config::model_config_from_user_config(
                super::cloud_registry_name(self.provider_name(model_id)),
                model_id,
            )?;
            let window =
                goose::context_mgmt::effective_context_limit(provider.as_ref(), &model_config)
                    .await;
            let fallback_limit = model_config.context_limit();
            // The provider returns exactly the model config's default when its probe missed or
            // timed out (`openai.rs::get_context_limit`, which logs `context_window_probe_missed`
            // at warn), and nothing else in the path returns that number for a LOCAL model with
            // no configured limit — a cloud model's limit is its config table's, not a probe.
            // A local model genuinely loaded at the default is indistinguishable here and the
            // event says so; that ambiguity is stated, not hidden.
            if model_config.context_limit.is_none()
                && !self.cloud_models.contains_key(model_id)
                && window == fallback_limit
            {
                self.events.write_value(serde_json::json!({
                    "event": "budgets_window_unprobed",
                    "model": model_id,
                    "fallback_limit": fallback_limit,
                    "detail": "the provider's context-window probe (meta.n_ctx / \
                               loaded_context_length) returned nothing for this model, so its \
                               window is the model config's default; the shown budgets are \
                               derived from the fleet MAXIMUM, so this model lowers nothing \
                               unless it is the only one — see budgets_resolved.windows",
                }));
                unprobed.push(model_id.clone());
            }
            windows.push((model_id.clone(), window));
        }
        let Some((from_model, window_tokens)) = fleet_window(&windows) else {
            anyhow::bail!("shown budgets: no fleet model to derive a context window from");
        };
        let budgets = ShownBudgets::for_window(window_tokens);
        self.events.write_value(serde_json::json!({
            "event": "budgets_resolved",
            "scope": "fleet_max",
            "window_tokens": window_tokens,
            "from_model": from_model,
            "reference_window_tokens": REFERENCE_WINDOW_TOKENS,
            "windows": windows
                .iter()
                .map(|(m, w)| (m.clone(), serde_json::Value::from(*w)))
                .collect::<serde_json::Map<String, serde_json::Value>>(),
            "unprobed": unprobed,
            "budgets": budgets.to_json(),
        }));
        Ok(budgets)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// THE REGRESSION PROOF. This fleet's three probed windows (r6h: mihai 262,144, workhorse
    /// 262,144, gabee 180,224) resolve to the maximum, and at that window every derived budget is
    /// the exact constant the engine shipped with — byte-identical briefs on all three hosts.
    #[test]
    fn this_fleets_windows_reproduce_todays_numbers_on_every_host() {
        let fleet = vec![
            ("mihai-qwen3.8-27b".to_string(), 262_144),
            ("workhorse-qwen3.8-27b".to_string(), 262_144),
            ("gabee-qwen3.8-27b".to_string(), 180_224),
        ];
        let (from, window) = fleet_window(&fleet).expect("three windows");
        assert_eq!(window, 262_144);
        assert_eq!(
            from, "mihai-qwen3.8-27b",
            "the first maximum names the source"
        );
        let b = ShownBudgets::for_window(window);
        assert_eq!(b.window_tokens, 262_144);
        assert_eq!(b.doc_fetch_chars, 24_000);
        assert_eq!(b.user_notes_chars, 1_500);
        assert_eq!(b.dep_sources_chars, 14_000);
        assert_eq!(b.dep_source_file_chars, 3_500);
        assert_eq!(b.vendor_body_chars, 6_000);
        assert_eq!(b.ledger_block_chars, 7_000);
        assert_eq!(b.repair_history_chars, 3_500);
        assert_eq!(b.owned_excerpt_total_chars, 2_400);
        assert_eq!(b.owned_excerpt_per_file_chars, 1_200);
        assert_eq!(b.look_tail_chars, 2_000);
        assert_eq!(b.recurrence_reach, 65_536);
        // The same numbers whichever host's window is listed first or last.
        let mut reversed = fleet.clone();
        reversed.reverse();
        assert_eq!(fleet_window(&reversed).map(|(_, w)| w), Some(262_144));
    }

    /// A 1M-window model (4× the reference) is briefed at 4× every budget — the ratio, not the
    /// number, is what the engine carries.
    #[test]
    fn a_1m_window_is_briefed_proportionally() {
        let b = ShownBudgets::for_window(1_048_576);
        assert_eq!(b.doc_fetch_chars, 96_000);
        assert_eq!(b.user_notes_chars, 6_000);
        assert_eq!(b.dep_sources_chars, 56_000);
        assert_eq!(b.dep_source_file_chars, 14_000);
        assert_eq!(b.vendor_body_chars, 24_000);
        assert_eq!(b.ledger_block_chars, 28_000);
        assert_eq!(b.repair_history_chars, 14_000);
        assert_eq!(b.owned_excerpt_total_chars, 9_600);
        assert_eq!(b.owned_excerpt_per_file_chars, 4_800);
        assert_eq!(b.look_tail_chars, 8_000);
        assert_eq!(b.recurrence_reach, 262_144);
    }

    /// What gabee ALONE would have received under a per-host derivation (the later, measured
    /// step): 0.6875× — recorded so the trace's numbers stay reproducible, not to ship them.
    #[test]
    fn gabees_window_alone_would_scale_by_eleven_sixteenths() {
        let b = ShownBudgets::for_window(180_224);
        assert_eq!(b.doc_fetch_chars, 16_500);
        assert_eq!(b.dep_sources_chars, 9_625);
        assert_eq!(b.dep_source_file_chars, 2_406);
        assert_eq!(b.vendor_body_chars, 4_125);
        assert_eq!(b.look_tail_chars, 1_375);
        assert_eq!(b.recurrence_reach, 45_056);
    }

    /// The old default the engine held before VA-112's probe (128,000, `DEFAULT_CONTEXT_LIMIT`):
    /// a fleet whose every probe misses would brief at ~0.49× — which is why an unprobed window
    /// is an event, never a quiet fold into the maximum.
    #[test]
    fn an_unprobed_default_window_would_halve_the_briefs() {
        let b = ShownBudgets::for_window(128_000);
        assert_eq!(b.dep_sources_chars, 6_835);
        assert_eq!(b.doc_fetch_chars, 11_718);
    }

    #[test]
    fn an_empty_fleet_has_no_window() {
        assert_eq!(fleet_window(&[]), None);
    }

    #[test]
    fn the_event_json_carries_every_field() {
        let j = ShownBudgets::for_window(REFERENCE_WINDOW_TOKENS).to_json();
        assert_eq!(j["window_tokens"], 262_144);
        assert_eq!(j["look_tail_chars"], 2_000);
        assert_eq!(j.as_object().map(|o| o.len()), Some(12));
    }

    /// VA-137: the recurrence meter's reach is one quarter of the window — 65,536 shingles on
    /// this fleet to the byte, 262,144 on a 1M-window model; verdict-identical on r6h (max rate
    /// 0.1465 < 0.25 at every reach), the `desk_look.detectors.span/recur_rate` fields differ
    /// only on a smaller-window host.
    #[test]
    fn recurrence_reach_is_one_quarter_of_the_window() {
        assert_eq!(ShownBudgets::for_window(262_144).recurrence_reach, 65_536);
        assert_eq!(
            ShownBudgets::for_window(1_048_576).recurrence_reach,
            262_144
        );
        assert_eq!(
            ShownBudgets::reference().recurrence_reach,
            RECURRENCE_REACH_SHINGLES
        );
    }
}
