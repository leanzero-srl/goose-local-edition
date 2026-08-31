//! The judge's nudge ladder: whether a call produced, which arm fired, how a nudge is
//! delivered, and what a fresh attempt is seeded with.
//!
//! Sibling module under the incremental-split law
//! (development_gates::swarm_rs_line_count_only_decreases). The five functions moved here from
//! swarm.rs pay for the r6a fix's wiring in the root; each keeps its own WHY. r6a's behavior
//! change rides in `nudge_delivery` (the `advancing` hold — see its doc); r6c's rides in
//! `write_progress`/`drift_streak_step` and the ladder's obedience arm (the deliverable decides,
//! never a tool-call count).

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

/// A FILE-OWNING lane advancing in the WRONG CHANNEL: it owes files, none exist on disk, and its
/// FORMED (answer/chat) channel has grown past the production floor since the last delivered nudge.
/// All three facts are progress facts — no clock, no counter cap.
///
/// MEASURED (r6c build, web-console at fan-out+51m): 0 owned files on disk, formed +70,600 chars —
/// the lane's actual CSS/HTML emitted as CHAT TEXT (".env-tag { font-size: 11px; ..."), while the
/// judge's exact-file directive ("write web/index.html NOW... no more contract deliberation before
/// the write") rode two steers (12:04, 12:07) that changed nothing. The `advancing` hold read that
/// pour as progress and shielded it, so the restream rung — the one delivery that carries the
/// directive INTO a fresh attempt's seed, the mechanism that broke the skeleton's equivalent stall
/// on the same run — never fired. Meanwhile ledgerd-core/notifierd delivered files steadily and
/// never resembled this shape.
///
/// WHY the trigger is derived rather than declared: the nudge direction is the judge's FREEFORM
/// `next_action` head (`head_to_sentence_end` at the omni seam) — the engine has no marker saying
/// "this is an exact-file directive". But every delivered nudge CARRIES the judge's direction, so
/// an unacted prior nudge (the ladder's `Some(false)` write-progress state) plus this conjunction
/// IS "directive pending, files still absent, content pouring into chat".
///
/// The floor is `OMNI_JUDGE_MIN_CHARS` — the SAME production floor every judge gate uses
/// (`produced_since_look`): growth that would count as production counts as wrong-channel
/// production. No new literal. `formed_chars_since_nudge` is `None` before the first delivered
/// nudge and after every restream (the seam resets this with the rest of the ladder), so a fresh
/// attempt can never trip it — the r6a wipe class stays closed. A reasoning lane (open, research,
/// judges: `owns_files` false) is structurally exempt and keeps the chars-based `advancing` hold.
pub(super) fn wrong_channel_stall(
    owns_files: bool,
    any_owned_on_disk: bool,
    formed_chars_since_nudge: Option<usize>,
) -> bool {
    owns_files
        && !any_owned_on_disk
        && formed_chars_since_nudge.is_some_and(|c| c >= OMNI_JUDGE_MIN_CHARS)
}

/// ONE definition of "this call moved its DELIVERABLE", used by the drift streak's reset and the
/// ladder's obedience arm. Progress facts only — no clock, no counter cap:
///   * owned bytes grew — a file the lane owes appeared or extended on disk;
///   * OR the formed/answer channel grew past the production floor (`OMNI_JUDGE_MIN_CHARS`, the
///     same floor every judge gate uses — no new literal) in a shape that is NOT the
///     wrong-channel pour. The carve-out is load-bearing: for a lane that owes files none of
///     which exist, formed growth IS the r6c web-console failure (70,600 chars of its own
///     CSS/HTML as chat text), and counting it as progress would shield the pour from the
///     restream that `wrong_channel_stall` exists to reach. A reasoning lane (owns nothing) and
///     a delivering builder (some owned file on disk) keep formed growth as honest progress.
///
/// What deliberately does NOT count: tool calls. MEASURED (r6c web-viz, BUILD+294m): 1-2
/// read-only sed/grep calls per look window (act=1/2/1/2/1) reset the old action-count arms for
/// five hours while the lane wrote ZERO files and its formed channel moved 772->924 bytes —
/// 63,506 think chars, nothing delivered, no steer ever received. Reading is not progress on a
/// deliverable; a write is, in whichever channel is the right one for the lane.
pub(super) fn write_progress(
    owned_bytes_grew: bool,
    owns_files: bool,
    any_owned_on_disk: bool,
    formed_chars_grown: usize,
) -> bool {
    owned_bytes_grew
        || (formed_chars_grown >= OMNI_JUDGE_MIN_CHARS && (!owns_files || any_owned_on_disk))
}

/// The escalation prompt's "Since then it has {…}" clause — what the judge is told the call did
/// after its last direction. A raw tool-call count launders READING into obedience (r6c web-viz,
/// BUILD+294m: 1-2 read-only sed/grep calls per look window read as "taken 2 action(s)" for five
/// hours while the lane wrote ZERO owned bytes), so a files-owing lane's clause carries the same
/// write-progress facts the ladder itself escalates on (`write_progress`). A lane that owns no
/// files keeps the plain count: its deliverable is text the judge already reads in the tail.
pub(super) fn escalation_moved(
    calls_since_nudge: usize,
    owns_files: bool,
    owned_bytes_grew: bool,
    any_owned_on_disk: bool,
    formed_chars_grown: usize,
) -> String {
    let n = calls_since_nudge;
    if n == 0 {
        "taken no action".to_string()
    } else if !owns_files {
        format!("taken {n} action(s)")
    } else if owned_bytes_grew {
        format!("taken {n} action(s) and its owned files grew")
    } else if write_progress(false, true, any_owned_on_disk, formed_chars_grown) {
        format!("taken {n} action(s) and its formed answer grew")
    } else {
        format!("taken {n} action(s) (read-only — no owned bytes written)")
    }
}

/// The tail's 48-char shingle set (stride 1), for RECURRENCE comparison across judge looks.
/// An exact tail hash cannot see the most classic loop: a repeating sentence SHIFTS through the
/// fixed-size tail window, so every look hashes differently and the two-consecutive-LOOPING streak
/// never arms — MEASURED live (qwen3.8 r0v2): a detail call repeated one sentence verbatim for 25+
/// minutes at repetition rate 1.00 while the judge looked on, streak pinned at 1. Shingle overlap
/// is shift-invariant: the same loop shows ≥ half-shared shingles on every look.
pub(super) fn tail_shingle_set(tail: &str) -> std::collections::HashSet<u64> {
    use std::hash::{Hash, Hasher};
    let norm: String = tail.split_whitespace().collect::<Vec<_>>().join(" ");
    let b: Vec<char> = norm.chars().collect();
    let mut out = std::collections::HashSet::new();
    // Stride 1: any window shift still shares nearly all shingles. A coarser stride is
    // phase-sensitive — a shift not divisible by it can yield DISJOINT shingles of the same
    // loop (the unit test caught exactly that with stride 16).
    let (win, step) = (48usize, 1usize);
    let mut i = 0;
    while i + win <= b.len() {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        b[i..i + win].iter().collect::<String>().hash(&mut h);
        out.insert(h.finish());
        i += step;
    }
    out
}

/// Do two judge-look tails show the SAME recurring content? Shift-invariant: true when at least
/// half of the smaller set's shingles recur in the other. Pure/testable.
pub(super) fn tails_recur(
    a: &std::collections::HashSet<u64>,
    b: &std::collections::HashSet<u64>,
) -> bool {
    if a.is_empty() || b.is_empty() {
        return false;
    }
    let inter = a.intersection(b).count();
    inter * 2 >= a.len().min(b.len())
}

/// One look's DRIFT-streak transition. The streak is DRIFTING's corroboration (delivery needs 2,
/// like LOOPING's second agreeing look), and what resets it is WRITE PROGRESS on the lane's
/// deliverable — never a tool-call count, and on a files-owing lane never a mere change of the
/// judge's mind.
///
/// MEASURED (r6c web-viz, five DRIFTING verdicts 15:51->18:37, ALL held, 0 files at BUILD+294m):
///   * the old rule reset on ANY action, and the lane's 1-2 read-only sed/grep calls per look
///     window disarmed the second-DRIFTING delivery for five hours;
///   * the old rule also reset on ANY non-drift verdict, and an interleaved ok (18:01,
///     established="" next="") disarmed the case the 17:39 hold had just armed — the hold's own
///     detail had promised "a second DRIFTING ... will be delivered".
///
/// So: write progress disarms, always. A DRIFTING with no write progress arms/corroborates. And
/// on a FILES-OWING lane an interleaved non-drift verdict leaves the armed case STANDING — the
/// judge is still respected (no steer is composed on an ok look; the arm merely survives to the
/// next DRIFTING). A reasoning lane (owns no files) keeps the old reset-on-disagreement: its
/// deliverable is text, and an ok about text is the judge reading real progress.
pub(super) fn drift_streak_step(
    streak: u32,
    drift_verdict: bool,
    write_progress_since_look: bool,
    owns_files: bool,
) -> u32 {
    if write_progress_since_look {
        0
    } else if drift_verdict {
        streak + 1
    } else if owns_files {
        streak
    } else {
        0
    }
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
/// AND THE HOLD MUST WEIGH FILES, NOT ONLY CHARS, FOR A LANE THAT OWES THEM (r6c, web-console):
/// a builder pouring its owned files into the FORMED channel is progress in the wrong channel, and
/// the chars-based `advancing` shielded exactly that — 70,600 formed chars, 0 owned files on disk,
/// two exact-file steers ignored, the restream never reached. So `wrong_channel` (the measured
/// conjunction in `wrong_channel_stall`) disqualifies an advance FOR HOLD PURPOSES ONLY: the
/// would-be hold escalates to the restream, whose seed carries the judge's directive plus the
/// carried tail — the delivery that broke the skeleton's equivalent stall on the same run. A lane
/// that made WRITE PROGRESS since its nudge still gets a steer (moving the deliverable is the
/// obedience the ladder escalates on), and a reasoning lane (no owned files) keeps the plain
/// advancing hold — the r6a wipe class.
///
/// AND OBEDIENCE IS WRITE PROGRESS, NOT A TOOL-CALL COUNT (r6c web-viz). The reset arm used to
/// compare tool-call counts across nudges, so ANY call — including a read-only sed/grep — read as
/// "acted since the previous nudge" and re-earned a steer forever: a reads-but-never-writes lane
/// could never walk the ladder past its first rung. `write_progress_since_nudge` is the
/// `write_progress` conjunction measured since the last DELIVERED nudge (owned bytes grew, or
/// non-wrong-channel material formed growth); `None` means no nudge has been delivered this
/// attempt — which since r6a is also every fresh attempt's state, because the restream seam
/// resets the ladder — so "obeyed" is measured on the deliverable's own record, not inferred
/// from what the judge hoped.
pub(super) fn nudge_delivery(
    pending_empty: bool,
    write_progress_since_nudge: Option<bool>,
    verdict: &goose_swarm::Verdict,
    advancing: bool,
    wrong_channel: bool,
) -> NudgeDelivery {
    if !pending_empty {
        return NudgeDelivery::Restream("tool request in flight");
    }
    if *verdict == goose_swarm::Verdict::Restart {
        return NudgeDelivery::Restream("judge said restart");
    }
    match write_progress_since_nudge {
        None => NudgeDelivery::Steer("first nudge"),
        Some(true) => NudgeDelivery::Steer("write progress since the previous nudge"),
        Some(false) if advancing && !wrong_channel => NudgeDelivery::Hold(
            "steer not acted on, but the stream is advancing: fresh non-recurring content since \
             the last look — held, not wiped",
        ),
        Some(false) if advancing => NudgeDelivery::Restream(
            "steer ignored and the advance is in the WRONG CHANNEL: this lane owes files, none \
             exist on disk, and the formed answer channel keeps growing — the directive rides a \
             fresh attempt's seed instead of watching file content pour into chat",
        ),
        Some(false) => NudgeDelivery::Restream(
            "steer ignored: no write progress since the previous nudge and the stream has \
             stopped advancing",
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
    /// but since r6a only once the stream has also STOPPED advancing (plateaued or recurring), and since
    /// r6c "obeyed" means the DELIVERABLE moved (write progress), not that some tool was called.
    #[test]
    fn nudge_delivery_escalates_on_measured_non_obedience() {
        use goose_swarm::Verdict;
        assert_eq!(
            nudge_delivery(true, None, &Verdict::Looping, false, false),
            NudgeDelivery::Steer("first nudge"),
            "the first nudge on a call is a steer: it keeps the partial and costs nothing"
        );
        assert_eq!(
            nudge_delivery(true, Some(false), &Verdict::Looping, false, false),
            NudgeDelivery::Restream(
                "steer ignored: no write progress since the previous nudge and the stream has \
                 stopped advancing"
            ),
            "a prior nudge with no write progress since AND a stopped stream is measured \
             non-obedience, so the anchor goes"
        );
        assert_eq!(
            nudge_delivery(true, Some(true), &Verdict::Looping, false, false),
            NudgeDelivery::Steer("write progress since the previous nudge"),
            "a call that moved its deliverable since the steer keeps getting steers"
        );
        assert_eq!(
            nudge_delivery(true, None, &Verdict::Restart, true, false),
            NudgeDelivery::Restream("judge said restart"),
            "RESTART is the judge saying a fresh attempt beats continuing, even on the first \
             nudge and even mid-production — the judge is the reader and said so outright; \
             this verdict is NEVER held"
        );
        assert_eq!(
            nudge_delivery(true, Some(true), &Verdict::Restart, true, false),
            NudgeDelivery::Restream("judge said restart"),
            "not even write progress holds a RESTART — the reader's outright verdict outranks \
             every arm below it"
        );
        assert_eq!(
            nudge_delivery(false, None, &Verdict::Looping, false, false),
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
    /// 0 actions): the previous attempt's nudge memory survived the restream, so the ladder read the
    /// very first look on a 45-second-old stream as "steer ignored" and wiped it. The restream seam
    /// resets the ladder (`owned_bytes_at_last_nudge = None` beside its siblings), and with `None`
    /// there is NO input — advancing or not, whatever the verdict short of the judge's own RESTART —
    /// that can read a fresh attempt as having ignored a steer it was never given.
    #[test]
    fn a_fresh_attempts_first_look_cannot_be_read_as_ignoring_a_steer() {
        use goose_swarm::Verdict;
        for verdict in [Verdict::Drifting, Verdict::Looping, Verdict::Ok] {
            for advancing in [false, true] {
                for wrong in [false, true] {
                    assert_eq!(
                        nudge_delivery(true, None, &verdict, advancing, wrong),
                        NudgeDelivery::Steer("first nudge"),
                        "a fresh attempt (write_progress_since_nudge = None after the seam \
                         reset) earns its own ladder: first delivery is a steer, never the wipe \
                         (verdict {verdict:?}, advancing {advancing}, wrong {wrong})"
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
        let d = nudge_delivery(true, Some(false), &Verdict::Drifting, true, false);
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
        let recurring = nudge_delivery(true, Some(false), &Verdict::Looping, false, false);
        assert!(
            matches!(recurring, NudgeDelivery::Restream(_)),
            "recurring after an unacted steer still escalates to the restream: {recurring:?}"
        );
        let plateaued = nudge_delivery(true, Some(false), &Verdict::Drifting, false, false);
        assert!(
            matches!(plateaued, NudgeDelivery::Restream(_)),
            "plateaued after an unacted steer still escalates to the restream: {plateaued:?}"
        );
    }

    /// THE WEB-CONSOLE SHAPE (r6c build, fan-out+51m): a file-owning lane with 0 owned files on
    /// disk, its CSS/HTML pouring into the FORMED channel (+70,600 chars), and the judge's
    /// exact-file directive pending across two unacted steers (12:04, 12:07). The chars-based hold
    /// shielded it and the restream — the rung that broke the skeleton's equivalent stall — never
    /// fired. Owned files absent + formed growing + directive pending -> restream.
    #[test]
    fn a_builder_pouring_owed_files_into_chat_is_restreamed_not_held() {
        use goose_swarm::Verdict;
        let wrong = wrong_channel_stall(true, false, Some(OMNI_JUDGE_MIN_CHARS));
        assert!(
            wrong,
            "owes files + none on disk + formed grew past the production floor since the nudge \
             IS the wrong-channel conjunction"
        );
        assert!(
            !write_progress(false, true, false, OMNI_JUDGE_MIN_CHARS * 10),
            "and the SAME pour is not write progress — counting it would shield the pour from \
             the restream via the obedience steer"
        );
        let d = nudge_delivery(true, Some(false), &Verdict::Drifting, true, wrong);
        assert!(
            matches!(d, NudgeDelivery::Restream(_)),
            "a formed-channel-only advance does not count as advancing for hold purposes: {d:?}"
        );
    }

    /// THE DELIVERING-BUILDER SHAPE (r6c ledgerd-core, files landing 15:41-15:46): once ANY owned
    /// file exists on disk the conjunction is false, so the lane keeps today's ladder — hold while
    /// advancing, steer when its deliverable moved. And a builder whose formed channel stays under
    /// the floor since its nudge (deliberating before the write) is likewise never tripped.
    #[test]
    fn a_delivering_builder_keeps_the_hold_and_the_steer() {
        use goose_swarm::Verdict;
        assert!(
            !wrong_channel_stall(true, true, Some(OMNI_JUDGE_MIN_CHARS * 10)),
            "a file on disk means the content is landing in the RIGHT channel"
        );
        assert!(
            write_progress(false, true, true, OMNI_JUDGE_MIN_CHARS * 10),
            "and with a file delivered, material formed growth is honest progress (the handoff \
             summary is real work), not a pour"
        );
        assert!(
            !wrong_channel_stall(true, false, Some(OMNI_JUDGE_MIN_CHARS - 1)),
            "formed growth under the production floor is deliberation, not a wrong-channel pour"
        );
        assert!(
            !wrong_channel_stall(true, false, None),
            "no delivered nudge yet (or a fresh attempt after the seam reset) cannot trip it"
        );
        assert!(
            write_progress(true, true, false, 0),
            "an owned file appearing or growing is write progress whatever the formed channel did"
        );
        let held = nudge_delivery(true, Some(false), &Verdict::Drifting, true, false);
        assert!(
            matches!(held, NudgeDelivery::Hold(_)),
            "an advancing builder whose files are landing is held exactly as before: {held:?}"
        );
        assert_eq!(
            nudge_delivery(true, Some(true), &Verdict::Drifting, true, true),
            NudgeDelivery::Steer("write progress since the previous nudge"),
            "moving the deliverable since the nudge is the obedience the ladder escalates on — a \
             steer, whatever the channel measurement says"
        );
    }

    /// THE REASONING-LANE SHAPE (r6a's opener: 0 owned files by design). `owns_files` false makes
    /// the conjunction structurally false, so open/research/judge lanes keep the chars-based
    /// advancing hold — the wipe class r6a closed stays closed. And a reasoning lane's material
    /// FORMED production counts as write progress (its words ARE its deliverable), so real output
    /// since a steer re-earns the steer instead of ever escalating.
    #[test]
    fn a_reasoning_lane_keeps_the_chars_based_advancing_hold() {
        use goose_swarm::Verdict;
        assert!(
            !wrong_channel_stall(false, false, Some(OMNI_JUDGE_MIN_CHARS * 40)),
            "a lane that owns no files has no wrong channel — its words ARE its deliverable"
        );
        let d = nudge_delivery(
            true,
            Some(false),
            &Verdict::Looping,
            true,
            wrong_channel_stall(false, false, Some(80_000)),
        );
        assert!(
            matches!(d, NudgeDelivery::Hold(_)),
            "r6a's converging opener (thinking advancing, formed flat) is still held, never \
             wiped: {d:?}"
        );
        assert!(
            write_progress(false, false, false, OMNI_JUDGE_MIN_CHARS),
            "material formed production on a no-files lane is write progress — its words are \
             the deliverable"
        );
        assert!(
            !write_progress(false, false, false, OMNI_JUDGE_MIN_CHARS - 1),
            "under the production floor is deliberation on every lane shape"
        );
    }

    /// DRIFT CORROBORATES; IT DOES NOT SUPPRESS FOREVER — and since r6c the streak is armed and
    /// reset by WRITE PROGRESS, not by tool-call counts (moved here from swarm.rs's inline rule
    /// when `drift_streak_step` was extracted).
    ///
    /// MEASURED (r6c web-viz, five DRIFTING verdicts 15:51->18:37, ALL held, 0 files and 63,506
    /// think chars at BUILD+294m): the lane's 1-2 READ-ONLY sed/grep calls per look window
    /// (act=1/2/1/2/1) reset the old action-count streak every single window, so the promised
    /// "second DRIFTING will be delivered" never arrived in five hours. Read-only calls are not
    /// an input here at all any more: only write progress resets.
    #[test]
    fn drift_is_delivered_on_the_second_look_with_no_write_progress() {
        // The delivery rule at the omni seam: corroborated = drift && streak >= 2.
        let delivered = |streak: u32, drift: bool| drift && streak >= 2;

        // (1, the r6c web-viz shape) Read-only calls between two DRIFTINGs change nothing: the
        // second DRIFTING delivers. 15:51 DRIFTING (held, armed) -> 16:21 DRIFTING (delivers).
        let mut s = 0u32;
        s = drift_streak_step(s, true, false, true);
        assert!(
            !delivered(s, true),
            "the FIRST drifting is held — the 33/34 evidence"
        );
        s = drift_streak_step(s, true, false, true);
        assert!(
            delivered(s, true),
            "a SECOND drifting with no write progress is corroborated — sed/grep in between is \
             not an input, so it cannot reset anything"
        );

        // (2) A WRITE between them resets the arm: this look holds and the case starts over.
        let mut s = 0u32;
        s = drift_streak_step(s, true, false, true);
        s = drift_streak_step(s, true, true, true);
        assert!(
            !delivered(s, true),
            "write progress resets — this look must not deliver"
        );
        assert_eq!(s, 0, "and the streak is cleared, not merely skipped");
    }

    /// THE INTERLEAVED-OK DISARM (r6c web-viz tick 22): 17:39 DRIFTING was held with the promise
    /// "a second DRIFTING ... will be delivered", then look 18:01 returned ok (established=""
    /// next="") and the old reset-on-any-non-drift-verdict disarmed the case — 6 DRIFTINGs total,
    /// zero delivered, the lane still owing its files at BUILD+324m. On a FILES-OWING lane the
    /// armed case now survives an interleaved ok (no steer is composed on the ok look — the judge
    /// is respected; the arm merely stands) and only actual write progress disarms it. A
    /// reasoning lane keeps the old rule: an ok about its text is the judge reading real progress
    /// (the r4b "a non-drift look breaks the streak" guardrail, scoped to where it was true).
    #[test]
    fn an_interleaved_ok_does_not_disarm_a_files_owing_lanes_armed_drift() {
        let delivered = |streak: u32, drift: bool| drift && streak >= 2;

        // Files-owing lane, zero write progress throughout (the r6c walk):
        let mut s = 0u32;
        s = drift_streak_step(s, true, false, true); // 17:39 DRIFTING — held, armed
        assert!(!delivered(s, true));
        s = drift_streak_step(s, false, false, true); // 18:01 ok — no steer, arm SURVIVES
        assert_eq!(s, 1, "an interleaved ok does not disarm the armed case");
        s = drift_streak_step(s, true, false, true); // 18:37 DRIFTING — delivers
        assert!(
            delivered(s, true),
            "the second DRIFTING with zero write progress DELIVERS despite the ok between"
        );

        // Write progress between disarms even on a files-owing lane:
        let mut s = 0u32;
        s = drift_streak_step(s, true, false, true);
        s = drift_streak_step(s, false, true, true);
        assert_eq!(s, 0, "an owned file growing is the disarm that counts");

        // Reasoning lane: the ok still breaks the streak (r4b guardrail).
        let mut s = 0u32;
        s = drift_streak_step(s, true, false, false);
        s = drift_streak_step(s, false, false, false);
        assert_eq!(
            s, 0,
            "a non-drift look breaks a reasoning lane's streak as before"
        );
    }

    /// THE ESCALATION TEXT READ OBEDIENCE FROM RAW COUNTS (r6c web-viz, BUILD+294m): the judge
    /// was told "taken 2 action(s)" about 1-2 read-only sed/greps per look window, five hours
    /// running, zero owned bytes ever written — it could not see the disobedience the ladder had
    /// already measured. The clause now carries the write-progress facts.
    #[test]
    fn the_escalation_clause_tells_the_judge_reads_from_writes() {
        assert_eq!(
            escalation_moved(2, true, false, false, 0),
            "taken 2 action(s) (read-only — no owned bytes written)",
            "the r6c web-viz shape: sed/greps with no owned bytes are not obedience"
        );
        assert_eq!(
            escalation_moved(2, true, true, true, 0),
            "taken 2 action(s) and its owned files grew"
        );
        assert_eq!(
            escalation_moved(1, true, false, true, OMNI_JUDGE_MIN_CHARS),
            "taken 1 action(s) and its formed answer grew",
            "formed growth with a delivered file on disk is the same progress write_progress counts"
        );
        assert_eq!(
            escalation_moved(1, true, false, false, OMNI_JUDGE_MIN_CHARS),
            "taken 1 action(s) (read-only — no owned bytes written)",
            "formed growth with NO owned file on disk is the wrong-channel pour, not progress"
        );
        assert_eq!(
            escalation_moved(3, false, false, false, 0),
            "taken 3 action(s)",
            "a reasoning lane keeps the plain count"
        );
        assert_eq!(
            escalation_moved(0, true, false, false, 0),
            "taken no action"
        );
    }

    #[test]
    fn a_shifting_repetition_loop_recurs_across_judge_looks() {
        // The measured pathology: one sentence repeated verbatim; the window SHIFTS between looks.
        let sentence = "Let me write the two files. First, `app/ledgerd/server.py`: ";
        let looped = sentence.repeat(60);
        let look1 = looped.get(..2000).unwrap();
        let look2 = looped.get(137..2137).unwrap(); // shifted window — exact-hash saw "new content" here
        let (s1, s2) = (tail_shingle_set(look1), tail_shingle_set(look2));
        assert!(
            tails_recur(&s1, &s2),
            "a shifted repetition window must RECUR"
        );
        // Healthy advancing reasoning must NOT recur: two disjoint passages.
        let h1 = tail_shingle_set(
            &"the sync module pages the vendor with cursors and applies rows by version "
                .repeat(30),
        );
        let h2 = tail_shingle_set(
            &"the webhook consumer verifies signatures over raw bytes then stages txn groups "
                .repeat(30),
        );
        assert!(!tails_recur(&h1, &h2), "distinct content must not recur");
    }
}
