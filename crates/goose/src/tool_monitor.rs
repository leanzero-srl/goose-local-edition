//! The repeat guard: a PROGRESS fact, not a count.
//!
//! A call identical (name + arguments) to the previous completed call that returns byte-identical
//! output made no progress. The first such call still runs and the model is told so in its result;
//! the next identical call is declined with the same fact, and every identical call after it too —
//! the turn never ends here. Any different call, or the same call returning different output,
//! breaks the chain.
//!
//! Measured 2026-09-23, desktop session 20260923_19 (Qwen3.8-27B over the MLX sidecar): 174
//! consecutive `cd /Users/mihaiperdum/.agents/skills/release-checklist && ls -la; echo "exit=$?"`
//! calls, one distinct output among them, until a human pressed Stop.
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
use rmcp::model::{CallToolRequestParams, CallToolResult, Content, Meta};
use serde_json::Value;

pub const REPEAT_SAME_OUTPUT_NOTE: &str = "This is the same call as the previous one and it returned the same output — the output will not change. Use it, or take a different step.";

pub const REPEAT_SKIPPED: &str = "Not run: this exact call already returned the same output twice in a row, so running it again would return that output again. Use it, or take a different step.";

pub const REPETITION_INSPECTOR_NAME: &str = "repetition";

const REPEAT_KEY: &str = "repeat";
const SAME_OUTPUT: &str = "same_output";
const SKIPPED: &str = "skipped";

struct PreviousCall<'a> {
    call: &'a ToolResult<CallToolRequestParams>,
    result: &'a ToolResult<CallToolResult>,
}

/// The most recent tool request that has a response. Calls issued in the same model response as the
/// candidate are not in `messages` yet — the model had not seen their output, so they are not
/// "the previous call" in the sense that matters here.
fn previous_call(messages: &[Message]) -> Option<PreviousCall<'_>> {
    let mut responses: HashMap<&str, &ToolResult<CallToolResult>> = HashMap::new();
    for message in messages.iter().rev() {
        for content in message.content.iter().rev() {
            match content {
                MessageContent::ToolResponse(response) => {
                    responses
                        .entry(response.id.as_str())
                        .or_insert(&response.tool_result);
                }
                MessageContent::ToolRequest(request) => {
                    if let Some(result) = responses.get(request.id.as_str()) {
                        return Some(PreviousCall {
                            call: &request.tool_call,
                            result,
                        });
                    }
                }
                _ => {}
            }
        }
    }
    None
}

fn same_call(previous: &ToolResult<CallToolRequestParams>, call: &CallToolRequestParams) -> bool {
    previous
        .as_ref()
        .is_ok_and(|p| p.name == call.name && p.arguments == call.arguments)
}

fn is_note(content: &Content) -> bool {
    content
        .as_text()
        .is_some_and(|t| t.text == REPEAT_SAME_OUTPUT_NOTE)
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

/// True when `call` repeats a call already shown to make no progress (it returned the same output
/// as its identical predecessor, or was itself declined for that reason).
pub fn repeats_a_settled_call(messages: &[Message], call: &CallToolRequestParams) -> bool {
    previous_call(messages).is_some_and(|previous| {
        same_call(previous.call, call)
            && matches!(repeat_marker(previous.result), Some(SAME_OUTPUT | SKIPPED))
    })
}

/// After `call` ran: when it is identical to the previous call and returned the same output, tell
/// the model so in the result it receives. An error result is folded into an error
/// `CallToolResult` so the note has somewhere to go.
pub fn note_if_unchanged(
    messages: &[Message],
    call: &CallToolRequestParams,
    output: &mut ToolResult<CallToolResult>,
) {
    let Some(previous) = previous_call(messages) else {
        return;
    };
    if !same_call(previous.call, call) || !same_output(previous.result, output) {
        return;
    }
    let mut result = match std::mem::replace(output, Ok(CallToolResult::success(vec![]))) {
        Ok(result) => result,
        Err(error) => CallToolResult::error(vec![Content::text(error.to_string())]),
    };
    result.content.push(Content::text(REPEAT_SAME_OUTPUT_NOTE));
    set_repeat_marker(&mut result, SAME_OUTPUT);
    *output = Ok(result);
}

/// The result a declined repeat hands the model: the fact, not a refusal to continue.
pub fn skipped_result() -> CallToolResult {
    let mut result = CallToolResult::error(vec![Content::text(REPEAT_SKIPPED)]);
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
                repeats_a_settled_call(messages, call).then(|| InspectionResult {
                    tool_request_id: request.id.clone(),
                    action: InspectionAction::Deny,
                    reason: REPEAT_SKIPPED.to_string(),
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

    fn shell(command: &str) -> CallToolRequestParams {
        CallToolRequestParams::new("shell").with_arguments(object!({ "command": command }))
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
                messages: vec![],
                inspector: RepetitionInspector::new(Arc::new(AtomicBool::new(true))),
                next_id: 0,
            }
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
                assert_eq!(denied[0].reason, REPEAT_SKIPPED);
                skipped_result()
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
        assert_eq!(texts(&second), vec![LOOP_OUT, REPEAT_SAME_OUTPUT_NOTE]);
        assert_eq!(marker(&second), Some(SAME_OUTPUT));
        assert_ne!(second.is_error, Some(true));

        let third = chat.call(shell(LOOP_CMD), LOOP_OUT).await;
        assert_eq!(texts(&third), vec![REPEAT_SKIPPED]);
        assert_eq!(third.is_error, Some(true));
        assert_eq!(marker(&third), Some(SKIPPED));

        let fourth = chat.call(shell(LOOP_CMD), LOOP_OUT).await;
        assert_eq!(texts(&fourth), vec![REPEAT_SKIPPED]);
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
        assert_eq!(texts(&same), vec!["a\nb\nc", REPEAT_SAME_OUTPUT_NOTE]);
        let declined = chat.call(shell("ls"), "a\nb\nc\nd").await;
        assert_eq!(texts(&declined), vec![REPEAT_SKIPPED]);
    }

    #[tokio::test]
    async fn a_different_call_resets_the_chain() {
        let mut chat = Chat::new();

        chat.call(shell(LOOP_CMD), LOOP_OUT).await;
        let noted = chat.call(shell(LOOP_CMD), LOOP_OUT).await;
        assert_eq!(marker(&noted), Some(SAME_OUTPUT));

        let other = chat.call(shell("cat SKILL.md"), "# Release").await;
        assert_eq!(marker(&other), None);

        let back = chat.call(shell(LOOP_CMD), LOOP_OUT).await;
        assert_eq!(texts(&back), vec![LOOP_OUT]);
        assert_eq!(marker(&back), None);
        let noted_again = chat.call(shell(LOOP_CMD), LOOP_OUT).await;
        assert_eq!(marker(&noted_again), Some(SAME_OUTPUT));
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
            Message::assistant().with_tool_request("a", Ok(call.clone())),
            Message::user().with_tool_response("a", Err(error.clone())),
        ];
        let mut output = Err(error);
        note_if_unchanged(&messages, &call, &mut output);
        let result = output.unwrap();
        assert_eq!(result.is_error, Some(true));
        assert_eq!(texts(&result)[1], REPEAT_SAME_OUTPUT_NOTE);
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
