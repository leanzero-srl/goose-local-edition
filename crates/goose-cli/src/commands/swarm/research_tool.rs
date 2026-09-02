//! The per-answer research landing (VA-118 item 4, wired r6j): a slice lane's `research_answer`
//! tool call lands ONE settled question as one ledger mini the moment the lane calls it —
//! persisted, emitted through the one outcome funnel, relayed to the sibling lanes, and KEPT on
//! the landing so the fan returns it to synthesis beside the final reply's remainder — instead
//! of every answer arriving in one final_output after an hour at 0 bytes (r6i's
//! research-web-console-structure lane: output frame empty for 63 minutes, 113,720 reasoning
//! chars, nine answers at once). The tool is a goose FRONTEND extension: the agent yields
//! `MessageContent::FrontendToolRequest` and parks the lane on its result channel until
//! `Agent::handle_tool_result` answers, so every arm here REPLIES — a request left unanswered
//! would park the lane forever. Nothing here bounds or ends anything: a call that cannot be
//! folded is a named stray and an error reply the lane can act on, never a stop.
//!
//! VA-128 (r6j, read from the words): the landing also holds the lane's HAND — its claimed
//! sections, dealt ONE per turn. The dispatch text carries section 1; every `research_answer`
//! call with `section_done: true` closes the section in hand (`research_section_settled`) and
//! its RESULT carries the next section's full text (`research_section_handed`), so a stateless
//! model never holds nine sections and re-sorts them across turns (r6j's api lane: 182k
//! reasoning chars, 79 minutes, nothing landed). The final reply folds only the remainder.
//!
//! VA-132 (r6j tick 10): every call is also an ACTION in the lane's own record. The worker loop
//! appends shell/tool calls to `<key>.calls.jsonl` from `call_records`, but a frontend tool is
//! answered by the engine right here and never reaches that list — so r6j's core lane landed
//! twelve answers (17:25:49Z–17:36:35Z) its calls record never showed: the desk read the
//! reasoning between landings as `growth_without_acting`, and a reader of the lane's record
//! could not see WHEN it landed. `ResearchCallRecord` appends one row per call through the
//! shell rows' writer, in their shape, so the desk counts it and the reader sees the landing.

use std::path::{Path, PathBuf};

use goose::agents::ExtensionConfig;
use goose::conversation::message::FrontendToolRequest;
use rmcp::model::{CallToolResult, Content, ErrorCode, ErrorData, Tool};

use super::research::{
    emit_research_outcome, emit_section_handed, persist_research_row, relay_note, relay_targets,
    research_answer_tool_schema, research_mini_name, section_in_hand_block, HandedSection,
    RelayTarget, ResearchLane, ResearchRow, ResearchToolCall, StrayAnswer, RESEARCH_ANSWER_TOOL,
};
use super::transcripts::{append_calls_row, AppendErrs};
use super::{activity_digest_key, tail_chars, EventSink, GooseAgentDispatcher};

/// One research lane's landing state for the life of its call, keyed by the lane's activity key
/// in `GooseAgentDispatcher::research_landing`: the slice the rows belong to, the model that
/// answers (row attribution), the call's start (`secs` on each row is the lane's elapsed at the
/// landing), the q_index the next landed entry takes and the rows landed so far. The fan takes
/// both back after the call (`close`): the count numbers the final reply's remainder
/// (`fold_research_lane_from`) and the rows seed the lane's returned rows — a mini on disk is
/// re-read only at resume, so a row that lived on disk alone never reached synthesis (the review
/// of the first wiring: the better a lane obeyed the tool, the blinder the plan). The index
/// advances only when a row lands: a stray never burns a number.
#[derive(Debug)]
pub(super) struct ResearchLanding {
    slice: String,
    model: String,
    started: std::time::Instant,
    next_q_index: usize,
    landed: Vec<ResearchRow>,
    /// VA-128: the lane's claimed sections and the cursor of the one in hand — `in_hand ==
    /// hand.len()` once every section has been dealt; `landed_in_section` counts the question
    /// rows landed since the section in hand was dealt (the `research_section_settled` fact).
    hand: Vec<HandedSection>,
    in_hand: usize,
    landed_in_section: usize,
}

/// What one `research_answer` call did to the hand (VA-128) — the reply the lane reads next is
/// rendered from it (`hand_reply_text`), and the tests read it as data.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum HandReply {
    /// No `section_done` on the call: the section at `index` (1-based) stays in hand.
    InHand {
        index: usize,
        of: usize,
        heading: String,
    },
    /// The call closed the section in hand and the next one is dealt — `block` is its text.
    Handed {
        closed: String,
        closed_landed: usize,
        index: usize,
        of: usize,
        block: String,
    },
    /// The call closed the LAST section: nothing remains to hand.
    LastClosed {
        closed: String,
        closed_landed: usize,
        of: usize,
    },
    /// Nothing is in hand — every section was dealt already, or (`of` 0) the slice has none.
    Exhausted { of: usize },
}

pub(super) struct Landed {
    pub(super) row: Option<ResearchRow>,
    pub(super) hand: HandReply,
}

impl ResearchLanding {
    pub(super) fn open(lane: &ResearchLane, model: &str) -> Self {
        Self {
            slice: lane.slice.clone(),
            model: model.to_string(),
            started: std::time::Instant::now(),
            next_q_index: 0,
            landed: Vec::new(),
            hand: lane.hand.clone(),
            in_hand: 0,
            landed_in_section: 0,
        }
    }

    /// ONE tool call → the row it lands NOW, if it lands one (`ResearchToolCall::into_row`):
    /// folded at the next q_index, persisted (`persist_research_row`), emitted through the one
    /// outcome funnel (`emit_research_outcome`) plus `research_answer_landed{task, slice,
    /// q_index, kind, status, chars, raised, via: tool}` so the vigil sees answers arrive
    /// mid-lane, and kept for `close`. Then the HAND (VA-128): a `section_done` call closes the
    /// section in hand — `research_section_settled{heading, index, of, landed, remaining}`, the
    /// loud record of a section with nothing to settle — and deals the next
    /// (`research_section_handed`, `section_in_hand_block`), or says nothing remains; a
    /// section_done past the end is `research_section_done_past_end`. A stray (an answer with
    /// no question) lands NO row and is the caller's to name; the index advances only on a row.
    fn land(
        &mut self,
        root: &Path,
        events: &dyn EventSink,
        key: &str,
        call: ResearchToolCall,
    ) -> Result<Landed, StrayAnswer> {
        let section_done = call.section_done();
        let row = call.into_row(
            &self.slice,
            self.next_q_index,
            &self.model,
            self.started.elapsed().as_secs(),
            self.hand.get(self.in_hand),
        )?;
        if let Some(row) = &row {
            self.next_q_index += 1;
            if !row.question.is_empty() {
                self.landed_in_section += 1;
            }
            persist_research_row(root, events, row);
            emit_research_outcome(events, row);
            events.write_value(serde_json::json!({
                "event": "research_answer_landed",
                "task": key,
                "slice": row.slice,
                "q_index": row.q_index,
                "kind": row.kind,
                "status": row.status,
                "chars": row.answer.chars().count(),
                "raised": row.raised.len(),
                "via": "tool",
            }));
            self.landed.push(row.clone());
        }
        let of = self.hand.len();
        let hand = if !section_done {
            match self.hand.get(self.in_hand) {
                Some(sec) => HandReply::InHand {
                    index: self.in_hand + 1,
                    of,
                    heading: sec.heading.clone(),
                },
                None => HandReply::Exhausted { of },
            }
        } else if let Some(closed) = self.hand.get(self.in_hand) {
            let closed_landed = self.landed_in_section;
            events.write_value(serde_json::json!({
                "event": "research_section_settled",
                "task": key,
                "slice": self.slice,
                "heading": closed.heading,
                "index": self.in_hand + 1,
                "of": of,
                "landed": closed_landed,
                "remaining": of - self.in_hand - 1,
            }));
            let closed = closed.heading.clone();
            self.in_hand += 1;
            self.landed_in_section = 0;
            if self.in_hand < of {
                emit_section_handed(events, key, &self.slice, &self.hand, self.in_hand);
                HandReply::Handed {
                    closed,
                    closed_landed,
                    index: self.in_hand + 1,
                    of,
                    block: section_in_hand_block(&self.hand, self.in_hand),
                }
            } else {
                HandReply::LastClosed {
                    closed,
                    closed_landed,
                    of,
                }
            }
        } else {
            events.write_value(serde_json::json!({
                "event": "research_section_done_past_end",
                "task": key,
                "slice": self.slice,
                "of": of,
            }));
            HandReply::Exhausted { of }
        };
        Ok(Landed { row, hand })
    }

    /// What the fan takes back after the call: the next q_index (the remainder's first) and the
    /// landed rows, in q_index order, already persisted, emitted and relayed.
    pub(super) fn close(self) -> (usize, Vec<ResearchRow>) {
        (self.next_q_index, self.landed)
    }
}

/// The extension the tool rides under. Frontend tools are keyed by their bare tool name in the
/// agent (`rebuild_frontend_derived_state`), so the model calls `research_answer`, never a
/// prefixed form.
const RESEARCH_ANSWER_EXTENSION: &str = "swarm_research";

/// The description the model reads beside the schema — what the call does and when to make it.
const RESEARCH_ANSWER_DESCRIPTION: &str = "Land ONE settled research question for this slice in \
     the swarm ledger, now: the question, its kind (design | external), the cite, the \
     alternatives and open_because of a design question, the answer, and any raised / \
     raised_for points. The entry is written the moment this returns and the other slices' \
     lanes can read it; the reply names the file it landed in. Call it once per question, as \
     you settle each one. When the section in hand has nothing (more) to settle, call it with \
     {\"section_done\": true} — and \"builder_decides\": [...] for the choices only this \
     slice's builder makes — and the reply carries the NEXT section's text; an entry may carry \
     section_done itself when it is the section's last. Your final_output then carries only \
     the entries you did not land here.";

/// The tool's extension — one tool, its argument exactly one derived entry
/// (`research_answer_tool_schema`, the same item schema the final reply's `answers` uses, so
/// the two landing doors cannot drift).
pub(super) fn research_answer_extension() -> ExtensionConfig {
    let input_schema = research_answer_tool_schema()
        .as_object()
        .cloned()
        .expect("research_answer_entry_schema is an object literal");
    ExtensionConfig::Frontend {
        name: RESEARCH_ANSWER_EXTENSION.to_string(),
        description: "lands one settled research question in the swarm ledger at once".to_string(),
        tools: vec![Tool::new(
            RESEARCH_ANSWER_TOOL.to_string(),
            RESEARCH_ANSWER_DESCRIPTION.to_string(),
            input_schema,
        )],
        instructions: Some(format!(
            "`{RESEARCH_ANSWER_TOOL}` lands one settled question in the ledger the moment you \
             call it; `{RESEARCH_ANSWER_TOOL}` with {{\"section_done\": true}} closes the \
             section in hand and its reply deals the next; final_output carries only what you \
             did not land through it."
        )),
        bundled: Some(true),
        available_tools: Vec::new(),
    }
}

/// The hand's part of the tool's reply (VA-128): the section that stays in hand and how to close
/// it; the closed section's count and the NEXT section's full text; or that nothing remains and
/// final_output follows. The lane's next turn is formed from THIS text — the harness forms the
/// message, the model lands one section at a time.
pub(super) fn hand_reply_text(hand: &HandReply) -> String {
    match hand {
        HandReply::InHand { index, of, heading } => format!(
            "; §{index} of {of} `{heading}` stays in hand — land its next question, or call \
             {RESEARCH_ANSWER_TOOL} with {{\"section_done\": true}} (\"builder_decides\": [...] \
             for the choices only this slice's builder makes) when nothing in it remains"
        ),
        HandReply::Handed {
            closed,
            closed_landed,
            index,
            of: _,
            block,
        } => format!(
            ".\n\n§{} `{closed}` closed ({closed_landed} landed). {block}",
            index - 1
        ),
        HandReply::LastClosed {
            closed,
            closed_landed,
            of,
        } => format!(
            ".\n\n§{of} `{closed}` closed ({closed_landed} landed). Every section ({of} of {of}) \
             has been handed and closed: call final_output now with {{\"answers\": [<only the \
             entries you did NOT land here>], \"builder_decides\": [...]}} — an empty answers \
             list is a complete reply."
        ),
        HandReply::Exhausted { of: 0 } => "; no section of the request is in hand for this \
             slice (it claimed none) — the request file under SOURCES holds every section; \
             final_output carries only the entries you did not land here"
            .to_string(),
        HandReply::Exhausted { of } => format!(
            "; every section ({of} of {of}) was already handed and closed — nothing more to \
             hand; call final_output with only the entries you did not land here"
        ),
    }
}

/// The tool's reply after a folded call: what landed (the mini's name, status, kind and the next
/// index — or that the call carried no entry), then the hand's part (`hand_reply_text`). One
/// function because the same text is the lane's next turn AND the `result_tail` of its calls row.
pub(super) fn landed_reply_text(landed: &Landed) -> String {
    let mut reply = match &landed.row {
        Some(row) if row.question.is_empty() => format!(
            "landed {}: {} builder_decides on an outcome row; q{} is the next index",
            research_mini_name(&row.slice, row.q_index),
            row.raised.len(),
            row.q_index + 1,
        ),
        Some(row) => format!(
            "landed {} ({}, kind {}); q{} is the next question you settle",
            research_mini_name(&row.slice, row.q_index),
            row.status,
            row.kind,
            row.q_index + 1,
        ),
        None => "nothing landed (no entry on this call)".to_string(),
    };
    reply.push_str(&hand_reply_text(&landed.hand));
    reply
}

/// The tool's error reply for a stray: what the call must carry, and what this one carried.
pub(super) fn stray_reply_text(stray: &StrayAnswer) -> String {
    format!(
        "nothing landed: {RESEARCH_ANSWER_TOOL} takes ONE JSON object with `question` (the \
         question text), `kind` (design | external) and `answer` — or {{\"section_done\": \
         true}} to close the section in hand; this call carried: {}",
        stray.answer_head
    )
}

/// The `outcome` a calls row names (VA-132): what ONE `research_answer` call did to the ledger
/// and the hand. `landed` is the only outcome with a `q_index`.
pub(super) const CALL_LANDED: &str = "landed";
/// A bare `section_done` that dealt the next section or closed the last.
pub(super) const CALL_SECTION_CLOSED: &str = "section_closed";
/// `section_done` with nothing in hand (`research_section_done_past_end`).
pub(super) const CALL_PAST_END: &str = "past_end";
/// A call that carried neither an entry nor the section signal.
pub(super) const CALL_EMPTY: &str = "empty";
/// An answer with no question, or nothing parseable (`research_batch_stray_answer{via: tool}`).
pub(super) const CALL_STRAY: &str = "stray";
/// No landing open for the key (`research_answer_unopened`).
pub(super) const CALL_UNOPENED: &str = "unopened";

/// One `research_answer` call as the lane's own `<key>.calls.jsonl` records it (VA-132). The
/// row rides the shell rows' shape (`append_calls_jsonl`: `ts`, `attempt`, `name`, `summary`,
/// `ok`, `result_tail` — the desk's `ingest_calls_bytes` reads `name`/`summary`/`result_tail`
/// as the repeat signature and counts every parseable row as the action that resets its
/// growth-without-acting meter) plus the call's own facts, so a reader of the record sees what
/// landed and when. `summary` names the q_index, so a run of landings is never a `repeat_run`.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct ResearchCallRecord {
    pub(super) outcome: &'static str,
    /// Whether the tool replied success (the call was folded) or error (stray, unopened).
    pub(super) ok: bool,
    pub(super) q_index: Option<usize>,
    pub(super) kind: Option<String>,
    pub(super) chars: Option<usize>,
    /// The call's own flag; None when the call could not be parsed (unknown, never defaulted).
    pub(super) section_done: Option<bool>,
    /// The reply the lane reads next — the row's `result_tail`.
    pub(super) reply: String,
}

impl ResearchCallRecord {
    /// The record of a call the landing folded (`ResearchLanding::land`'s Ok): a row landed at
    /// its q_index; else what `section_done` did to the hand; else an empty call.
    pub(super) fn folded(landed: &Landed, section_done: bool, reply: &str) -> Self {
        let outcome = match (&landed.row, section_done, &landed.hand) {
            (Some(_), _, _) => CALL_LANDED,
            (None, false, _) => CALL_EMPTY,
            (None, true, HandReply::Handed { .. } | HandReply::LastClosed { .. }) => {
                CALL_SECTION_CLOSED
            }
            (None, true, HandReply::Exhausted { .. }) => CALL_PAST_END,
            // `land` never leaves the section in hand on a section_done call.
            (None, true, HandReply::InHand { .. }) => CALL_EMPTY,
        };
        Self {
            outcome,
            ok: true,
            q_index: landed.row.as_ref().map(|r| r.q_index),
            kind: landed.row.as_ref().map(|r| r.kind.clone()),
            chars: landed.row.as_ref().map(|r| r.answer.chars().count()),
            section_done: Some(section_done),
            reply: reply.to_string(),
        }
    }

    /// The record of a call that landed nothing and got an error reply: a stray or an unopened
    /// lane.
    pub(super) fn refused(outcome: &'static str, section_done: Option<bool>, reply: &str) -> Self {
        Self {
            outcome,
            ok: false,
            q_index: None,
            kind: None,
            chars: None,
            section_done,
            reply: reply.to_string(),
        }
    }

    fn summary(&self) -> String {
        let mut s = match (self.q_index, &self.kind, self.chars) {
            (Some(q), Some(kind), Some(chars)) => {
                format!("{} q{q} [{kind}] {chars} chars", self.outcome)
            }
            _ => self.outcome.to_string(),
        };
        if self.section_done == Some(true) {
            s.push_str(", section_done");
        }
        s
    }

    pub(super) fn row(&self, attempt: u32) -> serde_json::Value {
        serde_json::json!({
            "ts": chrono::Utc::now().to_rfc3339(),
            "attempt": attempt,
            "name": RESEARCH_ANSWER_TOOL,
            "tool": RESEARCH_ANSWER_TOOL,
            "summary": self.summary(),
            "ok": self.ok,
            "result_tail": tail_chars(&self.reply, 2000),
            "outcome": self.outcome,
            "q_index": self.q_index,
            "kind": self.kind,
            "chars": self.chars,
            "section_done": self.section_done,
        })
    }
}

/// The lane's activity path exactly as `run_agent_in` derives it for a normal call
/// (`work_dir == self.working_dir`): `.swarm/activity/<activity_digest_key(key)>.json`; the
/// calls sibling is the writer's `with_extension("calls.jsonl")`.
pub(super) fn research_activity_path(root: &Path, key: &str) -> PathBuf {
    root.join(".swarm")
        .join("activity")
        .join(format!("{}.json", activity_digest_key(key)))
}

/// One calls row through the shell rows' writer (`append_calls_row`), primary only: a research
/// lane runs in the real tree, so `fix_shard_mirror_dir` is None for its key and there is no
/// mirror to feed. The activity dir exists — the worker loop created it at the lane's dispatch,
/// before any call could reach the tool. Errors are the caller's to name.
pub(super) fn append_research_call_row(activity: &Path, row: &serde_json::Value) -> AppendErrs {
    append_calls_row(activity, None, &row.to_string())
}

impl GooseAgentDispatcher {
    /// VA-132: the call's row into the lane's own record; a failed write is the existing
    /// `transcript_write_failed` event (`note_transcript_write_failure`, once per key and
    /// kind), never silent — and never a stop, the reply still reaches the lane.
    fn record_research_call(&self, key: &str, record: &ResearchCallRecord) {
        let activity = research_activity_path(&self.working_dir, key);
        // Every planner-side lane reaches `run_agent_in` through `run_agent_timed_at`, which
        // passes attempt 0 ("planner-side calls never retry through the scheduler"), so the
        // shell rows beside this one carry 0 too.
        let row = record.row(0);
        for (kind, e) in append_research_call_row(&activity, &row) {
            self.note_transcript_write_failure(&activity, kind, &e);
        }
    }

    /// The tool's extension for THIS call: Some only for a lane whose landing the fan opened
    /// before dispatch (`research_fan`), so no other call ever holds a tool nothing answers.
    pub(super) fn research_answer_extension_for(&self, key: &str) -> Option<ExtensionConfig> {
        self.research_landing
            .lock()
            .unwrap()
            .contains_key(key)
            .then(research_answer_extension)
    }

    /// The lane's reply to a frontend tool request — ALWAYS a reply, because the agent parks the
    /// lane on it. `research_answer` lands; any other frontend name is an error the lane reads
    /// (no other frontend tool is registered, so the arm exists for the parked-lane law alone);
    /// a request the provider could not parse echoes the provider's error.
    pub(super) fn frontend_tool_result(
        &self,
        key: Option<&str>,
        req: &FrontendToolRequest,
    ) -> Result<CallToolResult, ErrorData> {
        match req.tool_call.as_ref() {
            Ok(call) if call.name.as_ref() == RESEARCH_ANSWER_TOOL => {
                // No arguments object is a call that carried nothing: folded as such (a stray
                // with an empty head), never given a default entry.
                let arguments = match call.arguments.clone() {
                    Some(object) => serde_json::Value::Object(object),
                    None => serde_json::Value::Null,
                };
                Ok(self.land_research_answer(key, &arguments))
            }
            Ok(call) => Err(ErrorData::new(
                ErrorCode::METHOD_NOT_FOUND,
                format!("no swarm handler for frontend tool `{}`", call.name),
                None,
            )),
            Err(e) => Err(e.clone()),
        }
    }

    /// ONE `research_answer` call on this lane: `ResearchLanding::land` under the landing lock,
    /// then the relay to the still-running sibling lanes (`queue_research_relay`) and the reply
    /// naming the mini and the next index. A call on a key with no landing open (a lane the fan
    /// never registered — `research_answer_unopened`) or an entry with no question text lands
    /// NO row: `research_batch_stray_answer{via: tool}` names it and the tool's error reply
    /// tells the lane what was missing, so its next call can carry it. MILD: an error reply is
    /// information for the lane, never a stop. Every arm that has a lane also leaves the call's
    /// row in the lane's `.calls.jsonl` (VA-132, `record_research_call`) — the reply text IS the
    /// row's `result_tail`, so the record shows what the lane was told.
    pub(super) fn land_research_answer(
        &self,
        key: Option<&str>,
        arguments: &serde_json::Value,
    ) -> CallToolResult {
        let Some(key) = key else {
            return CallToolResult::error(vec![Content::text(format!(
                "{RESEARCH_ANSWER_TOOL} is open only on a research lane; this call has no lane"
            ))]);
        };
        let text = arguments.to_string();
        // Parsed outside the landing lock (pure); None is the stray it always was. The flag is
        // read here because a stray's row carries it too — unknown (None) when nothing parsed.
        let parsed = ResearchToolCall::parse(&text);
        let section_done = parsed.as_ref().map(ResearchToolCall::section_done);
        let landed = {
            let mut landing = self.research_landing.lock().unwrap();
            let Some(open) = landing.get_mut(key) else {
                self.events.write_value(serde_json::json!({
                    "event": "research_answer_unopened",
                    "task": key,
                    "answer_head": text.chars().take(200).collect::<String>(),
                }));
                let reply = format!(
                    "{RESEARCH_ANSWER_TOOL} is not open for lane {key}: nothing landed; carry \
                     the entry in final_output"
                );
                self.record_research_call(
                    key,
                    &ResearchCallRecord::refused(CALL_UNOPENED, section_done, &reply),
                );
                return CallToolResult::error(vec![Content::text(reply)]);
            };
            let slice = open.slice.clone();
            match parsed {
                Some(call) => open
                    .land(&self.working_dir, self.events.as_ref(), key, call)
                    .map_err(|stray| (slice, stray)),
                // Nothing parseable: the stray it always was, at the index it would have taken.
                None => Err((
                    slice,
                    StrayAnswer {
                        question_index: Some(open.next_q_index),
                        answer_head: text.chars().take(200).collect(),
                    },
                )),
            }
        };
        let (result, record) = match landed {
            Ok(landed) => {
                if let Some(row) = &landed.row {
                    self.queue_research_relay(row);
                }
                let reply = landed_reply_text(&landed);
                let record =
                    ResearchCallRecord::folded(&landed, section_done == Some(true), &reply);
                (CallToolResult::success(vec![Content::text(reply)]), record)
            }
            Err((slice, stray)) => {
                self.events.write_value(serde_json::json!({
                    "event": "research_batch_stray_answer",
                    "task": key,
                    "slice": slice,
                    "question_index": stray.question_index,
                    "answer_head": stray.answer_head,
                    "via": "tool",
                }));
                let reply = stray_reply_text(&stray);
                let record = ResearchCallRecord::refused(CALL_STRAY, section_done, &reply);
                (CallToolResult::error(vec![Content::text(reply)]), record)
            }
        };
        self.record_research_call(key, &record);
        result
    }

    /// r6e E7: hand a just-landed mini to every still-running lane of its slice
    /// (`relay_targets`); the target's own loop delivers it and names the delivery
    /// (`research_mini_relayed`). MEASURED: r6d research-ledger-core-q5 dispatched 04:46:55Z,
    /// q2's mini landed 04:47:28Z — 33 s too late for `prior_minis_block`, and q5 hedged
    /// against the rule q2 had settled. A queue, never a bound: nothing waits on it.
    pub(super) fn queue_research_relay(&self, row: &ResearchRow) {
        let running: Vec<(String, RelayTarget)> = self
            .research_running
            .lock()
            .unwrap()
            .iter()
            .map(|(k, t)| (k.clone(), t.clone()))
            .collect();
        let targets = relay_targets(row, &running);
        if targets.is_empty() {
            return;
        }
        let note = relay_note(row);
        let mut inbox = self.research_relay.lock().unwrap();
        for t in targets {
            inbox.entry(t).or_default().push(note.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::research::{
        briefs_from_slices, fold_research_lane_from, load_research_mini, section_hand,
        RESEARCH_ANSWERED, RESEARCH_UNANSWERED,
    };
    use super::super::{spec_sections, NullSink, OpenOutput, OpenSlice, SwarmEvent};
    use super::*;
    use std::sync::Mutex;

    /// The three-section request every hand test deals from: §1 Boot [1-3], §2 Endpoints [4-6],
    /// §3 Rules [7-8] — each body carries a marker word so a test can prove whose text rode.
    const THREE_SECTIONS: &str = "# Boot\nBOOT_WORDS on port 8850.\n\n# Endpoints\n\
                                  ENDPOINT_WORDS GET /api/health.\n\n# Rules\nRULE_WORDS bump \
                                  the version.\n";

    /// A slice lane as the fan builds it, its hand dealt from `THREE_SECTIONS` by the claimed
    /// headings (empty `claimed` = a slice with nothing in hand).
    fn lane_with_hand(slice: &str, claimed: &[&str]) -> ResearchLane {
        let claimed: Vec<String> = claimed.iter().map(|c| c.to_string()).collect();
        ResearchLane {
            slice: slice.to_string(),
            head: "HEAD".to_string(),
            siblings: String::new(),
            questions: Vec::new(),
            material: format!("Own `app/{slice}.py`."),
            hand: section_hand(slice, &claimed, &spec_sections(THREE_SECTIONS), &NullSink),
        }
    }

    fn call(v: serde_json::Value) -> ResearchToolCall {
        ResearchToolCall::parse(&v.to_string()).expect("a JSON object parses")
    }

    fn section_done() -> ResearchToolCall {
        call(serde_json::json!({"section_done": true}))
    }

    #[derive(Default)]
    struct ValueSink(Mutex<Vec<serde_json::Value>>);
    impl EventSink for ValueSink {
        fn emit(&self, _event: &SwarmEvent) {}
        fn write_value(&self, value: serde_json::Value) {
            self.0.lock().unwrap().push(value);
        }
    }

    #[test]
    fn the_tool_is_one_entry_under_its_bare_name() {
        let ExtensionConfig::Frontend { tools, name, .. } = research_answer_extension() else {
            panic!("the per-answer tool is a frontend extension");
        };
        assert_eq!(name, RESEARCH_ANSWER_EXTENSION);
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name.as_ref(), RESEARCH_ANSWER_TOOL);
        let schema = serde_json::Value::Object((*tools[0].input_schema).clone());
        assert_eq!(schema, research_answer_tool_schema());
        assert!(tools[0]
            .description
            .as_deref()
            .is_some_and(|d| d.contains("once per question")));
    }

    /// The fan's take after the call, as `research_fan`'s closure performs it: the landing's
    /// `close` seeds the lane's returned rows with the tool-landed rows, the final reply folds
    /// only the remainder numbered after them, and the brief renders the tool-landed answer under
    /// ANSWERS SETTLED AT PLAN TIME. (The closure itself needs a model; this is the seam it
    /// calls, with its two lines replicated verbatim.)
    #[test]
    fn a_tool_landed_row_reaches_the_lane_rows_and_the_brief() {
        let dir = tempfile::tempdir().unwrap();
        let sink = ValueSink::default();
        let mut landing = ResearchLanding::open(&lane_with_hand("api", &[]), "m");
        let entry = serde_json::json!({
            "question": "which port",
            "kind": "design",
            "cite": "request.md:12 'boots on'",
            "alternatives": ["8850", "8000"],
            "open_because": "L12 names the vendor's port, not the app's",
            "answer": "Port 8850, from the spec's own boot table."
        });
        let landed = landing
            .land(dir.path(), &sink, "research-api", call(entry))
            .unwrap();
        let row = landed.row.unwrap();
        assert_eq!((row.q_index, row.status.as_str()), (0, RESEARCH_ANSWERED));
        assert_eq!(
            landed.hand,
            HandReply::Exhausted { of: 0 },
            "a lane with no claimed section has nothing in hand"
        );
        assert!(load_research_mini(dir.path(), "api", 0).is_some());
        assert!(
            ResearchToolCall::parse("not json").is_none(),
            "nothing parseable is the caller's stray"
        );
        assert!(
            landing
                .land(
                    dir.path(),
                    &sink,
                    "research-api",
                    call(serde_json::json!({"answer": "an answer with no question"}))
                )
                .is_err(),
            "a stray lands no row"
        );
        {
            let ev = sink.0.lock().unwrap();
            let landed: Vec<&serde_json::Value> = ev
                .iter()
                .filter(|e| e["event"] == "research_answer_landed")
                .collect();
            assert_eq!(landed.len(), 1);
            assert_eq!(landed[0]["q_index"], 0);
            assert_eq!(landed[0]["via"], "tool");
            assert_eq!(landed[0]["task"], "research-api");
        }
        // research_fan, after the call:
        let (landed, mut out_rows) = Some(landing).map_or((0, Vec::new()), ResearchLanding::close);
        assert_eq!(landed, 1, "the stray burned no index");
        let (remainder, strays) = fold_research_lane_from(
            "api",
            "m",
            300,
            Ok(serde_json::json!({
                "answers": [{"question": "which storage", "kind": "design", "answer": ""}]
            })
            .to_string()),
            landed,
        );
        assert!(strays.is_empty());
        out_rows.extend(remainder);
        assert_eq!(
            out_rows
                .iter()
                .map(|r| (r.q_index, r.status.as_str()))
                .collect::<Vec<_>>(),
            vec![(0, RESEARCH_ANSWERED), (1, RESEARCH_UNANSWERED)],
            "tool-landed first, the remainder numbered after it"
        );
        let opened = OpenOutput {
            slices: vec![OpenSlice {
                id: "api".into(),
                title: "the api".into(),
                objective: "serve GET /health".into(),
                weight: 3,
                sections: Vec::new(),
            }],
            open_decisions: Vec::new(),
        };
        let briefs = briefs_from_slices(&opened, "build the app", &out_rows, &[], &NullSink);
        let b = &briefs[0].brief;
        let settled_at = b
            .find("ANSWERS SETTLED AT PLAN TIME")
            .expect("the tool-landed answer is settled at plan time");
        assert!(
            b.split_at(settled_at)
                .1
                .contains("Q: [design] which port\nA: Port 8850, from the spec's own boot table."),
            "the tool-landed row renders under ANSWERS SETTLED:\n{b}"
        );
        let questions_at = b.find("QUESTIONS this slice must settle").unwrap();
        assert!(
            settled_at < questions_at && b.split_at(questions_at).1.contains("- which storage")
        );
    }

    /// VA-128 (b): an entry landed for §1 leaves §1 in hand (the reply says how to close it);
    /// the `section_done` call closes §1 — `research_section_settled{index: 1, landed: 1}` — and
    /// the tool's RESULT carries §2's full text with the index of §3, never §3's words, beside
    /// `research_section_handed{index: 2, of: 3}`.
    #[test]
    fn landing_an_entry_keeps_the_section_in_hand_and_section_done_deals_the_next() {
        let dir = tempfile::tempdir().unwrap();
        let sink = ValueSink::default();
        let lane = lane_with_hand("api", &["Boot", "Endpoints", "Rules"]);
        let mut landing = ResearchLanding::open(&lane, "m");
        let entry = serde_json::json!({
            "question": "which port does the app bind",
            "kind": "external",
            "cite": "request.md:2",
            "answer": "8850"
        });
        let landed = landing
            .land(dir.path(), &sink, "research-api", call(entry))
            .unwrap();
        assert_eq!(landed.row.as_ref().map(|r| r.q_index), Some(0));
        assert_eq!(
            landed.hand,
            HandReply::InHand {
                index: 1,
                of: 3,
                heading: "Boot".into()
            }
        );
        let reply = hand_reply_text(&landed.hand);
        assert!(
            reply.contains("§1 of 3 `Boot` stays in hand")
                && reply.contains("{\"section_done\": true}"),
            "{reply}"
        );
        let landed = landing
            .land(dir.path(), &sink, "research-api", section_done())
            .unwrap();
        assert!(landed.row.is_none(), "a bare section_done lands no row");
        let HandReply::Handed {
            closed,
            closed_landed,
            index,
            of,
            block,
        } = &landed.hand
        else {
            panic!("§2 is dealt: {:?}", landed.hand);
        };
        assert_eq!(
            (closed.as_str(), *closed_landed, *index, *of),
            ("Boot", 1, 2, 3)
        );
        assert!(
            block.contains("SECTION 2 of 3, in hand now")
                && block
                    .contains("### Endpoints\n[request.md:4-6]\nENDPOINT_WORDS GET /api/health.")
                && block.contains("§3 Rules [request.md:7-8]")
                && !block.contains("RULE_WORDS"),
            "{block}"
        );
        let reply = hand_reply_text(&landed.hand);
        assert!(
            reply.contains("§1 `Boot` closed (1 landed).") && reply.contains("ENDPOINT_WORDS"),
            "the tool's result carries the next section's text:\n{reply}"
        );
        let ev = sink.0.lock().unwrap();
        let settled = ev
            .iter()
            .find(|e| e["event"] == "research_section_settled")
            .expect("the closed section is a loud fact");
        assert_eq!(settled["task"], "research-api");
        assert_eq!(settled["heading"], "Boot");
        assert_eq!(settled["index"], 1);
        assert_eq!(settled["landed"], 1);
        assert_eq!(settled["remaining"], 2);
        let handed = ev
            .iter()
            .find(|e| e["event"] == "research_section_handed")
            .expect("the landing hands section 2");
        assert_eq!(handed["task"], "research-api");
        assert_eq!(handed["slice"], "api");
        assert_eq!(handed["heading"], "Endpoints");
        assert_eq!(handed["index"], 2);
        assert_eq!(handed["of"], 3);
        assert_eq!(handed["lines"], "request.md:4-6");
    }

    /// VA-128 (c): after §3 the result says no section remains and points at final_output; a
    /// section_done past the end is `research_section_done_past_end`, never a silent no-op; an
    /// entry may carry `section_done` itself and lands before the section closes.
    #[test]
    fn closing_the_last_section_says_nothing_remains_and_a_call_past_the_end_is_named() {
        let dir = tempfile::tempdir().unwrap();
        let sink = ValueSink::default();
        let lane = lane_with_hand("api", &["Boot", "Endpoints", "Rules"]);
        let mut landing = ResearchLanding::open(&lane, "m");
        for _ in 0..2 {
            landing
                .land(dir.path(), &sink, "research-api", section_done())
                .unwrap();
        }
        let last = serde_json::json!({
            "question": "which sort values are accepted",
            "kind": "design",
            "alternatives": ["created_at", "date_desc"],
            "open_because": "L8 names the bump, not the sort",
            "answer": "created_at",
            "section_done": true
        });
        let landed = landing
            .land(dir.path(), &sink, "research-api", call(last))
            .unwrap();
        assert_eq!(landed.row.as_ref().map(|r| r.q_index), Some(0));
        assert_eq!(
            landed.hand,
            HandReply::LastClosed {
                closed: "Rules".into(),
                closed_landed: 1,
                of: 3
            }
        );
        let reply = hand_reply_text(&landed.hand);
        assert!(
            reply.contains("Every section (3 of 3) has been handed and closed")
                && reply.contains("call final_output now"),
            "{reply}"
        );
        let landed = landing
            .land(dir.path(), &sink, "research-api", section_done())
            .unwrap();
        assert_eq!(landed.hand, HandReply::Exhausted { of: 3 });
        assert!(hand_reply_text(&landed.hand).contains("nothing more to hand"));
        let past_end = sink
            .0
            .lock()
            .unwrap()
            .iter()
            .filter(|e| e["event"] == "research_section_done_past_end")
            .count();
        assert_eq!(past_end, 1);
        let (next, rows) = landing.close();
        assert_eq!(
            (next, rows.len()),
            (1, 1),
            "three closes, one row, one index burned"
        );
    }

    /// VA-128 (d): a section with nothing to settle is ONE call — `section_done` with the
    /// choices only the builder makes — which advances the hand and keeps the choices on a
    /// `builder_decides` outcome row (persisted, `research_builder_decides` per line), so a
    /// stateless lane's per-section choices survive to the brief without a later row to ride.
    #[test]
    fn a_builder_decides_section_done_call_advances_and_keeps_the_choices() {
        let dir = tempfile::tempdir().unwrap();
        let sink = ValueSink::default();
        let lane = lane_with_hand("api", &["Boot", "Endpoints", "Rules"]);
        let mut landing = ResearchLanding::open(&lane, "m");
        let landed = landing
            .land(
                dir.path(),
                &sink,
                "research-api",
                call(serde_json::json!({
                    "section_done": true,
                    "builder_decides": ["debounce interval", " ", "debounce interval"]
                })),
            )
            .unwrap();
        let row = landed.row.expect("the choices ride an outcome row");
        assert_eq!(
            (row.q_index, row.question.as_str(), row.reason.as_deref()),
            (0, "", Some("builder_decides"))
        );
        assert_eq!(
            row.raised,
            vec!["[builder decides] debounce interval".to_string()]
        );
        assert!(
            row.detail
                .as_deref()
                .unwrap()
                .contains("section `Boot` (request.md:1-3): no question; 1 choice(s)"),
            "{:?}",
            row.detail
        );
        assert!(load_research_mini(dir.path(), "api", 0).is_some());
        assert!(matches!(landed.hand, HandReply::Handed { index: 2, .. }));
        let ev = sink.0.lock().unwrap();
        let names: Vec<&str> = ev.iter().map(|e| e["event"].as_str().unwrap()).collect();
        assert_eq!(
            names,
            vec![
                "research_unanswered",
                "research_builder_decides",
                "research_answer_landed",
                "research_section_settled",
                "research_section_handed",
            ],
            "{names:?}"
        );
        assert_eq!(ev[0]["reason"], "builder_decides");
        assert_eq!(ev[1]["text"], "debounce interval");
        assert_eq!(ev[3]["landed"], 0, "a choice is not a question row");
        assert_eq!(ev[4]["index"], 2);
    }

    /// VA-132: a `research_answer` call is an ACTION in the lane's own record. r6j's core lane
    /// landed twelve answers whose only trace was run.jsonl — its `.calls.jsonl` held shell rows
    /// alone, so the desk read the reasoning between landings as growth without acting. Two
    /// landings and a stray each append one row at the lane's activity path through the shell
    /// rows' writer, in the shell rows' shape (the desk's `name`/`summary`/`result_tail`
    /// signature — distinct per landing, so a run of landings is never a `repeat_run`) plus the
    /// call's own facts; the `result_tail` is the reply the lane read. This is the seam
    /// `land_research_answer` walks minus the dispatcher's failure latch.
    #[test]
    fn a_landed_call_and_a_stray_each_leave_a_row_in_the_lanes_calls_jsonl() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let key = "research-api";
        let activity = research_activity_path(root, key);
        assert_eq!(
            activity,
            root.join(".swarm")
                .join("activity")
                .join("research-api.json")
        );
        assert_eq!(
            research_activity_path(root, "research-app/x")
                .file_name()
                .unwrap(),
            "research-app~sx.json",
            "the key flattens exactly as run_agent_in's activity_file does"
        );
        std::fs::create_dir_all(activity.parent().unwrap()).unwrap();
        let sink = ValueSink::default();
        let mut landing =
            ResearchLanding::open(&lane_with_hand("api", &["Boot", "Endpoints"]), "m");
        let entry = |q: &str| {
            serde_json::json!({
                "question": q,
                "kind": "design",
                "cite": "request.md:2",
                "alternatives": ["8850", "8000"],
                "open_because": "L2 names the vendor's port, not the app's",
                "answer": "8850, from the boot table"
            })
        };
        for q in ["which port", "which storage"] {
            let landed = landing.land(root, &sink, key, call(entry(q))).unwrap();
            let reply = landed_reply_text(&landed);
            let record = ResearchCallRecord::folded(&landed, false, &reply);
            assert!(append_research_call_row(&activity, &record.row(0)).is_empty());
        }
        let stray = landing
            .land(
                root,
                &sink,
                key,
                call(serde_json::json!({"answer": "an answer with no question"})),
            )
            .unwrap_err();
        let reply = stray_reply_text(&stray);
        let record = ResearchCallRecord::refused(CALL_STRAY, Some(false), &reply);
        assert!(append_research_call_row(&activity, &record.row(0)).is_empty());

        let text = std::fs::read_to_string(activity.with_extension("calls.jsonl")).unwrap();
        let rows: Vec<serde_json::Value> = text
            .lines()
            .map(|l| serde_json::from_str(l).expect("every row parses — the desk's contract"))
            .collect();
        assert_eq!(rows.len(), 3);
        let r0 = &rows[0];
        assert_eq!(
            r0["name"], RESEARCH_ANSWER_TOOL,
            "the desk's signature field"
        );
        assert_eq!(r0["tool"], RESEARCH_ANSWER_TOOL);
        assert_eq!(r0["outcome"], CALL_LANDED);
        assert_eq!(r0["q_index"], 0);
        assert_eq!(r0["kind"], "design");
        assert_eq!(r0["chars"], "8850, from the boot table".chars().count());
        assert_eq!(r0["section_done"], false);
        assert_eq!(r0["ok"], true);
        assert_eq!(r0["attempt"], 0);
        assert_eq!(r0["summary"], "landed q0 [design] 25 chars");
        assert!(r0["ts"].as_str().is_some_and(|t| t.contains('T')));
        let tail = r0["result_tail"].as_str().unwrap();
        assert!(
            tail.contains("landed research-api-q0.json (answered, kind design)")
                && tail.contains("§1 of 2 `Boot` stays in hand"),
            "the row carries the reply the lane read:\n{tail}"
        );
        assert_eq!(rows[1]["q_index"], 1);
        let sig = |r: &serde_json::Value| {
            (
                r["name"].clone(),
                r["summary"].clone(),
                r["result_tail"].clone(),
            )
        };
        assert_ne!(
            sig(&rows[0]),
            sig(&rows[1]),
            "two landings never share the desk's repeat signature"
        );
        let r2 = &rows[2];
        assert_eq!(r2["name"], RESEARCH_ANSWER_TOOL);
        assert_eq!(r2["outcome"], CALL_STRAY);
        assert_eq!(r2["ok"], false);
        assert_eq!(r2["summary"], CALL_STRAY);
        assert!(
            r2["q_index"].is_null() && r2["kind"].is_null() && r2["chars"].is_null(),
            "a stray burns no index and lands no facts"
        );
        assert_eq!(r2["section_done"], false);
        assert!(r2["result_tail"]
            .as_str()
            .unwrap()
            .starts_with("nothing landed:"));
    }

    /// VA-132: the outcomes with no row — a bare section_done that closes a section, one past
    /// the end, a call carrying nothing, an unopened lane — each name themselves; an
    /// unparseable call's `section_done` stays unknown (null), never a default.
    #[test]
    fn a_section_close_a_close_past_the_end_and_an_empty_call_name_their_outcome() {
        let dir = tempfile::tempdir().unwrap();
        let sink = ValueSink::default();
        let key = "research-api";
        let mut landing = ResearchLanding::open(&lane_with_hand("api", &["Boot"]), "m");
        let closed = landing
            .land(dir.path(), &sink, key, section_done())
            .unwrap();
        let record = ResearchCallRecord::folded(&closed, true, &landed_reply_text(&closed));
        assert_eq!(
            (
                record.outcome,
                record.ok,
                record.q_index,
                record.section_done
            ),
            (CALL_SECTION_CLOSED, true, None, Some(true))
        );
        assert_eq!(record.row(0)["summary"], "section_closed, section_done");
        let past = landing
            .land(dir.path(), &sink, key, section_done())
            .unwrap();
        assert_eq!(
            ResearchCallRecord::folded(&past, true, &landed_reply_text(&past)).outcome,
            CALL_PAST_END
        );
        let empty = landing
            .land(dir.path(), &sink, key, call(serde_json::json!({})))
            .unwrap();
        assert!(empty.row.is_none());
        assert_eq!(
            ResearchCallRecord::folded(&empty, false, &landed_reply_text(&empty)).outcome,
            CALL_EMPTY
        );
        let row = ResearchCallRecord::refused(CALL_UNOPENED, None, "not open").row(0);
        assert_eq!(row["outcome"], CALL_UNOPENED);
        assert_eq!(row["ok"], false);
        assert!(
            row["section_done"].is_null(),
            "an unparseable call's flag is unknown, never defaulted"
        );
    }

    /// VA-132: a failed row write is loud — the writer returns the `calls.jsonl` kind that
    /// `note_transcript_write_failure` turns into `transcript_write_failed` (the GEN-6a class).
    #[test]
    fn a_failed_calls_row_write_reports_the_calls_jsonl_kind() {
        let dir = tempfile::tempdir().unwrap();
        let activity = research_activity_path(dir.path(), "research-api");
        std::fs::create_dir_all(activity.with_extension("calls.jsonl")).unwrap();
        let row = ResearchCallRecord::refused(CALL_STRAY, Some(false), "x").row(0);
        let errs = append_research_call_row(&activity, &row);
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].0, "calls.jsonl");
    }
}
