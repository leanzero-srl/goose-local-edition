//! Q-169: a turn the person STOPS used to leave no trace — the prompt drops the reply stream, so the
//! agent's end-of-turn record never ran, and the chat kept only the user's message while the
//! sidebar read "15m ago". The stop is recorded where the stream is dropped.

#[allow(dead_code)]
#[path = "acp_common_tests/mod.rs"]
mod common_tests;

use agent_client_protocol::schema::v1::{
    CancelNotification, ContentBlock, PromptRequest, SessionUpdate, StopReason, TextContent,
};
use common_tests::fixtures::server::AcpServerConnection;
use common_tests::fixtures::{run_test, Connection, Session, SessionData, TestConnectionConfig};
use goose::conversation::message::{Message, MessageContent};
use goose::session::SessionManager;
use goose::turn_outcome::{self, TurnOutcomeState};
use serial_test::serial;
use std::path::PathBuf;
use std::sync::LazyLock;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use common_tests::fixtures::OpenAiFixture;

const WRITTEN: &str = "Here is the first part of a long migration plan, written before the stop";

static ACP_CONFIG_ROOT: LazyLock<tempfile::TempDir> =
    LazyLock::new(|| tempfile::tempdir().unwrap());

fn write_acp_global_config() -> PathBuf {
    std::env::set_var("GOOSE_PATH_ROOT", ACP_CONFIG_ROOT.path());
    std::env::set_var("GOOSE_DISABLE_KEYRING", "1");
    let config_dir = goose::config::paths::Paths::config_dir();
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(
        config_dir.join(goose::config::base::CONFIG_YAML_NAME),
        "GOOSE_MODEL: gpt-4o\nGOOSE_PROVIDER: openai\nGOOSE_DISABLE_KEYRING: true\n",
    )
    .unwrap();
    config_dir
}

/// An OpenAI-compatible endpoint whose answer streams its first words and then never finishes —
/// the answer the person stops. The held sockets stay open for the life of the test.
async fn never_finishing_openai() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        let mut held = Vec::new();
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let mut request = Vec::new();
            let mut buf = [0u8; 8192];
            loop {
                let n = socket.read(&mut buf).await.unwrap_or(0);
                if n == 0 {
                    break;
                }
                request.extend_from_slice(&buf[..n]);
                if let Some(head_end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                    let length = String::from_utf8_lossy(&request[..head_end])
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                        })
                        .unwrap_or(0);
                    if request.len() >= head_end + 4 + length {
                        break;
                    }
                }
            }
            let text = String::from_utf8_lossy(&request).to_string();
            if text.starts_with("GET") {
                let body = include_str!("acp_test_data/openai_models.json");
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(response.as_bytes()).await;
                continue;
            }
            let chunk = serde_json::json!({
                "id": "stopped-1",
                "object": "chat.completion.chunk",
                "created": 1,
                "model": "gpt-4o",
                "choices": [{"index": 0, "delta": {"role": "assistant", "content": WRITTEN}, "finish_reason": null}],
            });
            let head =
                "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n";
            let _ = socket.write_all(head.as_bytes()).await;
            let _ = socket
                .write_all(format!("data: {chunk}\n\n").as_bytes())
                .await;
            let _ = socket.flush().await;
            held.push(socket);
        }
    });
    base_url
}

#[test]
#[serial]
fn a_stopped_turn_is_recorded_with_its_time_and_tokens_and_a_chat_line() {
    write_acp_global_config();
    run_test(async move {
        let openai = OpenAiFixture::serving_from(never_finishing_openai().await).await;
        let mut conn = AcpServerConnection::new(TestConnectionConfig::default(), openai).await;
        let SessionData { session, .. } = conn.new_session().await.unwrap();
        let acp_session_id = session.session_id().clone();

        let cx = conn.cx().clone();
        let prompt_session = acp_session_id.clone();
        let prompt = tokio::spawn(async move {
            cx.send_request(PromptRequest::new(
                prompt_session,
                vec![ContentBlock::Text(TextContent::new(
                    "Plan the Jira migration end to end",
                ))],
            ))
            .block_task()
            .await
        });

        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        let mut streamed = false;
        while !streamed && tokio::time::Instant::now() < deadline {
            streamed = session
                .session_updates()
                .iter()
                .any(|update| matches!(update, SessionUpdate::AgentMessageChunk(_)));
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(streamed, "the answer started streaming before the stop");
        tokio::time::sleep(Duration::from_millis(50)).await;
        conn.cx()
            .send_notification(CancelNotification::new(acp_session_id))
            .unwrap();
        let response = prompt.await.unwrap().unwrap();
        assert_eq!(response.stop_reason, StopReason::Cancelled);

        let store = SessionManager::new(conn.data_root());
        let rows = store
            .sessions_with_extension_state::<TurnOutcomeState>()
            .await
            .unwrap();
        assert_eq!(rows.len(), 1, "the stopped turn is recorded");
        let stopped = rows[0].state.stopped.expect("recorded as stopped");
        assert!(!rows[0].state.failed);
        assert!(
            stopped.elapsed_ms >= 50,
            "the turn's wall time: {stopped:?}"
        );
        let counter = goose::token_counter::create_token_counter().await.unwrap();
        assert_eq!(
            stopped.output_tokens,
            Some(counter.count_tokens(WRITTEN) as u64),
            "the streamed output, counted: {stopped:?}"
        );

        let conversation = store
            .get_session(&rows[0].session_id, true)
            .await
            .unwrap()
            .conversation
            .unwrap();
        let last = conversation.messages().last().unwrap();
        assert!(last.is_user_visible() && !last.is_agent_visible());
        let line = last
            .content
            .iter()
            .find_map(|content| match content {
                MessageContent::SystemNotification(notification) => {
                    assert_eq!(turn_outcome::stopped_of(notification), Some(stopped));
                    Some(notification.msg.clone())
                }
                _ => None,
            })
            .expect("the chat line is stored where the answer would be");
        assert_eq!(line, turn_outcome::stopped_line(stopped));
        assert!(line.starts_with("You stopped this answer after "), "{line}");
        // Q-519: the words the answer had streamed stay above the line, as the model's own reply.
        let messages = conversation.messages();
        let partial = &messages[messages.len() - 2];
        assert_eq!(partial.role, rmcp::model::Role::Assistant);
        assert!(partial.is_user_visible() && partial.is_agent_visible());
        assert_eq!(
            partial.as_concat_text(),
            WRITTEN,
            "the streamed words are stored"
        );
        assert_eq!(partial.id.as_deref(), Some("stopped-1"));

        store
            .add_message(&rows[0].session_id, &Message::user().with_text("go on"))
            .await
            .unwrap();
        turn_outcome::record(&store, &rows[0].session_id, None)
            .await
            .unwrap();
        let after = store
            .sessions_with_extension_state::<TurnOutcomeState>()
            .await
            .unwrap();
        assert_eq!(
            after[0].state.stopped, None,
            "a later completed turn clears the stop"
        );
    });
}

#[test]
fn the_chat_line_reads_the_elapsed_and_the_tokens() {
    let line = |elapsed_ms, output_tokens| {
        turn_outcome::stopped_line(turn_outcome::Stopped {
            elapsed_ms,
            output_tokens,
        })
    };
    assert_eq!(
        line(6 * 60_000 + 12_000, Some(1_900)),
        "You stopped this answer after 6 min · 1.9k tokens"
    );
    assert_eq!(
        line(40_000, Some(850)),
        "You stopped this answer after 40 s · 850 tokens"
    );
    assert_eq!(
        line(3_900_000, Some(24_228)),
        "You stopped this answer after 1 h 5 min · 24k tokens"
    );
    assert_eq!(
        line(95_000, Some(0)),
        "You stopped this answer after 1 min, before the model wrote anything"
    );
    assert_eq!(line(95_000, None), "You stopped this answer after 1 min");
}

/// A finished call's reported usage covers what it streamed; the stopped call's text and forming
/// tool-call arguments are counted, since its usage never arrives.
#[tokio::test]
async fn the_meter_counts_what_no_reported_usage_covers() {
    use goose_providers::conversation::token_usage::{ProviderUsage, Usage};
    use goose_providers::formats::openai::ToolFormingEvent;

    let mut meter = turn_outcome::TurnMeter::start();
    meter.on_output("the first call's words, reported below");
    meter.on_usage(&ProviderUsage::new(
        "m".to_string(),
        Usage::new(Some(10), Some(120), Some(130)),
    ));
    meter.on_output("the stopped call began ");
    (meter.tool_forming_observer())(ToolFormingEvent::ArgsDelta {
        id: "call-1".to_string(),
        delta: r##"{"path":"notes/status.md","content":"# Status"##.to_string(),
    });
    let counter = goose::token_counter::create_token_counter().await.unwrap();
    let unreported = counter
        .count_tokens(r##"the stopped call began {"path":"notes/status.md","content":"# Status"##);
    assert_eq!(
        meter.stopped().await.output_tokens,
        Some(120 + unreported as u64)
    );
}

/// Q-519: a stop keeps only the words the agent had not stored. The agent stores a call's
/// messages when its iteration ends, so a stop in a later call must not store an earlier call
/// again: a message the model reads that is not its own (a tool's result) ends an iteration, and
/// the same id with the same words already stored is the net for an iteration that ended without
/// one. Reasoning with no text is kept for the person and never sent back to the model.
#[tokio::test]
async fn a_stop_stores_only_the_words_the_agent_had_not() {
    use goose::session::SessionType;
    use turn_outcome::StreamedReply;

    let root = tempfile::tempdir().unwrap();
    let store = SessionManager::new(root.path().to_path_buf());
    let session = store
        .create_session(
            root.path().to_path_buf(),
            "stopped".to_string(),
            SessionType::Acp,
            goose::config::GooseMode::default(),
        )
        .await
        .unwrap();
    let chunk = |id: &str, text: &str| Message::assistant().with_id(id).with_text(text);

    let prompt = Message::user().with_text("Find the config and fix it");
    store.add_message(&session.id, &prompt).await.unwrap();
    let mut streamed = StreamedReply::default();

    // Call 1 streams, asks a tool, and the agent stores its iteration.
    let first = [chunk("call-1", "Let me "), chunk("call-1", "look.")];
    let tool_result = Message::user().with_text("config.yaml: port: 80");
    for message in first.iter().chain([&tool_result]) {
        streamed.on_message(message);
    }
    store
        .add_message(&session.id, &chunk("call-1", "Let me look."))
        .await
        .unwrap();
    store.add_message(&session.id, &tool_result).await.unwrap();

    // Call 2 ends with no message between it and call 3; the agent stored it.
    streamed.on_message(&chunk("call-2", "The port is wrong."));
    store
        .add_message(&session.id, &chunk("call-2", "The port is wrong."))
        .await
        .unwrap();

    // Call 3 is stopped mid-answer, and call 4 had only begun to reason.
    streamed.on_message(&chunk("call-3", "Setting it to "));
    streamed.on_message(&chunk("call-3", "8080 in"));
    streamed.on_message(
        &Message::assistant()
            .with_id("call-4")
            .with_thinking("the user wants", ""),
    );
    streamed.store(&store, &session.id).await.unwrap();

    let stored = store
        .get_session(&session.id, true)
        .await
        .unwrap()
        .conversation
        .unwrap();
    let rows: Vec<_> = stored
        .messages()
        .iter()
        .map(|m| (m.id.clone(), m.as_concat_text(), m.is_agent_visible()))
        .collect();
    let texts: Vec<_> = rows.iter().map(|(_, text, _)| text.as_str()).collect();
    assert_eq!(
        texts,
        [
            "Find the config and fix it",
            "Let me look.",
            "config.yaml: port: 80",
            "The port is wrong.",
            "Setting it to 8080 in",
            "",
        ],
        "{rows:#?}"
    );
    assert_eq!(rows[4].0.as_deref(), Some("call-3"));
    assert!(rows[4].2, "the stopped words are the model's own reply");
    let reasoning = &stored.messages()[5];
    assert_eq!(reasoning.id.as_deref(), Some("call-4"));
    assert!(reasoning.is_user_visible() && !reasoning.is_agent_visible());
    assert!(matches!(
        reasoning.content.as_slice(),
        [MessageContent::Thinking(thinking)] if thinking.thinking == "the user wants"
    ));
}
