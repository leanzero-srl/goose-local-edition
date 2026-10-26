//! Reconnect to an unfinished native owner run after the agent rewrites stored tool IDs.
#[path = "acp_ws/mod.rs"]
mod acp_ws;

use acp_ws::{configure, eventually, user_turns_running, Answer, Model, Window};
use goose::acp::server_factory::{AcpServer, AcpServerFactoryConfig};
use goose::acp::transport::create_acp_router;
use goose::agents::GoosePlatform;
use goose::config::paths::Paths;
use goose::conversation::message::MessageContent;
use goose::session::SessionManager;
use serde_json::{json, Value};
use std::sync::Arc;

fn chunk_words(frames: &[Value], kind: &str, words: &str) -> Vec<Value> {
    frames
        .iter()
        .filter_map(|frame| {
            let update = &frame["params"]["update"];
            (update["sessionUpdate"] == kind && update["content"]["text"] == words)
                .then(|| update.clone())
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn reconnect_replays_earlier_words_once_before_live_delivery_and_preserves_owner_run() {
    let model = Model::start(vec![Answer::Finish("Already committed answer.")]).await;
    model.keep_side_requests_off_the_script();
    configure(&model, false);
    let config_path = Paths::config_dir().join(goose::config::base::CONFIG_YAML_NAME);
    let mut config = std::fs::read_to_string(&config_path).unwrap();
    config.push_str("GOOSE_DISABLE_TOOL_CALL_SUMMARY: true\n");
    std::fs::write(config_path, config).unwrap();
    let server = Arc::new(AcpServer::new(AcpServerFactoryConfig {
        builtins: vec!["developer".into()],
        data_dir: Paths::data_dir(),
        config_dir: Paths::config_dir(),
        goose_platform: GoosePlatform::GooseDesktop,
        additional_source_roots: vec![],
        scheduler: None,
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, create_acp_router(server))
            .await
            .unwrap();
    });
    let work = tempfile::tempdir().unwrap();
    let mut original = Window::open(addr, true).await;
    let chat = original.new_chat(work.path()).await;
    let first = original
        .prompt(&chat, "Keep this committed history", None)
        .await;
    original.response(first).await.unwrap();
    eventually("first owner run finished", || async {
        user_turns_running() == 0
    })
    .await;
    original.seen.clear();
    model.script(vec![
        Answer::ThinkingToolCall(
            "I will inspect the original directory.",
            "shell",
            r#"{"command":"printf 'owner tool result\\n'"}"#,
        ),
        Answer::Unfinished("The unfinished answer stays on the owner."),
    ]);
    original.prompt(&chat, "Inspect and continue", None).await;
    original
        .until_answer_streams(&chat, "The unfinished answer stays on the owner.")
        .await;
    let thinking = chunk_words(
        &original.seen,
        "agent_thought_chunk",
        "I will inspect the original directory.",
    );
    assert_eq!(thinking.len(), 1);
    let original_thought_id = thinking[0]["_meta"]["goose"]["messageId"].as_str().unwrap();
    let stored = SessionManager::instance()
        .get_session(&chat, true)
        .await
        .unwrap();
    let persisted_thought = stored.conversation.as_ref().unwrap().messages().iter().find(|message| message.content.iter().any(|content| {
        matches!(content, MessageContent::Thinking(value) if value.thinking == "I will inspect the original directory.")
    })).expect("thinking persisted with the split tool request");
    assert_ne!(
        persisted_thought.id.as_deref(),
        Some(original_thought_id),
        "control: persistence changed the streamed thinking identity"
    );
    original.close().await;
    let mut reconnected = Window::open(addr, true).await;
    let response = reconnected
        .request(
            "session/load",
            json!({"sessionId":chat,"cwd":work.path(),"mcpServers":[]}),
        )
        .await;
    assert!(!response.is_null());
    assert_eq!(
        chunk_words(
            &reconnected.seen,
            "agent_message_chunk",
            "Already committed answer."
        )
        .len(),
        1
    );
    assert_eq!(
        chunk_words(
            &reconnected.seen,
            "user_message_chunk",
            "Inspect and continue"
        )
        .len(),
        1
    );
    assert_eq!(
        chunk_words(
            &reconnected.seen,
            "agent_thought_chunk",
            "I will inspect the original directory."
        )
        .len(),
        1
    );
    assert_eq!(
        chunk_words(
            &reconnected.seen,
            "agent_message_chunk",
            "The unfinished answer stays on the owner."
        )
        .len(),
        1
    );
    assert_eq!(
        user_turns_running(),
        1,
        "loading never starts a duplicate run"
    );
    assert_eq!(model.completion_requests(), 3);
    reconnected.cancel(&chat).await;
    eventually("reconnected viewer stopped original owner", || async {
        user_turns_running() == 0
    })
    .await;
    reconnected.close().await;
}
