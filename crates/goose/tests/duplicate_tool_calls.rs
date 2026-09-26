//! Q-159 (E2E #3d, 2026-09-26): one assistant response from a local model carried 57 tool calls —
//! a write, 54 × `ledger__ledger_append` with identical arguments, and two truncated calls. goose ran
//! every one, and the chat ledger got 54 identical entries. These drive the real agent loop with a
//! provider that answers that shape, the real ledger extension behind it, and check what the model
//! is handed back and what lands on disk.

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::Result;
use async_trait::async_trait;
use futures::StreamExt;
use goose::agents::extension::ExtensionConfig;
use goose::agents::{Agent, AgentConfig, AgentEvent, GoosePlatform, SessionConfig};
use goose::config::permission::PermissionManager;
use goose::config::GooseMode;
use goose::conversation::message::{Message, MessageContent};
use goose::providers::base::{
    stream_from_single_message, MessageStream, Provider, ProviderDef, ProviderMetadata,
};
use goose::session::session_manager::SessionType;
use goose::session::SessionManager;
use goose_providers::conversation::token_usage::{ProviderUsage, Usage};
use goose_providers::errors::ProviderError;
use goose_providers::model::ModelConfig;
use rmcp::model::{CallToolRequestParams, CallToolResult, Tool};
use rmcp::object;

#[ctor::ctor]
fn hermetic_path_root() {
    goose_test_support::hermetic_path_root();
}

const FACT: &str =
    "Harbourline Freight DC→Cloud readiness: Jira DC 9.12.x single node + postgres, \
                    ~15 projects";

/// Answers each model call with the next scripted message; records what the model was sent.
struct ScriptedProvider {
    answers: Vec<Message>,
    calls: AtomicUsize,
    seen: Mutex<Vec<Vec<Message>>>,
}

impl goose::providers::base::ProviderDescriptor for ScriptedProvider {
    fn metadata() -> ProviderMetadata {
        ProviderMetadata {
            name: "mock-scripted".to_string(),
            display_name: "Mock scripted provider".to_string(),
            description: "Answers from a script".to_string(),
            default_model: "mock-model".to_string(),
            known_models: vec![],
            model_doc_link: String::new(),
            config_keys: vec![],
            setup_steps: vec![],
            model_selection_hint: None,
            fast_model: None,
        }
    }
}

impl ProviderDef for ScriptedProvider {
    type Provider = Self;

    fn from_env(
        _extensions: Vec<ExtensionConfig>,
        _tls_config: Option<goose::providers::api_client::TlsConfig>,
    ) -> futures::future::BoxFuture<'static, anyhow::Result<Self>> {
        Box::pin(async { anyhow::bail!("scripted provider is built by the test") })
    }
}

#[async_trait]
impl Provider for ScriptedProvider {
    async fn stream(
        &self,
        _model_config: &ModelConfig,
        _system_prompt: &str,
        messages: &[Message],
        _tools: &[Tool],
    ) -> Result<MessageStream, ProviderError> {
        self.seen.lock().unwrap().push(messages.to_vec());
        let n = self.calls.fetch_add(1, Ordering::SeqCst);
        let message = self
            .answers
            .get(n)
            .cloned()
            .unwrap_or_else(|| Message::assistant().with_text("Done."));
        let usage = ProviderUsage::new(
            "mock-model".to_string(),
            Usage::new(Some(10), Some(5), Some(15)),
        );
        Ok(stream_from_single_message(message, usage))
    }

    fn get_name(&self) -> &str {
        "mock-scripted"
    }
}

fn append(kind: &str, text: &str) -> CallToolRequestParams {
    CallToolRequestParams::new("ledger__ledger_append")
        .with_arguments(object!({ "kind": kind, "text": text }))
}

fn text_of(result: &CallToolResult) -> String {
    result
        .content
        .iter()
        .filter_map(|c| c.as_text().map(|t| t.text.clone()))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Runs one user turn against `answers` in a fresh project folder with the ledger extension on.
/// Returns every tool response the model was handed, by request id, and the ledger file's text.
async fn run_turn(
    project: &Path,
    answers: Vec<Message>,
) -> Result<(Vec<(String, CallToolResult)>, String)> {
    let data = tempfile::tempdir()?;
    let session_manager = Arc::new(SessionManager::new(data.path().to_path_buf()));
    let agent = Agent::with_config(AgentConfig::new(
        session_manager.clone(),
        Arc::new(PermissionManager::new(data.path().to_path_buf())),
        None,
        GooseMode::Auto,
        true,
        GoosePlatform::GooseCli,
    ));
    let session = session_manager
        .create_session(
            project.to_path_buf(),
            "duplicate-tool-calls".to_string(),
            SessionType::Hidden,
            GooseMode::Auto,
        )
        .await?;
    agent
        .add_extension(
            ExtensionConfig::Platform {
                name: "ledger".to_string(),
                description: "Ledger".to_string(),
                display_name: Some("Ledger".to_string()),
                bundled: Some(true),
                available_tools: vec![],
            },
            &session.id,
        )
        .await?;
    let provider = Arc::new(ScriptedProvider {
        answers,
        calls: AtomicUsize::new(0),
        seen: Mutex::new(Vec::new()),
    });
    agent
        .update_provider(
            provider.clone(),
            ModelConfig::new("mock-model"),
            &session.id,
        )
        .await?;

    let stream = agent
        .reply(
            Message::user().with_text("Assess Harbourline's readiness."),
            SessionConfig {
                id: session.id,
                schedule_id: None,
                max_turns: Some(5),
                retry_config: None,
            },
            None,
        )
        .await?;
    tokio::pin!(stream);
    let mut responses = Vec::new();
    while let Some(event) = stream.next().await {
        if let AgentEvent::Message(message) = event? {
            for content in &message.content {
                if let MessageContent::ToolResponse(response) = content {
                    let result = response
                        .tool_result
                        .clone()
                        .map_err(|e| anyhow::anyhow!("{} failed: {e}", response.id))?;
                    responses.push((response.id.clone(), result));
                }
            }
        }
    }

    let last_request = provider.seen.lock().unwrap().last().cloned();
    let handed_back: usize = last_request
        .iter()
        .flatten()
        .flat_map(|m| m.content.iter())
        .filter(|c| matches!(c, MessageContent::ToolResponse(_)))
        .count();
    assert_eq!(
        handed_back,
        responses.len(),
        "the model's next request carries every tool response the loop produced"
    );

    let ledger = std::fs::read_to_string(project.join(".goose").join("ledger.md"))?;
    Ok((responses, ledger))
}

#[tokio::test]
async fn copies_of_a_call_in_one_answer_run_once_and_are_named() -> Result<()> {
    let project = tempfile::tempdir()?;
    let mut answer = Message::assistant().with_tool_request("call_1", Ok(append("fact", FACT)));
    for n in 2..=5 {
        answer = answer.with_tool_request(format!("call_{n}"), Ok(append("fact", FACT)));
    }
    answer = answer.with_tool_request(
        "call_6",
        Ok(append("decision", "migrate Confluence before Jira")),
    );

    let (responses, ledger) = run_turn(project.path(), vec![answer]).await?;

    let by_id = |id: &str| {
        responses
            .iter()
            .find(|(rid, _)| rid == id)
            .map(|(_, r)| r.clone())
            .unwrap_or_else(|| panic!("no tool response for {id}: {responses:?}"))
    };
    assert_eq!(
        responses.len(),
        6,
        "every call gets a result: {responses:?}"
    );
    assert!(
        text_of(&by_id("call_1")).starts_with("Appended [fact] to .goose/ledger.md"),
        "{:?}",
        by_id("call_1")
    );
    for n in 2..=5 {
        let copy = by_id(&format!("call_{n}"));
        assert_eq!(copy.is_error, Some(true));
        assert_eq!(
            text_of(&copy),
            "Not run: identical to call #1 in this same answer (ledger__ledger_append, id call_1) \
             — same tool, same arguments, so that call's result is this call's result too. Ask for \
             each call once per answer."
        );
        let marker = copy
            .meta
            .as_ref()
            .and_then(|m| m.0.get("__goose_tool_update_meta"))
            .and_then(|g| g.get("repeat"))
            .and_then(|r| r.as_str());
        assert_eq!(
            marker,
            Some("in_answer"),
            "the desktop's marker rides the result"
        );
    }
    assert!(text_of(&by_id("call_6")).starts_with("Appended [decision]"));

    let entries: Vec<&str> = ledger.lines().filter(|l| l.starts_with("- ")).collect();
    assert_eq!(entries.len(), 2, "{ledger}");
    assert_eq!(entries.iter().filter(|e| e.contains(FACT)).count(), 1);
    Ok(())
}

#[tokio::test]
async fn an_entry_the_ledger_already_holds_is_refused_with_where_it_is() -> Result<()> {
    let project = tempfile::tempdir()?;
    let answers = vec![
        Message::assistant().with_tool_request("call_1", Ok(append("fact", FACT))),
        Message::assistant().with_tool_request("call_2", Ok(append("fact", FACT))),
    ];

    let (responses, ledger) = run_turn(project.path(), answers).await?;

    assert_eq!(responses.len(), 2, "{responses:?}");
    assert!(text_of(&responses[0].1).starts_with("Appended [fact]"));
    assert_eq!(responses[1].0, "call_2");
    assert_eq!(
        text_of(&responses[1].1),
        "Not appended: this [fact] entry is already in .goose/ledger.md as entry 1 of 1, word for \
         word. Append only what the ledger does not hold yet."
    );
    assert_ne!(responses[1].1.is_error, Some(true), "nothing failed");
    assert_eq!(
        ledger.lines().filter(|l| l.starts_with("- ")).count(),
        1,
        "{ledger}"
    );
    Ok(())
}
