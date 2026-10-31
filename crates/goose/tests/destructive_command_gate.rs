//! F17 end to end: in the default `auto` mode, the exact command a local model ran unasked on
//! 2026-10-31 (`git reset --hard 1192c90`, which wiped four commits) must stop at the approval
//! card, and declining must hand the model a clear result instead of running it.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use futures::StreamExt;
use goose::agents::{Agent, AgentConfig, AgentEvent, GoosePlatform, SessionConfig};
use goose::config::permission::PermissionManager;
use goose::config::GooseMode;
use goose::conversation::message::{ActionRequiredData, Message, MessageContent};
use goose::permission::permission_confirmation::PrincipalType;
use goose::permission::{Permission, PermissionConfirmation};
use goose::providers::base::{
    stream_from_single_message, MessageStream, Provider, ProviderDef, ProviderMetadata,
};
use goose::session::session_manager::SessionType;
use goose::session::SessionManager;
use goose_providers::conversation::token_usage::{ProviderUsage, Usage};
use goose_providers::errors::ProviderError;
use goose_providers::model::ModelConfig;
use rmcp::model::{CallToolRequestParams, Tool};
use rmcp::object;
use tempfile::TempDir;

#[ctor::ctor]
fn hermetic_path_root() {
    goose_test_support::hermetic_path_root();
}

const F17_COMMAND: &str = "cd '/Users/mihaiperdum/goose-builds/quality/RU-2026-10-31-1-single-qwen35-9b/work' && git reset --hard 1192c90";

struct ShellOnceProvider {
    command: String,
    calls: AtomicUsize,
}

impl goose::providers::base::ProviderDescriptor for ShellOnceProvider {
    fn metadata() -> ProviderMetadata {
        ProviderMetadata {
            name: "mock-shell-once".to_string(),
            display_name: "Mock shell-once provider".to_string(),
            description: "Asks for one shell call, then answers in words".to_string(),
            default_model: "mock-model".to_string(),
            known_models: vec![],
            model_doc_link: "".to_string(),
            config_keys: vec![],
            setup_steps: vec![],
            model_selection_hint: None,
            fast_model: None,
        }
    }
}

impl ProviderDef for ShellOnceProvider {
    type Provider = Self;

    fn from_env(
        _extensions: Vec<goose::config::ExtensionConfig>,
        _tls_config: Option<goose::providers::api_client::TlsConfig>,
    ) -> futures::future::BoxFuture<'static, anyhow::Result<Self>> {
        Box::pin(async {
            Ok(Self {
                command: String::new(),
                calls: AtomicUsize::new(0),
            })
        })
    }
}

#[async_trait]
impl Provider for ShellOnceProvider {
    async fn stream(
        &self,
        _model_config: &ModelConfig,
        _system_prompt: &str,
        _messages: &[Message],
        _tools: &[Tool],
    ) -> Result<MessageStream, ProviderError> {
        let message = if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
            Message::assistant().with_tool_request(
                "call_f17",
                Ok(CallToolRequestParams::new("shell")
                    .with_arguments(object!({ "command": self.command.clone() }))),
            )
        } else {
            Message::assistant().with_text("Understood, I left the repository as it is.")
        };
        let usage = ProviderUsage::new(
            "mock-model".to_string(),
            Usage::new(Some(10), Some(5), Some(15)),
        );
        Ok(stream_from_single_message(message, usage))
    }

    fn get_name(&self) -> &str {
        "mock-shell-once"
    }
}

struct Outcome {
    prompts: Vec<Option<String>>,
    tool_result_texts: Vec<String>,
}

/// One turn in the DEFAULT mode (auto); every approval card is answered with `answer`.
async fn run_turn(command: &str, answer: Permission) -> Result<Outcome> {
    let project = project()?;
    run_turn_in(project.path().to_path_buf(), command, answer).await
}

fn git(dir: &std::path::Path, args: &[&str]) -> Result<()> {
    let ok = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()?
        .success();
    anyhow::ensure!(ok, "git {args:?} failed");
    Ok(())
}

/// A throwaway project that ignores node_modules/ and tracks src/.
fn project() -> Result<TempDir> {
    let dir = TempDir::new()?;
    let root = dir.path();
    git(root, &["init", "-q"])?;
    std::fs::write(root.join(".gitignore"), "node_modules/\ndata/\n")?;
    std::fs::create_dir_all(root.join("node_modules/pkg"))?;
    std::fs::create_dir_all(root.join("data"))?;
    std::fs::write(root.join("data/customers.csv"), "only copy")?;
    std::fs::create_dir_all(root.join("src"))?;
    std::fs::write(root.join("src/a.js"), "a")?;
    git(root, &["add", "."])?;
    git(
        root,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "commit",
            "-qm",
            "init",
        ],
    )?;
    Ok(dir)
}

async fn run_turn_in(working_dir: PathBuf, command: &str, answer: Permission) -> Result<Outcome> {
    let temp_dir = TempDir::new()?;
    let data_dir = temp_dir.path().to_path_buf();
    let session_manager = Arc::new(SessionManager::new(data_dir.clone()));
    let agent = Agent::with_config(AgentConfig::new(
        session_manager.clone(),
        Arc::new(PermissionManager::new(data_dir)),
        None,
        GooseMode::default(),
        true,
        GoosePlatform::GooseCli,
    ));
    let session = session_manager
        .create_session(
            working_dir,
            "destructive-gate".to_string(),
            SessionType::Hidden,
            GooseMode::default(),
        )
        .await?;
    agent
        .update_provider(
            Arc::new(ShellOnceProvider {
                command: command.to_string(),
                calls: AtomicUsize::new(0),
            }),
            ModelConfig::new("mock-model"),
            &session.id,
        )
        .await?;
    let reply = agent
        .reply(
            Message::user().with_text("commit the empty body stuff separately"),
            SessionConfig {
                id: session.id,
                schedule_id: None,
                max_turns: Some(3),
                retry_config: None,
            },
            None,
        )
        .await?;
    tokio::pin!(reply);

    let mut outcome = Outcome {
        prompts: vec![],
        tool_result_texts: vec![],
    };
    while let Some(event) = reply.next().await {
        let AgentEvent::Message(message) = event? else {
            continue;
        };
        for content in &message.content {
            match content {
                MessageContent::ActionRequired(action) => {
                    if let ActionRequiredData::ToolConfirmation { id, prompt, .. } = &action.data {
                        outcome.prompts.push(prompt.clone());
                        agent
                            .handle_confirmation(
                                id.clone(),
                                PermissionConfirmation {
                                    principal_type: PrincipalType::Tool,
                                    permission: answer.clone(),
                                },
                            )
                            .await;
                    }
                }
                MessageContent::ToolResponse(response) if response.id == "call_f17" => {
                    if let Ok(result) = &response.tool_result {
                        for c in &result.content {
                            if let Some(t) = c.as_text() {
                                outcome.tool_result_texts.push(t.text.clone());
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
    Ok(outcome)
}

#[tokio::test]
async fn f17_reset_hard_asks_in_auto_mode_and_decline_runs_nothing() -> Result<()> {
    let outcome = run_turn(F17_COMMAND, Permission::DenyOnce).await?;
    assert_eq!(outcome.prompts.len(), 1, "exactly one approval card");
    let prompt = outcome.prompts[0].clone().expect("card carries the reason");
    assert!(prompt.contains("git reset --hard 1192c90"), "{prompt}");
    let text = outcome.tool_result_texts.join("\n");
    assert!(
        text.contains("declined this command because it would destroy work")
            && text.contains("`git reset --hard 1192c90`")
            && text.contains("Nothing in it was run"),
        "model must get the destructive decline, got: {text}"
    );
    Ok(())
}

#[tokio::test]
async fn always_deny_on_a_destructive_card_still_returns_the_clear_decline() -> Result<()> {
    let outcome = run_turn("git clean -fd", Permission::AlwaysDeny).await?;
    assert_eq!(outcome.prompts.len(), 1);
    assert!(outcome
        .tool_result_texts
        .join("\n")
        .contains("`git clean -fd`"));
    Ok(())
}

#[tokio::test]
async fn harmless_look_alike_is_not_asked_in_auto_mode() -> Result<()> {
    let outcome = run_turn("git reset --soft 8bcacbe", Permission::DenyOnce).await?;
    assert!(
        outcome.prompts.is_empty(),
        "auto mode must not ask for git reset --soft: {:?}",
        outcome.prompts
    );
    Ok(())
}

#[tokio::test]
async fn rm_of_an_ignored_build_dir_in_the_project_is_not_asked() -> Result<()> {
    let project = project()?;
    let outcome = run_turn_in(
        project.path().to_path_buf(),
        "rm -rf node_modules",
        Permission::DenyOnce,
    )
    .await?;
    assert!(outcome.prompts.is_empty(), "{:?}", outcome.prompts);
    Ok(())
}

#[tokio::test]
async fn rm_of_tracked_source_in_the_project_is_asked() -> Result<()> {
    let project = project()?;
    let outcome = run_turn_in(
        project.path().to_path_buf(),
        "rm -rf src",
        Permission::DenyOnce,
    )
    .await?;
    assert_eq!(outcome.prompts.len(), 1);
    assert!(
        project.path().join("src/a.js").exists(),
        "declined rm must not run"
    );
    Ok(())
}

#[tokio::test]
async fn rm_of_an_ignored_data_dir_is_asked() -> Result<()> {
    let project = project()?;
    let outcome = run_turn_in(
        project.path().to_path_buf(),
        "rm -rf data",
        Permission::DenyOnce,
    )
    .await?;
    assert_eq!(outcome.prompts.len(), 1, "ignored is not disposable");
    assert!(project.path().join("data/customers.csv").exists());
    Ok(())
}
