//! Q-171 / Q-97: a session whose title is still the first-words stump the swarm pool stores when
//! the turn itself holds the node ("Hi. I'm starting a" on a 1,361-message session) is asked again
//! at the end of a later completed turn, when the node is free.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use futures::StreamExt;
use goose::agents::{Agent, SessionConfig};
use goose::config::GooseMode;
use goose::conversation::message::Message;
use goose::providers::base::{stream_from_single_message, MessageStream, Provider};
use goose::session::session_manager::SessionType;
use goose_providers::conversation::token_usage::{ProviderUsage, Usage};
use goose_providers::errors::ProviderError;
use goose_providers::model::ModelConfig;
use rmcp::model::Tool;

#[ctor::ctor]
fn hermetic_path_root() {
    goose_test_support::hermetic_path_root();
}

const TITLE: &str = "Jira Migration Readiness";
const STUMP: &str = "Hi. I'm starting a";

/// Answers the title request with a title and every other call with a short reply.
struct TitleAnsweringProvider;

#[async_trait]
impl Provider for TitleAnsweringProvider {
    async fn stream(
        &self,
        _model_config: &ModelConfig,
        system: &str,
        _messages: &[Message],
        _tools: &[Tool],
    ) -> Result<MessageStream, ProviderError> {
        let text = if system.contains("four words or less") {
            TITLE
        } else {
            "Done."
        };
        Ok(stream_from_single_message(
            Message::assistant().with_text(text),
            ProviderUsage::new(
                "title-mock".to_string(),
                Usage::new(Some(10), Some(2), Some(12)),
            ),
        ))
    }

    fn get_name(&self) -> &str {
        "title-mock"
    }
}

#[tokio::test]
async fn a_stump_title_is_retried_at_the_end_of_a_completed_turn() -> Result<()> {
    let agent = Agent::new();
    let manager = agent.config.session_manager.clone();
    let session = manager
        .create_session(
            PathBuf::default(),
            "New Chat".to_string(),
            SessionType::User,
            GooseMode::default(),
        )
        .await?;
    for (prompt, reply) in [
        (
            "Hi. I'm starting a Jira Data Center to Cloud migration readiness assessment",
            "Sure.",
        ),
        ("Remind me what inactive cutoff we settled on", "24 months."),
        ("Draft the email to Aoife that goes with the PDF", "Drafted."),
    ] {
        manager
            .add_message(&session.id, &Message::user().with_text(prompt))
            .await?;
        manager
            .add_message(&session.id, &Message::assistant().with_text(reply))
            .await?;
    }
    manager
        .update(&session.id)
        .system_generated_name(STUMP)
        .apply()
        .await?;
    agent
        .update_provider(
            Arc::new(TitleAnsweringProvider),
            ModelConfig::new("title-mock-model"),
            &session.id,
        )
        .await?;

    let reply = agent
        .reply(
            Message::user().with_text("Last thing: write notes/status.md with a short checklist"),
            SessionConfig {
                id: session.id.clone(),
                schedule_id: None,
                max_turns: None,
                retry_config: None,
            },
            None,
        )
        .await?;
    tokio::pin!(reply);
    while let Some(event) = reply.next().await {
        event?;
    }

    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    let mut name = manager.get_session(&session.id, false).await?.name;
    while name != TITLE && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(20)).await;
        name = manager.get_session(&session.id, false).await?.name;
    }
    assert_eq!(
        name, TITLE,
        "the fourth prompt's completed turn asks for the title the stump never got"
    );
    Ok(())
}
