//! Session loops Q9 (DESIGN-SESSION-LOOPS §5.1, "A renderer reload mid-tick"): what this ACP stack
//! does to a prompt that is still running when its window's websocket closes. Measured over the
//! real router the desktop connects to (`create_acp_router`, a real TCP websocket), against a
//! model endpoint that streams its first words and never finishes.
//!
//! Measured answer (b): the websocket's close makes `agent-client-protocol-http` abort the
//! connection's task, and that drops every future the connection spawned — `on_prompt` included —
//! mid-await. Nothing after the await runs: no `clear_active_run`, no end-of-turn record. So the
//! state a prompt holds for its turn must be released by guards that drop with the future — and
//! since Q-519 the run's guard also settles the turn as stopped (`acp_closed_window_turn_test.rs`).

#[path = "acp_ws/mod.rs"]
mod acp_ws;

use acp_ws::{configure, eventually, serve, user_turns_running, Answer, Model, Window};
use goose::session::SessionManager;
use goose::turn_outcome::TurnOutcomeState;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_closed_websocket_drops_the_prompt_it_was_running() {
    let model = Model::start(vec![
        Answer::Unfinished("The first words of an answer that never finishes"),
        Answer::Unfinished("The reloaded window's answer"),
    ])
    .await;
    configure(&model, false);
    let addr = serve().await;

    let mut window = Window::open(addr, false).await;
    let work = tempfile::tempdir().unwrap();
    let session_id = window.new_chat(work.path()).await;

    assert_eq!(user_turns_running(), 0);
    window.prompt(&session_id, "Plan the migration", None).await;
    window.until_streaming(&session_id).await;
    assert_eq!(
        user_turns_running(),
        1,
        "the prompt holds its user turn while it streams"
    );

    // The window reloads: its websocket closes with the answer still streaming.
    window.close().await;

    // (b): the turn's guards drop with the prompt's future — the prompt did not run on unseen.
    eventually("the closed connection's prompt was dropped", || async {
        user_turns_running() == 0
    })
    .await;
    // Nothing after the await ran, so the prompt's run registration settles the turn it held as
    // stopped (Q-519): a closed window's answer reads Stopped, as the in-window Stop's does.
    eventually(
        "the dropped prompt's turn is recorded as stopped",
        || async {
            SessionManager::instance()
                .sessions_with_extension_state::<TurnOutcomeState>()
                .await
                .unwrap()
                .iter()
                .any(|row| row.session_id == session_id && row.state.stopped.is_some())
        },
    )
    .await;

    // A new window on the same goose serves the same chat at once: nothing of the old
    // connection's run holds the session busy.
    let mut reloaded = Window::open(addr, false).await;
    reloaded
        .request(
            "session/load",
            serde_json::json!({"sessionId": session_id, "cwd": work.path(), "mcpServers": []}),
        )
        .await;
    // The load replays the stopped answer's stored words as chunks; only the new answer's count.
    reloaded.seen.clear();
    let prompt = reloaded.prompt(&session_id, "Go on", None).await;
    reloaded
        .notification("session/update", |p| {
            p["sessionId"] == session_id.as_str()
                && p["update"]["sessionUpdate"] == "agent_message_chunk"
                && p.to_string().contains("The reloaded window's answer")
        })
        .await;
    assert!(
        !reloaded.seen.iter().any(|frame| frame["id"] == prompt),
        "the prompt was not refused: {:#?}",
        reloaded.seen
    );
    assert_eq!(user_turns_running(), 1);
}
