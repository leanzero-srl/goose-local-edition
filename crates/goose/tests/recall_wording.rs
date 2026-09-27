//! What the recall part says to the model and to the person, read as words.

use std::path::PathBuf;
use std::sync::Arc;

use goose::agents::platform_extensions::recall::{
    history_tool, render, render_with, Extras, PastSession,
};
use goose::agents::{Agent, AgentConfig, ExtensionConfig, GoosePlatform};
use goose::config::permission::PermissionManager;
use goose::config::GooseMode;
use goose::session::{SessionManager, SessionType};

#[ctor::ctor]
fn hermetic_path_root() {
    goose_test_support::hermetic_path_root();
}

fn past() -> PastSession {
    PastSession {
        session_id: "20260924_19".to_string(),
        description: "Alps description".to_string(),
        when: "2026-09-24 18:02".to_string(),
        role: "assistant".to_string(),
        headline: "The Alps stretch across eight countries.".to_string(),
    }
}

/// Q-14 (2026-09-25): chatrecall was `enabled: false`, the block still said "chatrecall(session_id)
/// loads it", and "Write three sentences about the Alps." became 26 shell calls over 9 minutes
/// looking for the previous session.
#[test]
fn a_past_session_names_no_tool_when_chatrecall_is_not_enabled() {
    let block = render(&[], &[], Some(&past())).unwrap();
    assert!(!block.contains("chatrecall"), "{block}");
    assert!(block.contains("do not go looking for it"), "{block}");
    assert!(block.contains("The Alps stretch across eight countries."));
}

#[test]
fn a_past_session_names_the_tool_the_session_lists_when_chatrecall_is_enabled() {
    let extras = Extras {
        history_tool: Some("chatrecall__chatrecall".to_string()),
        ..Extras::default()
    };
    let block = render_with(&[], &[], Some(&past()), &extras).unwrap();
    assert!(
        block.contains("chatrecall__chatrecall with session_id \"20260924_19\" loads it"),
        "{block}"
    );
    assert!(!block.contains("do not go looking for it"), "{block}");
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

/// The name comes from the session's own tool list: absent until the extension is enabled, then
/// the prefixed name the model is offered.
#[tokio::test]
async fn the_history_tool_is_the_one_the_session_lists_and_only_when_enabled() -> anyhow::Result<()>
{
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
    let session = session_manager
        .create_session(
            PathBuf::default(),
            "recall".to_string(),
            SessionType::User,
            GooseMode::default(),
        )
        .await?;
    assert_eq!(
        history_tool(&agent.extension_manager, &session.id).await,
        None
    );
    agent.add_extension(chatrecall(), &session.id).await?;
    assert_eq!(
        history_tool(&agent.extension_manager, &session.id).await,
        Some("chatrecall__chatrecall".to_string())
    );
    Ok(())
}
