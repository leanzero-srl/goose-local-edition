//! Real provider -> permission -> native owner shell path with every original viewer gone.
#[path = "acp_ws/mod.rs"]
mod acp_ws;

use acp_ws::{configure, eventually, user_turns_running, Answer, Model, Window, SETTLE};
use futures::{SinkExt, StreamExt};
use goose::acp::server_factory::{AcpServer, AcpServerFactoryConfig};
use goose::acp::transport::create_acp_router;
use goose::agents::GoosePlatform;
use goose::config::paths::Paths;
use goose::session::SessionManager;
use goose::work_control::{InputAnswer, OwnerRunRegistry};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use uuid::Uuid;

async fn permission(window: &mut Window) -> Value {
    if let Some(at) = window
        .seen
        .iter()
        .position(|frame| frame["method"] == "session/request_permission")
    {
        return window.seen.remove(at);
    }
    tokio::time::timeout(SETTLE, async {
        loop {
            let Some(Ok(WsMessage::Text(text))) = window.socket.next().await else {
                panic!("permission observer closed");
            };
            let frame: Value = serde_json::from_str(&text).unwrap();
            if frame["method"] == "session/request_permission" {
                return frame;
            }
            window.seen.push(frame);
        }
    })
    .await
    .expect("owner must expose its pending permission")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn original_viewer_close_does_not_answer_permission_and_reconnected_view_runs_native_owner_tool_once(
) {
    let work = tempfile::tempdir().unwrap();
    let receipt = work.path().join("owner-shell-receipt.txt");
    let arguments: &'static str = Box::leak(
        json!({"command": format!("printf 'owner tool executed\\n' >> '{}'", receipt.display())})
            .to_string()
            .into_boxed_str(),
    );
    let model = Model::start(vec![
        Answer::ToolCall("shell", arguments),
        Answer::Finish("The owner tool finished."),
    ])
    .await;
    model.keep_side_requests_off_the_script();
    configure(&model, false);
    let config_path = Paths::config_dir().join(goose::config::base::CONFIG_YAML_NAME);
    let mut config = std::fs::read_to_string(&config_path).unwrap();
    config.push_str("GOOSE_MODE: approve\nGOOSE_DISABLE_TOOL_CALL_SUMMARY: true\n");
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
    let mut original = Window::open(addr, true).await;
    let chat = original.new_chat(work.path()).await;
    original
        .prompt(&chat, "Write the owner receipt", None)
        .await;
    let old_request = permission(&mut original).await;
    let manager = Arc::new(SessionManager::instance());
    let registry = OwnerRunRegistry::for_store(Paths::data_dir(), manager.clone());
    let work_id = Uuid::parse_str(
        &manager
            .work_catalog()
            .await
            .unwrap()
            .lookup_local_session(&chat)
            .await
            .unwrap()
            .work_id,
    )
    .unwrap();
    let input = registry.pending_inputs(work_id).await.unwrap().remove(0);
    let wire = serde_json::to_value(&input).unwrap();
    assert_eq!(wire["payload"]["kind"], "permission");
    assert_eq!(
        wire["payload"]["request_id"],
        old_request["params"]["toolCall"]["toolCallId"]
    );
    assert_eq!(wire["payload"]["tool_name"], "shell");
    assert_eq!(
        wire["payload"]["arguments"],
        serde_json::from_str::<Value>(arguments).unwrap()
    );
    assert_eq!(input.prompt, "Allow shell?");
    original.close().await;

    let mut reconnected = Window::open(addr, true).await;
    reconnected
        .request(
            "session/load",
            json!({"sessionId":chat,"cwd":work.path(),"mcpServers":[]}),
        )
        .await;
    let current_request = permission(&mut reconnected).await;
    assert_eq!(
        serde_json::to_value(registry.pending_inputs(work_id).await.unwrap().remove(0)).unwrap(),
        wire
    );
    assert_eq!(
        old_request["params"]["toolCall"]["toolCallId"],
        current_request["params"]["toolCall"]["toolCallId"]
    );
    assert!(
        !receipt.exists(),
        "closing a permission observer must not approve the tool"
    );
    assert_eq!(user_turns_running(), 1);
    reconnected.socket.send(WsMessage::Text(json!({"jsonrpc":"2.0","id":current_request["id"],"result":{"outcome":{"outcome":"selected","optionId":"allow_once"}}}).to_string().into())).await.unwrap();
    reconnected
        .until_answer_streams(&chat, "The owner tool finished.")
        .await;
    eventually("the owner tool completed", || async {
        user_turns_running() == 0
    })
    .await;
    assert_eq!(
        std::fs::read_to_string(&receipt).unwrap(),
        "owner tool executed\n"
    );
    assert!(registry.pending_inputs(work_id).await.unwrap().is_empty());
    let late = registry
        .answer_input(
            input.input_id,
            input.run_id,
            InputAnswer {
                option_id: "allow_once".into(),
                value: None,
            },
        )
        .await
        .unwrap_err();
    assert_eq!(late.body.code, "input_already_answered");
    assert_eq!(
        std::fs::read_to_string(&receipt).unwrap().lines().count(),
        1
    );
}
