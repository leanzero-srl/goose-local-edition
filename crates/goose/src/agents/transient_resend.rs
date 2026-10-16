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
use goose_providers::conversation::Conversation;
use goose_providers::errors::ProviderError;
use goose_providers::retry::{should_retry, RetryConfig};

/// The resend state of one reply: the provider's transient-only retry policy, the attempts spent
/// on the call now in flight, and the rate-limit wait spent on it. `None` policy: this reply never
/// resends (a swarm worker, whose orchestrator re-dispatches the task itself, or a provider that
/// keeps its own conversation).
pub(crate) struct TransientResend {
    policy: Option<RetryConfig>,
    attempts: usize,
    rate_limit_streak: usize,
    rate_limit_waited: Duration,
}

/// How a failed call is sent again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Resend {
    /// A rate limit, waited out on the policy's own budget (`RetryConfig::next_rate_limit_wait`,
    /// the wait an HTTP 429 gets) without spending an attempt.
    RateLimitWait {
        delay: Duration,
        waited: Duration,
        budget: Duration,
    },
    /// One of the policy's quick resends: the attempt (1-based), the attempt count, the wait.
    Attempt {
        attempt: usize,
        attempts: usize,
        delay: Duration,
    },
}

impl Resend {
    pub(crate) fn delay(&self) -> Duration {
        match self {
            Resend::RateLimitWait { delay, .. } | Resend::Attempt { delay, .. } => *delay,
        }
    }
}

impl TransientResend {
    pub(crate) fn new(policy: Option<RetryConfig>) -> Self {
        Self {
            policy: policy.map(RetryConfig::transient_only),
            attempts: 0,
            rate_limit_streak: 0,
            rate_limit_waited: Duration::ZERO,
        }
    }

    pub(crate) fn enabled(&self) -> bool {
        self.policy.is_some()
    }

    fn rate_limit_wait(&self, policy: &RetryConfig, error: &ProviderError) -> Option<Duration> {
        policy.next_rate_limit_wait(error, self.rate_limit_streak + 1, self.rate_limit_waited)
    }

    /// Whether a call that failed with `error` is sent again: a rate limit with wait budget left, or
    /// a transient class with attempts left.
    pub(crate) fn allows(&self, error: &ProviderError) -> bool {
        self.policy.as_ref().is_some_and(|policy| {
            self.rate_limit_wait(policy, error).is_some()
                || (should_retry(error, policy) && self.attempts < policy.max_retries())
        })
    }

    /// Spends one rate-limit wait, or else one attempt; returns which, and the wait before it.
    pub(crate) fn spend(&mut self, error: &ProviderError) -> Resend {
        let policy = self
            .policy
            .as_ref()
            .expect("spend is reached only after allows() held");
        if let Some(delay) = self.rate_limit_wait(policy, error) {
            self.rate_limit_streak += 1;
            self.rate_limit_waited += delay;
            return Resend::RateLimitWait {
                delay,
                waited: self.rate_limit_waited,
                budget: policy.rate_limit_max_wait,
            };
        }
        self.attempts += 1;
        Resend::Attempt {
            attempt: self.attempts,
            attempts: policy.max_retries(),
            delay: policy.delay_before_retry(error, self.attempts),
        }
    }

    /// A call streamed to its end: the next failure starts a fresh count and a fresh wait budget.
    pub(crate) fn call_completed(&mut self) {
        self.attempts = 0;
        self.rate_limit_streak = 0;
        self.rate_limit_waited = Duration::ZERO;
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

/// Drops from a running copy of the reply the messages an [`AgentEvent::PartialDiscarded`] retracts:
/// for each, the last message equal to it.
///
/// [`AgentEvent::PartialDiscarded`]: crate::agents::AgentEvent::PartialDiscarded
pub fn drop_discarded(messages: &mut Vec<Message>, discarded: &[Message]) {
    for gone in discarded.iter().rev() {
        if let Some(at) = messages.iter().rposition(|message| message == gone) {
            messages.remove(at);
        }
    }
}

/// [`drop_discarded`] for a consumer that holds the reply as a [`Conversation`].
pub fn conversation_without(conversation: &Conversation, discarded: &[Message]) -> Conversation {
    let mut messages = conversation.messages().to_vec();
    drop_discarded(&mut messages, discarded);
    Conversation::new_unvalidated(messages)
}

/// What a consumer that already streamed a resent call's text onward (and cannot take it back)
/// ends on, instead of gluing the resent answer to the abandoned one.
pub const PARTIAL_ALREADY_STREAMED: &str = "The provider failed mid-answer after part of the \
     answer was streamed; goose resent the request, but streamed text cannot be withdrawn. Send \
     the request again.";

/// Whether a retracted partial had shown any text — what a consumer that streamed it onward (and
/// cannot take it back) has to answer for.
pub fn shown_text(discarded: &[Message]) -> bool {
    discarded.iter().any(|message| {
        message
            .content
            .iter()
            .any(|content| matches!(content, MessageContent::Text(text) if !text.text.is_empty()))
    })
}

pub(crate) fn resend_notice(
    error: &ProviderError,
    resend: Resend,
    answer_had_started: bool,
    kept_tool_calls: usize,
) -> String {
    let when = if answer_had_started {
        "mid-answer"
    } else {
        "before any answer arrived"
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
    match resend {
        Resend::RateLimitWait {
            delay,
            waited,
            budget,
        } => format!(
            "The provider rate-limited the request {when} ({error}). Resending after {delay:?} ({waited:?} of {budget:?} rate-limit wait used).{discarded}{kept}"
        ),
        Resend::Attempt {
            attempt, attempts, ..
        } => format!(
            "The provider failed {when} ({error}). Resending — attempt {attempt} of {attempts}.{discarded}{kept}"
        ),
    }
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
        assert!(matches!(resend.spend(&reset), Resend::Attempt { attempt: 1, .. }));
        assert!(resend.allows(&ProviderError::ServerError("502".into())));
        assert!(matches!(resend.spend(&reset), Resend::Attempt { attempt: 2, .. }));
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
    fn a_discarded_partial_leaves_the_rest_of_a_running_copy() {
        let earlier = Message::assistant()
            .with_id("a")
            .with_text("I will check the file.");
        let call = Message::assistant()
            .with_id("b")
            .with_tool_request("call-1", Ok(CallToolRequestParams::new("shell")));
        let first = Message::assistant().with_id("b").with_text("Now I will ");
        let second = Message::assistant().with_id("b").with_text("wri");
        let mut copy = vec![earlier.clone(), first.clone(), call.clone(), second.clone()];
        drop_discarded(&mut copy, &[first, second]);
        assert_eq!(copy, vec![earlier, call]);
    }

    fn attempt(attempt: usize, attempts: usize) -> Resend {
        Resend::Attempt {
            attempt,
            attempts,
            delay: Duration::ZERO,
        }
    }

    fn in_stream_rate_limit() -> ProviderError {
        ProviderError::RateLimitExceeded {
            details: "Provider returned error".into(),
            retry_delay: None,
        }
    }

    /// The wait an HTTP 429 gets before the stream opens (`retry::retry_operation`) is the wait a
    /// rate limit reported inside the stream gets: on the policy's own budget, no attempt spent.
    #[test]
    fn a_rate_limit_is_waited_out_on_its_budget_before_any_attempt_is_spent() {
        let policy = RetryConfig::new(3, 0, 1.0, 0).rate_limit_wait(Duration::from_secs(1_800));
        let mut resend = TransientResend::new(Some(policy));
        let limited = in_stream_rate_limit();

        let mut waits = Vec::new();
        loop {
            assert!(resend.allows(&limited));
            match resend.spend(&limited) {
                Resend::RateLimitWait { delay, .. } => waits.push(delay.as_secs()),
                Resend::Attempt { .. } => break,
            }
        }
        assert_eq!(
            waits[..6],
            [5, 10, 20, 40, 80, 120],
            "the policy's backoff, capped at its max interval"
        );
        assert_eq!(waits.iter().sum::<u64>(), 1_715, "the budget, never passed");
        assert_eq!(
            resend.attempts, 1,
            "a spent budget hands over to the quick resends"
        );
        assert!(resend.allows(&limited));
        resend.spend(&limited);
        resend.spend(&limited);
        assert!(!resend.allows(&limited), "then the attempts run out as before");

        resend.call_completed();
        assert!(matches!(
            resend.spend(&limited),
            Resend::RateLimitWait { delay, .. } if delay == Duration::from_secs(5)
        ), "a completed call starts a fresh budget");
    }

    #[test]
    fn a_server_fault_still_spends_the_quick_resends() {
        let policy = RetryConfig::new(3, 0, 1.0, 0).rate_limit_wait(Duration::from_secs(1_800));
        let mut resend = TransientResend::new(Some(policy));
        let fault = ProviderError::ServerError("Provider returned error".into());
        for n in 1..=3 {
            assert!(resend.allows(&fault));
            assert!(matches!(resend.spend(&fault), Resend::Attempt { attempt, .. } if attempt == n));
        }
        assert!(!resend.allows(&fault));
    }

    #[test]
    fn a_zero_rate_limit_budget_keeps_the_old_quick_resends() {
        let policy = RetryConfig::new(3, 0, 1.0, 0).rate_limit_wait(Duration::ZERO);
        let mut resend = TransientResend::new(Some(policy));
        assert!(matches!(
            resend.spend(&in_stream_rate_limit()),
            Resend::Attempt { attempt: 1, .. }
        ));
    }

    #[test]
    fn the_notice_names_the_error_and_the_attempt() {
        let reset = ProviderError::stream_decode_error("connection reset");
        assert_eq!(
            resend_notice(&reset, attempt(1, 3), true, 0),
            "The provider failed mid-answer (Network error: Stream decode error: connection reset). \
             Resending — attempt 1 of 3. The unfinished answer was discarded."
        );
        assert_eq!(
            resend_notice(&reset, attempt(2, 3), false, 2),
            "The provider failed before any answer arrived (Network error: Stream decode error: connection reset). \
             Resending — attempt 2 of 3. The 2 tool calls it already ran are kept with their results."
        );
        assert_eq!(
            resend_notice(
                &in_stream_rate_limit(),
                Resend::RateLimitWait {
                    delay: Duration::from_secs(10),
                    waited: Duration::from_secs(15),
                    budget: Duration::from_secs(1_800),
                },
                false,
                0
            ),
            "The provider rate-limited the request before any answer arrived (Rate limit exceeded: \
             Provider returned error). Resending after 10s (15s of 1800s rate-limit wait used)."
        );
    }
}
