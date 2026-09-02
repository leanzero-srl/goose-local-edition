//! The per-answer research landing (VA-118 item 4, wired r6j): a slice lane's `research_answer`
//! tool call lands ONE settled question as one ledger mini the moment the lane calls it —
//! persisted, emitted through the one outcome funnel, relayed to the sibling lanes — instead of
//! every answer arriving in one final_output after an hour at 0 bytes (r6i's
//! research-web-console-structure lane: output frame empty for 63 minutes, 113,720 reasoning
//! chars, nine answers at once). The tool is a goose FRONTEND extension: the agent yields
//! `MessageContent::FrontendToolRequest` and parks the lane on its result channel until
//! `Agent::handle_tool_result` answers, so every arm here REPLIES — a request left unanswered
//! would park the lane forever. Nothing here bounds or ends anything: a call that cannot be
//! folded is a named stray and an error reply the lane can act on, never a stop.

use goose::agents::ExtensionConfig;
use goose::conversation::message::FrontendToolRequest;
use rmcp::model::{CallToolResult, Content, ErrorCode, ErrorData, Tool};

use super::research::{
    emit_research_outcome, fold_research_entry, persist_research_row, relay_note, relay_targets,
    research_answer_tool_schema, research_mini_name, RelayTarget, ResearchRow,
    RESEARCH_ANSWER_TOOL,
};
use super::GooseAgentDispatcher;

/// One research lane's landing state for the life of its call, keyed by the lane's activity key
/// in `GooseAgentDispatcher::research_landing`: the slice the rows belong to, the model that
/// answers (row attribution), the call's start (`secs` on each row is the lane's elapsed at the
/// landing) and the q_index the next landed entry takes — the count landed so far, which the
/// fan reads back after the call so the final reply's remainder continues the numbering
/// (`fold_research_lane_from`). The index advances only when a row lands: a stray never burns
/// a number.
#[derive(Debug)]
pub(super) struct ResearchLanding {
    slice: String,
    model: String,
    started: std::time::Instant,
    pub(super) next_q_index: usize,
}

impl ResearchLanding {
    pub(super) fn open(slice: &str, model: &str) -> Self {
        Self {
            slice: slice.to_string(),
            model: model.to_string(),
            started: std::time::Instant::now(),
            next_q_index: 0,
        }
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

    /// ONE `research_answer` call → one row landed NOW: folded at the lane's next q_index
    /// (`fold_research_entry`), persisted (`persist_research_row`), emitted through the one
    /// outcome funnel (`emit_research_outcome`) plus `research_answer_landed{task, slice,
    /// q_index, kind, status, chars, raised, via: tool}` so the vigil sees answers arrive
    /// mid-lane, and relayed to the still-running sibling lanes (`queue_research_relay`). A call
    /// on a key with no landing open (a lane the fan never registered — `research_answer_unopened`)
    /// or an entry with no question text lands NO row: `research_batch_stray_answer{via: tool}`
    /// names it and the tool's error reply tells the lane what was missing, so its next call can
    /// carry it. MILD: an error reply is information for the lane, never a stop.
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
        let folded = {
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
            match fold_research_entry(
                &open.slice,
                open.next_q_index,
                &open.model,
                open.started.elapsed().as_secs(),
                &text,
            ) {
                Ok(row) => {
                    open.next_q_index += 1;
                    Ok(row)
                }
                Err(stray) => Err((open.slice.clone(), stray)),
            }
        };
        match folded {
            Ok(row) => {
                persist_research_row(&self.working_dir, self.events.as_ref(), &row);
                emit_research_outcome(self.events.as_ref(), &row);
                self.events.write_value(serde_json::json!({
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
    use super::*;

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
}
