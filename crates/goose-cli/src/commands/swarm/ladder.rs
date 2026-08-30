//! The judge's nudge ladder: whether a call produced, which arm fired, how a nudge is
//! delivered, and what a fresh attempt is seeded with.
//!
//! Sibling module under the incremental-split law
//! (development_gates::swarm_rs_line_count_only_decreases). The five functions moved here from
//! swarm.rs pay for the r6a fix's wiring in the root; each keeps its own WHY. The one behavior
//! change rides in `nudge_delivery` (the `advancing` hold — see its doc); everything else moved
//! verbatim.

use super::OMNI_JUDGE_MIN_CHARS;

/// ONE definition of "this call produced something since the judge last looked", used by EVERY judge
/// gate that asks the question.
///
/// It was four hand-written copies of `produced_since_last_look >= OMNI_JUDGE_MIN_CHARS` — the burst-gap
/// accounting, the tail-similarity veto, the DRIFTING trip and the DRIFTING hold — and all four counted
/// REASONING CHARACTERS ONLY. A worker whose production is ACTIONS therefore reads as dead in every one
/// of them: `omni_quiet_secs` climbs monotonically while `omni_longest_gap_secs` stays 0, so
/// `judge_quiet_within_rhythm` is unreachable after the first look, and DRIFTING fires on the first look
/// with no corroboration. MEASURED: `apptest-bad-input`, a read-only observer that ran 26 shell commands
/// and barely narrated, collected EIGHTEEN nudges; over one 8h20m run 138 of 242 looks sat below the
/// one-char-per-second the judge is told means DEAD STREAM.
///
/// A tool call is production the same way a paragraph is. The loop case this widening gives up is still
/// covered: `recur.recurring()` — the same command, the same bytes back — arms the streak on its own and
/// never consults this predicate.
pub(super) fn produced_since_look(chars: usize, actions: usize) -> bool {
    chars >= OMNI_JUDGE_MIN_CHARS || actions > 0
}

/// WHICH ARM produced a nudge, for the artefact. Three distinct triggers reach one emit — a measured
/// repeat, a DRIFTING verdict, and a LOOPING streak that is itself armed either by measured recurrence
/// or by tail similarity — and the payload named none of them, so "which trigger produces useful nudges"
/// could not be answered from any file the engine writes.
///
/// Ordered most-factual first: a measured repeat is an engine fact, DRIFTING is a verdict about taste,
/// and the streak's two arms are told apart by whether the detector could see the recurrence itself.
pub(super) fn nudge_arm(
    repeat_measured: bool,
    drifting_now: bool,
    recurring: bool,
) -> &'static str {
    if repeat_measured {
        "measured_repeat"
    } else if drifting_now {
        "drifting"
    } else if recurring {
        "measured_recurrence"
    } else {
        "tail_similarity_streak"
    }
}

/// Tool calls made SINCE the supervisor last redirected this call.
///
/// The number four separate surfaces claimed was zero. The terminator fires on "no tool call since the
/// last nudge", never on "no tool call ever" — `judge_out_of_moves` fired on a call with 2 early tool
/// calls and every message about it said ZERO. `None` means no nudge has landed yet, in which case the
/// whole call counts.
pub(super) fn calls_since_nudge(at_last_nudge: Option<usize>, now: usize) -> usize {
    at_last_nudge.map_or(now, |n| now.saturating_sub(n))
}

/// The three ways a wanted nudge can land. `Steer` interrupts the stream at a chunk boundary and
/// KEEPS the partial; `Restream` drops the socket, wipes the conversation and seeds a fresh
/// attempt; `Hold` delivers NOTHING this look — the call is watched for one more look instead,
/// the same shape as the DRIFTING hold. Each variant carries the measured reason so the artefact
/// says why, not just what.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum NudgeDelivery {
    Steer(&'static str),
    Restream(&'static str),
    Hold(&'static str),
}

/// HOW a nudge is delivered. Escalation on measured non-obedience, never a counter.
///
/// A steer wakes the in-flight stream at its next chunk and KEEPS the partial (agent.rs:2140), so it
/// costs nothing and is always the first move. MEASURED on r1 (`review-build-app-meridian`: six steers,
/// zero actions after any of them, 42k -> 53k reasoning chars) and r2 (the OPEN call: three steers, zero
/// actions, 38k -> 47k): a looping reasoning-only call reads the note and re-enters its loop, because the
/// kept partial IS the anchor it loops on. The re-stream is the one delivery that removes the anchor, so
/// it is taken once the call has proven a steer changes nothing — a prior nudge with no tool call since
/// AND no fresh forward progress — or the judge has said outright that a fresh attempt would beat
/// continuing.
///
/// A RESTREAM MAY ONLY TAKE WHAT HAS STOPPED. "No tool call since the previous nudge" alone was the
/// whole ignored test, and in a reasoning phase (OPEN plans with zero tool calls by design) it is
/// UNFALSIFIABLE: the call cannot prove obedience except by ending. MEASURED on r6a (KILLED, OPEN 49m,
/// five nudge->restream cycles, zero tool calls ever): the 21:49:17 restream took an attempt that had
/// produced 7,598 fresh chars since the last look at recurrence 0.011 (32,054 chars abandoned); the
/// 22:03:27 restream took a CONVERGING attempt at 11,953 fresh chars, recurrence 0.016 (27,520
/// abandoned). Both streams were advancing on their own convergence and the ladder read them as
/// disobedient. So the ignored arm now requires the stream to have STOPPED: `advancing` — fresh,
/// non-recurring production since the last look — turns the would-be restream into a `Hold`. A
/// plateaued stream (under the production floor) or a recurring one (the meter's own measurement —
/// r5's 87k-char loop, the restream that SAVED that run) still walks the ladder exactly as before.
///
/// `prior_nudge_calls` is the tool-call count at the previous nudge (`None` before the first — which
/// since r6a is also every fresh attempt's state, because the restream seam resets the ladder), so
/// "obeyed" is measured on the call's own record, not inferred from what the judge hoped.
pub(super) fn nudge_delivery(
    pending_empty: bool,
    prior_nudge_calls: Option<usize>,
    calls_now: usize,
    verdict: &goose_swarm::Verdict,
    advancing: bool,
) -> NudgeDelivery {
    if !pending_empty {
        return NudgeDelivery::Restream("tool request in flight");
    }
    if *verdict == goose_swarm::Verdict::Restart {
        return NudgeDelivery::Restream("judge said restart");
    }
    match prior_nudge_calls {
        None => NudgeDelivery::Steer("first nudge"),
        Some(n) if calls_now > n => NudgeDelivery::Steer("acted since the previous nudge"),
        Some(_) if advancing => NudgeDelivery::Hold(
            "steer not acted on, but the stream is advancing: fresh non-recurring content since \
             the last look — held, not wiped",
        ),
        Some(_) => NudgeDelivery::Restream(
            "steer ignored: no action since the previous nudge and the stream has stopped \
             advancing",
        ),
    }
}

/// The re-stream's seed message: the original task, what the judge established, the direction —
/// and the VERBATIM TAIL of the abandoned stream's own reasoning.
///
/// MEASURED (r5, `judge_restream` 09:24:12Z): the directive said "Call the output tool NOW and
/// emit the slice table with the rows you already have" — but the rows lived in the abandoned
/// stream's tail (fully-formed slice entries, id/title/objective) and the seed carried only the
/// judge's 971-char ESTABLISHED summary of an 87,892-char stream. The fresh attempt re-derived
/// 37k+ chars over ~18 minutes before emitting anything. Same class as GEN-4: a directive must
/// never assert context the seed did not deliver. The caller cuts the tail with `tail_chars(_,
/// LOOK_TAIL_CHARS)` — the judge's own look-tail scale, now the shared constant — a mechanical
/// span on carried TEXT, not a bound on any model work.
///
/// An EMPTY tail is reachable (the tail mirrors the thinking channel, and a re-stream can fire on
/// a call that emitted no thinking at all — a degenerate whitespace answer, or a tool request left
/// in flight at nudge time), so per the fallback gate the absence is STATED, never silently
/// omitted.
pub(super) fn restream_seed(
    user_text: &str,
    established: &str,
    direction: &str,
    wants_structured_reply: bool,
    abandoned_tail: &str,
) -> String {
    format!(
        "{user_text}\n\n\
         SUPERVISOR NOTE — {}\
         Your previous attempt at this task was abandoned: an independent reviewer \
         read its reasoning and found it looping, restating the same ground without \
         acting. {}\n\
         Do this next: {direction}\n\
         {}",
        if established.is_empty() {
            String::new()
        } else {
            format!("you have already established: {established}\n")
        },
        if abandoned_tail.is_empty() {
            "That attempt emitted no reasoning text for the engine to carry forward. Do not \
             re-derive what is established above and do not explain a plan."
                .to_string()
        } else {
            format!(
                "Most of that reasoning is gone. THE TAIL OF YOUR OWN ABANDONED REASONING, \
                 VERBATIM — the content you had already formed:\n{abandoned_tail}\n\
                 END OF THE ABANDONED TAIL. Reuse what is formed there; do not re-derive it, \
                 do not re-derive what is established above, and do not explain a plan."
            )
        },
        if wants_structured_reply {
            "Your reply must be the required structured output now, built from \
             what is established above."
        } else {
            "Take the next concrete action on your owned files now."
        }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// THE INVERSION THAT COST EIGHTEEN NUDGES ON ONE READ-ONLY OBSERVER.
    ///
    /// Every judge gate asked "did it produce?" by counting REASONING CHARACTERS ALONE, so a worker
    /// whose production is ACTIONS read as dead in all of them: the burst-gap accounting never reset its
    /// quiet spell, `judge_quiet_within_rhythm` became unreachable after the first look, the
    /// tail-similarity streak stayed armed, and DRIFTING fired on the first look with no corroboration.
    #[test]
    fn a_call_that_acts_without_narrating_is_producing() {
        assert!(
            produced_since_look(3, 1),
            "26 shell commands and three characters of prose is a WORKING call, not a dead one"
        );
        assert!(
            produced_since_look(OMNI_JUDGE_MIN_CHARS, 0),
            "a narrating lane still clears the floor on reasoning alone"
        );
        assert!(
            !produced_since_look(OMNI_JUDGE_MIN_CHARS - 1, 0),
            "silence on BOTH channels is the only silence"
        );
        assert!(
            produced_since_look(0, 5),
            "actions alone are production — there is no reasoning floor to clear as well"
        );
    }

    /// The loop case the widening above gives up must still be caught, or the fix trades one blindness
    /// for another. A call repeating ONE tool call reads as producing now — and the measured-recurrence
    /// arm never consults this predicate, which is why it stays covered.
    #[test]
    fn the_measured_recurrence_arm_does_not_consult_production() {
        assert_eq!(
            nudge_arm(false, false, true),
            "measured_recurrence",
            "a measured recurrence arms on its own, whatever the production predicate says"
        );
        assert_eq!(nudge_arm(true, true, true), "measured_repeat");
        assert_eq!(nudge_arm(false, true, false), "drifting");
        assert_eq!(nudge_arm(false, false, false), "tail_similarity_streak");
    }

    /// `judge_out_of_moves` fired on a call with TWO early tool calls and every surface said ZERO.
    #[test]
    fn calls_since_a_nudge_is_not_calls_ever() {
        assert_eq!(
            calls_since_nudge(Some(2), 2),
            0,
            "two calls made BEFORE the first nudge are not calls made since it"
        );
        assert_eq!(calls_since_nudge(Some(2), 5), 3);
        assert_eq!(
            calls_since_nudge(None, 7),
            7,
            "with no nudge yet, the whole call counts"
        );
        assert_eq!(
            calls_since_nudge(Some(9), 4),
            0,
            "a re-stream can reset the record below the mark; that is zero, not a panic"
        );
    }

    /// r1 delivered six steers to a looping review call and r2 three to the OPEN call; every one landed,
    /// none was obeyed, and the re-stream was never reached. Delivery escalates on that evidence alone —
    /// but since r6a only once the stream has also STOPPED advancing (plateaued or recurring).
    #[test]
    fn nudge_delivery_escalates_on_measured_non_obedience() {
        use goose_swarm::Verdict;
        assert_eq!(
            nudge_delivery(true, None, 0, &Verdict::Looping, false),
            NudgeDelivery::Steer("first nudge"),
            "the first nudge on a call is a steer: it keeps the partial and costs nothing"
        );
        assert_eq!(
            nudge_delivery(true, Some(3), 3, &Verdict::Looping, false),
            NudgeDelivery::Restream(
                "steer ignored: no action since the previous nudge and the stream has stopped \
                 advancing"
            ),
            "a prior nudge with no tool call since AND a stopped stream is measured \
             non-obedience, so the anchor goes"
        );
        assert_eq!(
            nudge_delivery(true, Some(3), 5, &Verdict::Looping, false),
            NudgeDelivery::Steer("acted since the previous nudge"),
            "a call that obeyed its steer keeps getting steers"
        );
        assert_eq!(
            nudge_delivery(true, None, 4, &Verdict::Restart, true),
            NudgeDelivery::Restream("judge said restart"),
            "RESTART is the judge saying a fresh attempt beats continuing, even on the first \
             nudge and even mid-production — the judge is the reader and said so outright"
        );
        assert_eq!(
            nudge_delivery(false, None, 0, &Verdict::Looping, false),
            NudgeDelivery::Restream("tool request in flight"),
            "a tool request in flight is never steered around"
        );
    }

    /// r5, `judge_restream` 09:24:12Z: the directive told the fresh stream to emit "the rows you
    /// already have" while the seed carried only the judge's 971-char summary — the formed rows
    /// lived in the abandoned tail and the fresh stream re-derived 37k+ chars over ~18 minutes.
    /// The seed must DELIVER the tail it points at, and state the absence honestly when the
    /// abandoned stream produced no reasoning text at all.
    #[test]
    fn restream_seed_carries_the_abandoned_tail_verbatim_or_states_its_absence() {
        let tail = "| open-payments | Payments API | serve /v3/payments from the fixture |";
        let seed = restream_seed(
            "Build the slice table.",
            "slice names and file ownership",
            "Call the output tool NOW and emit the slice table with the rows you already have",
            true,
            tail,
        );
        assert!(
            seed.contains("THE TAIL OF YOUR OWN ABANDONED REASONING, VERBATIM"),
            "the carried span is labeled for what it is: {seed}"
        );
        assert!(
            seed.contains(tail),
            "the formed content the directive points at is IN the seed, verbatim"
        );
        assert!(
            seed.contains("you have already established: slice names and file ownership"),
            "the judge's summary still rides along"
        );
        assert!(
            seed.contains("Do this next: Call the output tool NOW"),
            "the direction survives"
        );
        assert!(
            seed.contains("required structured output"),
            "a structured-reply call is still told its deliverable"
        );

        let empty = restream_seed("Build it.", "", "act", false, "");
        assert!(
            empty.contains("emitted no reasoning text for the engine to carry forward"),
            "an empty abandoned tail is STATED, never silently omitted: {empty}"
        );
        assert!(
            !empty.contains("THE TAIL OF YOUR OWN ABANDONED REASONING"),
            "no empty labeled section pretending content exists"
        );
        assert!(
            empty.contains("Take the next concrete action on your owned files now."),
            "a non-structured call keeps its action closing"
        );
    }

    /// r6a look 1 on each fresh attempt (22:11:09: 1,005 chars, 0 actions; 22:20:21: 1,709 chars,
    /// 0 actions): the previous attempt's nudge memory survived the restream, so `prior_nudge_calls`
    /// was still `Some` and the very first look on a 45-second-old stream returned "steer ignored" and
    /// wiped it. The restream seam now resets the ladder (`tool_calls_at_last_nudge = None`), and with
    /// `None` there is NO input — advancing or not, whatever the verdict short of the judge's own
    /// RESTART — that can read a fresh attempt as having ignored a steer it was never given.
    #[test]
    fn a_fresh_attempts_first_look_cannot_be_read_as_ignoring_a_steer() {
        use goose_swarm::Verdict;
        for verdict in [Verdict::Drifting, Verdict::Looping, Verdict::Ok] {
            for advancing in [false, true] {
                for calls_now in [0usize, 3] {
                    assert_eq!(
                        nudge_delivery(true, None, calls_now, &verdict, advancing),
                        NudgeDelivery::Steer("first nudge"),
                        "a fresh attempt (prior_nudge_calls = None after the seam reset) earns \
                         its own ladder: first delivery is a steer, never the wipe \
                         (verdict {verdict:?}, advancing {advancing}, calls {calls_now})"
                    );
                }
            }
        }
    }

    /// r6a 21:49:17 (nudge 2): the attempt had produced 7,598 fresh chars since the last look at
    /// recurrence 0.011 — advancing on its own convergence — and the ladder restreamed it anyway,
    /// abandoning 32,054 chars. A producing, non-recurring stream after a nudge is HELD for another
    /// look, the same shape as the DRIFTING hold; the wipe needs the stream to stop.
    #[test]
    fn a_producing_non_recurring_call_after_a_nudge_is_held_not_wiped() {
        use goose_swarm::Verdict;
        let d = nudge_delivery(true, Some(0), 0, &Verdict::Drifting, true);
        assert!(
            matches!(d, NudgeDelivery::Hold(_)),
            "advancing after an unacted steer is a hold, not a restream: {d:?}"
        );
    }

    /// The guardrail: r5's attempt-1 loop (87,892 chars, measured recurrence) was REAL and the
    /// restream saved that run. A recurring stream is not `advancing` (the caller computes
    /// `advancing = produced && !recurring`), and a plateaued one is not producing — both still walk
    /// the ladder to the restream exactly as before r6a.
    #[test]
    fn a_recurring_or_plateaued_call_after_a_nudge_still_walks_the_ladder() {
        use goose_swarm::Verdict;
        let recurring = nudge_delivery(true, Some(3), 3, &Verdict::Looping, false);
        assert!(
            matches!(recurring, NudgeDelivery::Restream(_)),
            "recurring after an unacted steer still escalates to the restream: {recurring:?}"
        );
        let plateaued = nudge_delivery(true, Some(0), 0, &Verdict::Drifting, false);
        assert!(
            matches!(plateaued, NudgeDelivery::Restream(_)),
            "plateaued after an unacted steer still escalates to the restream: {plateaued:?}"
        );
    }
}
