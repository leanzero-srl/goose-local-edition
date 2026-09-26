//! "Needs you": `ask_user` pins a durable item and ends the turn; the item survives a restart and
//! closes only when answered or dismissed; the tool never exists in a session no person answers.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::Result;
use async_trait::async_trait;
use futures::StreamExt;
use goose::agents::{Agent, AgentConfig, ExtensionConfig, GoosePlatform, SessionConfig};
use goose::config::permission::PermissionManager;
use goose::config::GooseMode;
use goose::conversation::message::{Message, MessageContent};
use goose::execution::manager::AgentManager;
use goose::needs_you::{self, NeedsYouState, NeedsYouStatus, NewQuestion, Resolution};
use goose::providers::base::{
    MessageStream, Provider, ProviderDef, ProviderDescriptor, ProviderMetadata,
};
use goose::session::extension_data::ExtensionState;
use goose::session::{SessionManager, SessionType, TodoState};
use goose::turn_outcome::{self, TurnOutcomeState};
use goose_providers::conversation::token_usage::{ProviderUsage, Usage};
use goose_providers::errors::ProviderError;
use goose_providers::model::ModelConfig;
use rmcp::model::{CallToolRequestParams, Role, Tool};
use rmcp::object;
use tokio_util::sync::CancellationToken;

#[ctor::ctor]
fn hermetic_path_root() {
    goose_test_support::hermetic_path_root();
}

const QUESTION: &str = "Which database should the service use?";
const WHY: &str = "The schema and the migration tool depend on it.";
const RECOMMENDED: &str = "PostgreSQL";

/// Call 0 asks the person; any later call means the turn did NOT end at the question.
struct AskingProvider {
    calls: AtomicUsize,
    offered_tools: Mutex<Vec<String>>,
}

impl AskingProvider {
    fn new() -> Self {
        Self {
            calls: AtomicUsize::new(0),
            offered_tools: Mutex::new(Vec::new()),
        }
    }
}

impl ProviderDescriptor for AskingProvider {
    fn metadata() -> ProviderMetadata {
        ProviderMetadata {
            name: "asking-mock".to_string(),
            display_name: "Asking Mock".to_string(),
            description: "Asks the user once".to_string(),
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

impl ProviderDef for AskingProvider {
    type Provider = Self;

    fn from_env(
        _extensions: Vec<goose::config::ExtensionConfig>,
        _tls_config: Option<goose::providers::api_client::TlsConfig>,
    ) -> futures::future::BoxFuture<'static, anyhow::Result<Self>> {
        unimplemented!()
    }
}

#[async_trait]
impl Provider for AskingProvider {
    async fn stream(
        &self,
        _model_config: &ModelConfig,
        _system_prompt: &str,
        _messages: &[Message],
        tools: &[Tool],
    ) -> Result<MessageStream, ProviderError> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        *self.offered_tools.lock().unwrap() = tools.iter().map(|t| t.name.to_string()).collect();
        let usage = ProviderUsage::new(
            "mock-model".to_string(),
            Usage::new(Some(10), Some(5), Some(15)),
        );
        let message = if call == 0 {
            Message::assistant().with_tool_request(
                "call_ask",
                Ok(
                    CallToolRequestParams::new("ask_user").with_arguments(object!({
                        "question": QUESTION,
                        "why": WHY,
                        "recommended_answer": RECOMMENDED,
                        "options": ["PostgreSQL", "SQLite", " ", "SQLite"],
                    })),
                ),
            )
        } else {
            Message::assistant().with_text("the turn went on after the question")
        };
        Ok(Box::pin(futures::stream::once(async move {
            Ok((Some(message), Some(usage)))
        })))
    }

    fn get_name(&self) -> &str {
        "asking-mock"
    }
}

fn needs_you_extension() -> ExtensionConfig {
    ExtensionConfig::Platform {
        name: "needs_you".to_string(),
        description: String::new(),
        display_name: None,
        bundled: None,
        available_tools: vec![],
    }
}

async fn agent_with_needs_you(
    session_manager: &Arc<SessionManager>,
    platform: GoosePlatform,
    session_type: SessionType,
    knowledge_blind: bool,
) -> Result<(Agent, String)> {
    let agent = Agent::with_config(AgentConfig::new(
        session_manager.clone(),
        PermissionManager::instance(),
        None,
        GooseMode::Auto,
        true,
        platform,
    ));
    agent.set_knowledge_blind(knowledge_blind);
    let session = session_manager
        .create_session(
            PathBuf::default(),
            "needs-you".to_string(),
            session_type,
            GooseMode::default(),
        )
        .await?;
    agent
        .add_extension(needs_you_extension(), &session.id)
        .await?;
    Ok((agent, session.id))
}

async fn tool_names(agent: &Agent, session_id: &str) -> Vec<String> {
    agent
        .list_tools(session_id, None)
        .await
        .into_iter()
        .map(|tool| tool.name.to_string())
        .collect()
}

fn question() -> NewQuestion {
    NewQuestion {
        question: QUESTION.to_string(),
        why: WHY.to_string(),
        recommended_answer: RECOMMENDED.to_string(),
        options: vec![],
    }
}

#[tokio::test]
async fn ask_user_pins_a_durable_item_and_ends_the_turn() -> Result<()> {
    let temp_dir = tempfile::tempdir()?;
    let session_manager = Arc::new(SessionManager::new(temp_dir.path().to_path_buf()));
    let (agent, session_id) = agent_with_needs_you(
        &session_manager,
        GoosePlatform::GooseDesktop,
        SessionType::User,
        false,
    )
    .await?;
    let provider = Arc::new(AskingProvider::new());
    agent
        .update_provider(
            provider.clone(),
            ModelConfig::new("mock-model"),
            &session_id,
        )
        .await?;

    let reply = agent
        .reply(
            Message::user().with_text("Set up the service"),
            SessionConfig {
                id: session_id.clone(),
                schedule_id: None,
                max_turns: None,
                retry_config: None,
            },
            None,
        )
        .await?;
    tokio::pin!(reply);
    while let Some(event) = reply.next().await {
        if let Err(error) = event {
            return Err(error);
        }
    }

    assert!(
        provider
            .offered_tools
            .lock()
            .unwrap()
            .iter()
            .any(|name| name == "ask_user"),
        "the model was never offered ask_user"
    );
    assert_eq!(
        provider.calls.load(Ordering::SeqCst),
        1,
        "the turn must end at the question, not call the model again"
    );

    let conversation = session_manager
        .get_session(&session_id, true)
        .await?
        .conversation
        .expect("conversation");
    let last = conversation.messages().last().expect("a last message");
    assert_eq!(last.role, Role::User);
    assert!(
        last.content.iter().any(|content| matches!(
            content,
            MessageContent::ToolResponse(response)
                if response.id == "call_ask" && response.tool_result.is_ok()
        )),
        "the conversation ends on ask_user's own result: {last:?}"
    );

    // A restart is a fresh SessionManager over the same database.
    let restarted = SessionManager::new(temp_dir.path().to_path_buf());
    let open = needs_you::open_items(&restarted).await?;
    assert_eq!(open.len(), 1);
    let item = &open[0].item;
    assert_eq!(open[0].session_id, session_id);
    assert_eq!(item.question, QUESTION);
    assert_eq!(item.why, WHY);
    assert_eq!(item.recommended_answer, RECOMMENDED);
    assert_eq!(item.options, vec!["PostgreSQL", "SQLite"]);
    assert_eq!(item.status, NeedsYouStatus::Open);

    let answered = needs_you::resolve(
        &restarted,
        &session_id,
        &item.id,
        Resolution::Answered("SQLite, it is a single-user tool".to_string()),
    )
    .await?;
    assert_eq!(answered.status, NeedsYouStatus::Answered);
    assert!(needs_you::open_items(&restarted).await?.is_empty());

    let stored = NeedsYouState::from_extension_data(
        &restarted
            .get_session(&session_id, false)
            .await?
            .extension_data,
    )
    .expect("state");
    assert_eq!(
        stored.items[0].answer.as_deref(),
        Some("SQLite, it is a single-user tool")
    );
    Ok(())
}

#[tokio::test]
async fn ask_user_is_registered_only_where_a_person_answers() -> Result<()> {
    let temp_dir = tempfile::tempdir()?;
    let session_manager = Arc::new(SessionManager::new(temp_dir.path().to_path_buf()));
    let cases = [
        (GoosePlatform::GooseDesktop, SessionType::User, false, true),
        // `goose run`, the benchmark's cloud/legacy entrants and every swarm worker are CLI hosts.
        (GoosePlatform::GooseCli, SessionType::User, false, false),
        (GoosePlatform::GooseCli, SessionType::Hidden, false, false),
        // A desktop-hosted session no person answers: swarm-style hidden, subagent, schedule.
        (
            GoosePlatform::GooseDesktop,
            SessionType::Hidden,
            false,
            false,
        ),
        (
            GoosePlatform::GooseDesktop,
            SessionType::SubAgent,
            false,
            false,
        ),
        (
            GoosePlatform::GooseDesktop,
            SessionType::Scheduled,
            false,
            false,
        ),
        (
            GoosePlatform::GooseDesktop,
            SessionType::Gateway,
            false,
            false,
        ),
        // A knowledge-blind (benchmark) agent, even on a user session.
        (GoosePlatform::GooseDesktop, SessionType::User, true, false),
    ];
    for (platform, session_type, blind, expected) in cases {
        let label = format!("{platform} {session_type} blind={blind}");
        let (agent, session_id) =
            agent_with_needs_you(&session_manager, platform, session_type, blind).await?;
        let names = tool_names(&agent, &session_id).await;
        assert_eq!(
            names.iter().any(|name| name == "ask_user"),
            expected,
            "{label}: tools {names:?}"
        );
    }
    Ok(())
}

#[tokio::test]
async fn an_item_closes_once_and_bad_input_is_refused() -> Result<()> {
    let temp_dir = tempfile::tempdir()?;
    let session_manager = SessionManager::new(temp_dir.path().to_path_buf());
    let session = session_manager
        .create_session(
            PathBuf::default(),
            "needs-you".to_string(),
            SessionType::User,
            GooseMode::default(),
        )
        .await?;

    let mut no_recommendation = question();
    no_recommendation.recommended_answer = "  ".to_string();
    assert!(
        needs_you::raise(&session_manager, &session.id, no_recommendation)
            .await
            .is_err()
    );
    assert!(
        needs_you::raise(&session_manager, "no-such-session", question())
            .await
            .is_err()
    );

    let item = needs_you::raise(&session_manager, &session.id, question()).await?;
    assert!(needs_you::resolve(
        &session_manager,
        &session.id,
        &item.id,
        Resolution::Answered(" ".to_string())
    )
    .await
    .is_err());
    assert!(needs_you::resolve(
        &session_manager,
        &session.id,
        "ny_missing",
        Resolution::Dismissed
    )
    .await
    .is_err());

    let dismissed = needs_you::resolve(
        &session_manager,
        &session.id,
        &item.id,
        Resolution::Dismissed,
    )
    .await?;
    assert_eq!(dismissed.status, NeedsYouStatus::Dismissed);
    assert!(needs_you::resolve(
        &session_manager,
        &session.id,
        &item.id,
        Resolution::Answered("late".to_string())
    )
    .await
    .is_err());
    assert!(needs_you::open_items(&session_manager).await?.is_empty());
    Ok(())
}

#[tokio::test]
async fn concurrent_extension_writes_never_erase_an_item() -> Result<()> {
    let temp_dir = tempfile::tempdir()?;
    let session_manager = Arc::new(SessionManager::new(temp_dir.path().to_path_buf()));
    let session = session_manager
        .create_session(
            PathBuf::default(),
            "needs-you".to_string(),
            SessionType::User,
            GooseMode::default(),
        )
        .await?;

    let writers = 8;
    let mut tasks = Vec::new();
    for n in 0..writers {
        let manager = session_manager.clone();
        let id = session.id.clone();
        tasks.push(tokio::spawn(async move {
            needs_you::raise(&manager, &id, question())
                .await
                .map(|_| ())
        }));
        let manager = session_manager.clone();
        let id = session.id.clone();
        tasks.push(tokio::spawn(async move {
            manager
                .set_extension_state(&id, &TodoState::new(format!("todo {n}")))
                .await
        }));
    }
    for task in tasks {
        task.await??;
    }

    let open = needs_you::open_items(&session_manager).await?;
    assert_eq!(open.len(), writers);
    let data = session_manager
        .get_session(&session.id, false)
        .await?
        .extension_data;
    assert!(TodoState::from_extension_data(&data).is_some());
    Ok(())
}

/// Fails the first call the way a cut split does, then answers.
struct FlakyProvider {
    calls: AtomicUsize,
}

impl ProviderDescriptor for FlakyProvider {
    fn metadata() -> ProviderMetadata {
        ProviderMetadata {
            name: "flaky-mock".to_string(),
            display_name: "Flaky Mock".to_string(),
            description: "Fails once".to_string(),
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

impl ProviderDef for FlakyProvider {
    type Provider = Self;

    fn from_env(
        _extensions: Vec<goose::config::ExtensionConfig>,
        _tls_config: Option<goose::providers::api_client::TlsConfig>,
    ) -> futures::future::BoxFuture<'static, anyhow::Result<Self>> {
        unimplemented!()
    }
}

#[async_trait]
impl Provider for FlakyProvider {
    async fn stream(
        &self,
        _model_config: &ModelConfig,
        _system_prompt: &str,
        _messages: &[Message],
        _tools: &[Tool],
    ) -> Result<MessageStream, ProviderError> {
        if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
            return Err(ProviderError::NetworkError(
                "the split across your Macs stopped mid-answer".to_string(),
            ));
        }
        let usage = ProviderUsage::new(
            "mock-model".to_string(),
            Usage::new(Some(10), Some(5), Some(15)),
        );
        Ok(Box::pin(futures::stream::once(async move {
            Ok((Some(Message::assistant().with_text("done")), Some(usage)))
        })))
    }

    fn get_name(&self) -> &str {
        "flaky-mock"
    }
}

async fn run_turn(agent: &Agent, session_id: &str, text: &str) -> Result<()> {
    let reply = agent
        .reply(
            Message::user().with_text(text),
            SessionConfig {
                id: session_id.to_string(),
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
    Ok(())
}

async fn flaky_agent(
    session_manager: &Arc<SessionManager>,
    swarm_worker: bool,
) -> Result<(Agent, String)> {
    let agent = Agent::with_config(AgentConfig::new(
        session_manager.clone(),
        PermissionManager::instance(),
        None,
        GooseMode::Auto,
        true,
        GoosePlatform::GooseDesktop,
    ));
    if swarm_worker {
        agent.configure_swarm_worker(None);
    }
    let session = session_manager
        .create_session(
            PathBuf::default(),
            "flaky".to_string(),
            SessionType::User,
            GooseMode::default(),
        )
        .await?;
    agent
        .update_provider(
            Arc::new(FlakyProvider {
                calls: AtomicUsize::new(0),
            }),
            ModelConfig::new("mock-model"),
            &session.id,
        )
        .await?;
    Ok((agent, session.id))
}

#[tokio::test]
async fn a_failed_turn_reads_failed_until_a_turn_completes() -> Result<()> {
    let temp_dir = tempfile::tempdir()?;
    let session_manager = Arc::new(SessionManager::new(temp_dir.path().to_path_buf()));
    let (agent, session_id) = flaky_agent(&session_manager, false).await?;

    run_turn(&agent, &session_id, "write the notes").await?;
    let failed = turn_outcome::failed_sessions(&session_manager).await?;
    assert_eq!(failed.len(), 1, "the cut turn must read failed");
    assert_eq!(failed[0].session_id, session_id);
    assert!(
        failed[0]
            .reason
            .as_deref()
            .is_some_and(|reason| reason.contains("stopped mid-answer")),
        "the reason is what the chat showed: {:?}",
        failed[0].reason
    );

    run_turn(&agent, &session_id, "try again").await?;
    assert!(turn_outcome::failed_sessions(&session_manager)
        .await?
        .is_empty());
    Ok(())
}

#[tokio::test]
async fn a_swarm_worker_never_records_a_turn_outcome() -> Result<()> {
    let temp_dir = tempfile::tempdir()?;
    let session_manager = Arc::new(SessionManager::new(temp_dir.path().to_path_buf()));
    let (agent, session_id) = flaky_agent(&session_manager, true).await?;

    run_turn(&agent, &session_id, "write the notes").await?;
    let data = session_manager
        .get_session(&session_id, false)
        .await?
        .extension_data;
    assert!(TurnOutcomeState::from_extension_data(&data).is_none());
    Ok(())
}

#[tokio::test]
async fn the_busy_set_says_when_each_turn_began() -> Result<()> {
    let temp_dir = tempfile::tempdir()?;
    let manager = AgentManager::new(
        AgentConfig::new(
            Arc::new(SessionManager::new(temp_dir.path().to_path_buf())),
            PermissionManager::instance(),
            None,
            GooseMode::Auto,
            true,
            GoosePlatform::GooseDesktop,
        ),
        None,
    )
    .await?;
    let before = chrono::Utc::now();
    manager
        .try_register_cancel_token("session-a", CancellationToken::new())
        .await?;
    let busy = manager.busy_sessions().await;
    assert_eq!(busy.len(), 1);
    assert_eq!(busy[0].0, "session-a");
    assert!(busy[0].1 >= before);

    manager.unregister_cancel_token("session-a").await;
    assert!(manager.busy_sessions().await.is_empty());
    Ok(())
}
