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

use std::path::Path;

use goose::agents::ExtensionConfig;
use goose::conversation::message::FrontendToolRequest;
use rmcp::model::{CallToolResult, Content, ErrorCode, ErrorData, Tool};

use super::research::{
    emit_research_outcome, fold_research_entry, persist_research_row, relay_note, relay_targets,
    research_answer_tool_schema, research_mini_name, RelayTarget, ResearchRow, StrayAnswer,
    RESEARCH_ANSWER_TOOL,
};
use super::{EventSink, GooseAgentDispatcher};

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
}

impl ResearchLanding {
    pub(super) fn open(slice: &str, model: &str) -> Self {
        Self {
            slice: slice.to_string(),
            model: model.to_string(),
            started: std::time::Instant::now(),
            next_q_index: 0,
            landed: Vec::new(),
        }
    }

    /// ONE tool call → one row landed NOW: folded at the next q_index (`fold_research_entry`),
    /// persisted (`persist_research_row`), emitted through the one outcome funnel
    /// (`emit_research_outcome`) plus `research_answer_landed{task, slice, q_index, kind, status,
    /// chars, raised, via: tool}` so the vigil sees answers arrive mid-lane, and kept for
    /// `close`. A stray (no question text, nothing parseable) lands NO row and is the caller's to
    /// name.
    fn land(
        &mut self,
        root: &Path,
        events: &dyn EventSink,
        key: &str,
        arguments: &str,
    ) -> Result<ResearchRow, StrayAnswer> {
        let row = fold_research_entry(
            &self.slice,
            self.next_q_index,
            &self.model,
            self.started.elapsed().as_secs(),
            arguments,
        )?;
        self.next_q_index += 1;
        persist_research_row(root, events, &row);
        emit_research_outcome(events, &row);
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
        Ok(row)
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
     you settle each one; your final_output then carries only the entries you did not land here.";

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
             call it; final_output carries only what you did not land through it."
        )),
        bundled: Some(true),
        available_tools: Vec::new(),
    }
}

impl GooseAgentDispatcher {
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
    /// information for the lane, never a stop.
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
        let landed = {
            let mut landing = self.research_landing.lock().unwrap();
            let Some(open) = landing.get_mut(key) else {
                self.events.write_value(serde_json::json!({
                    "event": "research_answer_unopened",
                    "task": key,
                    "answer_head": text.chars().take(200).collect::<String>(),
                }));
                return CallToolResult::error(vec![Content::text(format!(
                    "{RESEARCH_ANSWER_TOOL} is not open for lane {key}: nothing landed; carry \
                     the entry in final_output"
                ))]);
            };
            let slice = open.slice.clone();
            open.land(&self.working_dir, self.events.as_ref(), key, &text)
                .map_err(|stray| (slice, stray))
        };
        match landed {
            Ok(row) => {
                self.queue_research_relay(&row);
                CallToolResult::success(vec![Content::text(format!(
                    "landed {} ({}, kind {}); q{} is the next question you settle; final_output \
                     carries only the entries you have not landed here",
                    research_mini_name(&row.slice, row.q_index),
                    row.status,
                    row.kind,
                    row.q_index + 1,
                ))])
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
                CallToolResult::error(vec![Content::text(format!(
                    "nothing landed: {RESEARCH_ANSWER_TOOL} takes ONE JSON object with \
                     `question` (the question text), `kind` (design | external) and `answer`; \
                     this call carried: {}",
                    stray.answer_head
                ))])
            }
        }
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
        briefs_from_slices, fold_research_lane_from, load_research_mini, RESEARCH_ANSWERED,
        RESEARCH_UNANSWERED,
    };
    use super::super::{NullSink, OpenOutput, OpenSlice, SwarmEvent};
    use super::*;
    use std::sync::Mutex;

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
        let mut landing = ResearchLanding::open("api", "m");
        let entry = serde_json::json!({
            "question": "which port",
            "kind": "design",
            "cite": "request.md:12 'boots on'",
            "alternatives": ["8850", "8000"],
            "open_because": "L12 names the vendor's port, not the app's",
            "answer": "Port 8850, from the spec's own boot table."
        });
        let row = landing
            .land(dir.path(), &sink, "research-api", &entry.to_string())
            .unwrap();
        assert_eq!((row.q_index, row.status.as_str()), (0, RESEARCH_ANSWERED));
        assert!(load_research_mini(dir.path(), "api", 0).is_some());
        assert!(
            landing
                .land(dir.path(), &sink, "research-api", "not json")
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
}
