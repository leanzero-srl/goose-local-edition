//! What the recall part says to the model and to the person, read as words.

use std::path::PathBuf;
use std::sync::Arc;

use goose::agents::platform_extensions::recall::{
    history_tool, recall_line_of, render, render_with, Extras, PastSession,
};
use goose::agents::{Agent, AgentConfig, ExtensionConfig, GoosePlatform};
use goose::config::permission::PermissionManager;
use goose::config::GooseMode;
use goose::session::{SessionManager, SessionType};
use goose_memory_store::{MemoryEntry, SearchHit};
use goose_sdk_types::custom_requests::{SourceEntry, SourceType};

fn memory(category: &str) -> SearchHit {
    SearchHit {
        score: 9.0,
        matched_terms: 2,
        rare_terms: 2,
        phrase: false,
        name_terms: 2,
        specific_terms: 1,
        matched_specific: 1,
        named: true,
        topic_word_in_name: false,
        topic_in_name: false,
        identifier_in_name: false,
        identifier_in_body: false,
        together: true,
        occurrences: 2,
        entry: MemoryEntry {
            is_global: true,
            category: category.to_string(),
            tags: vec!["user".to_string()],
            content: format!("{category} headline\nbody"),
        },
    }
}

fn skill(name: &str) -> SourceEntry {
    SourceEntry {
        source_type: SourceType::Skill,
        name: name.to_string(),
        description: "Jira REST".to_string(),
        content: String::new(),
        path: format!("/skills/{name}"),
        supporting_files: Vec::new(),
        global: true,
        writable: false,
        properties: Default::default(),
    }
}

/// Q-24 (2026-09-25): the notice above an answer read "recalled: memories assistant-talk-and-swaps ·
/// past session 20260924_19" — a store slug and a session id, jargon to the person it is shown to.
/// It says in plain words what goose used; the names stay in the blocks the model reads.
#[test]
fn the_recall_line_says_what_goose_used_in_plain_words() {
    let line_of = |block: Option<String>| recall_line_of(&block.unwrap()).unwrap().to_string();

    let one = [memory("assistant-talk-and-swaps")];
    assert_eq!(
        line_of(render(&one, &[], Some(&past()))),
        "Remembered: 1 note, 1 earlier chat"
    );
    let two = [memory("postgres"), memory("assistant-talk-and-swaps")];
    assert_eq!(line_of(render(&two, &[], None)), "Remembered: 2 notes");
    assert_eq!(
        line_of(render(&[], &[], Some(&past()))),
        "Remembered: 1 earlier chat"
    );

    let skills = [skill("jira-api"), skill("confluence-api")];
    let refs: Vec<&SourceEntry> = skills.iter().collect();
    assert_eq!(
        line_of(render(&one, &refs[..1], None)),
        "Remembered: 1 note · suggested skill jira-api"
    );
    assert_eq!(
        line_of(render(&[], &refs, None)),
        "Suggested skills jira-api, confluence-api"
    );

    let extras = Extras {
        autoloaded: Some(("jira-api".to_string(), "BODY".to_string())),
        correction_of: Some("Deleted the tests".to_string()),
        answered: Some(("Which config?".to_string(), "prod".to_string())),
        history_tool: None,
    };
    assert_eq!(
        line_of(render_with(&[], &[], None, &extras)),
        "Loaded skill jira-api · noticed a correction · noticed your answer"
    );

    let block = render(&one, &refs[..1], Some(&past())).unwrap();
    assert!(!recall_line_of(&block).unwrap().contains("20260924_19"));
    assert!(
        block.contains("session 20260924_19"),
        "the model still reads the id"
    );
    assert!(block.contains("## assistant-talk-and-swaps"));
    assert!(block.contains("- jira-api: Jira REST"));
}

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
