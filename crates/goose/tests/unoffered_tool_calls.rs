//! A model's tool call runs only against the tools its request OFFERED (after the MCP Apps
//! visibility filter and code mode's filter). Before this, `resolve_tool` matched every listed
//! tool, mapped any `ext__tool` to a loaded extension and recovered mangled names against the full
//! list, so a tool goose deliberately hid from the model ran when the model named it — and the
//! LeanZero MLX engine now hands such calls back instead of dropping them. These drive the real
//! agent loop with a scripted provider and check that each such call gets goose's
//! RESOURCE_NOT_FOUND tool result naming offered tools, that nothing runs, and that the turn goes on.

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

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
use rmcp::model::{CallToolRequestParams, ErrorCode, ErrorData, Tool};
use rmcp::object;

#[ctor::ctor]
fn hermetic_path_root() {
    goose_test_support::hermetic_path_root();
}

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

/// A frontend tool the model must never see: MCP Apps `_meta.ui.visibility = ["app"]`.
fn app_only_frontend_tool() -> Tool {
    let mut tool = Tool::new(
        "ui__secret".to_string(),
        "An app-only tool".to_string(),
        object!({ "type": "object", "properties": {} }),
    );
    let mut meta = serde_json::Map::new();
    meta.insert("ui".into(), serde_json::json!({ "visibility": ["app"] }));
    tool.meta = Some(rmcp::model::Meta(meta));
    tool
}

struct Turn {
    results: Vec<(String, Result<rmcp::model::CallToolResult, ErrorData>)>,
    action_required: usize,
    provider_calls: usize,
    handed_back: usize,
    ledger: Option<String>,
    final_text: String,
}

/// One user turn against `answers` with the ledger extension and an app-only frontend tool loaded.
/// Bounded: a hidden call handed to the frontend, or put to the person for approval, waits forever,
/// which is the failure these tests exist to catch.
async fn run_turn(project: &Path, mode: GooseMode, answers: Vec<Message>) -> Result<Turn> {
    let data = tempfile::tempdir()?;
    let session_manager = Arc::new(SessionManager::new(data.path().to_path_buf()));
    let agent = Agent::with_config(AgentConfig::new(
        session_manager.clone(),
        Arc::new(PermissionManager::new(data.path().to_path_buf())),
        None,
        mode,
        true,
        GoosePlatform::GooseCli,
    ));
    let session = session_manager
        .create_session(
            project.to_path_buf(),
            "unoffered-tool-calls".to_string(),
            SessionType::Hidden,
            mode,
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
    agent
        .add_extension(
            ExtensionConfig::Frontend {
                name: "ui".to_string(),
                description: "The app".to_string(),
                tools: vec![app_only_frontend_tool()],
                instructions: None,
                bundled: None,
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

    let mut turn = Turn {
        results: Vec::new(),
        action_required: 0,
        provider_calls: 0,
        handed_back: 0,
        ledger: None,
        final_text: String::new(),
    };
    let drive = async {
        let stream = agent
            .reply(
                Message::user().with_text("Record the readiness fact."),
                SessionConfig {
                    id: session.id.clone(),
                    schedule_id: None,
                    max_turns: Some(5),
                    retry_config: None,
                },
                None,
            )
            .await?;
        tokio::pin!(stream);
        while let Some(event) = stream.next().await {
            if let AgentEvent::Message(message) = event? {
                for content in &message.content {
                    match content {
                        MessageContent::ToolResponse(response) => turn
                            .results
                            .push((response.id.clone(), response.tool_result.clone())),
                        MessageContent::ActionRequired(_)
                        | MessageContent::FrontendToolRequest(_) => turn.action_required += 1,
                        MessageContent::Text(text) => turn.final_text.push_str(&text.text),
                        _ => {}
                    }
                }
            }
        }
        anyhow::Ok(())
    };
    tokio::time::timeout(Duration::from_secs(60), drive)
        .await
        .map_err(|_| {
            anyhow::anyhow!("the turn stalled: a hidden call was handed out to run or approve")
        })??;

    turn.provider_calls = provider.calls.load(Ordering::SeqCst);
    turn.handed_back = provider
        .seen
        .lock()
        .unwrap()
        .last()
        .iter()
        .flat_map(|messages| messages.iter())
        .flat_map(|m| m.content.iter())
        .filter(|c| matches!(c, MessageContent::ToolResponse(_)))
        .count();
    turn.ledger = std::fs::read_to_string(project.join(".goose").join("ledger.md")).ok();
    Ok(turn)
}

fn result_of<'a>(turn: &'a Turn, id: &str) -> &'a Result<rmcp::model::CallToolResult, ErrorData> {
    &turn
        .results
        .iter()
        .find(|(rid, _)| rid == id)
        .unwrap_or_else(|| panic!("no tool response for {id}: {:?}", turn.results))
        .1
}

fn assert_not_offered(turn: &Turn, id: &str, called: &str) {
    let err = result_of(turn, id)
        .as_ref()
        .expect_err(&format!("{id} ({called}) must be refused"));
    assert_eq!(err.code, ErrorCode::RESOURCE_NOT_FOUND, "{id}: {err:?}");
    let msg = err.message.to_string();
    assert!(msg.contains(&format!("Tool '{called}' not found")), "{msg}");
    let listed = msg.split("available tools").nth(1).unwrap_or_default();
    assert!(
        listed.contains("ledger__ledger_append"),
        "names the offered tools: {msg}"
    );
    assert!(
        !listed.contains("ui__secret"),
        "never names a hidden tool: {msg}"
    );
}

fn call(name: &str) -> CallToolRequestParams {
    CallToolRequestParams::new(name.to_string()).with_arguments(object!({}))
}

#[tokio::test]
async fn unoffered_calls_are_not_found_and_the_turn_goes_on() -> Result<()> {
    let project = tempfile::tempdir()?;
    let answer = Message::assistant()
        .with_tool_request("call_read", Ok(call("read")))
        .with_tool_request("call_ghost", Ok(call("ledger__ghost")))
        .with_tool_request(
            "call_mangled",
            Ok(
                CallToolRequestParams::new("functions.ledger__ledger_append").with_arguments(
                    object!({ "kind": "fact", "text": "Harbourline runs Jira DC 9.12" }),
                ),
            ),
        );
    let turn = run_turn(project.path(), GooseMode::Auto, vec![answer]).await?;

    assert_not_offered(&turn, "call_read", "read");
    assert_not_offered(&turn, "call_ghost", "ledger__ghost");
    result_of(&turn, "call_mangled")
        .as_ref()
        .expect("a mangled name for an offered tool still runs");
    assert!(
        turn.ledger
            .as_deref()
            .is_some_and(|l| l.contains("Harbourline runs Jira DC 9.12")),
        "the recovered append ran: {:?}",
        turn.ledger
    );
    assert_eq!(turn.provider_calls, 2, "the turn went on to a second call");
    assert_eq!(turn.handed_back, 3, "the model was handed every result");
    assert!(turn.final_text.contains("Done."), "{}", turn.final_text);
    Ok(())
}

/// A hidden FRONTEND tool used to be handed to the app to run, and the turn waited on it.
#[tokio::test]
async fn a_hidden_frontend_tool_is_never_handed_to_the_app() -> Result<()> {
    let project = tempfile::tempdir()?;
    let answer = Message::assistant().with_tool_request("call_secret", Ok(call("ui__secret")));
    let turn = run_turn(project.path(), GooseMode::Auto, vec![answer]).await?;

    assert_not_offered(&turn, "call_secret", "ui__secret");
    assert_eq!(turn.action_required, 0, "nothing was handed out");
    assert_eq!(turn.provider_calls, 2, "the turn went on");
    Ok(())
}

/// In Approve mode a hidden call used to be put to the person, and the turn waited on the answer.
#[tokio::test]
async fn a_hidden_call_is_never_put_to_the_person_for_approval() -> Result<()> {
    let project = tempfile::tempdir()?;
    let answer = Message::assistant()
        .with_tool_request("call_ghost", Ok(call("ledger__ghost")))
        .with_tool_request("call_hidden", Ok(call("functions.hidden")));
    let turn = run_turn(project.path(), GooseMode::Approve, vec![answer]).await?;

    assert_not_offered(&turn, "call_ghost", "ledger__ghost");
    assert_not_offered(&turn, "call_hidden", "functions.hidden");
    assert_eq!(turn.action_required, 0, "no approval was asked");
    assert_eq!(turn.provider_calls, 2, "the turn went on");
    Ok(())
}
