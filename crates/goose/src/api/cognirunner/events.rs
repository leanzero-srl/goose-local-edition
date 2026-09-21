//! The events a task pushes, and the ONE mapping from goose's reply stream onto them.
//!
//! What crosses the wire is a receipt, never content: a tool event carries the tool's name,
//! its phase, its verdict and a ≤300-char summary that is built from COUNTS and error text —
//! never the arguments (a shell command line) and never the result (file contents).

use crate::api::openai_compat::is_provider_error_message;
use crate::conversation::message::{ActionRequiredData, Message, MessageContent, TokenState};
use rmcp::model::Role;
use serde::{Deserialize, Serialize};

pub const SUMMARY_MAX_CHARS: usize = 300;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolPhase {
    Started,
    Finished,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub input_tokens: i32,
    pub output_tokens: i32,
    pub total_tokens: i32,
}

impl From<&TokenState> for Usage {
    fn from(t: &TokenState) -> Self {
        Self {
            input_tokens: t.input_tokens.max(0),
            output_tokens: t.output_tokens.max(0),
            total_tokens: t.total_tokens.max(0),
        }
    }
}

/// One event, tagged by `type`. The envelope ([`super::Envelope`]) adds `taskId`, `threadId`,
/// `seq` and `at` around it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum TaskEvent {
    #[serde(rename_all = "camelCase")]
    Started {
        model: String,
        session_id: String,
    },
    Text {
        text: String,
    },
    Tool {
        name: String,
        phase: ToolPhase,
        ok: Option<bool>,
        summary: String,
        /// The tool request id, so a `finished` joins its `started`.
        #[serde(rename = "ref")]
        reference: String,
    },
    Question {
        prompt: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        options: Option<Vec<String>>,
        /// The action-required id the answer would name.
        #[serde(rename = "ref")]
        reference: String,
    },
    #[serde(rename_all = "camelCase")]
    Done {
        finish_reason: String,
        usage: Usage,
    },
    Failed {
        error: String,
    },
}

impl TaskEvent {
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Done { .. } | Self::Failed { .. })
    }
}

pub fn clamp(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max_chars.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// A streamed assistant text, tagged with the message id it belongs to so the batcher can glue
/// deltas of one message and separate a new one.
#[derive(Debug, Clone, PartialEq)]
pub struct MappedText {
    pub id: Option<String>,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Mapped {
    Text(MappedText),
    Event(TaskEvent),
}

/// goose's reply stream → task events. Assistant text is text (or `failed` when it is the
/// agent's fixed provider-failure sign-off); a tool request is `tool/started`; a tool response
/// (which goose files under the USER role) is `tool/finished` with the verdict; a tool
/// confirmation or an elicitation is `question`; thinking is dropped.
pub fn map_message(message: &Message) -> Vec<Mapped> {
    let mut out = Vec::new();
    for content in &message.content {
        match content {
            MessageContent::Text(text) if message.role == Role::Assistant => {
                if text.text.is_empty() {
                    continue;
                }
                if is_provider_error_message(&text.text) {
                    out.push(Mapped::Event(TaskEvent::Failed {
                        error: text.text.clone(),
                    }));
                } else {
                    out.push(Mapped::Text(MappedText {
                        id: message.id.clone(),
                        text: text.text.clone(),
                    }));
                }
            }
            MessageContent::ToolRequest(request) => {
                let (name, summary) = match &request.tool_call {
                    Ok(call) => (
                        call.name.to_string(),
                        request.persisted_title().unwrap_or("").to_string(),
                    ),
                    Err(error) => ("unknown".to_string(), error.message.to_string()),
                };
                out.push(Mapped::Event(TaskEvent::Tool {
                    name,
                    phase: ToolPhase::Started,
                    ok: None,
                    summary: clamp(&summary, SUMMARY_MAX_CHARS),
                    reference: request.id.clone(),
                }));
            }
            MessageContent::ToolResponse(response) => {
                let (ok, summary) = match &response.tool_result {
                    Ok(result) => {
                        let failed = result.is_error.unwrap_or(false);
                        let items = result.content.len();
                        (
                            !failed,
                            format!(
                                "{} · {} item{}",
                                if failed { "error" } else { "ok" },
                                items,
                                if items == 1 { "" } else { "s" }
                            ),
                        )
                    }
                    Err(error) => (false, error.message.to_string()),
                };
                out.push(Mapped::Event(TaskEvent::Tool {
                    name: String::new(),
                    phase: ToolPhase::Finished,
                    ok: Some(ok),
                    summary: clamp(&summary, SUMMARY_MAX_CHARS),
                    reference: response.id.clone(),
                }));
            }
            MessageContent::ActionRequired(action) => match &action.data {
                ActionRequiredData::ToolConfirmation {
                    id,
                    tool_name,
                    prompt,
                    ..
                } => out.push(Mapped::Event(TaskEvent::Question {
                    prompt: clamp(
                        prompt
                            .as_deref()
                            .unwrap_or(&format!("Allow the tool {tool_name} to run?")),
                        SUMMARY_MAX_CHARS,
                    ),
                    options: Some(vec!["allow".to_string(), "deny".to_string()]),
                    reference: id.clone(),
                })),
                ActionRequiredData::Elicitation { id, message, .. } => {
                    out.push(Mapped::Event(TaskEvent::Question {
                        prompt: clamp(message, SUMMARY_MAX_CHARS),
                        options: None,
                        reference: id.clone(),
                    }))
                }
                ActionRequiredData::ElicitationResponse { .. } => {}
            },
            MessageContent::ToolConfirmationRequest(confirmation) => {
                out.push(Mapped::Event(TaskEvent::Question {
                    prompt: clamp(
                        confirmation.prompt.as_deref().unwrap_or(&format!(
                            "Allow the tool {} to run?",
                            confirmation.tool_name
                        )),
                        SUMMARY_MAX_CHARS,
                    ),
                    options: Some(vec!["allow".to_string(), "deny".to_string()]),
                    reference: confirmation.id.clone(),
                }))
            }
            MessageContent::Text(_)
            | MessageContent::Image(_)
            | MessageContent::Thinking(_)
            | MessageContent::RedactedThinking(_)
            | MessageContent::FrontendToolRequest(_)
            | MessageContent::SystemNotification(_) => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversation::message::{ToolRequest, ToolResponse};
    use rmcp::model::{CallToolRequestParams, CallToolResult, Content, ErrorData};

    fn request(id: &str, name: &str) -> MessageContent {
        MessageContent::ToolRequest(ToolRequest {
            id: id.to_string(),
            tool_call: Ok(CallToolRequestParams::new(name.to_string()).with_arguments(
                serde_json::from_value(serde_json::json!({"command": "cat /etc/passwd"})).unwrap(),
            )),
            metadata: None,
            tool_meta: None,
        })
    }

    fn response(id: &str, result: Result<CallToolResult, ErrorData>) -> MessageContent {
        MessageContent::ToolResponse(ToolResponse {
            id: id.to_string(),
            tool_result: result,
            metadata: None,
        })
    }

    #[test]
    fn assistant_text_is_text_tagged_with_its_message_id() {
        let message = Message::assistant().with_text("hello").with_id("m1");
        assert_eq!(
            map_message(&message),
            vec![Mapped::Text(MappedText {
                id: Some("m1".into()),
                text: "hello".into()
            })]
        );
        assert!(map_message(&Message::user().with_text("typed")).is_empty());
        assert!(map_message(&Message::assistant().with_text("")).is_empty());
    }

    #[test]
    fn a_provider_failure_phrased_as_a_message_is_failed() {
        let message = Message::assistant()
            .with_text("Ran into this error: boom.\n\nPlease resend your message to try again.");
        assert!(matches!(
            map_message(&message).as_slice(),
            [Mapped::Event(TaskEvent::Failed { error })] if error.contains("boom")
        ));
    }

    #[test]
    fn a_tool_request_is_started_without_its_arguments() {
        let mut message = Message::assistant();
        message.content.push(request("call-1", "developer__shell"));
        let mapped = map_message(&message);
        let Mapped::Event(TaskEvent::Tool {
            name,
            phase,
            ok,
            summary,
            reference,
        }) = &mapped[0]
        else {
            panic!("expected a tool event, got {mapped:?}");
        };
        assert_eq!(name, "developer__shell");
        assert_eq!(*phase, ToolPhase::Started);
        assert_eq!(*ok, None);
        assert_eq!(reference, "call-1");
        assert!(
            !summary.contains("passwd"),
            "arguments must never cross: {summary}"
        );
        assert!(!serde_json::to_string(&mapped_event(&mapped[0]))
            .unwrap()
            .contains("passwd"));
    }

    #[test]
    fn a_tool_response_is_finished_with_a_verdict_and_never_its_content() {
        let secret = "-----BEGIN PRIVATE KEY-----";
        let mut message = Message::user();
        message.content.push(response(
            "call-1",
            Ok(CallToolResult::success(vec![
                Content::text(secret),
                Content::text("more"),
            ])),
        ));
        message.content.push(response(
            "call-2",
            Ok(CallToolResult::error(vec![Content::text(
                "permission denied",
            )])),
        ));
        message.content.push(response(
            "call-3",
            Err(ErrorData::internal_error("tool crashed", None)),
        ));
        let mapped = map_message(&message);
        assert_eq!(mapped.len(), 3);
        let wire =
            serde_json::to_string(&mapped.iter().map(mapped_event).collect::<Vec<_>>()).unwrap();
        assert!(
            !wire.contains("PRIVATE KEY"),
            "content must never cross: {wire}"
        );
        assert!(!wire.contains("permission denied"));
        match (&mapped[0], &mapped[1], &mapped[2]) {
            (
                Mapped::Event(TaskEvent::Tool {
                    ok: Some(true),
                    summary: s1,
                    reference: r1,
                    phase: ToolPhase::Finished,
                    ..
                }),
                Mapped::Event(TaskEvent::Tool {
                    ok: Some(false),
                    summary: s2,
                    ..
                }),
                Mapped::Event(TaskEvent::Tool {
                    ok: Some(false),
                    summary: s3,
                    reference: r3,
                    ..
                }),
            ) => {
                assert_eq!(r1, "call-1");
                assert_eq!(s1, "ok · 2 items");
                assert_eq!(s2, "error · 1 item");
                assert_eq!(r3, "call-3");
                assert_eq!(s3, "tool crashed");
            }
            other => panic!("unexpected mapping {other:?}"),
        }
    }

    #[test]
    fn a_confirmation_and_an_elicitation_are_questions() {
        let mut message = Message::assistant();
        message.content.push(MessageContent::ActionRequired(
            crate::conversation::message::ActionRequired {
                data: ActionRequiredData::ToolConfirmation {
                    id: "c1".into(),
                    tool_name: "developer__shell".into(),
                    arguments: Default::default(),
                    prompt: None,
                },
            },
        ));
        message.content.push(MessageContent::ActionRequired(
            crate::conversation::message::ActionRequired {
                data: ActionRequiredData::Elicitation {
                    id: "e1".into(),
                    message: "main or develop?".into(),
                    requested_schema: serde_json::json!({}),
                },
            },
        ));
        let mapped = map_message(&message);
        assert_eq!(
            mapped,
            vec![
                Mapped::Event(TaskEvent::Question {
                    prompt: "Allow the tool developer__shell to run?".into(),
                    options: Some(vec!["allow".into(), "deny".into()]),
                    reference: "c1".into(),
                }),
                Mapped::Event(TaskEvent::Question {
                    prompt: "main or develop?".into(),
                    options: None,
                    reference: "e1".into(),
                }),
            ]
        );
    }

    #[test]
    fn summaries_are_clamped_to_300_chars() {
        let long = "x".repeat(1000);
        assert_eq!(clamp(&long, SUMMARY_MAX_CHARS).chars().count(), 300);
        assert_eq!(clamp("short", SUMMARY_MAX_CHARS), "short");
    }

    #[test]
    fn events_serialize_with_the_agreed_tags() {
        let done = TaskEvent::Done {
            finish_reason: "stop".into(),
            usage: Usage::from(&TokenState::default()),
        };
        let json = serde_json::to_value(&done).unwrap();
        assert_eq!(json["type"], "done");
        assert_eq!(json["finishReason"], "stop");
        assert_eq!(json["usage"]["totalTokens"], 0);
        let tool = TaskEvent::Tool {
            name: "n".into(),
            phase: ToolPhase::Finished,
            ok: Some(true),
            summary: "s".into(),
            reference: "r".into(),
        };
        let json = serde_json::to_value(&tool).unwrap();
        assert_eq!(json["type"], "tool");
        assert_eq!(json["phase"], "finished");
        assert_eq!(json["ref"], "r");
        let started = TaskEvent::Started {
            model: "m".into(),
            session_id: "s".into(),
        };
        assert_eq!(serde_json::to_value(&started).unwrap()["sessionId"], "s");
    }

    fn mapped_event(m: &Mapped) -> TaskEvent {
        match m {
            Mapped::Event(e) => e.clone(),
            Mapped::Text(t) => TaskEvent::Text {
                text: t.text.clone(),
            },
        }
    }
}
