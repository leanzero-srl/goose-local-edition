//! THE SHADOW JUDGE DESK — r6 measurement infrastructure, zero behavior change.
//!
//! DESIGN-JUDGE-DESK.md (SOUND_WITH_AMENDMENTS) parks the full out-of-phase judge desk on r6's
//! qualifying measurement. THIS module is only the SHADOW: a standing background reader of the
//! durable lane artifacts (`<key>.think.log`, `<key>.log`, `<key>.calls.jsonl`, run.jsonl) that
//! runs the deterministic detectors for free and records what it WOULD have done. It judges
//! nothing, delivers nothing, touches no worker stream, makes no model call, and claims no fleet
//! node. Its whole output is its own event family, from which the r7 go/no-go A/B is computed
//! post-run (desk_summon timestamps paired against the in-loop judge_look/judge_nudge record:
//! did the desk see what the judge saw — earlier, later, or never — and vice versa).
//!
//! THE LAWS THIS MODULE LIVES UNDER, quoted so a reader does not have to trust memory:
//!
//! - "A CLOCK MAY SUMMON THE JUDGE. IT MAY NEVER CUT THE CALL." (the worker loop's JUDGE_WAKE
//!   law). The desk's poll clock (JUDGE_WAKE, reused — no new seconds literal) summons FILE
//!   READS only. Nothing time-based decides a verdict, bounds a call, or feeds a detector:
//!   look-eligibility and every detector floor are char/count floors reused from the in-loop
//!   judge (OMNI_JUDGE_MIN_CHARS, OMNI_JUDGE_GROWTH_CHARS, REPEAT_BREAK_N). The in-loop repeat
//!   breaker's REPEAT_BREAK_MIN_SECS half is deliberately NOT mirrored — a seconds floor may not
//!   enter desk eligibility (gate 5; refuter objection 8) — so the shadow can flag a repeat
//!   earlier than the in-loop detector would.
//!
//! - CHAR COUNTS CUT A UTF-8 FILE (refuter objection 5): `judge_restream` carries
//!   `abandoned_thinking_chars` — a CHAR count — while file reads advance in BYTES. The replay
//!   reconstructs the cut by cumulative char-walking the think.log text (attempt-marker lines
//!   stripped first; they are appended to the transcripts but were never fed to the live meter),
//!   holding back partial UTF-8 sequences and partial marker lines at read boundaries. The
//!   ≤400ms transcript flush lag (DIGEST_IO_CADENCE) means a cut can name chars not yet on
//!   disk; the cut is then applied when the text catches up, never guessed. A replay that cannot
//!   be validated — a cut landing behind what was already fed, a cut never reached before its
//!   attempt ended (producer write loss: `append_calls_jsonl` returns silently and
//!   `transcript_write_failed` fires only once per key), an unparseable marker, invalid UTF-8 —
//!   emits `desk_replay_unvalidated{lane, reason}` and every later shadow row for that lane
//!   carries `replay_validated: false`. Never silently wrong.
//!
//! - THE SILENCE HOLE IS A KNOWN BLIND SPOT (refuter objection 1): a silent lane appends
//!   nothing, so every detector here has nothing to read and the shadow reports nothing about
//!   its health. The full desk must solve that (the self-calibrating quiet-vs-recovered-gap
//!   idiom); the shadow only MARKS silence as data: `desk_silent{lane, polls_silent}` (emitted
//!   at power-of-two poll counts, count-based, no seconds threshold) so the post-run join
//!   against tick.py's own `lms ps` record can separate queued/forming lanes from dead sockets.
//!   The desk does not shell `lms ps` itself — that ground truth already lives in the operator
//!   instrument, and the join is one timestamp pairing.
//!
//! - EVENT NAMES ARE THE DESK'S OWN (refuter objection 7): desk_look / desk_summon /
//!   desk_replay_unvalidated / desk_silent / desk_read_failed / desk_poll_panicked. Zero
//!   pollution of the judge_* family, so every existing reconciliation and the r0-r6 ledger
//!   comparability survive untouched.
//!
//! Known shadow approximations, stated rather than discovered: the degenerate-answer check reads
//! a rolling 400-char tail of `<key>.log` (the in-loop reads the whole current stream's answer),
//! and `.log` is not cut at restream boundaries; the repeat detector reads `result_tail` clipped
//! to 2,000 chars where the in-loop compares 4,000-char clips; growth-without-acting measures
//! chars-since-last-observed-action at poll granularity where the in-loop measures
//! chars-since-last-look. Each degradation only affects when the SHADOW would summon — nothing
//! rides these values.

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use goose_swarm::EventSink;

use super::supervision::supervision_lane_kind;
use super::{
    tail_chars, JUDGE_WAKE, OMNI_JUDGE_GROWTH_CHARS, OMNI_JUDGE_MIN_CHARS, REPEAT_BREAK_N,
};

/// #F924 — a loop detector whose REACH is the whole call instead of its last 2,400 characters.
///
/// Every loop check the engine had read `last_thinking`, which the stream loop truncates to a
/// 2,400-char rolling tail. A repetition whose PERIOD exceeds that window was therefore invisible
/// to the omni-judge, to `tails_recur` and to the digest — structurally, not by tuning. MEASURED
/// live on the sb-7 qwen3.8 r2 straggler (`detail-api-server-api`): 8,205 contiguous characters
/// captured from the running call carried 47.7% duplicated 48-char shingles and 17 of its 59
/// sentences were verbatim repeats ("For the buckets endpoint, I'm iterating through all
/// payments…" twice), yet the judge's 2,000-char window scored 0.00 and its corroboration streak
/// never once reached 2 across 191,000 characters and two hours. Healthy `detail` calls max out
/// at 1,384 chars over 58 archived samples, so this one ran at 138x the worst healthy case with
/// nothing able to see it.
///
/// Keeps FINGERPRINTS, never the text: a bounded deque of 48-char shingle hashes plus their
/// counts, so the rate is available at any instant and a long call costs a few MB. Also keeps one
/// far-back TEXT snapshot, so the judge can be shown "then" beside "now" rather than being asked
/// to infer recurrence from a single window it cannot see past.
///
/// Moved here verbatim from swarm.rs under the incremental-split law: the shadow desk REPLAYS
/// this meter from the durable transcript, and the replay being the same type as the live meter
/// is what makes the r7 fidelity comparison (replayed rate/span vs the judge_look record) mean
/// anything. The worker loop keeps using it through `use desk::RecurrenceMeter`.
pub(crate) struct RecurrenceMeter {
    counts: std::collections::HashMap<u64, u32>,
    order: std::collections::VecDeque<u64>,
    carry: String,
    recent: String,
    mid: Option<String>,
    older: Option<String>,
    since_rotate: usize,
}

/// Shingle reach. 65,536 shingles ~= 65k characters of memory at ~3 MB per live call — 16x the
/// longest repetition period measured (~4,000 chars), and 27x the window that was blind to it.
const RECURRENCE_REACH: usize = 65_536;
/// Below this much observed reasoning the rate is noise: a call restating a structured prompt to
/// itself shares shingles with itself early on. 8,000 is the span the r2 pathology was measured
/// over, so the threshold below is calibrated on a directly comparable number.
pub(crate) const RECURRENCE_MIN_SPAN: usize = 8_000;
/// Duplicated-over-distinct shingle ratio that SUMMONS THE JUDGE. Never kills on its own — under
/// UNCAPPED the judge decides, and this only makes sure it is looking and knows what was measured.
/// The r2 pathology read 0.4766; a healthy advancing call reads ~0.00-0.05.
const RECURRENCE_TRIGGER: f32 = 0.25;

impl RecurrenceMeter {
    const WIN: usize = 48;

    pub(crate) fn new() -> Self {
        Self {
            counts: std::collections::HashMap::new(),
            order: std::collections::VecDeque::new(),
            carry: String::new(),
            recent: String::new(),
            mid: None,
            older: None,
            since_rotate: 0,
        }
    }

    pub(crate) fn reset(&mut self) {
        *self = Self::new();
    }

    pub(crate) fn push(&mut self, chunk: &str) {
        if chunk.is_empty() {
            return;
        }
        self.note_text(chunk);
        self.carry.push_str(chunk);
        let chars: Vec<char> = self.carry.chars().collect();
        if chars.len() < Self::WIN {
            return;
        }
        use std::hash::{Hash, Hasher};
        let mut i = 0;
        while i + Self::WIN <= chars.len() {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            chars[i..i + Self::WIN]
                .iter()
                .collect::<String>()
                .hash(&mut h);
            self.push_hash(h.finish());
            i += 1;
        }
        self.carry = chars[chars.len() - (Self::WIN - 1)..].iter().collect();
    }

    fn push_hash(&mut self, h: u64) {
        *self.counts.entry(h).or_insert(0) += 1;
        self.order.push_back(h);
        if self.order.len() > RECURRENCE_REACH {
            if let Some(old) = self.order.pop_front() {
                if let Some(c) = self.counts.get_mut(&old) {
                    *c -= 1;
                    if *c == 0 {
                        self.counts.remove(&old);
                    }
                }
            }
        }
    }

    /// Rolls a 1,200-char text snapshot so `earlier()` hands the judge reasoning from 20k-40k
    /// characters ago — far outside the tail window, which is the whole point.
    fn note_text(&mut self, chunk: &str) {
        self.recent.push_str(chunk);
        if self.recent.chars().count() > 1_600 {
            self.recent = tail_chars(&self.recent, 1_200);
        }
        self.since_rotate += chunk.chars().count();
        if self.since_rotate >= 10_000 {
            self.older = self.mid.take();
            self.mid = Some(self.recent.clone());
            self.since_rotate = 0;
        }
    }

    pub(crate) fn span(&self) -> usize {
        self.order.len()
    }

    /// Duplicated shingles over DISTINCT shingles — the same formula the live capture was scored
    /// with, so 0.4766 in the ledger and 0.4766 here mean the identical thing.
    pub(crate) fn rate(&self) -> f32 {
        let distinct = self.counts.len();
        if distinct == 0 {
            return 0.0;
        }
        (self.order.len().saturating_sub(distinct)) as f32 / distinct as f32
    }

    pub(crate) fn recurring(&self) -> bool {
        self.span() >= RECURRENCE_MIN_SPAN && self.rate() >= RECURRENCE_TRIGGER
    }

    /// The judge's out-of-tail span. TRACED on r4b (gate 8): right after a rotation `mid` is the
    /// chars just behind the live tail — showing it as "tens of thousands of characters ago"
    /// overlapped the tail by ~1,000 of its 1,200 chars and made the COMPARE instruction judge two
    /// near-identical windows BY CONSTRUCTION on any healthy call. `mid` is exposed only once the
    /// stream has moved >=8,000 chars past it; `older` (a full rotation further back) is always
    /// safe. Rotation is 10,000 so a 2-4k char/min stream gets its first honest span at ~minute
    /// 4-5 instead of ~7.
    pub(crate) fn earlier(&self) -> Option<&str> {
        let far = self.older.as_deref();
        if self.since_rotate >= 8_000 {
            far.or(self.mid.as_deref())
        } else {
            far
        }
    }

    /// Exact-identity fingerprint of the shingle sequence, for the replay tests: two meters that
    /// saw the same chars in any chunking agree on this to the char.
    #[cfg(test)]
    fn fingerprint(&self) -> (usize, usize, u64) {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for x in &self.order {
            x.hash(&mut h);
        }
        (self.order.len(), self.counts.len(), h.finish())
    }
}

/// Mirrors the worker loop's `acted_enough_to_judge` (call_records since stream start >= 6): a
/// call that ACTS rather than narrates clears the readiness floor on its actions. Same count, one
/// name, so a change there is findable from here by grep.
const ACTED_ENOUGH_ROWS: usize = 6;

/// The attempt-marker line prefix as `attempt_marker_line` writes it (leading newline included).
/// The desk strips these lines from the replay — they reach the transcripts but were never fed to
/// the live meter — and reads the attempt's `dispatched` timestamp off them to join
/// `judge_restream` cuts to the right attempt. The cross-check test below builds a real marker via
/// `super::attempt_marker_line`, so a format drift there fails here instead of mis-cutting silently.
const MARKER_START: &str = "\n===== swarm attempt ";
const MARKER_TAIL: &str = " =====";
const MARKER_DISPATCHED: &str = "· dispatched ";

fn parse_marker_line(line: &str) -> Option<chrono::DateTime<chrono::FixedOffset>> {
    let body = line
        .strip_prefix(MARKER_START.trim_start_matches('\n'))?
        .strip_suffix(MARKER_TAIL)?;
    let ts = body.split(MARKER_DISPATCHED).nth(1)?;
    chrono::DateTime::parse_from_rfc3339(ts.trim()).ok()
}

/// Longest suffix of `s` that is a proper prefix of MARKER_START — held back at feed time so a
/// marker split across two reads is never half-fed to the meter. MARKER_START is ASCII, so the
/// returned byte length always lands on a char boundary of `s`.
fn marker_holdback_len(s: &str) -> usize {
    let max = MARKER_START.len().saturating_sub(1).min(s.len());
    for k in (1..=max).rev() {
        if s.is_char_boundary(s.len() - k) && s.as_bytes().ends_with(&MARKER_START.as_bytes()[..k])
        {
            return k;
        }
    }
    0
}

/// Byte length of the first `n` chars of `s` (all of `s` if shorter).
fn chars_prefix_bytes(s: &str, n: usize) -> usize {
    s.char_indices().nth(n).map_or(s.len(), |(i, _)| i)
}

/// Append `new` to the undecoded carry and return the longest valid UTF-8 prefix, keeping an
/// incomplete trailing sequence for the next read. A genuinely INVALID sequence returns Err — the
/// caller invalidates the replay loudly instead of guessing a replacement char into the meter.
fn decode_utf8_append(carry: &mut Vec<u8>, new: &[u8]) -> Result<String, String> {
    carry.extend_from_slice(new);
    match std::str::from_utf8(carry) {
        Ok(s) => {
            let out = s.to_string();
            carry.clear();
            Ok(out)
        }
        Err(e) => {
            let valid = e.valid_up_to();
            if e.error_len().is_some() {
                return Err(format!("invalid utf-8 at byte {valid}"));
            }
            let out = std::str::from_utf8(&carry[..valid])
                .expect("valid_up_to prefix is valid")
                .to_string();
            carry.drain(..valid);
            Ok(out)
        }
    }
}

/// One deterministic detector's identity in desk_summon events and in the per-stream latch.
const DET_RECURRENCE: &str = "recurrence";
const DET_GROWTH: &str = "growth_without_acting";
const DET_DEGENERATE: &str = "degenerate_answer";
const DET_REPEAT: &str = "repeat_run";

/// Everything the desk knows about one lane, reconstructed purely from its durable files plus the
/// run.jsonl restream record. All counters are chars/counts — no clock state exists here.
struct LaneWatch {
    key: String,
    think_offset: u64,
    log_offset: u64,
    calls_offset: u64,
    think_utf8_carry: Vec<u8>,
    log_utf8_carry: Vec<u8>,
    calls_partial: Vec<u8>,
    /// Decoded think text not yet fed: holds back partial marker lines across reads.
    think_text: String,
    log_text: String,
    /// Rolling tail of the answer transcript (markers stripped), for the degenerate check.
    log_tail: String,
    recur: RecurrenceMeter,
    attempt_dispatched_at: Option<chrono::DateTime<chrono::FixedOffset>>,
    /// (restream ts, abandoned_thinking_chars) cuts not yet applied, oldest first.
    pending_cuts: VecDeque<(chrono::DateTime<chrono::FixedOffset>, usize)>,
    /// Chars fed since the last attempt marker (the in-loop `thinking_total` mirror — restreams
    /// do not reset it, so the readiness floor behaves identically).
    attempt_chars: usize,
    /// Chars fed since the last marker OR restream cut — the `thinking_chars` mirror the cut
    /// offsets count in.
    segment_chars: usize,
    /// Monotonic total across the lane's whole life, for look-eligibility bookkeeping.
    fed_total: usize,
    chars_at_last_look: usize,
    rows_total: usize,
    rows_this_stream: usize,
    chars_at_last_action: usize,
    repeat_run: usize,
    last_row_sig: Option<(String, String, String)>,
    /// Detectors already summoned for the current stream segment (edge-trigger latch).
    summoned: HashSet<&'static str>,
    /// Producer-side loss (`transcript_write_failed` for think.log, invalid UTF-8): the file can
    /// no longer be trusted to hold what the meter saw, for the rest of the run.
    sticky_invalid: Option<String>,
    /// Segment-scoped replay damage (mis-joined cut); cleared by the next attempt marker, which
    /// starts a genuinely fresh replay.
    segment_invalid: Option<String>,
    /// `transcript_write_failed` for `<key>.log`: the degenerate tail is frozen, stop reading it.
    log_frozen: bool,
    polls_silent: u32,
}

impl LaneWatch {
    fn new(key: String) -> Self {
        Self {
            key,
            think_offset: 0,
            log_offset: 0,
            calls_offset: 0,
            think_utf8_carry: Vec::new(),
            log_utf8_carry: Vec::new(),
            calls_partial: Vec::new(),
            think_text: String::new(),
            log_text: String::new(),
            log_tail: String::new(),
            recur: RecurrenceMeter::new(),
            attempt_dispatched_at: None,
            pending_cuts: VecDeque::new(),
            attempt_chars: 0,
            segment_chars: 0,
            fed_total: 0,
            chars_at_last_look: 0,
            rows_total: 0,
            rows_this_stream: 0,
            chars_at_last_action: 0,
            repeat_run: 0,
            last_row_sig: None,
            summoned: HashSet::new(),
            sticky_invalid: None,
            segment_invalid: None,
            log_frozen: false,
            polls_silent: 0,
        }
    }

    fn replay_validated(&self) -> bool {
        self.sticky_invalid.is_none() && self.segment_invalid.is_none()
    }

    fn invalidate_sticky(&mut self, reason: &str, sink: &dyn EventSink) {
        if self.sticky_invalid.is_none() {
            self.sticky_invalid = Some(reason.to_string());
            sink.write_value(serde_json::json!({
                "event": "desk_replay_unvalidated",
                "lane": self.key,
                "reason": reason,
            }));
        }
    }

    fn invalidate_segment(&mut self, reason: &str, sink: &dyn EventSink) {
        if self.replay_validated() {
            sink.write_value(serde_json::json!({
                "event": "desk_replay_unvalidated",
                "lane": self.key,
                "reason": reason,
            }));
        }
        if self.segment_invalid.is_none() {
            self.segment_invalid = Some(reason.to_string());
        }
    }

    fn note_restream_cut(
        &mut self,
        ts: chrono::DateTime<chrono::FixedOffset>,
        abandoned_chars: usize,
    ) {
        self.pending_cuts.push_back((ts, abandoned_chars));
    }

    fn ingest_think_bytes(&mut self, bytes: &[u8], sink: &dyn EventSink) {
        match decode_utf8_append(&mut self.think_utf8_carry, bytes) {
            Ok(text) => {
                self.think_text.push_str(&text);
                self.drain_think_text(sink);
            }
            Err(e) => self.invalidate_sticky(&format!("think.log {e}"), sink),
        }
    }

    fn drain_think_text(&mut self, sink: &dyn EventSink) {
        loop {
            if let Some(i) = self.think_text.find(MARKER_START) {
                // All boundaries here are byte offsets of ASCII patterns, so split_at cannot
                // land inside a multi-byte char (and split_at over indexing keeps the
                // string_slice lint honest about it).
                let line_start = i + 1;
                let (nl_rel, dispatched, looks_like_marker) = {
                    let after = self.think_text.split_at(line_start).1;
                    let Some(nl_rel) = after.find('\n') else {
                        // Marker line still streaming in. Do not feed the head yet: the marker's
                        // `dispatched` ts is the cutoff that keeps a future attempt's restream
                        // cut out of THIS segment, and it is not readable until the line
                        // completes.
                        return;
                    };
                    let line = after.split_at(nl_rel).0;
                    (nl_rel, parse_marker_line(line), line.ends_with(MARKER_TAIL))
                };
                let line_end = line_start + nl_rel;
                match dispatched {
                    Some(ts) => {
                        let head = self.think_text.split_at(i).0.to_string();
                        self.feed_segment_text(&head, Some(ts), sink);
                        self.reset_attempt(Some(ts), sink);
                        self.think_text.drain(..=line_end);
                    }
                    None => {
                        // Not a real marker (thinking that happens to contain the prefix, or a
                        // marker whose ts does not parse). Feed through it and keep scanning; if
                        // it WAS a malformed marker the cut join is now untrustworthy — say so.
                        let head = self.think_text.split_at(line_end + 1).0.to_string();
                        self.feed_segment_text(&head, None, sink);
                        self.think_text.drain(..=line_end);
                        if looks_like_marker {
                            self.invalidate_segment("unparseable_attempt_marker", sink);
                        }
                    }
                }
            } else {
                let hold = marker_holdback_len(&self.think_text);
                let feed_to = self.think_text.len() - hold;
                if feed_to > 0 {
                    let head = self.think_text.split_at(feed_to).0.to_string();
                    self.feed_segment_text(&head, None, sink);
                    self.think_text.drain(..feed_to);
                }
                return;
            }
        }
    }

    /// Feed decoded thinking text into the replay, applying restream cuts at their exact char
    /// offsets. `next_attempt` bounds which queued cuts may apply: a cut stamped at/after the next
    /// attempt's dispatch belongs to a later segment and stays queued (this is what makes a
    /// cold-start catch-up over a whole historical file land every cut in its own attempt).
    fn feed_segment_text(
        &mut self,
        text: &str,
        next_attempt: Option<chrono::DateTime<chrono::FixedOffset>>,
        sink: &dyn EventSink,
    ) {
        let mut rest = text;
        while !rest.is_empty() {
            let applicable = match self.pending_cuts.front().copied() {
                Some((ts, _)) if next_attempt.is_some_and(|na| ts >= na) => None,
                Some((ts, cut_at)) => match self.attempt_dispatched_at {
                    None => {
                        self.invalidate_segment("cut_without_attempt_marker", sink);
                        self.pending_cuts.pop_front();
                        continue;
                    }
                    Some(disp) if ts < disp => {
                        // Stale cut from an attempt already superseded; its segment is gone.
                        self.pending_cuts.pop_front();
                        continue;
                    }
                    Some(_) => Some(cut_at),
                },
                None => None,
            };
            match applicable {
                Some(cut_at) if self.segment_chars >= cut_at => {
                    // The event named a cut at chars we already fed: the meter carries
                    // pre-restream bytes in the post-restream segment.
                    self.invalidate_segment("cut_behind_replay", sink);
                    self.pending_cuts.pop_front();
                }
                Some(cut_at) => {
                    let need = cut_at - self.segment_chars;
                    let take = chars_prefix_bytes(rest, need);
                    let (fed, remainder) = rest.split_at(take);
                    self.push_chars(fed);
                    rest = remainder;
                    if self.segment_chars == cut_at {
                        self.apply_cut();
                        self.pending_cuts.pop_front();
                    }
                }
                None => {
                    self.push_chars(rest);
                    rest = "";
                }
            }
        }
    }

    fn push_chars(&mut self, s: &str) {
        if s.is_empty() {
            return;
        }
        self.recur.push(s);
        let n = s.chars().count();
        self.attempt_chars += n;
        self.segment_chars += n;
        self.fed_total += n;
    }

    fn apply_cut(&mut self) {
        self.recur.reset();
        self.segment_chars = 0;
        self.rows_this_stream = 0;
        self.chars_at_last_action = self.attempt_chars;
        self.summoned.clear();
    }

    fn reset_attempt(
        &mut self,
        dispatched: Option<chrono::DateTime<chrono::FixedOffset>>,
        sink: &dyn EventSink,
    ) {
        // A cut that belonged to the ending attempt but was never reached means the transcript
        // holds fewer chars than the engine counted — producer write loss (objection 5b).
        if let (Some(old), Some(new)) = (self.attempt_dispatched_at, dispatched) {
            if self
                .pending_cuts
                .iter()
                .any(|(ts, _)| *ts >= old && *ts < new)
            {
                self.invalidate_segment("cut_unapplied_at_attempt_end", sink);
                self.pending_cuts.retain(|(ts, _)| *ts >= new);
            }
        }
        self.recur.reset();
        self.attempt_dispatched_at = dispatched;
        self.attempt_chars = 0;
        self.segment_chars = 0;
        self.rows_this_stream = 0;
        self.chars_at_last_action = 0;
        self.repeat_run = 0;
        self.last_row_sig = None;
        self.summoned.clear();
        // A marker starts a genuinely fresh replay: segment damage does not carry over.
        // Producer-side loss (sticky) does — transcript_write_failed fires once per run.
        self.segment_invalid = None;
    }

    fn ingest_log_bytes(&mut self, bytes: &[u8], sink: &dyn EventSink) {
        if self.log_frozen {
            return;
        }
        match decode_utf8_append(&mut self.log_utf8_carry, bytes) {
            Ok(text) => {
                self.log_text.push_str(&text);
                self.drain_log_text();
            }
            Err(_) => {
                // The answer transcript is corrupt; the degenerate tail cannot be trusted. This
                // does not touch the think replay, so it freezes only this detector — loudly.
                self.log_frozen = true;
                sink.write_value(serde_json::json!({
                    "event": "desk_read_failed",
                    "lane": self.key,
                    "artifact": "log",
                    "error": "invalid utf-8 in answer transcript; degenerate detector frozen",
                }));
            }
        }
    }

    fn drain_log_text(&mut self) {
        loop {
            if let Some(i) = self.log_text.find(MARKER_START) {
                // split_at over indexing for the same boundary-honesty as drain_think_text.
                let line_start = i + 1;
                let (nl_rel, is_marker) = {
                    let after = self.log_text.split_at(line_start).1;
                    let Some(nl_rel) = after.find('\n') else {
                        return;
                    };
                    (
                        nl_rel,
                        parse_marker_line(after.split_at(nl_rel).0).is_some(),
                    )
                };
                let line_end = line_start + nl_rel;
                let (head, pseudo_line) = {
                    let (head, rest) = self.log_text.split_at(i);
                    (
                        head.to_string(),
                        rest.split_at(line_end + 1 - i).0.to_string(),
                    )
                };
                self.log_tail.push_str(&head);
                if is_marker {
                    self.log_tail.clear();
                } else {
                    self.log_tail.push_str(&pseudo_line);
                }
                self.log_text.drain(..=line_end);
            } else {
                let hold = marker_holdback_len(&self.log_text);
                let feed_to = self.log_text.len() - hold;
                if feed_to > 0 {
                    let head = self.log_text.split_at(feed_to).0.to_string();
                    self.log_tail.push_str(&head);
                    self.log_text.drain(..feed_to);
                }
                break;
            }
        }
        if self.log_tail.chars().count() > 400 {
            self.log_tail = tail_chars(&self.log_tail, 400);
        }
    }

    fn ingest_calls_bytes(&mut self, bytes: &[u8], sink: &dyn EventSink) {
        self.calls_partial.extend_from_slice(bytes);
        while let Some(nl) = self.calls_partial.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = self.calls_partial.drain(..=nl).collect();
            let line = &line[..line.len() - 1];
            if line.is_empty() {
                continue;
            }
            match serde_json::from_slice::<serde_json::Value>(line) {
                Ok(row) => {
                    let sig = (
                        row.get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        row.get("summary")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        row.get("result_tail")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                    );
                    self.rows_total += 1;
                    self.rows_this_stream += 1;
                    if self.last_row_sig.as_ref() == Some(&sig) {
                        self.repeat_run += 1;
                    } else {
                        self.repeat_run = 1;
                        self.last_row_sig = Some(sig);
                    }
                    self.chars_at_last_action = self.attempt_chars;
                }
                Err(e) => {
                    // A complete line that does not parse is a real defect in the record the
                    // repeat detector reads — named, never skipped quietly.
                    sink.write_value(serde_json::json!({
                        "event": "desk_read_failed",
                        "lane": self.key,
                        "artifact": "calls.jsonl",
                        "error": format!("unparseable row: {e}"),
                    }));
                }
            }
        }
    }

    fn degenerate_tail(&self) -> Option<String> {
        if self.log_frozen {
            return None;
        }
        let n = self.log_tail.chars().count();
        if n >= 400 && self.log_tail.trim().is_empty() {
            return Some(format!(
                "the last {n} answer chars are nothing but whitespace"
            ));
        }
        if n >= 200 {
            let last200 = tail_chars(&self.log_tail, 200);
            let distinct = last200.chars().collect::<HashSet<char>>().len();
            if distinct <= 3 {
                return Some(format!(
                    "the last 200 answer chars use only {distinct} distinct character(s): {:?}",
                    tail_chars(&last200, 24)
                ));
            }
        }
        None
    }

    /// One shadow assessment after this poll's ingests. Emits desk_summon on each detector's
    /// false→true edge (latched per stream segment, exactly as the live triggers latch), and a
    /// desk_look when the lane produced another OMNI_JUDGE_MIN_CHARS since the last look or any
    /// detector newly fired. Pure derivation over counters — the poll clock decided nothing here.
    fn assess(&mut self, sink: &dyn EventSink) {
        let recur_rate = self.recur.rate();
        let recur_span = self.recur.span();
        let growth_chars = self.attempt_chars - self.chars_at_last_action;
        let ready = self.attempt_chars >= OMNI_JUDGE_MIN_CHARS
            || self.rows_this_stream >= ACTED_ENOUGH_ROWS;
        let recurring = ready && self.recur.recurring();
        let grew = ready && growth_chars >= OMNI_JUDGE_GROWTH_CHARS;
        let degenerate = self.degenerate_tail();
        let repeat = self.repeat_run >= REPEAT_BREAK_N;
        let would_summon = recurring || grew || degenerate.is_some() || repeat;

        let mut newly_fired = false;
        let mut fire = |lane: &str,
                        summoned: &mut HashSet<&'static str>,
                        det: &'static str,
                        active: bool,
                        evidence: String| {
            if active && summoned.insert(det) {
                newly_fired = true;
                sink.write_value(serde_json::json!({
                    "event": "desk_summon",
                    "lane": lane,
                    "detector": det,
                    "evidence_head": tail_chars(&evidence, 300),
                }));
            }
        };
        fire(
            &self.key,
            &mut self.summoned,
            DET_RECURRENCE,
            recurring,
            format!(
                "replayed rate {recur_rate:.3} over {recur_span} shingles; live tail: {}",
                tail_chars(&self.recur.recent, 200)
            ),
        );
        fire(
            &self.key,
            &mut self.summoned,
            DET_GROWTH,
            grew,
            format!(
                "{growth_chars} reasoning chars since the last tool call ({} rows); tail: {}",
                self.rows_total,
                tail_chars(&self.recur.recent, 200)
            ),
        );
        fire(
            &self.key,
            &mut self.summoned,
            DET_DEGENERATE,
            degenerate.is_some(),
            degenerate.clone().unwrap_or_default(),
        );
        fire(
            &self.key,
            &mut self.summoned,
            DET_REPEAT,
            repeat,
            format!(
                "{} consecutive identical calls: {} {}",
                self.repeat_run,
                self.last_row_sig
                    .as_ref()
                    .map(|s| s.0.as_str())
                    .unwrap_or(""),
                self.last_row_sig
                    .as_ref()
                    .map(|s| s.1.as_str())
                    .unwrap_or(""),
            ),
        );

        let eligible =
            self.fed_total - self.chars_at_last_look >= OMNI_JUDGE_MIN_CHARS || newly_fired;
        if eligible {
            self.chars_at_last_look = self.fed_total;
            sink.write_value(serde_json::json!({
                "event": "desk_look",
                "lane": self.key,
                "offsets": {
                    "think_log": self.think_offset,
                    "log": self.log_offset,
                    "calls_jsonl": self.calls_offset,
                },
                "detectors": {
                    "recur_rate": recur_rate,
                    "span": recur_span,
                    "growth_chars": growth_chars,
                    "actions": self.rows_total,
                },
                "would_summon": would_summon,
                "replay_validated": self.replay_validated(),
            }));
        }
    }
}

/// The desk itself: lane discovery, incremental file reads, run.jsonl routing. Every fs error is
/// a named `desk_read_failed` (deduped per lane+artifact) — a desk failure never touches the run.
struct Desk {
    activity_dir: PathBuf,
    run_jsonl: PathBuf,
    run_offset: u64,
    run_partial: Vec<u8>,
    lanes: HashMap<String, LaneWatch>,
    finished: HashSet<String>,
    read_failures: HashSet<(String, String)>,
    sink: Arc<dyn EventSink>,
}

impl Desk {
    fn new(activity_dir: PathBuf, run_jsonl: PathBuf, sink: Arc<dyn EventSink>) -> Self {
        Self {
            activity_dir,
            run_jsonl,
            run_offset: 0,
            run_partial: Vec::new(),
            lanes: HashMap::new(),
            finished: HashSet::new(),
            read_failures: HashSet::new(),
            sink,
        }
    }

    fn note_read_failure(&mut self, lane: &str, artifact: &str, err: &str) {
        if self
            .read_failures
            .insert((lane.to_string(), artifact.to_string()))
        {
            self.sink.write_value(serde_json::json!({
                "event": "desk_read_failed",
                "lane": lane,
                "artifact": artifact,
                "error": err,
            }));
        }
    }

    /// Read the bytes appended to `path` since `offset`. A missing file is an honest empty — the
    /// lane simply has not created that artifact yet (workers create transcripts lazily), so it
    /// is Ok(None), distinct from an error. Shrinkage is an error: these files are append-only.
    fn read_appended(path: &PathBuf, offset: &mut u64) -> std::io::Result<Option<Vec<u8>>> {
        let meta = match std::fs::metadata(path) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        if meta.len() < *offset {
            return Err(std::io::Error::other(format!(
                "shrank from {} to {} bytes (append-only file)",
                offset,
                meta.len()
            )));
        }
        if meta.len() == *offset {
            return Ok(Some(Vec::new()));
        }
        let mut f = std::fs::File::open(path)?;
        f.seek(SeekFrom::Start(*offset))?;
        let mut buf = Vec::new();
        f.read_to_end(&mut buf)?;
        *offset += buf.len() as u64;
        Ok(Some(buf))
    }

    fn poll(&mut self) {
        self.ingest_run_events();
        self.discover_lanes();
        let keys: Vec<String> = self.lanes.keys().cloned().collect();
        for key in keys {
            self.poll_lane(&key);
        }
    }

    fn ingest_run_events(&mut self) {
        let bytes = match Self::read_appended(&self.run_jsonl.clone(), &mut self.run_offset) {
            Ok(Some(b)) => b,
            Ok(None) => return,
            Err(e) => {
                self.note_read_failure("", "run.jsonl", &e.to_string());
                return;
            }
        };
        self.run_partial.extend_from_slice(&bytes);
        while let Some(nl) = self.run_partial.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = self.run_partial.drain(..=nl).collect();
            self.route_run_line(&line[..line.len() - 1]);
        }
    }

    fn route_run_line(&mut self, line: &[u8]) {
        let Ok(v) = serde_json::from_slice::<serde_json::Value>(line) else {
            return;
        };
        match v.get("event").and_then(|e| e.as_str()) {
            Some("judge_restream") => {
                let Some(task) = v.get("task_id").and_then(|t| t.as_str()) else {
                    return;
                };
                let lane = self
                    .lanes
                    .entry(task.to_string())
                    .or_insert_with(|| LaneWatch::new(task.to_string()));
                let ts = v
                    .get("ts")
                    .and_then(|t| t.as_str())
                    .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok());
                let chars = v
                    .get("abandoned_thinking_chars")
                    .and_then(|c| c.as_u64())
                    .map(|c| c as usize);
                match (ts, chars) {
                    (Some(ts), Some(chars)) => lane.note_restream_cut(ts, chars),
                    _ => lane.invalidate_sticky("unparseable_restream_event", self.sink.as_ref()),
                }
            }
            Some("transcript_write_failed") => {
                let Some(key) = v.get("activity_key").and_then(|k| k.as_str()) else {
                    return;
                };
                let (task, which) = key.rsplit_once('/').unwrap_or((key, ""));
                if let Some(lane) = self.lanes.get_mut(task) {
                    if which == "think.log" {
                        lane.invalidate_sticky("transcript_write_failed", self.sink.as_ref());
                    } else if which == "log" {
                        lane.log_frozen = true;
                    }
                }
            }
            _ => {}
        }
    }

    fn discover_lanes(&mut self) {
        let entries = match std::fs::read_dir(&self.activity_dir) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
            Err(e) => {
                self.note_read_failure("", "activity_dir", &e.to_string());
                return;
            }
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            let key = if let Some(k) = name.strip_suffix(".think.log") {
                k
            } else if let Some(k) = name.strip_suffix(".calls.jsonl") {
                k
            } else if let Some(k) = name.strip_suffix(".json") {
                k
            } else if let Some(k) = name.strip_suffix(".log") {
                k
            } else {
                continue;
            };
            if key.is_empty() || self.finished.contains(key) || self.lanes.contains_key(key) {
                continue;
            }
            // SUPERVISION LANES ARE NOT WATCHED (r6e E15). The judge/ask
            // lanes write the same artifacts as workers, so the desk
            // discovered them and ran its detectors on the supervisors: r6d run.jsonl carried 62
            // desk_summon on `judge-research-*` keys ("[growth_without_acting] 4297 reasoning
            // chars since the last tool call") against 22 on workers, plus 236 desk_look / 160
            // desk_silent rows — and no judge look is ever dispatched on a judge-* key, so every
            // one was noise in the A/B this shadow exists to feed. The classifier is the one
            // `supervision_lane_kind` already keeps (no second name list).
            if supervision_lane_kind(key).is_some() {
                continue;
            }
            self.lanes
                .insert(key.to_string(), LaneWatch::new(key.to_string()));
        }
    }

    fn lane_file(&self, key: &str, ext: &str) -> PathBuf {
        // format! rather than with_extension: the engine writes these names as `<key>.<ext>` and
        // with_extension would eat anything after a dot inside the key.
        self.activity_dir.join(format!("{key}.{ext}"))
    }

    fn lane_phase_done(&self, key: &str) -> bool {
        let Ok(text) = std::fs::read_to_string(self.lane_file(key, "json")) else {
            return false;
        };
        serde_json::from_str::<serde_json::Value>(&text)
            .ok()
            .and_then(|d| d.get("phase").and_then(|p| p.as_str()).map(|p| p == "done"))
            .unwrap_or(false)
    }

    fn poll_lane(&mut self, key: &str) {
        let mut appended_any = false;
        for (ext, artifact) in [
            ("calls.jsonl", "calls.jsonl"),
            ("think.log", "think.log"),
            ("log", "log"),
        ] {
            let path = self.lane_file(key, ext);
            let read = {
                let lane = self.lanes.get_mut(key).expect("lane polled exists");
                let offset = match artifact {
                    "calls.jsonl" => &mut lane.calls_offset,
                    "think.log" => &mut lane.think_offset,
                    _ => &mut lane.log_offset,
                };
                Self::read_appended(&path, offset)
            };
            match read {
                Ok(Some(bytes)) if !bytes.is_empty() => {
                    appended_any = true;
                    let sink = self.sink.clone();
                    let lane = self.lanes.get_mut(key).expect("lane polled exists");
                    match artifact {
                        "calls.jsonl" => lane.ingest_calls_bytes(&bytes, sink.as_ref()),
                        "think.log" => lane.ingest_think_bytes(&bytes, sink.as_ref()),
                        _ => lane.ingest_log_bytes(&bytes, sink.as_ref()),
                    }
                }
                Ok(_) => {}
                Err(e) => self.note_read_failure(key, artifact, &e.to_string()),
            }
        }
        if appended_any {
            let sink = self.sink.clone();
            let lane = self.lanes.get_mut(key).expect("lane polled exists");
            lane.polls_silent = 0;
            lane.assess(sink.as_ref());
        } else {
            if self.lane_phase_done(key) {
                self.lanes.remove(key);
                self.finished.insert(key.to_string());
                return;
            }
            let sink = self.sink.clone();
            let lane = self.lanes.get_mut(key).expect("lane polled exists");
            lane.polls_silent = lane.polls_silent.saturating_add(1);
            // Power-of-two cadence keeps a long-quiet lane from flooding the record while still
            // marking silence as DATA (the shadow's named blind spot — see the module doc). A
            // count, not a clock: nothing here decides anything about the lane.
            if lane.polls_silent >= 2 && lane.polls_silent.is_power_of_two() {
                sink.write_value(serde_json::json!({
                    "event": "desk_silent",
                    "lane": lane.key,
                    "polls_silent": lane.polls_silent,
                }));
            }
        }
    }
}

/// Aborts the desk task on Drop — the HeartbeatGuard pattern, so the desk never outlives the run
/// on ANY exit path.
pub(crate) struct DeskGuard {
    stop: Arc<AtomicBool>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for DeskGuard {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.task.abort();
    }
}

/// Spawn the shadow desk for one run. Reads only; emits only desk_* events; a poll that panics
/// ends the DESK loudly (`desk_poll_panicked`) and never the engine.
pub(crate) fn spawn_shadow_desk(
    working_dir: PathBuf,
    run_jsonl: PathBuf,
    sink: Arc<dyn EventSink>,
) -> DeskGuard {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_task = stop.clone();
    let task = tokio::spawn(async move {
        let activity_dir = working_dir.join(".swarm").join("activity");
        let mut desk = Desk::new(activity_dir, run_jsonl, sink.clone());
        loop {
            if stop_task.load(Ordering::SeqCst) {
                break;
            }
            let poll = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| desk.poll()));
            if let Err(e) = poll {
                let msg = e
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_else(|| "non-string panic payload".to_string());
                sink.write_value(serde_json::json!({
                    "event": "desk_poll_panicked",
                    "error": msg,
                }));
                break;
            }
            tokio::time::sleep(JUDGE_WAKE).await;
        }
    });
    DeskGuard { stop, task }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct RecordingSink(Mutex<Vec<serde_json::Value>>);
    impl RecordingSink {
        fn new() -> Self {
            Self(Mutex::new(Vec::new()))
        }
        fn events(&self, name: &str) -> Vec<serde_json::Value> {
            self.0
                .lock()
                .unwrap()
                .iter()
                .filter(|v| v.get("event").and_then(|e| e.as_str()) == Some(name))
                .cloned()
                .collect()
        }
    }
    impl EventSink for RecordingSink {
        fn emit(&self, _event: &goose_swarm::SwarmEvent) {}
        fn write_value(&self, value: serde_json::Value) {
            self.0.lock().unwrap().push(value);
        }
    }

    /// Mirrors `attempt_marker_line`'s format byte for byte. Deliberately NOT a call into the
    /// producer: at the time of writing that function is mid-move into the transcripts sibling
    /// module. The desk does not fail silently on a format drift anyway — a marker that stops
    /// parsing hits the `unparseable_attempt_marker` arm and every affected lane's shadow rows
    /// go out with replay_validated:false, which is the loud failure the tests below pin.
    fn marker(attempt: u32, ts: &str) -> String {
        format!("\n===== swarm attempt {attempt} · dispatched {ts} =====\n")
    }

    fn ts(s: &str) -> chrono::DateTime<chrono::FixedOffset> {
        chrono::DateTime::parse_from_rfc3339(s).unwrap()
    }

    /// The replay reconstruction, validated to the char: a live meter fed token-sized chunks with
    /// an attempt marker written to the transcript (never fed) and a mid-attempt restream (meter
    /// reset, no in-band marker — only the run.jsonl cut with its CHAR count), against the desk
    /// replaying the same file in awkward 7-byte reads across multi-byte UTF-8 and a split marker.
    #[test]
    fn replay_reconstructs_the_live_meter_to_the_char() {
        let stream_a: Vec<&str> = vec![
            "Считаю панель — the notifier design keeps returning to the same ",
            "envelope shape, field by field, and I will restate it once more. ",
            "The ledgerd row carries идентификатор, amount, and status. ",
        ];
        let stream_b: Vec<&str> = vec![
            "Fresh attempt after the restream: write app/__main__.py now, ",
            "then verify boot with python -m app and stop deliberating. ",
        ];

        let mut live = RecurrenceMeter::new();
        for c in &stream_a {
            live.push(c);
        }
        let abandoned: usize = stream_a.iter().map(|c| c.chars().count()).sum();
        live.reset(); // the worker loop's restream reset — no marker reaches the file
        for c in &stream_b {
            live.push(c);
        }

        let mut file = String::new();
        file.push_str(&marker(0, "2026-08-30T10:00:00+00:00"));
        for c in stream_a.iter().chain(stream_b.iter()) {
            file.push_str(c);
        }

        let sink = RecordingSink::new();
        let mut lane = LaneWatch::new("skeleton".into());
        lane.note_restream_cut(ts("2026-08-30T10:05:00+00:00"), abandoned);
        for chunk in file.as_bytes().chunks(7) {
            lane.ingest_think_bytes(chunk, &sink);
        }

        assert_eq!(
            lane.recur.fingerprint(),
            live.fingerprint(),
            "replayed shingle sequence must equal the live meter's exactly"
        );
        assert_eq!(
            lane.segment_chars,
            stream_b.iter().map(|c| c.chars().count()).sum::<usize>()
        );
        assert!(lane.replay_validated());
        assert!(sink.events("desk_replay_unvalidated").is_empty());
    }

    #[test]
    fn attempt_marker_lines_are_stripped_never_fed() {
        let text_a = "attempt zero reasoning that ends mid-sentence because the node dropped ";
        let text_b = "attempt one starts clean and reasons about the same files again here. ";

        let mut live = RecurrenceMeter::new();
        live.push(text_a);
        live.reset(); // new attempt = new meter in a fresh run_agent call
        live.push(text_b);

        let file = format!(
            "{}{}{}{}",
            marker(0, "2026-08-30T09:00:00+00:00"),
            text_a,
            marker(1, "2026-08-30T09:10:00+00:00"),
            text_b
        );
        let sink = RecordingSink::new();
        let mut lane = LaneWatch::new("boot".into());
        for chunk in file.as_bytes().chunks(11) {
            lane.ingest_think_bytes(chunk, &sink);
        }
        assert_eq!(lane.recur.fingerprint(), live.fingerprint());
        assert_eq!(lane.attempt_chars, text_b.chars().count());
        assert!(lane.replay_validated());
    }

    #[test]
    fn a_cut_behind_the_replay_is_named_never_silent() {
        let sink = RecordingSink::new();
        let mut lane = LaneWatch::new("late".into());
        let file = format!(
            "{}{}",
            marker(0, "2026-08-30T09:00:00+00:00"),
            "x".repeat(500)
        );
        lane.ingest_think_bytes(file.as_bytes(), &sink);
        // The restream event arrives AFTER its chars were already fed (the one poll-ordering race
        // the module doc admits): the desk must say so, not shrug.
        lane.note_restream_cut(ts("2026-08-30T09:01:00+00:00"), 200);
        lane.ingest_think_bytes("more".as_bytes(), &sink);
        assert!(!lane.replay_validated());
        let ev = sink.events("desk_replay_unvalidated");
        assert_eq!(ev.len(), 1);
        assert_eq!(ev[0]["reason"], "cut_behind_replay");
        // ...and the shadow rows carry the flag from here on.
        lane.ingest_think_bytes("y".repeat(3000).as_bytes(), &sink);
        lane.assess(&sink);
        let looks = sink.events("desk_look");
        assert_eq!(looks.len(), 1);
        assert_eq!(looks[0]["replay_validated"], false);
    }

    #[test]
    fn a_cut_stamped_after_the_next_attempt_stays_out_of_the_dead_segment() {
        // Cold-start catch-up: the whole history arrives in one read, with a queued cut that
        // belongs to attempt 1. Attempt 0's text must NOT be cut by it.
        let a0 = "a".repeat(300);
        let a1_pre = "b".repeat(120);
        let a1_post = "c".repeat(80);
        let file = format!(
            "{}{}{}{}{}",
            marker(0, "2026-08-30T09:00:00+00:00"),
            a0,
            marker(1, "2026-08-30T09:20:00+00:00"),
            a1_pre,
            a1_post
        );
        let sink = RecordingSink::new();
        let mut lane = LaneWatch::new("catchup".into());
        lane.note_restream_cut(ts("2026-08-30T09:25:00+00:00"), 120);
        lane.ingest_think_bytes(file.as_bytes(), &sink);
        assert!(lane.replay_validated(), "{:?}", lane.segment_invalid);
        assert_eq!(
            lane.segment_chars, 80,
            "cut must land inside attempt 1 only"
        );
        assert_eq!(lane.attempt_chars, 200);
    }

    #[test]
    fn growth_fires_at_the_reused_floor_and_an_action_resets_it() {
        let sink = RecordingSink::new();
        let mut lane = LaneWatch::new("g".into());
        lane.ingest_think_bytes(marker(0, "2026-08-30T09:00:00+00:00").as_bytes(), &sink);
        lane.ingest_think_bytes("z".repeat(OMNI_JUDGE_GROWTH_CHARS - 1).as_bytes(), &sink);
        lane.assess(&sink);
        assert!(
            sink.events("desk_summon").is_empty(),
            "one char under the floor"
        );
        lane.ingest_think_bytes("z".as_bytes(), &sink);
        lane.assess(&sink);
        let summons = sink.events("desk_summon");
        assert_eq!(summons.len(), 1);
        assert_eq!(summons[0]["detector"], DET_GROWTH);
        // Latched: assessing again does not re-summon.
        lane.assess(&sink);
        assert_eq!(sink.events("desk_summon").len(), 1);
        // An action moves the baseline.
        lane.ingest_calls_bytes(
            b"{\"name\":\"shell\",\"summary\":\"ls\",\"ok\":true,\"result_tail\":\"ok\"}\n",
            &sink,
        );
        assert_eq!(lane.chars_at_last_action, lane.attempt_chars);
    }

    #[test]
    fn repeat_fires_at_repeat_break_n_identical_rows() {
        let sink = RecordingSink::new();
        let mut lane = LaneWatch::new("r".into());
        let row = b"{\"name\":\"shell\",\"summary\":\"curl :8850\",\"ok\":true,\"result_tail\":\"same\"}\n";
        for _ in 0..REPEAT_BREAK_N - 1 {
            lane.ingest_calls_bytes(row, &sink);
        }
        lane.assess(&sink);
        assert!(sink.events("desk_summon").is_empty());
        lane.ingest_calls_bytes(row, &sink);
        lane.assess(&sink);
        let summons = sink.events("desk_summon");
        assert_eq!(summons.len(), 1);
        assert_eq!(summons[0]["detector"], DET_REPEAT);
        // No seconds floor here on purpose (module doc): the desk may flag earlier than in-loop.
    }

    #[test]
    fn degenerate_answer_bypasses_the_readiness_floor() {
        let sink = RecordingSink::new();
        let mut lane = LaneWatch::new("d".into());
        // Zero reasoning chars: readiness floor NOT met — degenerate must still summon, exactly
        // as the in-loop trigger lets degenerate_answer bypass the char floor.
        lane.ingest_log_bytes(" \n\t".repeat(140).as_bytes(), &sink);
        lane.assess(&sink);
        let summons = sink.events("desk_summon");
        assert_eq!(summons.len(), 1);
        assert_eq!(summons[0]["detector"], DET_DEGENERATE);
        let looks = sink.events("desk_look");
        assert_eq!(looks.len(), 1, "a new summon forces its look row");
        assert_eq!(looks[0]["would_summon"], true);
    }

    #[test]
    fn recurrence_respects_the_readiness_floor_and_the_meter_thresholds() {
        let sink = RecordingSink::new();
        let mut lane = LaneWatch::new("rec".into());
        lane.ingest_think_bytes(marker(0, "2026-08-30T09:00:00+00:00").as_bytes(), &sink);
        // A verbatim cycle long past RECURRENCE_MIN_SPAN: recurring() goes true, floor is met.
        let period = "The webhook envelope carries id, kind, payment, and signature fields. ";
        let looped = period.repeat(200);
        assert!(looped.chars().count() > RECURRENCE_MIN_SPAN);
        lane.ingest_think_bytes(looped.as_bytes(), &sink);
        lane.assess(&sink);
        let summons = sink.events("desk_summon");
        assert!(
            summons.iter().any(|s| s["detector"] == DET_RECURRENCE),
            "a repeated period past the span floor must summon: rate {:.3} span {}",
            lane.recur.rate(),
            lane.recur.span()
        );
        let looks = sink.events("desk_look");
        assert_eq!(looks.last().unwrap()["would_summon"], true);
    }

    #[test]
    fn a_quiet_lane_below_min_chars_emits_no_look() {
        let sink = RecordingSink::new();
        let mut lane = LaneWatch::new("q".into());
        lane.ingest_think_bytes(marker(0, "2026-08-30T09:00:00+00:00").as_bytes(), &sink);
        lane.ingest_think_bytes("short healthy reasoning".as_bytes(), &sink);
        lane.assess(&sink);
        assert!(sink.events("desk_look").is_empty());
        assert!(sink.events("desk_summon").is_empty());
    }

    #[test]
    fn marker_matcher_tracks_the_real_producer_format() {
        let line = marker(3, "2026-08-30T11:05:51.158283+00:00");
        let inner = line.trim_matches('\n');
        let parsed = parse_marker_line(inner).expect("real marker parses");
        assert_eq!(
            parsed,
            ts("2026-08-30T11:05:51.158283+00:00"),
            "dispatched ts read off the marker joins restream cuts to attempts"
        );
        assert!(parse_marker_line("===== swarm attempt x · dispatched nonsense =====").is_none());
    }

    #[test]
    fn utf8_split_across_reads_never_corrupts_and_invalid_bytes_are_loud() {
        let mut carry = Vec::new();
        let s = "панель";
        let bytes = s.as_bytes();
        let first = decode_utf8_append(&mut carry, &bytes[..3]).unwrap();
        let second = decode_utf8_append(&mut carry, &bytes[3..]).unwrap();
        assert_eq!(format!("{first}{second}"), s);
        let mut carry2 = Vec::new();
        assert!(decode_utf8_append(&mut carry2, &[0xff, 0xfe]).is_err());
    }

    /// r6e E15 (r6d: 62 of 84 desk_summon named `judge-research-*` keys): a supervision lane's
    /// artifacts — here a judge lane whose think.log has grown past the growth-without-acting
    /// trip with zero tool calls — never become a desk lane, so no desk_look and no desk_summon
    /// can carry its key; the worker lane beside it is discovered and watched as before.
    #[test]
    fn a_supervision_lanes_artifacts_never_produce_a_desk_summon() {
        let dir = tempfile::tempdir().unwrap();
        let activity = dir.path().join("activity");
        std::fs::create_dir_all(&activity).unwrap();
        let big = format!(
            "{}{}",
            marker(0, "2026-09-01T04:47:59+00:00"),
            "judge reasoning that never acts ".repeat(400)
        );
        for key in [
            "judge-research-ledger-core-q5",
            "ask-answer",
            "judge-web-viz",
        ] {
            std::fs::write(activity.join(format!("{key}.think.log")), &big).unwrap();
            std::fs::write(activity.join(format!("{key}.json")), "{}").unwrap();
        }
        std::fs::write(activity.join("research-ledger-core-q5.think.log"), &big).unwrap();
        let sink = Arc::new(RecordingSink::new());
        let sink_dyn: Arc<dyn EventSink> = sink.clone();
        let mut desk = Desk::new(activity, dir.path().join("run.jsonl"), sink_dyn);
        desk.poll();
        desk.poll();
        let keys: Vec<&String> = desk.lanes.keys().collect();
        assert_eq!(keys, vec!["research-ledger-core-q5"], "{keys:?}");
        for ev in ["desk_summon", "desk_look", "desk_silent"] {
            for e in sink.events(ev) {
                let lane = e["lane"].as_str().unwrap_or("");
                assert!(
                    supervision_lane_kind(lane).is_none(),
                    "{ev} on a supervision lane: {e}"
                );
            }
        }
    }
}
