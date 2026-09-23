//! The repeat guard: a PROGRESS fact, not a count.
//!
//! Within the current turn — everything since the last message the user actually sent — a call
//! identical (name + arguments) to an EARLIER call of the turn that returned byte-identical output
//! made no progress, whether the two were back to back or had other calls between them. That call
//! still runs, and the model is told in its result how many times this exact call has already
//! returned this exact output this turn. Once that has been said, the same call is declined with the
//! same fact — but only while nothing new has come back since: a call between them that returned
//! something new (an edit, a fresh read) is progress, and the repeat runs again and is noted again.
//! A call whose own output CHANGED since its last run (a poll that moves: tests, a build, a status)
//! starts over: never noted, never declined. The turn never ends here.
//!
//! Measured 2026-09-23. Session 20260923_19 (Qwen3.8-27B over the MLX sidecar): 174 consecutive
//! `cd /Users/mihaiperdum/.agents/skills/release-checklist && ls -la; echo "exit=$?"` calls, one
//! distinct output among them, until a human pressed Stop. Session 20260923_32: repeats interleaved
//! with other calls (`search_available_extensions` four times, one `cp` backup twice, a
//! `realpath('/usr/bin/uvx')` probe twice) — "identical to the previous call" never matched.
//! Why the decline waits for "nothing new since": replayed over the 318 desktop sessions in
//! sessions.db (6,246 calls), declining every repeat after its note would have declined 463 calls, 130
//! of which would have returned DIFFERENT output (`tree .`, `analyze app`, a `sed -n '1,240p'` re-read
//! after edits); gated on nothing-new-since it declines 14, none of which would have.
//!
//! The history read is the conversation as the agent loop holds it, which carries every EARLIER
//! model response of the turn. Calls issued in the same model response as the candidate are not in
//! it yet: the model had not seen their output when it asked, and they run concurrently.
//!
//! All state is derived from the conversation, so it survives a resumed session and there is
//! nothing to advance or reset. Which calls were zero-progress rides the tool result's trusted
//! meta, the same channel the ACP server forwards to the desktop as `_meta.goose`.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::agents::extension_manager::TRUSTED_TOOL_UPDATE_META_KEY;
use crate::config::GooseMode;
use crate::conversation::message::{Message, MessageContent, ToolRequest};
use crate::mcp_utils::ToolResult;
use crate::tool_inspection::{InspectionAction, InspectionResult, ToolInspector};
use anyhow::Result;
use async_trait::async_trait;
use rmcp::model::{CallToolRequestParams, CallToolResult, Content, Meta, Role};
use serde_json::Value;

const NOTE_HEAD: &str = "This exact call already returned this same output ";
const NOTE_TAIL: &str = " this turn; it will not change. Use it, or take a different step.";

pub const REPETITION_INSPECTOR_NAME: &str = "repetition";

const REPEAT_KEY: &str = "repeat";
const SAME_OUTPUT: &str = "same_output";
const SKIPPED: &str = "skipped";

fn times(n: usize) -> String {
    if n == 1 {
        "1 time".to_string()
    } else {
        format!("{n} times")
    }
}

fn same_output_note(returned: usize) -> String {
    format!("{NOTE_HEAD}{}{NOTE_TAIL}", times(returned))
}

fn skipped_reason(returned: usize) -> String {
    format!(
        "Not run: this exact call already returned this same output {} this turn, you were told so, and no call since has returned anything new — running it again would return that output again. Use it, or take a different step.",
        times(returned)
    )
}

type CompletedCall<'a> = (
    &'a ToolResult<CallToolRequestParams>,
    &'a ToolResult<CallToolResult>,
);

/// A message the user actually sent: not a tool response, not an injected agent-only nudge.
fn is_user_prompt(message: &Message) -> bool {
    message.role == Role::User
        && message.is_user_visible()
        && message.is_agent_visible()
        && message
            .content
            .iter()
            .any(|c| matches!(c, MessageContent::Text(_)))
        && !message
            .content
            .iter()
            .any(|c| matches!(c, MessageContent::ToolResponse(_)))
}

/// The turn's completed calls, in the order they were issued.
fn turn_calls(messages: &[Message]) -> Vec<CompletedCall<'_>> {
    let start = messages
        .iter()
        .rposition(is_user_prompt)
        .map_or(0, |i| i + 1);
    let turn = &messages[start..];
    let responses: HashMap<&str, &ToolResult<CallToolResult>> = turn
        .iter()
        .flat_map(|m| m.content.iter())
        .filter_map(|c| match c {
            MessageContent::ToolResponse(r) => Some((r.id.as_str(), &r.tool_result)),
            _ => None,
        })
        .collect();
    turn.iter()
        .flat_map(|m| m.content.iter())
        .filter_map(|c| match c {
            MessageContent::ToolRequest(r) => Some((&r.tool_call, *responses.get(r.id.as_str())?)),
            _ => None,
        })
        .collect()
}

fn same_call(previous: &ToolResult<CallToolRequestParams>, call: &CallToolRequestParams) -> bool {
    previous
        .as_ref()
        .is_ok_and(|p| p.name == call.name && p.arguments == call.arguments)
}

fn is_note(content: &Content) -> bool {
    content
        .as_text()
        .is_some_and(|t| t.text.starts_with(NOTE_HEAD) && t.text.ends_with(NOTE_TAIL))
}

fn same_output(a: &ToolResult<CallToolResult>, b: &ToolResult<CallToolResult>) -> bool {
    match (a, b) {
        (Ok(a), Ok(b)) => {
            a.is_error == b.is_error
                && a.structured_content == b.structured_content
                && a.content
                    .iter()
                    .filter(|c| !is_note(c))
                    .eq(b.content.iter().filter(|c| !is_note(c)))
        }
        (Err(a), Err(b)) => a == b,
        _ => false,
    }
}

fn repeat_marker(result: &ToolResult<CallToolResult>) -> Option<&str> {
    result
        .as_ref()
        .ok()?
        .meta
        .as_ref()?
        .0
        .get(TRUSTED_TOOL_UPDATE_META_KEY)?
        .get(REPEAT_KEY)?
        .as_str()
}

fn set_repeat_marker(result: &mut CallToolResult, marker: &str) {
    let meta = result.meta.get_or_insert_with(Meta::new);
    let trusted = meta
        .0
        .entry(TRUSTED_TOOL_UPDATE_META_KEY.to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    if let Some(trusted) = trusted.as_object_mut() {
        trusted.insert(REPEAT_KEY.to_string(), Value::String(marker.to_string()));
    }
}

/// Where one call identity stands this turn: the output its latest run returned, how many runs in a
/// row returned exactly that, and where the model was last told so (a note or a decline).
struct Standing<'a> {
    output: &'a ToolResult<CallToolResult>,
    returned: usize,
    told_at: Option<usize>,
}

fn standing<'a>(turn: &[CompletedCall<'a>], call: &CallToolRequestParams) -> Option<Standing<'a>> {
    let mut standing: Option<Standing<'a>> = None;
    for (index, (previous, result)) in turn.iter().enumerate() {
        if !same_call(previous, call) {
            continue;
        }
        match (repeat_marker(result), standing.as_mut()) {
            (Some(SKIPPED), Some(s)) => s.told_at = Some(index),
            (Some(SKIPPED), None) => {}
            (marker, Some(s)) if same_output(s.output, result) => {
                s.returned += 1;
                if marker == Some(SAME_OUTPUT) {
                    s.told_at = Some(index);
                }
            }
            _ => {
                standing = Some(Standing {
                    output: result,
                    returned: 1,
                    told_at: None,
                })
            }
        }
    }
    standing
}

/// The reason to decline `call`: it already returned this output, the model was told, and every
/// call completed since then was itself a noted or declined repeat — nothing new came back.
pub fn settled_call_reason(messages: &[Message], call: &CallToolRequestParams) -> Option<String> {
    let turn = turn_calls(messages);
    let standing = standing(&turn, call)?;
    let told_at = standing.told_at?;
    turn[told_at + 1..]
        .iter()
        .all(|(_, result)| repeat_marker(result).is_some())
        .then(|| skipped_reason(standing.returned))
}

/// After `call` ran: when an earlier call of the turn, identical to it, returned this same output,
/// tell the model how many times in the result it receives. An error result is folded into an
/// error `CallToolResult` so the note has somewhere to go.
pub fn note_if_unchanged(
    messages: &[Message],
    call: &CallToolRequestParams,
    output: &mut ToolResult<CallToolResult>,
) {
    let turn = turn_calls(messages);
    let Some(standing) = standing(&turn, call) else {
        return;
    };
    if !same_output(standing.output, output) {
        return;
    }
    let mut result = match std::mem::replace(output, Ok(CallToolResult::success(vec![]))) {
        Ok(result) => result,
        Err(error) => CallToolResult::error(vec![Content::text(error.to_string())]),
    };
    result
        .content
        .push(Content::text(same_output_note(standing.returned)));
    set_repeat_marker(&mut result, SAME_OUTPUT);
    *output = Ok(result);
}

/// The result a declined repeat hands the model: the fact, not a refusal to continue.
pub fn skipped_result(reason: &str) -> CallToolResult {
    let mut result = CallToolResult::error(vec![Content::text(reason)]);
    set_repeat_marker(&mut result, SKIPPED);
    result
}

/// Declines a call that repeats a settled call. Enabled per agent: the swarm switches it off for its
/// workers, whose loops are supervised by the judge and whose golden benchmark predates this guard.
#[derive(Debug)]
pub struct RepetitionInspector {
    enabled: Arc<AtomicBool>,
}

impl RepetitionInspector {
    pub fn new(enabled: Arc<AtomicBool>) -> Self {
        Self { enabled }
    }
}

#[async_trait]
impl ToolInspector for RepetitionInspector {
    fn name(&self) -> &'static str {
        REPETITION_INSPECTOR_NAME
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    async fn inspect(
        &self,
        _session_id: &str,
        tool_requests: &[ToolRequest],
        messages: &[Message],
        _goose_mode: GooseMode,
    ) -> Result<Vec<InspectionResult>> {
        Ok(tool_requests
            .iter()
            .filter_map(|request| {
                let call = request.tool_call.as_ref().ok()?;
                let reason = settled_call_reason(messages, call)?;
                Some(InspectionResult {
                    tool_request_id: request.id.clone(),
                    action: InspectionAction::Deny,
                    reason,
                    confidence: 1.0,
                    inspector_name: REPETITION_INSPECTOR_NAME.to_string(),
                    finding_id: Some("REP-001".to_string()),
                })
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::object;

    const LOOP_CMD: &str =
        "cd /Users/mihaiperdum/.agents/skills/release-checklist && ls -la; echo \"exit=$?\"";
    const LOOP_OUT: &str =
        "total 8\n-rw-r--r--@ 1 mihaiperdum staff 3227 Sep 23 09:07 SKILL.md\nexit=0";
    // Session 20260923_32's two interleaved probes.
    const PROBE_CMD: &str =
        "python3 -c \"import os; print(os.path.realpath('/usr/bin/uvx'))\" 2>&1";
    const PROBE_OUT: &str = "/usr/bin/uvx";
    const EXTENSIONS: &str = "extensionmanager__search_available_extensions";
    const EXTENSIONS_OUT: &str = "Extensions available to enable:\n- todo\n- summarize";

    fn shell(command: &str) -> CallToolRequestParams {
        CallToolRequestParams::new("shell").with_arguments(object!({ "command": command }))
    }

    fn extensions() -> CallToolRequestParams {
        CallToolRequestParams::new(EXTENSIONS).with_arguments(object!({}))
    }

    /// Plays one round the way the agent loop does: inspect against the history, run (the tool
    /// returns `output`) or decline, note, then append request and response to the history.
    struct Chat {
        messages: Vec<Message>,
        inspector: RepetitionInspector,
        next_id: usize,
    }

    impl Chat {
        fn new() -> Self {
            Self {
                messages: vec![Message::user().with_text("install the fetch extension")],
                inspector: RepetitionInspector::new(Arc::new(AtomicBool::new(true))),
                next_id: 0,
            }
        }

        fn user(&mut self, text: &str) {
            self.messages.push(Message::user().with_text(text));
        }

        async fn call(&mut self, call: CallToolRequestParams, output: &str) -> CallToolResult {
            self.next_id += 1;
            let id = format!("call_{}", self.next_id);
            let request = ToolRequest {
                id: id.clone(),
                tool_call: Ok(call.clone()),
                metadata: None,
                tool_meta: None,
            };
            let denied = self
                .inspector
                .inspect(
                    "s",
                    std::slice::from_ref(&request),
                    &self.messages,
                    GooseMode::Auto,
                )
                .await
                .unwrap();
            let result = if denied.is_empty() {
                let mut result = Ok(CallToolResult::success(vec![Content::text(output)]));
                note_if_unchanged(&self.messages, &call, &mut result);
                result.unwrap()
            } else {
                assert_eq!(denied[0].action, InspectionAction::Deny);
                skipped_result(&denied[0].reason)
            };
            self.messages
                .push(Message::assistant().with_tool_request(id.clone(), Ok(call)));
            self.messages
                .push(Message::user().with_tool_response(id, Ok(result.clone())));
            result
        }
    }

    fn texts(result: &CallToolResult) -> Vec<&str> {
        result
            .content
            .iter()
            .filter_map(|c| c.as_text().map(|t| t.text.as_str()))
            .collect()
    }

    fn marker(result: &CallToolResult) -> Option<&str> {
        result
            .meta
            .as_ref()?
            .0
            .get(TRUSTED_TOOL_UPDATE_META_KEY)?
            .get(REPEAT_KEY)?
            .as_str()
    }

    #[tokio::test]
    async fn identical_call_with_the_same_output_is_noted_then_declined() {
        let mut chat = Chat::new();

        let first = chat.call(shell(LOOP_CMD), LOOP_OUT).await;
        assert_eq!(texts(&first), vec![LOOP_OUT]);
        assert_eq!(marker(&first), None);

        let second = chat.call(shell(LOOP_CMD), LOOP_OUT).await;
        let note = "This exact call already returned this same output 1 time this turn; it will not change. Use it, or take a different step.";
        assert_eq!(texts(&second), vec![LOOP_OUT, note]);
        assert_eq!(marker(&second), Some(SAME_OUTPUT));
        assert_ne!(second.is_error, Some(true));

        let third = chat.call(shell(LOOP_CMD), LOOP_OUT).await;
        assert_eq!(texts(&third), vec![skipped_reason(2).as_str()]);
        assert!(texts(&third)[0].starts_with(
            "Not run: this exact call already returned this same output 2 times this turn"
        ));
        assert_eq!(third.is_error, Some(true));
        assert_eq!(marker(&third), Some(SKIPPED));

        let fourth = chat.call(shell(LOOP_CMD), LOOP_OUT).await;
        assert_eq!(texts(&fourth), vec![skipped_reason(2).as_str()]);
    }

    #[tokio::test]
    async fn identical_call_with_different_output_is_progress_and_resets() {
        let mut chat = Chat::new();

        chat.call(shell("ls"), "a").await;
        let changed = chat.call(shell("ls"), "a\nb").await;
        assert_eq!(texts(&changed), vec!["a\nb"]);
        assert_eq!(marker(&changed), None);

        let changed_again = chat.call(shell("ls"), "a\nb\nc").await;
        assert_eq!(marker(&changed_again), None);

        let same = chat.call(shell("ls"), "a\nb\nc").await;
        assert_eq!(texts(&same), vec!["a\nb\nc", same_output_note(1).as_str()]);
        let declined = chat.call(shell("ls"), "a\nb\nc\nd").await;
        assert_eq!(marker(&declined), Some(SKIPPED));
    }

    #[tokio::test]
    async fn interleaved_repeats_with_the_same_outputs_are_noted_then_declined() {
        let mut chat = Chat::new();

        chat.call(shell(PROBE_CMD), PROBE_OUT).await;
        chat.call(extensions(), EXTENSIONS_OUT).await;

        let probe_again = chat.call(shell(PROBE_CMD), PROBE_OUT).await;
        assert_eq!(
            texts(&probe_again),
            vec![PROBE_OUT, same_output_note(1).as_str()]
        );
        let extensions_again = chat.call(extensions(), EXTENSIONS_OUT).await;
        assert_eq!(
            texts(&extensions_again),
            vec![EXTENSIONS_OUT, same_output_note(1).as_str()]
        );

        let probe_declined = chat.call(shell(PROBE_CMD), PROBE_OUT).await;
        assert_eq!(texts(&probe_declined), vec![skipped_reason(2).as_str()]);
        assert_eq!(marker(&probe_declined), Some(SKIPPED));
        let extensions_declined = chat.call(extensions(), EXTENSIONS_OUT).await;
        assert_eq!(marker(&extensions_declined), Some(SKIPPED));

        let probe_still_declined = chat.call(shell(PROBE_CMD), PROBE_OUT).await;
        assert_eq!(marker(&probe_still_declined), Some(SKIPPED));
    }

    #[tokio::test]
    async fn an_interleaved_poll_whose_output_changes_is_never_noted() {
        let mut chat = Chat::new();

        for (tick, status) in ["1 passed", "2 passed", "3 passed", "4 passed"]
            .into_iter()
            .enumerate()
        {
            let poll = chat.call(shell("cargo test 2>&1 | tail -1"), status).await;
            assert_eq!(texts(&poll), vec![status], "poll {tick}");
            assert_eq!(marker(&poll), None, "poll {tick}");
            chat.call(shell(PROBE_CMD), PROBE_OUT).await;
        }
    }

    #[tokio::test]
    async fn something_new_since_the_note_lets_the_repeat_run_again() {
        let mut chat = Chat::new();

        chat.call(shell("cat app.py"), "v1").await;
        let noted = chat.call(shell("cat app.py"), "v1").await;
        assert_eq!(marker(&noted), Some(SAME_OUTPUT));

        chat.call(shell("sed -i '' s/v1/v2/ app.py"), "(no output)")
            .await;
        let reread = chat.call(shell("cat app.py"), "v2").await;
        assert_eq!(texts(&reread), vec!["v2"]);
        assert_eq!(marker(&reread), None);

        chat.call(shell("cat app.py"), "v2").await;
        chat.call(shell("git diff --stat"), "1 file changed").await;
        let same_again = chat.call(shell("cat app.py"), "v2").await;
        assert_eq!(texts(&same_again), vec!["v2", same_output_note(2).as_str()]);
        assert_eq!(marker(&same_again), Some(SAME_OUTPUT));
    }

    #[tokio::test]
    async fn a_new_user_message_starts_a_new_turn() {
        let mut chat = Chat::new();

        chat.call(shell(LOOP_CMD), LOOP_OUT).await;
        chat.call(shell(LOOP_CMD), LOOP_OUT).await;
        chat.user("check it again");

        let fresh = chat.call(shell(LOOP_CMD), LOOP_OUT).await;
        assert_eq!(texts(&fresh), vec![LOOP_OUT]);
        assert_eq!(marker(&fresh), None);
    }

    #[tokio::test]
    async fn an_agent_only_nudge_does_not_start_a_new_turn() {
        let mut chat = Chat::new();

        chat.call(shell(LOOP_CMD), LOOP_OUT).await;
        chat.messages.push(
            Message::user()
                .with_text("Keep working. The grind goal is not yet complete")
                .agent_only(),
        );
        let noted = chat.call(shell(LOOP_CMD), LOOP_OUT).await;
        assert_eq!(marker(&noted), Some(SAME_OUTPUT));
    }

    #[tokio::test]
    async fn a_different_call_between_back_to_back_repeats_is_not_progress_when_it_repeats_too() {
        let mut chat = Chat::new();

        chat.call(shell(LOOP_CMD), LOOP_OUT).await;
        let noted = chat.call(shell(LOOP_CMD), LOOP_OUT).await;
        assert_eq!(marker(&noted), Some(SAME_OUTPUT));

        let other = chat.call(shell("cat SKILL.md"), "# Release").await;
        assert_eq!(marker(&other), None);

        let back = chat.call(shell(LOOP_CMD), LOOP_OUT).await;
        assert_eq!(texts(&back), vec![LOOP_OUT, same_output_note(2).as_str()]);
        let declined = chat.call(shell(LOOP_CMD), LOOP_OUT).await;
        assert_eq!(texts(&declined), vec![skipped_reason(3).as_str()]);
    }

    #[tokio::test]
    async fn same_name_with_different_arguments_is_a_different_call() {
        let mut chat = Chat::new();
        chat.call(shell("ls"), "x").await;
        let other_args = chat.call(shell("ls -la"), "x").await;
        assert_eq!(marker(&other_args), None);
    }

    #[test]
    fn an_identical_error_is_noted_as_an_error_result() {
        let call = shell("false");
        let error = rmcp::model::ErrorData::internal_error("boom", None);
        let messages = vec![
            Message::user().with_text("run it"),
            Message::assistant().with_tool_request("a", Ok(call.clone())),
            Message::user().with_tool_response("a", Err(error.clone())),
        ];
        let mut output = Err(error);
        note_if_unchanged(&messages, &call, &mut output);
        let result = output.unwrap();
        assert_eq!(result.is_error, Some(true));
        assert_eq!(texts(&result)[1], same_output_note(1));
        assert_eq!(marker(&result), Some(SAME_OUTPUT));
    }

    #[tokio::test]
    async fn a_disabled_guard_is_skipped_by_the_manager() {
        let enabled = Arc::new(AtomicBool::new(false));
        let inspector = RepetitionInspector::new(enabled.clone());
        assert!(!inspector.is_enabled());
        enabled.store(true, Ordering::Relaxed);
        assert!(inspector.is_enabled());
    }
}
