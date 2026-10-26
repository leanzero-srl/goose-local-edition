//! A provider that refuses as too long only after it streamed part of the reply must still meet
//! the two-compaction stop. Since every OpenAI-format stream maps a mid-stream
//! `context_length_exceeded` to ContextLengthExceeded, a count reset on each streamed item would
//! compact and retry such a provider without end.

use anyhow::Result;
use async_trait::async_trait;
use futures::StreamExt;
use goose::agents::{Agent, AgentEvent, SessionConfig};
use goose::config::GooseMode;
use goose::conversation::message::{Message, MessageContent};
use goose::providers::base::{MessageStream, Provider};
use goose::session::session_manager::SessionType;
use goose_providers::conversation::token_usage::{ProviderUsage, Usage};
use goose_providers::errors::ProviderError;
use goose_providers::model::ModelConfig;
use rmcp::model::Tool;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tempfile::TempDir;

#[ctor::ctor]
fn hermetic_path_root() {
    goose_test_support::hermetic_path_root();
}

/// Beyond this many refused chat calls the provider answers, so a test of the old reset ends
/// instead of looping.
const GIVE_UP_AFTER: usize = 6;

/// Every chat call streams a partial reply and then refuses it as too long; compaction calls get a
/// summary.
struct RefusesMidStream {
    chat_calls: AtomicUsize,
}

fn is_compaction(system: &str, messages: &[Message]) -> bool {
    system.contains("Summarize this conversation")
        || messages.iter().any(|m| {
            let text = m.as_concat_text();
            text.contains("Please summarize the conversation history")
                || text.contains("Summarize this conversation for yourself")
        })
}

#[async_trait]
impl Provider for RefusesMidStream {
    fn get_name(&self) -> &str {
        "refuses-mid-stream"
    }

    async fn stream(
        &self,
        _model_config: &ModelConfig,
        system: &str,
        messages: &[Message],
        _tools: &[Tool],
    ) -> Result<MessageStream, ProviderError> {
        let usage = ProviderUsage::new(
            "refuses".to_string(),
            Usage::new(Some(1_000), Some(10), Some(1_010)),
        );
        if is_compaction(system, messages) {
            let summary =
                Message::assistant().with_text("SUMMARY: the chat fixes the field write.");
            return Ok(Box::pin(futures::stream::iter([Ok((
                Some(summary),
                Some(usage),
            ))])));
        }
        let call = self.chat_calls.fetch_add(1, Ordering::SeqCst);
        let partial = Message::assistant()
            .with_id(format!("partial-{call}"))
            .with_text("Looking at the batching code");
        if call >= GIVE_UP_AFTER {
            return Ok(Box::pin(futures::stream::iter([Ok((
                Some(partial),
                Some(usage),
            ))])));
        }
        Ok(Box::pin(futures::stream::iter([
            Ok((Some(partial), None)),
            Err(ProviderError::ContextLengthExceeded(
                "Provider returned error".to_string(),
            )),
        ])))
    }
}

#[tokio::test]
async fn a_refusal_after_streamed_text_still_stops_after_two_compactions() -> Result<()> {
    let _env = env_lock::lock_env([("GOOSE_DISABLE_SESSION_NAMING", Some("true"))]);
    let dir = TempDir::new()?;
    let agent = Agent::new();
    let session = agent
        .config
        .session_manager
        .create_session(
            dir.path().to_path_buf(),
            "mid-stream-refusal".to_string(),
            SessionType::Hidden,
            GooseMode::default(),
        )
        .await?;
    let provider = Arc::new(RefusesMidStream {
        chat_calls: AtomicUsize::new(0),
    });
    agent
        .update_provider(provider.clone(), ModelConfig::new("refuses"), &session.id)
        .await?;
    let config = SessionConfig {
        id: session.id.clone(),
        schedule_id: None,
        max_turns: None,
        retry_config: None,
    };
    let stream = agent
        .reply(
            Message::user().with_text("Fix the field write."),
            config,
            None,
        )
        .await?;
    tokio::pin!(stream);
    let mut notices = Vec::new();
    while let Some(event) = stream.next().await {
        if let AgentEvent::Message(message) = event? {
            for content in &message.content {
                if let MessageContent::SystemNotification(n) = content {
                    notices.push(n.msg.clone());
                }
            }
        }
    }
    assert_eq!(
        provider.chat_calls.load(Ordering::SeqCst),
        2,
        "the refused request went twice, then the turn stopped"
    );
    assert!(
        notices
            .iter()
            .any(|n| n.contains("Context limit still exceeded after compaction")),
        "{notices:?}"
    );
    Ok(())
}
