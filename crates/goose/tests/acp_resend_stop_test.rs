//! A provider call that resets mid-answer is resent (`transient_resend`); its streamed partial is
//! not the model's answer. A person who stops the turn while the resend runs must find only what
//! the RESENT call streamed stored above the stop line — never the abandoned partial, which the
//! stopped-turn record (`settle_stopped_turn`, Q-519) would otherwise store as the model's words.

#[allow(dead_code)]
#[path = "acp_common_tests/mod.rs"]
mod common_tests;

use agent_client_protocol::schema::v1::{
    CancelNotification, ContentBlock, PromptRequest, StopReason, TextContent,
};
use common_tests::fixtures::server::AcpServerConnection;
use common_tests::fixtures::OpenAiFixture;
use common_tests::fixtures::{run_test, Connection, Session, SessionData, TestConnectionConfig};
use goose::session::SessionManager;
use goose::turn_outcome::TurnOutcomeState;
use serial_test::serial;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const ABANDONED: &str = "ABANDONED partial words of the call that reset";
const RESENT: &str = "RESENT words of the call goose sent again";
/// The session title request carries this marker (session_naming.rs); it is answered apart so it
/// cannot take a reply call's script.
const TITLE_REQUEST: &str = "---BEGIN USER MESSAGES---";

static ACP_CONFIG_ROOT: LazyLock<tempfile::TempDir> =
    LazyLock::new(|| tempfile::tempdir().unwrap());

fn write_acp_global_config() -> PathBuf {
    std::env::set_var("GOOSE_PATH_ROOT", ACP_CONFIG_ROOT.path());
    std::env::set_var("GOOSE_DISABLE_KEYRING", "1");
    std::env::set_var("GOOSE_PROVIDER_SKIP_BACKOFF", "true");
    let config_dir = goose::config::paths::Paths::config_dir();
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(
        config_dir.join(goose::config::base::CONFIG_YAML_NAME),
        "GOOSE_MODEL: gpt-4o\nGOOSE_PROVIDER: openai\nGOOSE_DISABLE_KEYRING: true\n",
    )
    .unwrap();
    config_dir
}

fn sse_chunk(id: &str, text: &str) -> String {
    let chunk = serde_json::json!({
        "id": id,
        "object": "chat.completion.chunk",
        "created": 1,
        "model": "gpt-4o",
        "choices": [{"index": 0, "delta": {"role": "assistant", "content": text}, "finish_reason": null}],
    });
    format!("data: {chunk}\n\n")
}

async fn read_request(socket: &mut tokio::net::TcpStream) -> String {
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
    String::from_utf8_lossy(&request).to_string()
}

/// The first reply call streams [`ABANDONED`] and drops its connection before the answer's end
/// (a truncated stream: transient); every later reply call streams [`RESENT`] and never finishes.
async fn resetting_then_holding_openai(reply_calls: Arc<AtomicUsize>) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        let mut held = Vec::new();
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let request = read_request(&mut socket).await;
            if request.starts_with("GET") {
                let body = include_str!("acp_test_data/openai_models.json");
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(response.as_bytes()).await;
                continue;
            }
            let head =
                "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n";
            let _ = socket.write_all(head.as_bytes()).await;
            if request.contains(TITLE_REQUEST) {
                held.push(socket);
                continue;
            }
            let call = reply_calls.fetch_add(1, Ordering::SeqCst);
            let text = if call == 0 { ABANDONED } else { RESENT };
            let _ = socket
                .write_all(sse_chunk(&format!("reply-{call}"), text).as_bytes())
                .await;
            let _ = socket.flush().await;
            if call == 0 {
                drop(socket);
            } else {
                held.push(socket);
            }
        }
    });
    base_url
}

#[test]
#[serial]
fn a_stop_during_the_resend_stores_no_word_of_the_abandoned_call() {
    write_acp_global_config();
    run_test(async move {
        let reply_calls = Arc::new(AtomicUsize::new(0));
        let openai =
            OpenAiFixture::serving_from(resetting_then_holding_openai(reply_calls.clone()).await)
                .await;
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

        let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
        let mut resent_streamed = false;
        while !resent_streamed && tokio::time::Instant::now() < deadline {
            resent_streamed = session
                .session_updates()
                .iter()
                .any(|update| format!("{update:?}").contains(RESENT));
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(
            resent_streamed,
            "the reset call was resent and the resend streamed ({} reply calls)",
            reply_calls.load(Ordering::SeqCst)
        );
        assert_eq!(reply_calls.load(Ordering::SeqCst), 2);
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
        assert!(rows[0].state.stopped.is_some(), "recorded as stopped");
        let conversation = store
            .get_session(&rows[0].session_id, true)
            .await
            .unwrap()
            .conversation
            .unwrap();
        let texts: Vec<String> = conversation
            .messages()
            .iter()
            .map(|message| message.as_concat_text())
            .collect();
        assert!(
            texts.iter().all(|text| !text.contains(ABANDONED)),
            "the abandoned call's words were stored: {texts:?}"
        );
        assert!(
            texts.iter().any(|text| text == RESENT),
            "the resend's streamed words are the stopped answer: {texts:?}"
        );
    });
}
