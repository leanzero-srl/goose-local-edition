//! VA-107: the ONE measured context line a swarm lane reads every turn.
//!
//! r6h (2026-09-02), shard lane `viz-engine-data-stream-render-pick` on gabee. The engine's per-turn
//! `<turn-context>` block (MOIM, `agents/moim.rs`) carried `<compaction>~11k tokens
//! remaining</compaction>` — a figure relative to the COMPACTION POINT (context_limit × 0.8), computed
//! from the 128,000 `DEFAULT_CONTEXT_LIMIT` because the n_ctx probe reads llama.cpp's `meta.n_ctx`
//! while LM Studio serves `loaded_context_length` (180,224 for gabee) on another endpoint. The lane
//! read it as its context: "I'm running low on context (about 11k left)… Context is nearly exhausted
//! (~9k)… write compact but complete code… (about 3k left)… Context is about 2k — keep it concise."
//! 11k / 9k / 3k / 2k are (102,400 − total) / 1000 at the lane's measured totals 90,821 / 92,714 /
//! 99,331 / 99,647 — to the digit — and the pieces it shipped were compressed to fit a budget the
//! engine had misreported. This line replaces that figure for swarm lanes with the two numbers the
//! compaction guard actually compares (`session.usage.total_tokens`, `effective_context_limit`),
//! rendered as a fact and nothing else: no sentence about what to do with it.

/// What the LAST provider call in this reply said about its usage — decides which arm renders.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LastCallUsage {
    /// No provider call has completed in this reply yet (the first turn).
    NoCallYet,
    /// The last call's usage carried a total: the session figure is current.
    Reported,
    /// The last call returned no usage (or no total): the session figure is STALE, so the line
    /// says so instead of showing the previous call's number as if it were current.
    NotReported,
    /// The last call was CUT by the engine at a chunk boundary for a steer (a judge nudge, a
    /// research relay): no provider call completed, so the session figure is that of the last
    /// COMPLETED call and the cut partial is unmeasured. VA-116: r6i's opener (13:09:29) and r6j's
    /// api lane (turn 2, at the relay) rendered the not-reported arm here — "usage not reported by
    /// the provider" about a call the engine itself had cut. A fact about the engine, named as one.
    CutForSteer,
}

/// The not-reported arm, verbatim. The swarm matches this prefix on the notice core yields with it
/// and emits ONE `usage_unavailable{task, attempt, turn}` event per lane.
pub const USAGE_UNAVAILABLE_LINE: &str =
    "context: usage not reported by the provider on the last call";

/// The steer-turn arm's prefix, verbatim. The swarm matches it on the notice core yields with it
/// and emits `context_line_skipped{task, attempt, turn, reason: "steer_turn"}` — never
/// `usage_unavailable`, whose once-per-lane latch a steer turn would otherwise spend.
pub const STEER_CUT_LINE: &str =
    "context: the last call was cut at a chunk boundary for a steer, its usage unmeasured";

/// Render the line. `total_tokens` is the session's usage total (what the compaction guard reads);
/// `window` is `effective_context_limit` (None only when the session carries no model config).
pub fn context_line(
    last_call: LastCallUsage,
    total_tokens: Option<i64>,
    window: Option<usize>,
) -> String {
    match (last_call, total_tokens, window) {
        (LastCallUsage::NoCallYet, _, Some(w)) => format!(
            "context: {} tokens window; no call completed yet",
            with_commas(w as i64)
        ),
        (LastCallUsage::NoCallYet, _, None) => {
            "context: no call completed yet; window unknown to the engine".to_string()
        }
        (LastCallUsage::CutForSteer, Some(t), Some(w)) if w > 0 => {
            let pct = ((t as f64 / w as f64) * 100.0).round() as i64;
            format!(
                "{STEER_CUT_LINE}; {} of {} tokens used ({pct}%) at the last completed call",
                with_commas(t),
                with_commas(w as i64)
            )
        }
        (LastCallUsage::CutForSteer, _, _) => STEER_CUT_LINE.to_string(),
        (LastCallUsage::NotReported, _, _) | (LastCallUsage::Reported, None, _) => {
            USAGE_UNAVAILABLE_LINE.to_string()
        }
        (LastCallUsage::Reported, Some(t), Some(w)) if w > 0 => {
            let pct = ((t as f64 / w as f64) * 100.0).round() as i64;
            format!(
                "context: {} of {} tokens used ({pct}%)",
                with_commas(t),
                with_commas(w as i64)
            )
        }
        (LastCallUsage::Reported, Some(t), _) => format!(
            "context: {} tokens used; window unknown to the engine",
            with_commas(t)
        ),
    }
}

fn with_commas(n: i64) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3 + 1);
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    if n < 0 {
        out.insert(0, '-');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// r6h, tick 4: the brief's pinned reading — 74,112 of gabee's 180,224 loaded window.
    #[test]
    fn r6h_tick_4_reads_as_a_fact() {
        assert_eq!(
            context_line(LastCallUsage::Reported, Some(74_112), Some(180_224)),
            "context: 74,112 of 180,224 tokens used (41%)"
        );
    }

    /// r6h, the turn that wrote "about 11k left": total 90,821. Against the loaded window it is
    /// 50%; against the engine's actual belief (the 128,000 default) it is 71% — either line is a
    /// fact about the window it names, and neither contains an "11k".
    #[test]
    fn r6h_eleven_k_turn_reads_as_percent_of_the_named_window() {
        assert_eq!(
            context_line(LastCallUsage::Reported, Some(90_821), Some(180_224)),
            "context: 90,821 of 180,224 tokens used (50%)"
        );
        assert_eq!(
            context_line(LastCallUsage::Reported, Some(90_821), Some(128_000)),
            "context: 90,821 of 128,000 tokens used (71%)"
        );
    }

    #[test]
    fn a_silent_provider_is_named_not_substituted() {
        assert_eq!(
            context_line(LastCallUsage::NotReported, Some(90_821), Some(180_224)),
            USAGE_UNAVAILABLE_LINE
        );
        assert_eq!(
            context_line(LastCallUsage::Reported, None, Some(180_224)),
            USAGE_UNAVAILABLE_LINE
        );
    }

    /// VA-116 — r6i's opener, 13:09:29: the judge's steer cut the stream mid-generation; the next
    /// turn is the engine's doing, not the provider's, and the line says so, keeping the last
    /// completed call's figure as exactly that.
    #[test]
    fn a_steer_cut_turn_names_the_engine_not_the_provider() {
        let line = context_line(LastCallUsage::CutForSteer, Some(74_112), Some(180_224));
        assert_eq!(
            line,
            format!(
                "{STEER_CUT_LINE}; 74,112 of 180,224 tokens used (41%) at the last completed call"
            )
        );
        assert!(!line.contains("not reported by the provider"));
        assert_eq!(
            context_line(LastCallUsage::CutForSteer, None, None),
            STEER_CUT_LINE
        );
        // The two prefixes the swarm matches never shadow each other.
        assert!(!STEER_CUT_LINE.starts_with(USAGE_UNAVAILABLE_LINE));
        assert!(!USAGE_UNAVAILABLE_LINE.starts_with(STEER_CUT_LINE));
    }

    #[test]
    fn the_first_turn_states_the_window_and_that_nothing_ran() {
        assert_eq!(
            context_line(LastCallUsage::NoCallYet, None, Some(180_224)),
            "context: 180,224 tokens window; no call completed yet"
        );
        assert_eq!(
            context_line(LastCallUsage::NoCallYet, None, None),
            "context: no call completed yet; window unknown to the engine"
        );
        assert_eq!(
            context_line(LastCallUsage::Reported, Some(12), None),
            "context: 12 tokens used; window unknown to the engine"
        );
    }

    #[test]
    fn thousands_separators() {
        assert_eq!(with_commas(0), "0");
        assert_eq!(with_commas(999), "999");
        assert_eq!(with_commas(1_000), "1,000");
        assert_eq!(with_commas(1_234_567), "1,234,567");
        assert_eq!(with_commas(-1_234), "-1,234");
    }
}
