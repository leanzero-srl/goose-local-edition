//! The judge's nudge ladder: whether a call produced, which arm fired, how a nudge is
//! delivered, and what a fresh attempt is seeded with.
//!
//! Sibling module under the incremental-split law
//! (development_gates::swarm_rs_line_count_only_decreases). The five functions moved here from
//! swarm.rs pay for the r6a fix's wiring in the root; each keeps its own WHY. r6a's behavior
//! change rides in `nudge_delivery` (the `advancing` hold — see its doc); r6c's rides in
//! `write_progress`/`drift_streak_step` and the ladder's obedience arm (the deliverable decides,
//! never a tool-call count).

// THE JUDGE'S LOOK CONSTANTS, moved verbatim from swarm.rs (incremental-split law, paying for
// VA-056/VA-058's wiring in the worker loop). Each keeps its own WHY below; the module's other
// readers (`desk.rs`, the worker loop) reach them through swarm.rs's `use ladder::...`.
// REMOVED (VA-056, 2026-09-01): OMNI_JUDGE_FIRST_LOOK_SECS (45) and OMNI_JUDGE_INTERVAL_SECS (60, backing
// off to 300 after six looks) — the judge's cadence clock. VA-013 had already made build/repair lanes
// evidence-only; the output-tool lanes were the last to keep the clock and the growth-without-acting
// chunk trigger. MEASURED r6e: 58 planner-side looks, every one cadence or growth, ~185 node-min, three
// nudges none acted on, and no look on any lane would have been a recurrence or forming-stall look.
// Every lane now summons on evidence alone (`ladder::judge_summon_trigger`); two seconds literals that
// could reach a model call are gone (gate 5). #135's intent ("you will see it immediately") lives on in
// the recurrence meter and the repeat detector, which fire the moment the evidence exists.
/// Minimum reasoning before a look is meaningful — below this there is nothing to assess yet.
///
/// Set to 2_000, having been 1_200 then briefly 4_000. Raising it was the WRONG fix and the data says so:
/// the judge fired at 1,200, 1,201, 3,759 and 4,003 chars — i.e. immediately past whatever floor was set —
/// so the threshold only moved WHEN the misread happened, never whether. The real fix is corroboration
/// (two consecutive LOOPING verdicts, see the abort site), which makes an early first look safe again and
/// restores the #135 intent of catching a loop in its first minute rather than its tenth.
///
/// Kept modestly above the original 1,200 so the first look still has a paragraph or two to read.
///
/// ORIGINAL NOTE, retained: RAISED 1_200 -> 4_000 on measured evidence. The judge is asked whether "the SAME content clearly
/// recurs", and recurrence needs the same thing to appear TWICE. At 1,200 characters — roughly one
/// paragraph — there is not enough text for that to be observable, so a yes is the weak model
/// pattern-matching on "this looks repetitive" rather than seeing an actual repeat. It is not a gating
/// bug: the verdict already requires HIGH confidence, and the model gave HIGH confidence anyway.
///
/// MEASURED, three omni fires observed to date:
///   1,200 chars  -> FALSE positive. Killed `verify-e2e::0` seconds after it started, costing 166s and a
///                   retry on a task that was doing nothing wrong.
///   8,988 chars  -> true positive, task retried and completed.
///   15,097 chars -> true positive, task retried and completed.
/// 4_000 sits well above the false positive and well below both true ones, and is about the point where
/// the 2,000-char tail the judge is shown can actually contain a repeat.
///
/// Deliberately a raise, not a disable — the intent of #135 stands ("you will see it immediately"; waiting
/// for 26,000 threw away minutes of a node per incident). This only stops it firing before there is
/// anything to read.
/// How often the streaming loop wakes when the provider is sending nothing.
///
/// NOT A CAP, and the distinction is the whole point: expiry runs the judge-look trigger and then goes
/// back to awaiting the same stream. Nothing is cancelled, failed, or given up on. It exists because
/// every judge look lives inside the stream loop, so a call that has gone silent is a call the judge
/// cannot see — which is precisely the call it most needs to see.
pub(super) const JUDGE_WAKE: std::time::Duration = std::time::Duration::from_secs(30);

pub(super) const OMNI_JUDGE_MIN_CHARS: usize = 2_000;

/// The IO coalesce cadence every digest/sidecar write site rides — the two worker-digest write
/// sites (main loop + judge-probe branch) and the forming observer's ArgsDelta coalescing. One
/// constant so the three sites cannot drift apart: their comments already promised "the digest's
/// own 400ms cadence" and only convention held them equal. An IO cadence, never a bound on model
/// work (gate 5): expiry writes a file; nothing about the call changes.
pub(super) const DIGEST_IO_CADENCE: std::time::Duration = std::time::Duration::from_millis(400);

/// The look-tail SCALE: how many chars of live reasoning ride in the digest's `last_thinking`,
/// in the judge's own look tail, and in the re-stream seed's carried tail. One constant because
/// the three are the SAME window by design — the judge reads what the digest carries, and the
/// re-stream seed hands back exactly what was read. A message-formation scale on carried text,
/// never a cap on model work (the transcripts are append-only and complete).
///
/// VA-126: this is the REFERENCE value on the 262,144 window. The two MODEL-facing tails (the
/// judge's look tail, the re-stream seed's carried tail) read the live
/// `budgets::ShownBudgets::look_tail_chars` — scaled from the fleet's probed window, identical
/// here — so the seed still hands back exactly what the judge read; the digest's `last_thinking`
/// is a VIEW the panel and tick.py read, not a model, and keeps this scale.
pub(super) const LOOK_TAIL_CHARS: usize = 2_000;

/// Cap the looks per call so a very long healthy call cannot spend unbounded judge time.
// REMOVED: OMNI_JUDGE_MAX_LOOKS (was 6). A cap on how many times the judge may look is a cap on how
// long a call is supervised, and the judge is now the only thing watching.
//
// How much further reasoning, with NO intervening tool call, summons the judge again. This is not a
// budget the call can exceed — nothing happens when it is crossed except that someone READS the call.
pub(super) const OMNI_JUDGE_GROWTH_CHARS: usize = 4_000;

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

/// THE METER'S CLAIM OF GROWTH IS CHECKED AGAINST THE DURABLE TRANSCRIPT (r6c web-viz, look 14).
///
/// `produced_since_last_look` is computed at a look's TRIGGER against the PREVIOUS trigger — but
/// the verdict that consumes it lands a whole judge probe later (a full model call on the same
/// saturated fleet; ~22 minutes live), so chars that arrived early in the trigger window read as
/// fresh production at a verdict delivered deep into silence. MEASURED (r6c, run
/// swarm-20260831-072930517): web-viz.think.log froze at 158,911 bytes at 18:10:25Z; look 13's
/// verdict (18:13:49Z) and look 14's (18:36:54Z) both read thinking_total 156,267 — ZERO growth
/// between them — yet look 14 carried produced_since_last_look 14,487 (its baseline was look 13's
/// trigger, 141,780), `advancing` stayed true, and the drift verdict was HELD on "fresh
/// non-recurring content" for a stream 26 minutes dead.
///
/// The durable `<task>.think.log` is fed by the SAME event loop that feeds the meter and flushed
/// at digest cadence, so a full look cycle (previous verdict stat -> this verdict stat) with ZERO
/// new bytes on disk means the stream is silent whatever the trigger-window arithmetic says.
/// DELTAS, never absolutes: the meter counts CHARS and the file counts BYTES, so the two sizes
/// are incomparable — grew-vs-frozen is the unit-free comparison. `None` means there is nothing
/// to check against (no durable transcript on disk — the pre-fce592811 world where a lane had
/// digests only — or no prior stat yet, the first verdict-bearing look): the meter stands alone,
/// exactly the pre-fix behavior. Reader-based and progress-based: one fs metadata read per look,
/// no clock, no cap — the only effect is that a hold's "it is producing" evidence must exist on
/// disk. Actions still count as production on their own (`produced_since_look`).
pub(super) fn durable_clamped_produced(
    meter_chars: usize,
    durable_think_grew: Option<bool>,
) -> usize {
    match durable_think_grew {
        Some(false) => 0,
        Some(true) | None => meter_chars,
    }
}

/// WHICH ARM produced a nudge, for the artefact. Three distinct triggers reach one emit — a measured
/// repeat, a DRIFTING verdict, and a LOOPING streak that is itself armed either by measured recurrence
/// or by tail similarity — and the payload named none of them, so "which trigger produces useful nudges"
/// could not be answered from any file the engine writes.
///
/// Ordered most-factual first: a measured repeat is an engine fact; a REPEATED NEXT (VA-056) is the
/// judge's previous look's undelivered direction standing while the call took no action — the
/// prior look is the first witness, this DRIFTING the second; plain DRIFTING is a verdict about
/// taste; and the streak's two arms are told apart by whether the detector could see the
/// recurrence itself.
pub(super) fn nudge_arm(
    repeat_measured: bool,
    repeated_next: bool,
    drifting_now: bool,
    recurring: bool,
) -> &'static str {
    if repeat_measured {
        "measured_repeat"
    } else if repeated_next {
        "repeated_next"
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

/// IS THE DRIFT HOLD'S PROMISE DUE? (r6c web-viz, tick 26 — the full walk is on
/// `nudge_delivery`'s promise paragraph.) All four inputs are progress facts; none is a new
/// counter or a clock:
///   * `owns_files` — only a files-owing lane; a reasoning lane keeps the advancing hold (the
///     r6a wipe class stays closed);
///   * `drift_verdict` — the promise is drift-shaped ("A second DRIFTING ... will be
///     delivered"), so only a drift-class look collects it;
///   * `drift_streak >= 2` — the EXISTING streak is the promise's memory: the hold at streak 1
///     made the promise, and write progress is what resets the streak (`drift_streak_step`),
///     so an armed streak means zero write progress across every held look — no new state;
///   * `calls_since_nudge == 0` — zero actions since the delivered steer. This is what tells
///     "cannot be reached any other way" from "reading before writing": a lane making tool
///     calls has turn boundaries a steer lands on and keeps today's hold; web-viz's
///     calls.jsonl was frozen 162 minutes while think advanced 30k+.
pub(super) fn delivery_promise_due(
    owns_files: bool,
    drift_verdict: bool,
    drift_streak: u32,
    calls_since_nudge: usize,
) -> bool {
    owns_files && drift_verdict && drift_streak >= 2 && calls_since_nudge == 0
}

/// IS THE STREAM DELIVERING AT THE MOMENT OF THE WIPE? (r6c web-viz, look 14.)
///
/// The restream is the one delivery that DESTROYS work: it drops the socket and replaces the
/// conversation with an empty one. Every fact the ladder decides on is a fact about what the call
/// has FINISHED — reasoning chars flushed, tool calls recorded, owned bytes on disk — and a model
/// streaming a large tool-call ARGUMENT has finished none of them. MEASURED: web-viz composed
/// `web/viz.js` (38,927 bytes, 979 lines) inside a single `write` call whose arguments streamed
/// for ~43 minutes; across that window `thinking_chars` was frozen at 156,267 (looks 13, 14, 15),
/// `calls.jsonl` had not moved since 15:28:09Z, and the owned file did not exist. At look 14
/// (18:36:54Z) every ladder input therefore said "stopped", and the clamp (`durable_clamped_
/// produced`) correctly zeroed the meter's stale 14,487 — so the ladder's last arm reads
/// "no write progress and the stream has stopped advancing" and takes the restream, SIXTEEN
/// MINUTES AND THIRTY-NINE KILOBYTES before that same stream landed the file at 18:53:35Z.
///
/// So the wipe is re-checked against the ONE channel that shows a delivery in flight. Both inputs
/// are progress facts measured across this look — no clock, no counter, no cap:
///   * `forming` argument bytes GREW since this look was triggered — a tool call is being
///     composed right now and it is bigger than it was;
///   * or an owned file grew — the deliverable itself moved.
///
/// GROWTH, not presence: a frame that is open but frozen has stopped, and the ladder keeps its
/// escape (the r5 87k-char loop and the r6a wedge are both unaffected — a looping reasoning
/// stream opens no frame at all). Deliberately NOT here: durable think growth and the raw tool
/// call count — those are exactly what a loop produces, and reading them as "delivering" would
/// disarm the restream for the cases it exists for.
///
/// The read is one file stat+parse at the delivery seam (`forming_args_bytes`), so it sees the
/// stream as it is when the socket is about to be dropped, not as the probe's input described it
/// twenty minutes earlier.
pub(super) fn stream_woke(
    forming_bytes_at_look: Option<u64>,
    forming_bytes_now: Option<u64>,
    owned_grew_since_look: bool,
) -> bool {
    owned_grew_since_look || forming_bytes_now.unwrap_or(0) > forming_bytes_at_look.unwrap_or(0)
}

/// The three ways a wanted nudge can land. `Steer` interrupts the stream at a chunk boundary and
/// KEEPS the partial; `Restream` drops the socket, wipes the conversation and seeds a fresh
/// attempt; `Hold` delivers NOTHING this look — the call is watched for one more look instead,
/// the same shape as the DRIFTING hold. Each variant carries the measured reason so the artefact
/// says why, not just what.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum NudgeDelivery {
    Steer(&'static str),
    Restream(&'static str),
    Hold(&'static str),
}

/// THE READER'S ANSWER ON THE STEER (VA-117, r6i OPEN look 3): what the judge's OWN TEXT said
/// about the direction it last delivered, read off an explicit `STEER_FOLLOWED: yes|no|unclear`
/// line it is asked for over the SINCE-STEER span (the call's reasoning from the steer's durable
/// byte offset to now — never a fixed tail). `None` = the judge wrote no such line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SteerFollowed {
    Yes,
    No,
    Unclear,
}

impl SteerFollowed {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            SteerFollowed::Yes => "yes",
            SteerFollowed::No => "no",
            SteerFollowed::Unclear => "unclear",
        }
    }
}

/// The reasons the reader's answer produces, shared with the tests so a reworded reason cannot
/// silently split the artefact from the pin.
pub(super) const JUDGE_SAYS_FOLLOWED: &str = "the judge read the since-steer span and said the      direction IS being followed — composing or acting on it — so the recurrence meter's reading      does not override the reader; the direction rides another note";
pub(super) const JUDGE_SAID_NOTHING: &str = "the judge wrote no STEER_FOLLOWED line, so no reader      has said the steer was ignored — the meter alone never decides a wipe; held for a look that      answers over the since-steer span";
pub(super) const JUDGE_UNCLEAR: &str = "the judge read the since-steer span and could not tell      whether the direction was followed — held, never wiped on a reader's shrug";
pub(super) const STEER_IGNORED: &str = "steer ignored: the judge read the since-steer span and      said the direction was NOT followed, and the deliverable did not move since the steer";

/// Read the `STEER_FOLLOWED` line out of a judge reply and STRIP it, so the lenient four-field
/// parser never folds it into NEXT — the ETA-token leak class (`parse_judge_reply`): a trailing
/// non-label line is read as the tail of NEXT, and a direction ending in "STEER_FOLLOWED: yes"
/// would have gone to the worker verbatim. Accepts `STEER_FOLLOWED`/`STEER FOLLOWED` in any
/// case, `:`/`=`/`|` separators, the value word as the first alphabetic run after them; the line
/// is cut AT the label so a same-line NEXT before it survives. A label with an unreadable value is
/// `Unclear` — the judge answered, illegibly; a missing label is `None` — it did not answer.
pub(super) fn split_steer_followed(reply: &str) -> (Option<SteerFollowed>, String) {
    let mut found: Option<SteerFollowed> = None;
    let mut kept: Vec<String> = Vec::new();
    for line in reply.lines() {
        let up = line.to_ascii_uppercase();
        let hit = up
            .find("STEER_FOLLOWED")
            .or_else(|| up.find("STEER FOLLOWED"));
        let Some(i) = hit else {
            kept.push(line.to_string());
            continue;
        };
        // `i` came from the ASCII-uppercased copy, which preserves byte positions; `get` keeps
        // the slice panic-free anyway (an empty tail reads as an illegible answer below).
        let after = up.get(i + "STEER_FOLLOWED".len()..).unwrap_or("");
        let value =
            after.trim_start_matches(|c: char| matches!(c, ':' | '=' | '|') || c.is_whitespace());
        let word: String = value
            .chars()
            .take_while(|c| c.is_ascii_alphabetic())
            .collect();
        found = Some(match word.as_str() {
            "YES" => SteerFollowed::Yes,
            "NO" => SteerFollowed::No,
            _ => SteerFollowed::Unclear,
        });
        // ASCII uppercasing preserves byte length, so `i` indexes the original line too.
        let head = line
            .get(..i)
            .unwrap_or(line)
            .trim_end_matches(|c: char| matches!(c, '|' | '-' | '—') || c.is_whitespace());
        if !head.is_empty() {
            kept.push(head.to_string());
        }
    }
    (found, kept.join("\n"))
}

/// The since-steer span: every byte `<task>.think.log` gained after `from_byte` (the durable
/// length recorded when the steer was delivered). `None` when the durable transcript cannot be
/// read — stated to the judge, never rendered as an empty span (fallback gate).
pub(super) fn since_steer_span(think_log: &std::path::Path, from_byte: u64) -> Option<String> {
    let bytes = std::fs::read(think_log).ok()?;
    let from = usize::try_from(from_byte).ok()?.min(bytes.len());
    Some(String::from_utf8_lossy(&bytes[from..]).into_owned())
}

/// The system-prompt ask that accompanies a since-steer span: the one extra line the judge
/// answers, and what each value means — with the r6c channel trap named (prose ABOUT a file that
/// was to land on disk is not following), because the deterministic `write_progress` fact the
/// judge is also shown is the tie-breaker it must weigh.
pub(super) fn steer_followed_ask() -> &'static str {
    "\n\nYou delivered a direction to this call, and every character of reasoning it produced \
     SINCE that note is quoted below under SINCE YOUR LAST DIRECTION. Add ONE extra line to your \
     reply, on its own line after the four fields:\n\
     STEER_FOLLOWED: yes|no|unclear\n\
     `yes` = the since-direction reasoning is DOING what you directed — assembling the exact \
     deliverable you named (a call whose deliverable is an emitted reply and is composing that \
     reply IS following, however long it takes) or taking the named action. `no` = it went back \
     to re-deriving, re-reading or re-verifying what you told it not to, or the deliverable you \
     named as bytes on disk did not appear while it wrote prose about it. `unclear` = the words do \
     not show either. This line decides whether the call is redirected in place (yes/unclear) or \
     restarted from a seed (no). A deterministic recurrence meter may have summoned this look; it \
     never decides that — you do, from the words. Name in ESTABLISHED the words you decided on."
}

/// The user-text block carrying the since-steer span. Three honest shapes: the span, a span that
/// has not grown (the call produced nothing after the note), and an unreadable transcript (the
/// absence is stated, and the judge is told to answer from the tail and say `unclear` if it does
/// not show).
pub(super) fn since_steer_block(span: Option<&str>) -> String {
    match span {
        Some(s) if !s.trim().is_empty() => format!(
            "\n\nSINCE YOUR LAST DIRECTION — every character of reasoning this call produced after \
             your note was delivered ({} chars, verbatim, oldest first):\n{}",
            s.chars().count(),
            s
        ),
        Some(_) => "\n\nSINCE YOUR LAST DIRECTION the durable transcript has not grown: the call \
                    produced no reasoning after your note was delivered."
            .to_string(),
        None => "\n\nSINCE YOUR LAST DIRECTION: the durable transcript could not be read, so the \
                 since-direction span is UNAVAILABLE this look — decide STEER_FOLLOWED from the most \
                 recent reasoning above and say `unclear` if it does not show."
            .to_string(),
    }
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
///
/// AND A PROMISED DELIVERY CANNOT BE DEFERRED BY THINK-ADVANCE FOREVER (r6c web-viz, tick 26).
/// The drift hold's own event promises "A second DRIFTING with no write progress since will be
/// delivered" — and on a ZERO-ACTION lane the promise deferred indefinitely: after the one steer
/// (17:09:15, "write web/viz.js NOW ... bytes on disk at web/viz.js are the deliverable") the
/// lane took zero tool calls (calls.jsonl frozen since 15:28), wrote zero owned bytes
/// (web/viz.js never existed), moved its formed channel ~170 chars in 64 minutes (2,433 bytes
/// TOTAL — under the wrong-channel floor), and produced 13-17k fresh NON-recurring think chars
/// per look, composing and then auditing the file in reasoning. Every rung was unreachable:
/// Steer needs `None`/`Some(true)`, the wrong-channel restream needs formed growth, the plain
/// restream needs the stream to stop — so 17:51's hold promised and 18:13's look held again
/// ("the stream is advancing"), and would forever. `promise_due` (`delivery_promise_due`) is
/// the escape: the promise exists (the files-owing lane's armed drift streak), the verdict is
/// drift-class again, and the lane has taken zero actions since the delivered steer — measured
/// non-obedience with no other reachable delivery — so the would-be hold escalates to the
/// restream, whose seed carries the directive plus the composed tail. A lane with real writes
/// since its nudge still steers (`Some(true)` outranks), a lane making calls keeps today's hold
/// (a steer can land where turns happen), and a reasoning lane is structurally exempt.
///
/// AND THE METER MAY SUMMON A LOOK, NEVER DECIDE DELIVERY (VA-117, r6i OPEN look 3, 10:13:34Z).
/// The `advancing` input above was `produced && !recur.recurring()` — the deterministic
/// recurrence meter — and it decided the wipe: r6i's judge wrote, in its own text, "Each sed
/// command reads a NEW range. This is advancing, not looping" and "recent reasoning is in final
/// section-to-slice assignment phase", verdict DRIFTING; the meter read 0.2986 over 65,536 chars
/// as recurring, so `advancing` was false, the `Some(false)` arm fell through to "the stream has
/// stopped advancing", and `judge_restream` abandoned 82,872 chars — 16k of them the emit being
/// composed in reasoning after the steer (`open.forming.json` empty because the composition was
/// thinking, not a tool frame). The emit came 16 minutes later from the fresh attempt. Gate 7:
/// a detector may SUMMON; only a reader may judge. So the reader's answer replaces the meter in
/// this signature: `judge_says_followed` is the judge's `STEER_FOLLOWED` line, answered over the
/// since-steer span (`since_steer_span`, from the steer's durable byte offset to now). The wipe
/// needs the reader to say `No` AND no write progress; `Yes` steers whatever the meter says;
/// `Unclear`/no line HOLDS, loudly named (a reader's shrug or silence is never a wipe). The
/// file-fact arms (wrong channel, due promise) keep their reasons under `No` — they say WHY the
/// reader's "not followed" is also measured on disk. The meter still summons the look and still
/// rides `judge_delivery_decided` as evidence beside the reader's answer.
pub(super) fn nudge_delivery(
    pending_empty: bool,
    write_progress_since_nudge: Option<bool>,
    verdict: &goose_swarm::Verdict,
    judge_says_followed: Option<SteerFollowed>,
    wrong_channel: bool,
    promise_due: bool,
) -> NudgeDelivery {
    if !pending_empty {
        return NudgeDelivery::Restream("tool request in flight");
    }
    if *verdict == goose_swarm::Verdict::Restart {
        return NudgeDelivery::Restream("judge said restart");
    }
    match write_progress_since_nudge {
        None => NudgeDelivery::Steer("first nudge of this attempt"),
        Some(true) => NudgeDelivery::Steer("write progress since the previous nudge"),
        Some(false) => match judge_says_followed {
            Some(SteerFollowed::Yes) => NudgeDelivery::Steer(JUDGE_SAYS_FOLLOWED),
            None => NudgeDelivery::Hold(JUDGE_SAID_NOTHING),
            Some(SteerFollowed::Unclear) => NudgeDelivery::Hold(JUDGE_UNCLEAR),
            Some(SteerFollowed::No) if wrong_channel => NudgeDelivery::Restream(
                "steer ignored (the judge said so over the since-steer span) and the advance is \
                 in the WRONG CHANNEL: this lane owes files, none exist on disk, and the formed \
                 answer channel keeps growing — the directive rides a fresh attempt's seed \
                 instead of watching file content pour into chat",
            ),
            Some(SteerFollowed::No) if promise_due => NudgeDelivery::Restream(
                "steer ignored (the judge said so over the since-steer span) and the promised \
                 delivery is due: zero actions and zero write progress since the steer on this \
                 files-owing lane — the directive rides a fresh attempt's seed instead of \
                 deferring the promise another look",
            ),
            Some(SteerFollowed::No) => NudgeDelivery::Restream(STEER_IGNORED),
        },
    }
}

/// The steer's SUPERVISOR NOTE, queued into the SAME running session as the next user message
/// (moved verbatim from the swarm.rs call site under the incremental-split law, paying for the
/// promise wiring). The note lands in the durable `<task>.log`; the ISO stamp makes each
/// appended block datable without file mtimes, like the dispatch header (r5 assessment:
/// reconstructing the steer sequence needed mtimes). Timestamp as data — nothing reads it,
/// nothing is bounded by it. An empty ESTABLISHED is omitted, never rendered as an empty claim
/// (the GEN-4 class: assert only what was actually delivered).
pub(super) fn steer_note(established: &str, direction: &str) -> String {
    format!(
        "SUPERVISOR NOTE ({}) — an independent reviewer read this call's own reasoning.\n\
         {}Do this next: {direction}\n\
         Continue the SAME task. Do not restart work you have already done, and do not \
         re-explain your plan.",
        chrono::Utc::now().to_rfc3339(),
        if established.is_empty() {
            String::new()
        } else {
            format!("You have already established: {established}\n")
        }
    )
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

/// The measured facts one loop pass hands `judge_summon_trigger` (VA-013, VA-056). Every one is
/// derived from the call's own counters or files at that pass — no clock is among them.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct SummonFacts {
    /// The repeat detector measured the same command returning the same bytes N times.
    pub(super) repeat: bool,
    /// The answer is 400+ chars of whitespace.
    pub(super) degenerate: bool,
    /// The readiness floor: enough reasoning or enough actions to judge from.
    pub(super) ready: bool,
    /// The shingle meter says recurring (span and rate both over their floors).
    pub(super) recurring: bool,
    /// A forming tool call's argument bytes stopped while reasoning grew (`forming_stalled`).
    pub(super) forming_stall: bool,
    /// VA-124: the settled-list meter measured the same list — the same item territories —
    /// written again with no material read between (`desk::SettledListMeter::relist_pending`).
    pub(super) settled_relist: bool,
}

/// Which measured fact summons the judge on this pass — or none. EVERY lane kind is looked at on
/// EVIDENCE only: the repeat detector, a degenerate answer, the recurrence meter, or a
/// forming-channel stall. VA-013 made BUILD/REPAIR lanes evidence-only (r6c: 132 build-lane
/// looks — 99 cadence, 33 growth, ~925 look-minutes — for two compliances and zero kills, and
/// not one would have been a recurrence look). VA-056 deleted the cadence and
/// growth-without-acting triggers for the output-tool lanes too, which were the last lanes that
/// had them. MEASURED r6e (run swarm-20260901-141137451): 58 looks on planner-side lanes, ALL
/// cadence (32) or growth (26) — OPEN 12 looks / 0 nudges / ~34 node-min; research 46 looks / 3
/// nudges / 0 acted on / ~151 node-min, every research look on a node already generating a
/// sibling lane (inflight=1); the meter's highest reading on any of them was rate 0.088 (trigger
/// 0.25) and no forming frame was ever open — under this rule zero of the 58 dispatch. A
/// REPEATED judge NEXT across two looks is evidence too, but it is evidence about DELIVERY, not
/// summoning (the omni seam's `repeated_next`: the prior look's undelivered NEXT is the first
/// witness, the next DRIFTING the second, and that NEXT is delivered instead of a third look).
/// `ready` gates the meter only; repeat and degenerate bypass it, as they always did. VA-124 adds
/// the SETTLED-LIST arm: a list of three or more items whose territories equal an earlier list's,
/// with no material read between — evidence by construction (two multi-thousand-char lists), so
/// it is not gated by `ready` either; r6j's opener re-listed six settled slices five times over
/// 27 minutes at a meter rate of 0.056 and was never looked at. The name returned rides
/// `judge_look_dispatched.trigger`, so a replay never re-derives the trigger from counters.
pub(super) fn judge_summon_trigger(f: SummonFacts) -> Option<&'static str> {
    if f.repeat {
        return Some("repeat");
    }
    if f.degenerate {
        return Some("degenerate_answer");
    }
    if f.ready && f.recurring {
        return Some("recurrence");
    }
    if f.settled_relist {
        return Some("settled_list_relisted");
    }
    if f.forming_stall {
        return Some("forming_stall");
    }
    None
}

/// The judge's evidence for a settled-list summon (VA-124): BOTH lists verbatim — the first
/// occurrence of this territory and the current re-list — beside the shared item territories, and
/// the steer the judge delivers when it agrees. The read-the-words gate: the judge is shown the
/// two texts side by side, never a count about them; renamed titles are exactly what the
/// territory lines make visible. `span_chars` is the caller's carried-text scale
/// (`ShownBudgets::look_tail_chars`); a list longer than it is cut at the tail and the cut is
/// stated, never silent (a partial list read as whole would hide a moved boundary).
pub(super) fn settled_list_block(r: &super::desk::SettledRelist, span_chars: usize) -> String {
    let clamp = |text: &str, already_cut: usize| {
        let total = text.chars().count();
        if total > span_chars {
            let shown: String = text.chars().take(span_chars).collect();
            format!(
                "{shown}\n[… {} more chars of this list not shown]",
                total - span_chars + already_cut
            )
        } else if already_cut > 0 {
            format!("{text}\n[… {already_cut} more chars of this list not shown]")
        } else {
            text.to_string()
        }
    };
    let items = r
        .items
        .iter()
        .map(|i| format!("\n- {i}"))
        .collect::<String>();
    let n = r.items.len();
    format!(
        "\n\nTHE SAME LIST, TWICE (settled-list meter): this call has written an ordered list of \
         {n} items whose TERRITORY — the files, sections or title each item claims — is \
         IDENTICAL to a list it wrote earlier in this same call (list #{} at char \
         {}, now list #{} at char {}), with {} lookup call(s) and no material read between them. \
         The territories both lists carve:{items}\n\
         THE FIRST SETTLED LIST, VERBATIM (from char {}):\n{}\n\
         THE CURRENT RE-LIST, VERBATIM (from char {}):\n{}\n\
         READ BOTH. Renamed titles do not make a new list — compare what each item OWNS. If the \
         two carve the same territory, re-listing it is the loop: the verdict is LOOPING and NEXT \
         says, in these words, 'the {n} slices are settled since char {}; write their objectives \
         and sections now and call the output tool with them'. If the current list genuinely \
         moves a boundary the territory lines could not see, say OK and NAME the boundary that \
         moved.",
        r.first_settled_occurrence,
        r.first_settled_offset,
        r.occurrence,
        r.current_offset,
        r.lookups_between,
        r.first_settled_offset,
        clamp(&r.first_span, r.first_span_cut_chars),
        r.current_offset,
        clamp(&r.current_span, r.current_span_cut_chars),
        r.first_settled_offset,
    )
}

/// The TASK ATTEMPT's supervision history — what the judge has recorded about and delivered to
/// this call across EVERY stream it has had. Distinct from the per-attempt ladder
/// (`tool_calls_at_last_nudge` and its siblings, which the restream seam resets so a fresh
/// attempt is never read as having ignored a steer it was never given — r6a): this history is
/// never reset, because the restream is the same task's next stream, not a new task.
///
/// VA-058, MEASURED r6e research-viz3d-engine: 15:38:20Z steer (nudge 1), 15:45:38Z restream
/// (nudge 2, 22,621 chars abandoned), then 15:48:14Z the post-restream steer was recorded
/// `judge_nudge nudge=1 reason="first nudge"` — the counter had reset with the stream, so the
/// escalation clause told the judge it had never redirected this call, `judge_out_of_moves`
/// could never see a direction repeated across the wipe, and steer→restream→steer could cycle
/// with every wipe re-buying the same derivation. And the seed carried only the LAST look's
/// ESTABLISHED (238 chars) — look 4's "tie cases (e.g. 0.3*5→1.5) resolve identically under both
/// rounding modes in float64" was gone, and the fresh attempt re-derived exactly that ("c=5:
/// 1.5->2 either way … settled g=150 -> 82.5 real divergence"). `established` keeps every look's
/// record so the seed carries the whole settled content, in the judge's words drawn from the
/// call's own.
#[derive(Debug, Clone, Default)]
pub(super) struct NudgeHistory {
    /// Nudges delivered to this call, every stream counted. UNBOUNDED — the old JUDGE_NUDGE_MAX
    /// is gone; what escalates a nudge is the judge seeing its own previous direction and
    /// whether the call obeyed it, never this number.
    pub(super) nudges_used: u32,
    /// The judge's last DELIVERED direction, fed back to it verbatim so it can tell "it ignored
    /// me" from "it tried".
    pub(super) last_direction: String,
    /// Every non-empty ESTABLISHED the judge recorded, in look order, exact repeats folded.
    pub(super) established: Vec<String>,
    /// The previous look's NEXT when that look delivered nothing — an OK verdict, a held drift.
    /// `None` once a direction is delivered (the delivered text is `last_direction`) or when the
    /// previous look named no NEXT.
    pub(super) undelivered_next: Option<String>,
}

impl NudgeHistory {
    /// A verdict-bearing look landed: keep what the judge recorded as established.
    pub(super) fn record_established(&mut self, established: &str) {
        let e = established.trim();
        if !e.is_empty() && self.established.last().is_none_or(|last| last != e) {
            self.established.push(e.to_string());
        }
    }

    /// The look delivered nothing; its NEXT stands as the direction the call never heard.
    pub(super) fn note_undelivered(&mut self, next: &str) {
        let n = next.trim();
        self.undelivered_next = (!n.is_empty()).then(|| n.to_string());
    }

    /// A direction was delivered (steer or restream): it is the last direction, and nothing is
    /// pending undelivered.
    pub(super) fn direction_delivered(&mut self, direction: &str) {
        self.last_direction = direction.to_string();
        self.undelivered_next = None;
    }

    /// The whole settled record for a restream seed — every look's ESTABLISHED, oldest first,
    /// one line each. Empty when no look recorded anything (the seed then omits the claim).
    pub(super) fn seed_established(&self) -> String {
        match self.established.len() {
            0 => String::new(),
            1 => self.established[0].clone(),
            _ => self
                .established
                .iter()
                .map(|e| format!("\n- {e}"))
                .collect::<String>(),
        }
    }
}

/// VA-013 (b): a write the model announced and walked away from. Sampled on REASONING GROWTH
/// (every OMNI_JUDGE_GROWTH_CHARS of new thinking re-reads `<key>.forming.json`), never on a
/// clock: a frame is OPEN on both samples and its argument bytes did not move while the reasoning
/// did. `None` on either side is the writer's honest empty (no frame open) and is never a stall;
/// `Some(0)` twice is an unparseable sidecar — not progress, not a stall.
pub(super) fn forming_stalled(prev: Option<u64>, now: Option<u64>) -> bool {
    matches!((prev, now), (Some(p), Some(n)) if p == n && n > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use goose_swarm::Verdict;

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

    /// r6c look 14 pinned: the meter claimed 14,487 fresh chars while web-viz.think.log had grown
    /// ZERO bytes across the whole look cycle — the backlog of a stream 26 minutes dead draining
    /// through the trigger-window arithmetic. A meter claim the durable transcript does not back
    /// is 0; a claim the transcript does back passes through untouched.
    #[test]
    fn a_meter_claim_the_durable_transcript_does_not_back_is_zero() {
        // r6c look 14: meter delta 14,487, durable delta 0 -> produced 0.
        assert_eq!(durable_clamped_produced(14_487, Some(false)), 0);
        // The inverse: the durable file grew, so the meter's claim stands.
        assert_eq!(durable_clamped_produced(14_487, Some(true)), 14_487);
        // Nothing to check against (no durable transcript on disk — the pre-fce592811 world —
        // or the first verdict-bearing look): the meter stands alone, the pre-fix behavior.
        assert_eq!(durable_clamped_produced(14_487, None), 14_487);
        // A zero meter is zero whatever the file did (the clamp only ever reduces).
        assert_eq!(durable_clamped_produced(0, Some(true)), 0);
    }

    /// The loop case the widening above gives up must still be caught, or the fix trades one blindness
    /// for another. A call repeating ONE tool call reads as producing now — and the measured-recurrence
    /// arm never consults this predicate, which is why it stays covered.
    #[test]
    fn the_measured_recurrence_arm_does_not_consult_production() {
        assert_eq!(
            nudge_arm(false, false, false, true),
            "measured_recurrence",
            "a measured recurrence arms on its own, whatever the production predicate says"
        );
        assert_eq!(nudge_arm(true, false, true, true), "measured_repeat");
        assert_eq!(nudge_arm(false, false, true, false), "drifting");
        assert_eq!(
            nudge_arm(false, false, false, false),
            "tail_similarity_streak"
        );
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
            nudge_delivery(true, None, &Verdict::Looping, None, false, false),
            NudgeDelivery::Steer("first nudge of this attempt"),
            "the first nudge on a call is a steer: it keeps the partial and costs nothing"
        );
        assert_eq!(
            nudge_delivery(
                true,
                Some(false),
                &Verdict::Looping,
                Some(SteerFollowed::No),
                false,
                false
            ),
            NudgeDelivery::Restream(STEER_IGNORED),
            "a prior nudge with no write progress since AND the judge's own text saying the \
             direction was not followed is measured non-obedience, so the anchor goes"
        );
        assert_eq!(
            nudge_delivery(true, Some(true), &Verdict::Looping, None, false, false),
            NudgeDelivery::Steer("write progress since the previous nudge"),
            "a call that moved its deliverable since the steer keeps getting steers"
        );
        assert_eq!(
            nudge_delivery(
                true,
                None,
                &Verdict::Restart,
                Some(SteerFollowed::Yes),
                false,
                false
            ),
            NudgeDelivery::Restream("judge said restart"),
            "RESTART is the judge saying a fresh attempt beats continuing, even on the first \
             nudge and even mid-production — the judge is the reader and said so outright; \
             this verdict is NEVER held"
        );
        assert_eq!(
            nudge_delivery(
                true,
                Some(true),
                &Verdict::Restart,
                Some(SteerFollowed::Yes),
                false,
                true
            ),
            NudgeDelivery::Restream("judge said restart"),
            "not even write progress holds a RESTART — the reader's outright verdict outranks \
             every arm below it"
        );
        assert_eq!(
            nudge_delivery(false, None, &Verdict::Looping, None, false, false),
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

    /// r6i OPEN look 3 (10:13:34Z) — THE METER OVERRODE THE READER. The judge's own text: "Each sed
    /// command reads a NEW range. This is advancing, not looping" and "recent reasoning is in final
    /// section-to-slice assignment phase" (verdict DRIFTING); the meter: recur 0.2986 over 65,536 —
    /// recurring, so the old `advancing` was false; open owns no files and its formed channel was
    /// empty, so write progress since the 10:09:29 steer was `Some(false)`. The old ladder wiped
    /// 82,872 chars (16k of them the emit being composed); asked `STEER_FOLLOWED` over that
    /// since-steer span the judge's words say `yes`, and the reader's yes is a steer whatever the
    /// meter says. The true loop (the judge reads a verbatim repeat and says `no`) still wipes; a
    /// judge that wrote no line, or could not tell, holds — named, never a wipe on silence.
    #[test]
    fn the_readers_steer_followed_answer_decides_delivery_not_the_meter() {
        use goose_swarm::Verdict;
        assert_eq!(
            nudge_delivery(
                true,
                Some(false),
                &Verdict::Drifting,
                Some(SteerFollowed::Yes),
                false,
                false,
            ),
            NudgeDelivery::Steer(JUDGE_SAYS_FOLLOWED),
            "r6i look 3: the judge says the emit is being composed -> steer, not the wipe"
        );
        assert_eq!(
            nudge_delivery(
                true,
                Some(false),
                &Verdict::Looping,
                Some(SteerFollowed::No),
                false,
                false,
            ),
            NudgeDelivery::Restream(STEER_IGNORED),
            "the true loop: the judge read the since-steer span as a verbatim repeat -> wipe"
        );
        assert_eq!(
            nudge_delivery(true, Some(false), &Verdict::Looping, None, false, false),
            NudgeDelivery::Hold(JUDGE_SAID_NOTHING),
            "no STEER_FOLLOWED line: no reader said ignored, the meter alone never wipes"
        );
        assert_eq!(
            nudge_delivery(
                true,
                Some(false),
                &Verdict::Drifting,
                Some(SteerFollowed::Unclear),
                false,
                false,
            ),
            NudgeDelivery::Hold(JUDGE_UNCLEAR),
            "a reader's shrug holds"
        );
    }

    /// The line is read in the shapes a 27B writes it and STRIPPED before the four-field parse,
    /// so it can never ride into NEXT the way `ETA=5m` once did; a same-line NEXT before it
    /// survives the cut.
    #[test]
    fn split_steer_followed_reads_every_shape_and_strips_the_line() {
        let (a, rest) = split_steer_followed(
            "VERDICT|DRIFTING|HIGH|split settled|emit it now\nSTEER_FOLLOWED: yes",
        );
        assert_eq!(a, Some(SteerFollowed::Yes));
        assert_eq!(rest, "VERDICT|DRIFTING|HIGH|split settled|emit it now");
        let (b, rest) = split_steer_followed(
            "VERDICT|LOOPING|HIGH|x|y\nsteer_followed=no — same ten items again",
        );
        assert_eq!(b, Some(SteerFollowed::No));
        assert_eq!(rest, "VERDICT|LOOPING|HIGH|x|y");
        let (c, rest) =
            split_steer_followed("VERDICT|OK|HIGH|x|emit now | STEER FOLLOWED: unclear");
        assert_eq!(c, Some(SteerFollowed::Unclear));
        assert_eq!(
            rest, "VERDICT|OK|HIGH|x|emit now",
            "a same-line NEXT before the label survives"
        );
        let (d, rest) = split_steer_followed("VERDICT|OK|HIGH|x|y\nSTEER_FOLLOWED: maybe");
        assert_eq!(
            d,
            Some(SteerFollowed::Unclear),
            "an unreadable value is an illegible answer, not silence"
        );
        assert_eq!(rest, "VERDICT|OK|HIGH|x|y");
        let (e, rest) = split_steer_followed("VERDICT|OK|HIGH|x|y");
        assert_eq!(e, None, "no line is no answer");
        assert_eq!(rest, "VERDICT|OK|HIGH|x|y");
    }

    /// The span is the durable transcript from the steer's byte offset to now — no fixed tail;
    /// an unreadable transcript is `None` (stated to the judge), a non-grown one is empty.
    #[test]
    fn since_steer_span_reads_from_the_steer_offset_to_now() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("open.think.log");
        std::fs::write(&p, "before the steer|after the steer, composing the emit").unwrap();
        let from = "before the steer|".len() as u64;
        assert_eq!(
            since_steer_span(&p, from).as_deref(),
            Some("after the steer, composing the emit")
        );
        assert_eq!(
            since_steer_span(&p, 10_000).as_deref(),
            Some(""),
            "not grown -> empty, not None"
        );
        assert!(since_steer_span(&dir.path().join("missing.think.log"), 0).is_none());
        assert!(since_steer_block(None).contains("UNAVAILABLE"));
        assert!(since_steer_block(Some("")).contains("has not grown"));
        assert!(since_steer_block(Some("composing")).contains("(9 chars, verbatim"));
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
            for judge in [
                None,
                Some(SteerFollowed::Yes),
                Some(SteerFollowed::No),
                Some(SteerFollowed::Unclear),
            ] {
                for wrong in [false, true] {
                    for promise in [false, true] {
                        assert_eq!(
                            nudge_delivery(true, None, &verdict, judge, wrong, promise),
                            NudgeDelivery::Steer("first nudge of this attempt"),
                            "a fresh attempt (write_progress_since_nudge = None after the seam \
                             reset) earns its own ladder: first delivery is a steer, never the \
                             wipe (verdict {verdict:?}, judge {judge:?}, wrong {wrong}, \
                             promise {promise})"
                        );
                    }
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
        let d = nudge_delivery(
            true,
            Some(false),
            &Verdict::Drifting,
            Some(SteerFollowed::Unclear),
            false,
            false,
        );
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
        let recurring = nudge_delivery(
            true,
            Some(false),
            &Verdict::Looping,
            Some(SteerFollowed::No),
            false,
            false,
        );
        assert!(
            matches!(recurring, NudgeDelivery::Restream(_)),
            "recurring after an unacted steer still escalates to the restream: {recurring:?}"
        );
        let plateaued = nudge_delivery(
            true,
            Some(false),
            &Verdict::Drifting,
            Some(SteerFollowed::No),
            false,
            false,
        );
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
        let d = nudge_delivery(
            true,
            Some(false),
            &Verdict::Drifting,
            Some(SteerFollowed::No),
            wrong,
            false,
        );
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
        let held = nudge_delivery(
            true,
            Some(false),
            &Verdict::Drifting,
            Some(SteerFollowed::Unclear),
            false,
            false,
        );
        assert!(
            matches!(held, NudgeDelivery::Hold(_)),
            "an advancing builder whose files are landing is held exactly as before: {held:?}"
        );
        assert_eq!(
            nudge_delivery(
                true,
                Some(true),
                &Verdict::Drifting,
                Some(SteerFollowed::No),
                true,
                true
            ),
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
            Some(SteerFollowed::Unclear),
            wrong_channel_stall(false, false, Some(80_000)),
            delivery_promise_due(false, true, 5, 0),
        );
        assert!(
            matches!(d, NudgeDelivery::Hold(_)),
            "r6a's converging opener (thinking advancing, formed flat) is still held, never \
             wiped — a no-files lane's promise is never due: {d:?}"
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
        // And on a ZERO-ACTION lane the delivery is the seeded restream — the interleaved ok
        // could not disarm the promise either, and think-advance does not defer it (tick 26:
        // the advancing hold otherwise absorbs every corroborated drift forever).
        assert!(delivery_promise_due(true, true, s, 0));
        let due = nudge_delivery(
            true,
            Some(false),
            &goose_swarm::Verdict::Drifting,
            Some(SteerFollowed::No),
            false,
            true,
        );
        assert!(
            matches!(due, NudgeDelivery::Restream(_)),
            "a due promise on a zero-action lane delivers by restream, not another hold: {due:?}"
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

    /// THE PROMISE-EVASION SEQUENCE (r6c web-viz, tick 26, run swarm-20260831-072930517):
    /// 17:51:37 `judge_drift_held` (streak 1, actions 0) promised "A second DRIFTING with no
    /// write progress since will be delivered"; 18:13:49 the second DRIFTING arrived (streak 2,
    /// actions 0, recurring false, 16,901 fresh think chars) and was HELD AGAIN via the
    /// advancing branch — and would be forever, because the lane composes and audits web/viz.js
    /// in REASONING: zero tool calls (no boundary for pending), zero owned bytes (no
    /// `Some(true)` steer), formed frozen at 2,433 bytes total (~170 chars in 64 minutes — no
    /// wrong-channel restream), think always past the floor (no plateaued restream).
    #[test]
    fn a_due_promise_on_a_zero_action_lane_is_not_deferred_by_think_advance() {
        use goose_swarm::Verdict;
        // 17:51 DRIFTING arms and promises; the promise is being MADE, not yet due.
        let mut s = 0u32;
        s = drift_streak_step(s, true, false, true);
        assert!(
            !delivery_promise_due(true, true, s, 0),
            "streak 1 is the hold that makes the promise — nothing delivers on the first drift"
        );
        // 18:13 second DRIFTING, zero actions and zero write progress since the steer: due,
        // and the delivery is the seeded restream carrying the directive plus the composed tail.
        s = drift_streak_step(s, true, false, true);
        assert!(delivery_promise_due(true, true, s, 0));
        let d = nudge_delivery(
            true,
            Some(false),
            &Verdict::Drifting,
            Some(SteerFollowed::No),
            false,
            true,
        );
        assert!(
            matches!(d, NudgeDelivery::Restream(_)),
            "the promised second DRIFTING on a zero-action lane delivers, not holds: {d:?}"
        );

        // A write between the two driftings disarms the whole case: the streak resets and the
        // obedience arm steers — an advancing lane with real writes keeps today's ladder.
        let mut s = 0u32;
        s = drift_streak_step(s, true, false, true);
        s = drift_streak_step(s, true, true, true);
        assert_eq!(s, 0, "write progress resets the promise's memory");
        assert!(!delivery_promise_due(true, true, s, 0));
        assert_eq!(
            nudge_delivery(
                true,
                Some(true),
                &Verdict::Drifting,
                Some(SteerFollowed::Unclear),
                false,
                false
            ),
            NudgeDelivery::Steer("write progress since the previous nudge"),
            "a lane that moved its deliverable is steered, never wiped"
        );

        // A lane making tool calls since its steer keeps the hold: a steer can land where
        // turns happen, so the promise is reachable by today's rungs.
        assert!(!delivery_promise_due(true, true, 3, 2));
        let held = nudge_delivery(
            true,
            Some(false),
            &Verdict::Drifting,
            Some(SteerFollowed::Unclear),
            false,
            false,
        );
        assert!(
            matches!(held, NudgeDelivery::Hold(_)),
            "reading-before-writing with calls flowing keeps the advancing hold: {held:?}"
        );

        // Write progress since the nudge outranks even a set flag (the streak-reset race: a
        // write between the nudge and the last look), and a reasoning lane is never due.
        assert_eq!(
            nudge_delivery(
                true,
                Some(true),
                &Verdict::Drifting,
                Some(SteerFollowed::No),
                false,
                true
            ),
            NudgeDelivery::Steer("write progress since the previous nudge")
        );
        assert!(!delivery_promise_due(false, true, 5, 0));
    }

    /// r6c web-viz, look 14 (18:36:54Z) — THE COUNTERFACTUAL THIS GUARD EXISTS FOR. Every ladder
    /// input said "stopped": think frozen at 156,267 chars across looks 13/14/15, so the durable
    /// clamp zeroed the meter's stale 14,487; calls.jsonl frozen since 15:28:09Z; web/viz.js
    /// absent. The ladder's last arm therefore takes the restream — while that very stream was
    /// 16 minutes and 39 KB into the `write` whose arguments became the 979-line web/viz.js at
    /// 18:53:35Z. The forming sidecar is the only channel that saw it.
    #[test]
    fn a_restream_is_aborted_while_a_tool_argument_stream_is_still_growing() {
        assert_eq!(
            nudge_delivery(
                true,
                Some(false),
                &Verdict::Drifting,
                Some(SteerFollowed::No),
                false,
                false
            ),
            NudgeDelivery::Restream(STEER_IGNORED),
            "the ladder still reaches the wipe on the r6c facts — the abort is the seam's job"
        );
        assert!(
            stream_woke(Some(12_000), Some(31_500), false),
            "a growing tool-call argument stream is a delivery in flight"
        );
        // The same look one probe later (19:00:18Z): the write COMPLETED, the frame is gone and
        // the owned file exists — the delivery is on disk and the wipe is refused on that.
        assert!(stream_woke(Some(31_500), None, true));
        // And a frame that OPENED during the probe counts from nothing.
        assert!(stream_woke(None, Some(1_400), false));
    }

    /// The escape stays open, or the guard would be a cap on the restream. A frozen frame has
    /// STOPPED, and the loops the restream exists for (r5's 87k-char reasoning loop, r6a's wedge)
    /// open no frame at all — so neither is shielded.
    #[test]
    fn a_frozen_or_absent_argument_stream_still_walks_the_ladder() {
        assert!(!stream_woke(Some(9_000), Some(9_000), false));
        assert!(!stream_woke(None, None, false));
        // A frame that completed with nothing else moving is not, by itself, a delivery: the
        // owned/write-progress facts decide that, exactly as they did before this guard.
        assert!(!stream_woke(Some(9_000), None, false));
    }

    /// The steer note asserts only what was delivered (the GEN-4 class): the direction always,
    /// ESTABLISHED only when the judge produced one — an empty summary is omitted, never
    /// rendered as an empty claim.
    #[test]
    fn the_steer_note_carries_the_direction_and_omits_an_empty_established() {
        let note = steer_note(
            "spec contracts extracted; file design settled in-head",
            "write web/viz.js NOW as a first minimal version",
        );
        assert!(note.contains("SUPERVISOR NOTE ("));
        assert!(
            note.contains("You have already established: spec contracts extracted; file design")
        );
        assert!(note.contains("Do this next: write web/viz.js NOW as a first minimal version"));
        assert!(note.contains("Continue the SAME task."));
        let bare = steer_note("", "act");
        assert!(
            !bare.contains("You have already established"),
            "an empty ESTABLISHED is omitted: {bare}"
        );
        assert!(bare.contains("Do this next: act"));
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

#[cfg(test)]
mod summon_tests {
    use super::*;

    // Two lane kinds, one trigger set (VA-056): the helpers are kept so a future per-kind fact
    // has a place to land — and so the test says in its own words that it tried both kinds.
    fn build_lane() -> SummonFacts {
        SummonFacts {
            ready: true,
            ..SummonFacts::default()
        }
    }
    fn output_lane() -> SummonFacts {
        SummonFacts {
            ready: true,
            ..SummonFacts::default()
        }
    }

    /// r6c web-viz, look 10 (a build lane at recurrence 0.003 over 65,536 shingles, 34,286 fresh
    /// chars, zero actions since the last look) and r6e's 58 planner-side looks (OPEN 12, research
    /// 46 — all cadence or growth; meter max 0.088, no forming frame ever open): with no evidence
    /// NOTHING summons, whatever the lane kind. Cadence and growth-without-acting are not facts a
    /// pass can hand in any more — the fields are gone, not gated.
    #[test]
    fn no_lane_kind_is_summoned_without_evidence() {
        assert_eq!(judge_summon_trigger(build_lane()), None);
        assert_eq!(judge_summon_trigger(output_lane()), None);
        // Below the readiness floor, likewise nothing (a fresh lane with a paragraph of reasoning).
        assert_eq!(
            judge_summon_trigger(SummonFacts {
                ready: false,
                ..output_lane()
            }),
            None
        );
    }

    /// The history survives the restream seam: the seam replaces the per-attempt ladder and never
    /// names `NudgeHistory`, so nudge 3 after a wipe is recorded as nudge 3 (r6e recorded it as
    /// "nudge 1, first nudge"), the last direction stays visible to the escalation clause, and
    /// the seed carries EVERY look's ESTABLISHED — look 4's tie-case record included.
    #[test]
    fn the_nudge_history_accumulates_across_looks_and_is_never_reset_by_the_seam() {
        let mut h = NudgeHistory::default();
        // Looks 1-5 of r6e viz3d (abridged), the steer at look 2, the restream at look 5.
        h.record_established("pinned the spec constraints: ≤8 default-FBO draws at N=12,288");
        h.note_undelivered("call the output tool NOW and emit final_output now");
        assert_eq!(
            h.undelivered_next.as_deref(),
            Some("call the output tool NOW and emit final_output now")
        );
        h.record_established(
            "Color derivation chain established (dim=round(0.30·c), side=round(0.55·dim))",
        );
        h.nudges_used += 1;
        h.direction_delivered("Your next message must be a tool call: invoke the output tool");
        assert_eq!(
            h.undelivered_next, None,
            "a delivered direction leaves nothing undelivered"
        );
        h.record_established(
            "tie cases (e.g. 0.3*5→1.5) resolve identically under both rounding modes in float64",
        );
        h.record_established(
            "tie cases (e.g. 0.3*5→1.5) resolve identically under both rounding modes in float64",
        );
        assert_eq!(h.established.len(), 3, "an exact repeat folds");
        h.record_established("   ");
        assert_eq!(h.established.len(), 3, "an empty record is not a record");
        h.nudges_used += 1;
        h.direction_delivered(
            "Your next message must be a tool call to the output tool — not reasoning text.",
        );
        // THE SEAM: the ladder is reset (its own fields, elsewhere); the history is untouched by
        // construction — there is no method on it that forgets.
        let after_seam = h.clone();
        assert_eq!(after_seam.nudges_used, 2);
        assert_eq!(
            after_seam.last_direction,
            "Your next message must be a tool call to the output tool — not reasoning text."
        );
        let seed = after_seam.seed_established();
        assert!(
            seed.contains("tie cases (e.g. 0.3*5→1.5) resolve identically"),
            "look 4's record rides the seed: {seed}"
        );
        assert!(seed.contains("pinned the spec constraints"));
        assert_eq!(
            seed.matches("\n- ").count(),
            3,
            "one line per recorded look"
        );
        // The post-restream steer is nudge 3, not "nudge 1".
        let mut fresh_attempt_look = after_seam;
        fresh_attempt_look.nudges_used += 1;
        assert_eq!(fresh_attempt_look.nudges_used, 3);
        // A single record renders bare, no bullet.
        let mut one = NudgeHistory::default();
        one.record_established("only this");
        assert_eq!(one.seed_established(), "only this");
        assert_eq!(NudgeHistory::default().seed_established(), "");
        assert_eq!(nudge_arm(false, true, true, false), "repeated_next");
        assert_eq!(nudge_arm(true, true, true, true), "measured_repeat");
    }

    /// r6d research-ledger-core-q3, look 1: recurrence 0.353 over 10,760 shingles — the meter
    /// summons on any lane kind; on a build lane it is one of the two triggers left.
    #[test]
    fn evidence_summons_on_every_lane_kind() {
        for base in [build_lane(), output_lane()] {
            assert_eq!(
                judge_summon_trigger(SummonFacts {
                    recurring: true,
                    ..base
                }),
                Some("recurrence")
            );
            assert_eq!(
                judge_summon_trigger(SummonFacts {
                    forming_stall: true,
                    ..base
                }),
                Some("forming_stall")
            );
            // VA-124: a settled re-list is evidence by construction — not gated by `ready`.
            assert_eq!(
                judge_summon_trigger(SummonFacts {
                    settled_relist: true,
                    ready: false,
                    ..base
                }),
                Some("settled_list_relisted")
            );
            // repeat and degenerate bypass the readiness floor, as before.
            assert_eq!(
                judge_summon_trigger(SummonFacts {
                    repeat: true,
                    ready: false,
                    ..base
                }),
                Some("repeat")
            );
            assert_eq!(
                judge_summon_trigger(SummonFacts {
                    degenerate: true,
                    ready: false,
                    ..base
                }),
                Some("degenerate_answer")
            );
        }
        // Below the readiness floor the meter's word alone does not summon (no tail to read).
        assert_eq!(
            judge_summon_trigger(SummonFacts {
                recurring: true,
                ready: false,
                ..build_lane()
            }),
            None
        );
    }

    /// VA-124: the summon's prompt block carries BOTH lists verbatim (the words, gate 7), the
    /// shared territories, the lookup count, and the steer in the words the judge is to deliver;
    /// a list longer than the carried-text scale is cut with the cut STATED (gate 1).
    #[test]
    fn a_settled_re_list_summons_with_both_lists_verbatim_and_the_cut_stated() {
        let r = super::super::desk::SettledRelist {
            occurrence: 4,
            first_settled_occurrence: 3,
            first_settled_offset: 76_768,
            current_offset: 89_878,
            items: vec![
                "files: app/auth.py, app/drafts.py, app/webhooks.py".into(),
                "files: app/notifierd.py".into(),
                "files: web/viz.js".into(),
            ],
            first_span: "**S3: notifierd** — weight 3\nOwns: `app/notifierd.py`\n".into(),
            first_span_cut_chars: 0,
            current_span: "3. **notifierd** (weight 3) — standalone idempotent consumer service.\n"
                .into(),
            current_span_cut_chars: 0,
            lookups_between: 0,
        };
        let block = settled_list_block(&r, 2_000);
        assert!(block.contains("list #3 at char 76768, now list #4 at char 89878"));
        assert!(block.contains("with 0 lookup call(s)"));
        assert!(block.contains("- files: app/notifierd.py"));
        assert!(block.contains("**S3: notifierd** — weight 3\nOwns: `app/notifierd.py`"));
        assert!(block.contains("3. **notifierd** (weight 3) — standalone idempotent consumer"));
        assert!(
            block.contains("'the 3 slices are settled since char 76768; write their objectives")
        );
        assert!(!block.contains("not shown"), "nothing was cut: {block}");
        // Cut at the carried-text scale, stated with the chars the meter itself did not carry.
        let mut long = r.clone();
        long.first_span = "x".repeat(2_050);
        long.current_span_cut_chars = 400;
        let block = settled_list_block(&long, 2_000);
        assert!(
            block.contains("[… 50 more chars of this list not shown]"),
            "{block}"
        );
        assert!(
            block.contains("[… 400 more chars of this list not shown]"),
            "{block}"
        );
    }

    /// r6c web-viz's one delivery streamed 38,927 argument bytes across three looks — GROWING
    /// bytes are a write in progress, never a stall; a frozen open frame is; no frame is nothing.
    #[test]
    fn a_forming_stall_is_an_open_frame_whose_bytes_stopped_while_reasoning_grew() {
        assert!(forming_stalled(Some(2_433), Some(2_433)));
        assert!(
            !forming_stalled(Some(12_000), Some(38_927)),
            "growing = delivering"
        );
        assert!(!forming_stalled(None, None), "no frame open");
        assert!(
            !forming_stalled(Some(2_433), None),
            "the frame closed (the call landed)"
        );
        assert!(!forming_stalled(None, Some(500)), "a frame just opened");
        assert!(
            !forming_stalled(Some(0), Some(0)),
            "an unparseable sidecar is not evidence"
        );
    }
}
