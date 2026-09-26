//! A tool call an OpenAI-compatible engine returned as TEXT must not pass for a finished reply
//! (Q-133).
//!
//! Measured on goose 3.0.49, 2026-09-26: Qwen3.8-Flash on the Rapid-MLX pipeline split called
//! `bash` where the request declared `shell`. The engine's parser refuses an undeclared name, so
//! the whole `<tool_call>\n<function=bash>…` block streamed as `content`; goose showed the XML as
//! the answer, the command never ran and the turn ended. Two signals now turn such a reply into a
//! FAILED tool request — the agent answers it with the error and the model gets another step:
//!
//! - the engine SAYS so: the fork's final choice carries
//!   `refused_tool_calls: [{"name": "bash"}]` (Rapid-MLX `lz-pipeline-qwen4.4`);
//! - or the whole visible reply IS one framed call block (`<tool_call>`/`<function=` … closed),
//!   which no answer is — any engine, flagged or not.
//!
//! A call block after prose on an engine that does not flag it stays text: honestly
//! under-included, because prose can quote the wire format.

use crate::conversation::message::MessageContent;
use regex::Regex;
use rmcp::model::{ErrorCode, ErrorData};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::sync::OnceLock;

const CALL_OPENERS: [&str; 2] = ["<tool_call>", "<function="];
const CALL_CLOSERS: [&str; 2] = ["</tool_call>", "</function>"];

/// One entry of the engine's `refused_tool_calls` choice field.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct RefusedToolCall {
    pub name: String,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum HoldState {
    Undecided,
    Passing,
    Holding,
}

/// Withholds a reply's text only while it may still be a bare call block: leading whitespace,
/// then a prefix of an opener. The first byte that rules that out releases everything held.
#[derive(Debug)]
pub struct UnparsedCallHold {
    held: String,
    state: HoldState,
}

/// Whether a reply that is only tool-call markup becomes a failed tool call or stays text for a
/// caller that parses such text itself (ollama's XML fallback wraps the OpenAI decoder).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnparsedToolCalls {
    Fail,
    KeepAsText,
}

impl UnparsedCallHold {
    pub fn new(mode: UnparsedToolCalls) -> Self {
        let state = match mode {
            UnparsedToolCalls::Fail => HoldState::Undecided,
            UnparsedToolCalls::KeepAsText => HoldState::Passing,
        };
        Self {
            held: String::new(),
            state,
        }
    }

    /// The text to show now.
    pub fn push(&mut self, text: &str) -> String {
        match self.state {
            HoldState::Passing => text.to_string(),
            HoldState::Holding => {
                self.held.push_str(text);
                String::new()
            }
            HoldState::Undecided => {
                self.held.push_str(text);
                let lead = self.held.trim_start();
                if lead.is_empty() || CALL_OPENERS.iter().any(|o| o.starts_with(lead)) {
                    return String::new();
                }
                if CALL_OPENERS.iter().any(|o| lead.starts_with(o)) {
                    self.state = HoldState::Holding;
                    return String::new();
                }
                self.state = HoldState::Passing;
                std::mem::take(&mut self.held)
            }
        }
    }

    /// Everything held, as text to show; later pushes pass straight through.
    pub fn release(&mut self) -> String {
        self.state = HoldState::Passing;
        std::mem::take(&mut self.held)
    }

    /// End of the reply: the held text is either one whole call block (returned as `Err`
    /// with the block) or ordinary text to show (`Ok`).
    pub fn finish(&mut self) -> Result<String, String> {
        let whole_block = self.state == HoldState::Holding && is_whole_call_block(&self.held);
        let held = self.release();
        if whole_block {
            Err(held)
        } else {
            Ok(held)
        }
    }
}

/// A whole (non-streamed) reply through the same hold.
pub fn settle_text(text: &str, mode: UnparsedToolCalls) -> Result<String, String> {
    let mut hold = UnparsedCallHold::new(mode);
    let shown = hold.push(text);
    match hold.finish() {
        Ok(rest) => Ok(shown + &rest),
        Err(block) => Err(block),
    }
}

fn is_whole_call_block(text: &str) -> bool {
    let body = text.trim();
    CALL_OPENERS.iter().any(|o| body.starts_with(o))
        && CALL_CLOSERS.iter().any(|c| body.ends_with(c))
        && call_name(body).is_some()
}

fn call_name(text: &str) -> Option<&str> {
    static NAME: OnceLock<Regex> = OnceLock::new();
    NAME.get_or_init(|| Regex::new(r"<function=([^>\s]+)>").expect("static regex"))
        .captures(text)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str())
}

/// The failed tool request that replaces (or follows) the text: the agent answers it with this
/// error, so the model sees why nothing ran and calls again.
pub fn unparsed_call_request(refused: Option<&str>, block: &str) -> MessageContent {
    let block = block.trim();
    let message = match (refused, call_name(block)) {
        (Some(name), _) => format!(
            "The engine refused your tool call to `{name}`: `{name}` is not one of the tools this \
             request declared, so the call came back as plain text and did not run. Call one of \
             the declared tools by its exact name.{}",
            quoted_block(block)
        ),
        (None, Some(name)) => format!(
            "Your tool call to `{name}` came back from the engine as plain text, not as a tool \
             call, so it did not run: the engine did not parse it (check that `{name}` is one \
             of the declared tools).{}",
            quoted_block(block)
        ),
        (None, None) => format!(
            "The engine returned tool-call markup as plain text, not as a tool call, so nothing \
             ran.{}",
            quoted_block(block)
        ),
    };
    MessageContent::tool_request(
        format!("unparsed_{}", uuid::Uuid::new_v4().simple()),
        Err(ErrorData {
            code: ErrorCode::INVALID_REQUEST,
            message: Cow::from(message),
            data: None,
        }),
    )
}

fn quoted_block(block: &str) -> String {
    if block.is_empty() {
        String::new()
    } else {
        format!(" What arrived:\n{block}")
    }
}

/// The failed requests for a finished reply. `text` is what the hold settled on (`Err` = the
/// reply was one whole call block); `refused` is the engine's own list. Returns the text still
/// to show and the requests to add after it.
pub fn settle_reply(
    text: Result<String, String>,
    refused: &[RefusedToolCall],
) -> (String, Vec<MessageContent>) {
    let (shown, block) = match text {
        Ok(shown) => (shown, None),
        Err(block) => (String::new(), Some(block)),
    };
    if !refused.is_empty() {
        let quoted = block.as_deref().unwrap_or("");
        let requests = refused
            .iter()
            .map(|call| unparsed_call_request(Some(&call.name), quoted))
            .collect();
        return (shown, requests);
    }
    match block {
        Some(block) => (shown, vec![unparsed_call_request(None, &block)]),
        None => (shown, Vec::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// goose's raw log of the Q-133 call: the content deltas exactly as they arrived.
    const Q133_CONTENT: [&str; 12] = [
        "\n\n",
        "<tool_call>\n<function=bash>",
        "\n",
        "<parameter",
        "=",
        "command",
        ">",
        "\n",
        "cd /work && cp /tmp/VENDORED.md ./VENDORED.md && wc -l VENDORED.md",
        "\n</parameter",
        ">\n</function",
        "></tool_call>",
    ];

    fn error_text(content: &MessageContent) -> String {
        match content {
            MessageContent::ToolRequest(request) => match &request.tool_call {
                Err(error) => error.message.to_string(),
                Ok(call) => panic!("expected a failed request, got a call to {}", call.name),
            },
            other => panic!("expected a tool request, got {other:?}"),
        }
    }

    #[test]
    fn the_q133_reply_shows_nothing_and_becomes_a_failed_request() {
        let mut hold = UnparsedCallHold::new(UnparsedToolCalls::Fail);
        let shown: String = Q133_CONTENT.iter().map(|d| hold.push(d)).collect();
        assert_eq!(shown, "");

        let refused = [RefusedToolCall {
            name: "bash".into(),
        }];
        let (text, requests) = settle_reply(hold.finish(), &refused);
        assert_eq!(text, "");
        assert_eq!(requests.len(), 1);
        let error = error_text(&requests[0]);
        assert!(
            error.contains("refused your tool call to `bash`"),
            "{error}"
        );
        assert!(error.contains("<function=bash>"), "{error}");
    }

    #[test]
    fn an_unflagged_whole_block_is_still_a_failed_request() {
        let mut hold = UnparsedCallHold::new(UnparsedToolCalls::Fail);
        for delta in Q133_CONTENT {
            hold.push(delta);
        }
        let (text, requests) = settle_reply(hold.finish(), &[]);
        assert_eq!(text, "");
        let error = error_text(&requests[0]);
        assert!(error.contains("tool call to `bash` came back"), "{error}");
    }

    #[test]
    fn ordinary_replies_pass_at_the_first_telling_byte() {
        let mut hold = UnparsedCallHold::new(UnparsedToolCalls::Fail);
        assert_eq!(hold.push("\n"), "");
        assert_eq!(hold.push("<"), "");
        assert_eq!(hold.push("b>bold"), "\n<b>bold");
        assert_eq!(hold.push(" more"), " more");
        assert_eq!(
            settle_reply(hold.finish(), &[]),
            (String::new(), Vec::new())
        );

        let mut hold = UnparsedCallHold::new(UnparsedToolCalls::Fail);
        assert_eq!(hold.push("Done."), "Done.");
    }

    #[test]
    fn an_unclosed_block_is_shown_as_text() {
        let mut hold = UnparsedCallHold::new(UnparsedToolCalls::Fail);
        hold.push("<tool_call>\n<function=shell>\n<parameter=command>\nls");
        let (text, requests) = settle_reply(hold.finish(), &[]);
        assert!(text.starts_with("<tool_call>"), "{text}");
        assert!(requests.is_empty());
    }

    #[test]
    fn prose_then_a_block_is_text_unless_the_engine_flags_it() {
        let reply = "Running it now.\n\n<tool_call>\n<function=bash>\n<parameter=command>\nls\n</parameter>\n</function>\n</tool_call>";
        let mut hold = UnparsedCallHold::new(UnparsedToolCalls::Fail);
        assert_eq!(hold.push(reply), reply);
        let (text, requests) = settle_reply(hold.finish(), &[]);
        assert_eq!(text, "");
        assert!(requests.is_empty());

        let mut hold = UnparsedCallHold::new(UnparsedToolCalls::Fail);
        hold.push(reply);
        let refused = [RefusedToolCall {
            name: "bash".into(),
        }];
        let (_, requests) = settle_reply(hold.finish(), &refused);
        assert_eq!(requests.len(), 1);
        assert!(error_text(&requests[0]).contains("`bash`"));
    }
}
