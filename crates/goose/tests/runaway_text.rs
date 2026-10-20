//! A reply that writes one unit again and again is stopped by goose, keeps one copy in the
//! history, and the turn continues (Grok 4.7 on Forge 2.0, 2026-10: four runaways of 286-325k
//! characters each ran to the 65,536-token cap and stayed whole in the history).

use anyhow::Result;
use async_trait::async_trait;
use futures::StreamExt;
use goose::agents::{Agent, SessionConfig};
use goose::config::GooseMode;
use goose::conversation::message::Message;
use goose::providers::base::{MessageStream, Provider};
use goose::session::session_manager::SessionType;
use goose_providers::conversation::token_usage::{ProviderUsage, Usage};
use goose_providers::errors::ProviderError;
use goose_providers::model::ModelConfig;
use rmcp::model::{Role, Tool};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tempfile::TempDir;

#[ctor::ctor]
fn hermetic_path_root() {
    goose_test_support::hermetic_path_root();
}

/// Grok 4.7's message 485, verbatim: the unit it repeated 2,728 times.
const UNIT: &str = "I'll cap each request at 200 issues.The field write grouped by value, so one \
                    request touched 342 issues. ";
const COPIES: usize = 2_728;
const DELTA_CHARS: usize = 24;

/// The chat's first call streams the runaway in small deltas, counting how many it handed out;
/// every later chat call answers once. The session's title request is answered apart.
struct RunawayProvider {
    calls: AtomicUsize,
    deltas_pulled: Arc<AtomicUsize>,
}

#[async_trait]
impl Provider for RunawayProvider {
    fn get_name(&self) -> &str {
        "runaway"
    }

    async fn stream(
        &self,
        _model_config: &ModelConfig,
        system: &str,
        _messages: &[Message],
        _tools: &[Tool],
    ) -> Result<MessageStream, ProviderError> {
        if system.starts_with("Generate a short title") {
            let title = Message::assistant().with_text("Field write fix");
            return Ok(Box::pin(futures::stream::iter([Ok((Some(title), None))])));
        }
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        let usage = ProviderUsage::new(
            "runaway".to_string(),
            Usage::new(Some(1_000), Some(10), Some(1_010)),
        );
        if call > 0 {
            let message = Message::assistant()
                .with_id(format!("answer-{call}"))
                .with_text("I'll take a different step: the cap is already in the source.");
            return Ok(Box::pin(futures::stream::iter([Ok((
                Some(message),
                Some(usage),
            ))])));
        }
        let text: Vec<char> = format!("Start. {}", UNIT.repeat(COPIES)).chars().collect();
        let pulled = self.deltas_pulled.clone();
        let deltas: Vec<String> = text
            .chunks(DELTA_CHARS)
            .map(|chunk| chunk.iter().collect())
            .collect();
        Ok(Box::pin(futures::stream::iter(deltas).map(move |delta| {
            pulled.fetch_add(1, Ordering::SeqCst);
            Ok((
                Some(Message::assistant().with_id("runaway-1").with_text(delta)),
                None,
            ))
        })))
    }
}

struct Run {
    pulled: usize,
    chat_calls: usize,
    stored: Vec<Message>,
}

/// One reply over the runaway provider with the watch's span set to `span` (`None`: default).
async fn run(span: Option<&str>) -> Result<Run> {
    let _env = env_lock::lock_env([
        ("GOOSE_RUNAWAY_TEXT_CHARS", span),
        ("GOOSE_DISABLE_SESSION_NAMING", Some("true")),
    ]);
    let dir = TempDir::new()?;
    let agent = Agent::new();
    let session = agent
        .config
        .session_manager
        .create_session(
            dir.path().to_path_buf(),
            "runaway".to_string(),
            SessionType::Hidden,
            GooseMode::default(),
        )
        .await?;
    let deltas_pulled = Arc::new(AtomicUsize::new(0));
    let provider = Arc::new(RunawayProvider {
        calls: AtomicUsize::new(0),
        deltas_pulled: deltas_pulled.clone(),
    });
    agent
        .update_provider(provider.clone(), ModelConfig::new("runaway"), &session.id)
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
    while let Some(event) = stream.next().await {
        event?;
    }
    let stored = agent
        .config
        .session_manager
        .get_session(&session.id, true)
        .await?
        .conversation
        .expect("a conversation")
        .messages()
        .clone();
    Ok(Run {
        pulled: deltas_pulled.load(Ordering::SeqCst),
        chat_calls: provider.calls.load(Ordering::SeqCst),
        stored,
    })
}

fn total_deltas() -> usize {
    format!("Start. {}", UNIT.repeat(COPIES))
        .chars()
        .count()
        .div_ceil(DELTA_CHARS)
}

#[tokio::test]
async fn a_runaway_reply_is_stopped_kept_once_and_the_turn_goes_on() -> Result<()> {
    let run = run(None).await?;
    assert!(
        run.pulled * 10 < total_deltas(),
        "the stream was stopped early: {} of {} deltas read",
        run.pulled,
        total_deltas()
    );
    assert!(run.chat_calls >= 2, "the turn continued after the stop");
    let runaway = run
        .stored
        .iter()
        .find(|m| m.role == Role::Assistant && m.as_concat_text().contains("I'll cap each"))
        .expect("the stopped reply is in the history")
        .as_concat_text();
    assert_eq!(
        runaway.matches("I'll cap each request").count(),
        1,
        "one copy kept: {runaway}"
    );
    assert!(
        runaway.contains(goose_providers::formats::openai::OUTPUT_STOPPED_AS_LOOP.trim()),
        "{runaway}"
    );
    assert!(runaway.chars().count() < 1_000, "{}", runaway.len());
    assert!(
        run.stored.iter().any(|m| m
            .as_concat_text()
            .contains("the cap is already in the source")),
        "the model's next reply is in the history"
    );
    Ok(())
}

/// The control: with the watch turned off the same stream is read to its end, so the count above
/// measures the stop and not the provider.
#[tokio::test]
async fn with_the_watch_off_the_runaway_is_read_to_its_end() -> Result<()> {
    let run = run(Some("0")).await?;
    assert_eq!(run.pulled, total_deltas());
    assert_eq!(run.chat_calls, 1, "nothing stopped it, so nothing nudged");
    Ok(())
}
