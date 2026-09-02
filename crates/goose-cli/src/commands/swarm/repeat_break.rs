//! #136 THE REPEAT BREAKER — the consecutive-identical (call, result) detector the worker loop
//! runs on every tool result: its identity key, its count threshold and, since VA-137, the floor
//! that arms it. It SUMMONS the judge with what it measured; it kills nothing (the judge nudges,
//! never kills).
//!
//! VA-137 (gate 5: no seconds value decides model work). The floor used to be
//! `REPEAT_BREAK_MIN_SECS = 60` of wall-clock. What that clock defended was ONE false positive: a
//! deliberate poll loop (re-curl a booting server, byte-identical "connection refused") legitimately
//! hits six quick identical results in a few seconds, while the measured pathology (val-lean-02
//! `verify::cli-module`, 31 identical `cat deals/__main__.py`) spent ~18 s of REASONING per call.
//! What separates the two is not the clock but what the lane PRODUCES between the repeats: the
//! poll loop emits next to nothing between its polls; the pathology reasons at length and re-runs
//! the same command. So the breaker may summon only once the lane has produced, since its first
//! repeat, at least its OWN median chars-between-calls — the rhythm of the calls that preceded the
//! run, never a typed byte count — and more than nothing.
//!
//! Sibling module under the incremental-split law: `REPEAT_BREAK_N`, `repeat_call_hash` and its
//! test moved here verbatim from swarm.rs; `ProducedRhythm` is the floor's bookkeeping, fed by
//! `run_agent_in_inner` with `thinking_total + produced_answer_chars` at every tool result.

/// #136: consecutive identical tool calls that trip the repeat breaker. MEASURED across 268 shell-bearing
/// tasks in 44 real runs: the longest legitimate consecutive identical run was 4 (`swift build` re-runs); the
/// one pathology hit 31. 6 leaves a 2-call margin above every observed legitimate run.
pub(super) const REPEAT_BREAK_N: usize = 6;

// Compile-time guards on the threshold. MEASURED over 268 shell-bearing tasks in 44 real runs: the longest
// LEGITIMATE consecutive identical run was 4 (a repeated `swift build`); the one pathology was 31. A runtime
// assert on a const proves nothing (clippy::assertions_on_constants rightly rejects it) — these fail the
// BUILD if the threshold is ever retuned into a range that would cut legitimate work.
const _: () = assert!(REPEAT_BREAK_N > 4);
const _: () = assert!(REPEAT_BREAK_N < 31);

/// #136: identity of one tool call's OUTCOME, for the repeat breaker. A repeat with a DIFFERENT result is
/// progress (a re-run `pytest` after an edit returns different output) and must not count, so the result is
/// part of the key — as is `ok`, so a call that flips success/failure breaks the run.
///
/// HONESTY about precision: neither term is byte-exact. `summary` is `summarize_tool_call`'s ~200-char
/// display string and `result` is a 4000-char tail clip, so two genuinely different calls CAN collide. That is
/// acceptable only because a collision alone is harmless: tripping additionally requires REPEAT_BREAK_N
/// consecutive collisions AND the produced-chars floor (`ProducedRhythm::floor`). Never describe this key as
/// exact.
pub(super) fn repeat_call_hash(name: &str, summary: &str, ok: bool, result: &str) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    name.hash(&mut h);
    0u8.hash(&mut h);
    summary.hash(&mut h);
    0u8.hash(&mut h);
    ok.hash(&mut h);
    0u8.hash(&mut h);
    result.hash(&mut h);
    h.finish()
}

/// The lane's own rhythm: the median chars produced (thinking + answer text) between consecutive
/// tool calls, over the calls that PRECEDED the repeat run — the run's own gaps are excluded
/// because a poll loop would drag its own median to zero and arm itself. `None` until three such
/// calls exist: with fewer the "median" is an endpoint, not a middle, and the floor stays inert —
/// a lane whose first calls are the repeats has no rhythm to compare against, and the judge still
/// reaches it on the recurrence meter and the forming-stall trigger.
pub(super) fn median_produced_per_call(history: &[usize]) -> Option<usize> {
    if history.len() < 3 {
        return None;
    }
    let mut sorted = history.to_vec();
    sorted.sort_unstable();
    Some(sorted[sorted.len() / 2])
}

/// The floor itself: met once the lane has produced, since its first repeat, at least its own
/// median per call — and more than nothing, because identical results with NOTHING produced
/// between them is the poll loop by definition, whatever the median says.
pub(super) fn repeat_floor_met(median_per_call: usize, produced_since_first_repeat: usize) -> bool {
    produced_since_first_repeat > 0 && produced_since_first_repeat >= median_per_call
}

/// The floor's bookkeeping, fed at every tool result with the lane's monotonic produced total
/// (`thinking_total + produced_answer_chars` in the worker loop). Restreams do not reset it —
/// like the repeat counter it rides beside, its samples are engine facts that legitimately span
/// attempts of one call.
#[derive(Debug, Default)]
pub(super) struct ProducedRhythm {
    per_call: Vec<usize>,
    at_last_call: usize,
    /// How many `per_call` samples precede the current run (the run's first call included — the
    /// chars before it are pre-run reasoning).
    history_len: usize,
    at_run_start: usize,
}

/// What the floor measured at one check — the numbers the judge's evidence text carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RepeatFloor {
    /// `None`: inert — fewer than three calls preceded the run.
    pub(super) median_per_call: Option<usize>,
    pub(super) produced_since_first_repeat: usize,
}

impl RepeatFloor {
    /// The median the lane cleared, when it did.
    pub(super) fn met(&self) -> Option<usize> {
        self.median_per_call
            .filter(|m| repeat_floor_met(*m, self.produced_since_first_repeat))
    }
}

impl ProducedRhythm {
    /// A tool result landed with the lane at `produced_total` chars.
    pub(super) fn note_call(&mut self, produced_total: usize) {
        self.per_call
            .push(produced_total.saturating_sub(self.at_last_call));
        self.at_last_call = produced_total;
    }

    /// The call just noted opened a new run of identical results.
    pub(super) fn note_run_start(&mut self, produced_total: usize) {
        self.history_len = self.per_call.len();
        self.at_run_start = produced_total;
    }

    pub(super) fn floor(&self, produced_total: usize) -> RepeatFloor {
        RepeatFloor {
            median_per_call: median_produced_per_call(&self.per_call[..self.history_len]),
            produced_since_first_repeat: produced_total.saturating_sub(self.at_run_start),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeat_call_hash_keys_on_result_and_ok_not_just_the_command() {
        let base = repeat_call_hash(
            "shell",
            "cat deals/__main__.py",
            true,
            "from .cli import cli",
        );
        // The measured pathology: identical command, identical result -> identical key (counts as a repeat).
        assert_eq!(
            base,
            repeat_call_hash(
                "shell",
                "cat deals/__main__.py",
                true,
                "from .cli import cli"
            )
        );
        // THE false-positive defence: same command, DIFFERENT result = progress (a re-run pytest after an
        // edit). Must NOT be counted as a repeat.
        assert_ne!(
            base,
            repeat_call_hash(
                "shell",
                "cat deals/__main__.py",
                true,
                "from .cli import main"
            )
        );
        // A call that flips success/failure breaks the run even with identical text.
        assert_ne!(
            base,
            repeat_call_hash(
                "shell",
                "cat deals/__main__.py",
                false,
                "from .cli import cli"
            )
        );
        // Different command, different tool -> different key.
        assert_ne!(
            base,
            repeat_call_hash("shell", "cat deals/cli.py", true, "from .cli import cli")
        );
        assert_ne!(
            base,
            repeat_call_hash(
                "text_editor",
                "cat deals/__main__.py",
                true,
                "from .cli import cli"
            )
        );
        // TRUNCATION COLLISION (documented, deliberately accepted): `summary` is a ~200-char display string
        // and `result` a 4000-char tail clip, so two genuinely different calls whose visible prefixes match
        // DO collide. This is safe only because a collision alone cannot trip anything — REPEAT_BREAK_N
        // consecutive collisions AND the produced-chars floor are both required. Asserting the collision
        // keeps the key honest: it is an equality of the OBSERVABLE summary+result, not of the call.
        let a = repeat_call_hash("shell", &"x".repeat(200), true, &"y".repeat(4000));
        let b = repeat_call_hash("shell", &"x".repeat(200), true, &"y".repeat(4000));
        assert_eq!(a, b, "identical observables must hash equal — collisions are bounded by N + the floor, not by key precision");
        // Separator guard: ("ab","c") must not equal ("a","bc").
        assert_ne!(
            repeat_call_hash("shell", "ab", true, "c"),
            repeat_call_hash("shell", "a", true, "bc")
        );
    }

    /// Drives the bookkeeping the way `run_agent_in_inner` does: `note_call` at every tool
    /// result, `note_run_start` when the result opens a run, `floor` at the check.
    fn walk(history_gaps: &[usize], repeat_gaps: &[usize]) -> (ProducedRhythm, usize) {
        let mut rhythm = ProducedRhythm::default();
        let mut produced = 0usize;
        for gap in history_gaps {
            produced += gap;
            rhythm.note_call(produced);
        }
        for (i, gap) in repeat_gaps.iter().enumerate() {
            produced += gap;
            rhythm.note_call(produced);
            if i == 0 {
                rhythm.note_run_start(produced);
            }
        }
        (rhythm, produced)
    }

    /// VA-137: six identical results with NOTHING produced between them — the booting-server poll
    /// loop the 60 s clock used to defend — never arm the breaker, whatever the lane's history.
    #[test]
    fn six_identical_calls_producing_nothing_between_them_do_not_break() {
        // A lane that reasoned before the loop (median 1,800 chars between calls), then six polls
        // with the same result and nothing produced across them.
        let (rhythm, produced) = walk(&[2_400, 900, 1_800], &[1_500, 0, 0, 0, 0, 0]);
        let floor = rhythm.floor(produced);
        assert_eq!(floor.median_per_call, Some(1_800));
        assert_eq!(floor.produced_since_first_repeat, 0);
        assert_eq!(floor.met(), None);
        // Twenty chars of "still not up, retry" per poll is not the lane's rhythm either.
        let (rhythm, produced) = walk(&[2_400, 900, 1_800], &[1_500, 20, 20, 20, 20, 20]);
        assert_eq!(rhythm.floor(produced).met(), None);
        // A lane whose whole life is the poll loop: the run opened at its first call, so no
        // history precedes it and the floor is inert.
        let (rhythm, produced) = walk(&[], &[0, 0, 0, 0, 0, 0]);
        let floor = rhythm.floor(produced);
        assert_eq!(floor.median_per_call, None);
        assert_eq!(floor.met(), None);
        // Even three preceding calls that produced nothing do not make nothing-since enough.
        let (rhythm, produced) = walk(&[0, 0, 0], &[0, 0, 0, 0, 0, 0]);
        assert_eq!(rhythm.floor(produced).median_per_call, Some(0));
        assert_eq!(rhythm.floor(produced).met(), None);
    }

    /// VA-137: the measured pathology's shape — reasoning at length between identical re-runs.
    #[test]
    fn six_identical_calls_after_the_lane_produced_its_median_do_break() {
        let (rhythm, produced) = walk(&[2_400, 900, 1_800], &[1_500, 700, 700, 700, 700, 700]);
        let floor = rhythm.floor(produced);
        assert_eq!(floor.median_per_call, Some(1_800));
        assert_eq!(floor.produced_since_first_repeat, 3_500);
        assert_eq!(floor.met(), Some(1_800));
        // The run's own gaps never enter the median: with them the six 700s would have pulled
        // it to 700 and a poll loop of 700-char gaps would arm itself the same way.
        let (rhythm, produced) = walk(&[2_400, 900, 1_800], &[1_500, 300, 300, 300, 300, 300]);
        assert_eq!(rhythm.floor(produced).median_per_call, Some(1_800));
        // 1,500 since the first repeat: under the lane's 1,800 — not yet.
        assert_eq!(rhythm.floor(produced).met(), None);
        // Reasoning after the sixth result counts too: the check runs on every stream event.
        assert_eq!(rhythm.floor(produced + 300).met(), Some(1_800));
    }

    #[test]
    fn the_median_is_the_lanes_own_never_a_typed_count() {
        assert_eq!(median_produced_per_call(&[]), None);
        assert_eq!(median_produced_per_call(&[5, 1]), None);
        assert_eq!(median_produced_per_call(&[5, 1, 100]), Some(5));
        assert_eq!(median_produced_per_call(&[1, 2, 3, 4]), Some(3));
        assert!(repeat_floor_met(5, 5));
        assert!(!repeat_floor_met(5, 4));
        assert!(!repeat_floor_met(0, 0), "nothing produced is never enough");
    }
}
