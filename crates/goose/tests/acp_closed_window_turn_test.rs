//! Q-519: a chat whose window was closed while it answered ("Stop the answer and close", Q-490)
//! reopened with the person's prompt and nothing after it — no streamed words, no "You stopped this
//! answer" line — while the in-window Stop left the line. The close drops the websocket, which drops
//! `on_prompt` mid-await (Q9, `acp_connection_close_test.rs`), so nothing after the await ran. A
//! turn ended by its window closing keeps what it streamed and the stopped line, as a Stop does.

#[path = "acp_ws/mod.rs"]
mod acp_ws;

use acp_ws::{configure, eventually, serve, user_turns_running, Answer, Model, Window};
use goose::conversation::message::{Message, MessageContent};
use goose::session::SessionManager;
use goose::turn_outcome::{self, TurnOutcomeState};
use rmcp::model::Role;

const STREAMED: &str = "The light went out on a Tuesday. That was the trouble with lighthouses";

async fn stored(session_id: &str) -> Vec<Message> {
    SessionManager::instance()
        .get_session(session_id, true)
        .await
        .unwrap()
        .conversation
        .unwrap()
        .messages()
        .clone()
}

fn stopped_line_of(message: &Message) -> Option<String> {
    message.content.iter().find_map(|content| match content {
        MessageContent::SystemNotification(notification) => {
            turn_outcome::stopped_of(notification).map(|_| notification.msg.clone())
        }
        _ => None,
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_turn_ended_by_closing_its_window_keeps_its_words_and_the_stopped_line() {
    let model = Model::start(vec![Answer::Unfinished(STREAMED)]).await;
    configure(&model, false);
    let addr = serve().await;

    let mut window = Window::open(addr, true).await;
    let work = tempfile::tempdir().unwrap();
    let session_id = window.new_chat(work.path()).await;

    window
        .prompt(
            &session_id,
            "Write a 400-word story about a lighthouse keeper. Just the story.",
            None,
        )
        .await;
    window.until_streaming(&session_id).await;

    // "Stop the answer and close": the window goes with the answer still streaming.
    window.close().await;

    eventually(
        "the closed window's turn was recorded as stopped",
        || async {
            stored(&session_id)
                .await
                .last()
                .is_some_and(|last| stopped_line_of(last).is_some())
        },
    )
    .await;
    eventually("the closed window's run was released", || async {
        user_turns_running() == 0
    })
    .await;

    let messages = stored(&session_id).await;
    let texts: Vec<_> = messages
        .iter()
        .map(|m| (m.role.clone(), m.as_concat_text()))
        .collect();
    assert_eq!(
        messages.len(),
        3,
        "the prompt, the streamed words, the stopped line: {texts:#?}"
    );
    assert_eq!(messages[0].role, Role::User);
    assert_eq!(
        messages[0].as_concat_text(),
        "Write a 400-word story about a lighthouse keeper. Just the story."
    );

    let partial = &messages[1];
    assert_eq!(partial.role, Role::Assistant);
    assert_eq!(
        partial.as_concat_text(),
        STREAMED,
        "the words the window showed"
    );
    assert!(partial.is_user_visible() && partial.is_agent_visible());

    let notice = &messages[2];
    assert!(notice.is_user_visible() && !notice.is_agent_visible());
    let line = stopped_line_of(notice).expect("the stopped line");
    assert!(line.starts_with("You stopped this answer after "), "{line}");

    let row = SessionManager::instance()
        .sessions_with_extension_state::<TurnOutcomeState>()
        .await
        .unwrap()
        .into_iter()
        .find(|row| row.session_id == session_id)
        .expect("the turn's outcome is recorded");
    let stopped = row.state.stopped.expect("recorded as stopped, not failed");
    assert!(!row.state.failed);
    assert_eq!(
        stopped.output_tokens,
        Some(
            goose::token_counter::create_token_counter()
                .await
                .unwrap()
                .count_tokens(STREAMED) as u64
        ),
        "the streamed words, counted"
    );
}
