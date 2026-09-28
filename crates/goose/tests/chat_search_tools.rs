//! Q-358 S2: the chat search tools as a session is offered them — three tools under the
//! `chatrecall` id, none at all on a knowledge-blind (benchmark) agent.

use std::path::PathBuf;
use std::sync::Arc;

use goose::agents::{Agent, AgentConfig, ExtensionConfig, GoosePlatform};
use goose::config::permission::PermissionManager;
use goose::config::GooseMode;
use goose::session::{SessionManager, SessionType};

#[ctor::ctor]
fn hermetic_path_root() {
    goose_test_support::hermetic_path_root();
}

fn chatrecall() -> ExtensionConfig {
    ExtensionConfig::Platform {
        name: "chatrecall".to_string(),
        description: String::new(),
        display_name: None,
        bundled: None,
        available_tools: vec![],
    }
}

async fn agent_with_chat_search(blind: bool) -> anyhow::Result<(tempfile::TempDir, Agent, String)> {
    let temp_dir = tempfile::tempdir()?;
    let session_manager = Arc::new(SessionManager::new(temp_dir.path().to_path_buf()));
    let agent = Agent::with_config(AgentConfig::new(
        session_manager.clone(),
        PermissionManager::instance(),
        None,
        GooseMode::Auto,
        true,
        GoosePlatform::GooseDesktop,
    ));
    agent.set_knowledge_blind(blind);
    let session = session_manager
        .create_session(
            PathBuf::default(),
            "chat search".to_string(),
            SessionType::User,
            GooseMode::default(),
        )
        .await?;
    agent.add_extension(chatrecall(), &session.id).await?;
    Ok((temp_dir, agent, session.id))
}

async fn chat_tools(agent: &Agent, session_id: &str) -> anyhow::Result<Vec<String>> {
    let mut names: Vec<String> = agent
        .extension_manager
        .get_prefixed_tools(session_id, Some("chatrecall".to_string()))
        .await?
        .into_iter()
        .map(|tool| tool.name.to_string())
        .collect();
    names.sort();
    Ok(names)
}

#[tokio::test]
async fn a_session_is_offered_search_read_and_list() -> anyhow::Result<()> {
    let (_dir, agent, session_id) = agent_with_chat_search(false).await?;
    assert_eq!(
        chat_tools(&agent, &session_id).await?,
        vec![
            "chatrecall__list_chats",
            "chatrecall__read_chat",
            "chatrecall__search_chats"
        ]
    );
    Ok(())
}

/// THE BENCHMARK INVARIANT (frame 1.14 §2.6), recall's predicate: a knowledge-blind agent is never
/// given the person's other chats. The swarm sets the flag right after building the agent, before any
/// extension is added — the order this test runs.
#[tokio::test]
async fn a_knowledge_blind_agent_is_offered_no_chat_search() -> anyhow::Result<()> {
    let (_dir, agent, session_id) = agent_with_chat_search(true).await?;
    assert!(chat_tools(&agent, &session_id).await?.is_empty());
    Ok(())
}
