//! A reply's provider call that fails with a transient error — a reset connection, a server fault,
//! a rate limit — is sent again under the provider's own retry policy instead of ending the turn.
//!
//! Measured 2026-10-02 (SB7.1 `openrouter-cloud-01dae737`): turn 87 of a live build died on
//! "Stream decode error: … connection reset" mid-answer, the reply loop wrote it as the session's
//! last assistant text, `goose run` exited, and the half-built app was scored as the result.
//!
//! What a failed attempt leaves behind decides what is resent. Its unfinished text and thinking
//! are discarded — the history never holds a half answer. A tool call it already RAN is kept with
//! its result: the request/result pair is complete, and resending without it would let the model
//! ask for the same call again and run it twice.

use std::time::Duration;

use goose_providers::conversation::message::{Message, MessageContent};
use goose_providers::errors::ProviderError;
use goose_providers::retry::{should_retry, RetryConfig};

/// The resend state of one reply: the provider's transient-only retry policy and the attempts spent
/// on the call now in flight. `None` policy: this reply never resends (a swarm worker, whose
/// orchestrator re-dispatches the task itself, or a provider that keeps its own conversation).
pub(crate) struct TransientResend {
    policy: Option<RetryConfig>,
    attempts: usize,
}

impl TransientResend {
    pub(crate) fn new(policy: Option<RetryConfig>) -> Self {
        Self {
            policy: policy.map(RetryConfig::transient_only),
            attempts: 0,
        }
    }

    pub(crate) fn enabled(&self) -> bool {
        self.policy.is_some()
    }

    /// Whether a call that failed with `error` is sent again: a transient class with attempts left.
    pub(crate) fn allows(&self, error: &ProviderError) -> bool {
        self.policy.as_ref().is_some_and(|policy| {
            should_retry(error, policy) && self.attempts < policy.max_retries()
        })
    }

    /// Spends one attempt; returns it (1-based), the policy's attempt count, and the wait before it.
    pub(crate) fn spend(&mut self, error: &ProviderError) -> (usize, usize, Duration) {
        let policy = self
            .policy
            .as_ref()
            .expect("spend is reached only after allows() held");
        self.attempts += 1;
        (
            self.attempts,
            policy.max_retries(),
            policy.delay_before_retry(error, self.attempts),
        )
    }

    /// A call streamed to its end: the next failure starts a fresh count.
    pub(crate) fn call_completed(&mut self) {
        self.attempts = 0;
    }
}

/// What survives a failed attempt: each tool call it already ran, with that call's result.
pub(crate) fn completed_tool_exchanges(
    messages: impl IntoIterator<Item = Message>,
) -> Vec<Message> {
    messages
        .into_iter()
        .filter(|message| {
            message.content.iter().any(|content| {
                matches!(
                    content,
                    MessageContent::ToolRequest(_) | MessageContent::ToolResponse(_)
                )
            })
        })
        .collect()
}

pub(crate) fn resend_notice(
    error: &ProviderError,
    attempt: usize,
    attempts: usize,
    answer_had_started: bool,
    kept_tool_calls: usize,
) -> String {
    let when = if answer_had_started {
        "mid-answer"
    } else {
        "before answering"
    };
    let discarded = if answer_had_started {
        " The unfinished answer was discarded."
    } else {
        ""
    };
    let kept = match kept_tool_calls {
        0 => String::new(),
        1 => " The tool call it already ran is kept with its result.".to_string(),
        n => format!(" The {n} tool calls it already ran are kept with their results."),
    };
    format!("The provider failed {when} ({error}). Resending — attempt {attempt} of {attempts}.{discarded}{kept}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::{CallToolRequestParams, CallToolResult, Content};

    fn policy(max_retries: usize) -> Option<RetryConfig> {
        Some(RetryConfig::new(max_retries, 0, 1.0, 0))
    }

    #[test]
    fn transient_classes_are_resent_until_the_policy_runs_out() {
        let mut resend = TransientResend::new(policy(2));
        let reset = ProviderError::stream_decode_error("connection reset");
        assert!(resend.allows(&reset));
        assert_eq!(resend.spend(&reset).0, 1);
        assert!(resend.allows(&ProviderError::ServerError("502".into())));
        assert_eq!(resend.spend(&reset).0, 2);
        assert!(
            !resend.allows(&reset),
            "the policy's two attempts are spent"
        );

        resend.call_completed();
        assert!(
            resend.allows(&reset),
            "a completed call starts a fresh count"
        );
    }

    #[test]
    fn permanent_classes_are_never_resent() {
        let resend = TransientResend::new(policy(3));
        for error in [
            ProviderError::Authentication("bad key".into()),
            ProviderError::ContextLengthExceeded("too long".into()),
            ProviderError::RequestFailed("Bad request (400)".into()),
            ProviderError::Refusal {
                details: "declined".into(),
                category: None,
            },
        ] {
            assert!(!resend.allows(&error), "{error:?} was resent");
        }
    }

    #[test]
    fn a_reply_without_a_policy_never_resends() {
        let resend = TransientResend::new(None);
        assert!(!resend.enabled());
        assert!(!resend.allows(&ProviderError::NetworkError("reset".into())));
    }

    #[test]
    fn a_failed_attempt_keeps_only_the_tool_calls_it_ran() {
        let call = Message::assistant()
            .with_thinking("check the file", "sig")
            .with_tool_request("call-1", Ok(CallToolRequestParams::new("shell")));
        let result = Message::user().with_tool_response(
            "call-1",
            Ok(CallToolResult::success(vec![Content::text("ok")])),
        );
        let partial_text = Message::assistant().with_text("Now I will wri");
        let partial_thinking = Message::assistant().with_thinking("half a thou", "");

        let kept = completed_tool_exchanges(vec![
            partial_thinking,
            call.clone(),
            result.clone(),
            partial_text,
        ]);
        assert_eq!(kept, vec![call, result]);
    }

    #[test]
    fn the_notice_names_the_error_and_the_attempt() {
        let reset = ProviderError::stream_decode_error("connection reset");
        assert_eq!(
            resend_notice(&reset, 1, 3, true, 0),
            "The provider failed mid-answer (Network error: Stream decode error: connection reset). \
             Resending — attempt 1 of 3. The unfinished answer was discarded."
        );
        assert_eq!(
            resend_notice(&reset, 2, 3, false, 2),
            "The provider failed before answering (Network error: Stream decode error: connection reset). \
             Resending — attempt 2 of 3. The 2 tool calls it already ran are kept with their results."
        );
    }
}
