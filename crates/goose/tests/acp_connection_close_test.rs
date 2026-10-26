//! The real ACP websocket is a viewer of an original-owner task.
#[path = "acp_ws/mod.rs"]
mod acp_ws;

use acp_ws::{configure, eventually, serve, user_turns_running, Answer, Model, Window};
use goose::session::SessionManager;
use serde_json::json;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_owner_run_survives_every_viewer_and_a_reconnected_view_can_control_it() {
    let model = Model::start(vec![
        Answer::Unfinished("The original owner's answer"),
        Answer::Unfinished("The following turn"),
    ])
    .await;
    configure(&model, false);
    let addr = serve().await;
    let mut original = Window::open(addr, false).await;
    let work = tempfile::tempdir().unwrap();
    let chat = original.new_chat(work.path()).await;
    let mut second = Window::open(addr, false).await;
    second
        .request(
            "session/load",
            json!({"sessionId":chat,"cwd":work.path(),"mcpServers":[]}),
        )
        .await;
    original
        .prompt(&chat, "Keep working on the original Mac", None)
        .await;
    original
        .until_answer_streams(&chat, "The original owner's answer")
        .await;
    second
        .until_answer_streams(&chat, "The original owner's answer")
        .await;
    let catalog = SessionManager::instance().work_catalog().await.unwrap();
    let first_run = catalog
        .lookup_local_session(&chat)
        .await
        .unwrap()
        .current_run_id
        .unwrap();
    original.close().await;
    second.close().await;

    let mut reloaded = Window::open(addr, false).await;
    reloaded
        .request(
            "session/load",
            json!({"sessionId":chat,"cwd":work.path(),"mcpServers":[]}),
        )
        .await;
    let duplicate = reloaded
        .prompt(&chat, "This must not start another model call", None)
        .await;
    assert!(reloaded.response(duplicate).await.is_err());
    assert_eq!(
        catalog
            .lookup_local_session(&chat)
            .await
            .unwrap()
            .current_run_id
            .as_deref(),
        Some(first_run.as_str())
    );
    assert_eq!(user_turns_running(), 1);
    assert_eq!(model.completion_requests(), 1);
    reloaded.cancel(&chat).await;
    eventually("explicit stop releases the detached task", || async {
        user_turns_running() == 0
    })
    .await;
    reloaded.prompt(&chat, "Start the next turn", None).await;
    reloaded
        .until_answer_streams(&chat, "The following turn")
        .await;
    let stale = reloaded.send("_goose/unstable/session/steer", json!({"sessionId":chat,"expectedRunId":first_run,"prompt":[{"type":"text","text":"old steer"}]})).await;
    assert!(reloaded.response(stale).await.is_err());
    assert_eq!(user_turns_running(), 1);
    reloaded.cancel(&chat).await;
    eventually("next turn explicitly stopped", || async {
        user_turns_running() == 0
    })
    .await;
}
