//! Actual ask_user -> idle owner question -> remote continuation with no original viewer.
#[path = "acp_ws/mod.rs"]
mod acp_ws;

use acp_ws::{configure, eventually, serve, user_turns_running, Answer, Model, Window};
use async_trait::async_trait;
use goose::config::paths::Paths;
use goose::needs_you::{NeedsYouState, NeedsYouStatus};
use goose::session::extension_data::ExtensionState;
use goose::session::SessionManager;
use goose::work_control::OwnerRunRegistry;
use leanzero_link::controller_auth::{
    authorize, AuthorizationError, ControllerAuthority, DispatchAuthorization, ReceiverBinding,
};
use leanzero_link::controller_wire::*;
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

struct Authority(AuthorizationLease);
#[async_trait]
impl ControllerAuthority for Authority {
    async fn introspect(
        &self,
        _: &str,
        _: &ReceiverBinding,
        _: ControllerScope,
    ) -> Result<Option<AuthorizationLease>, AuthorizationError> {
        Ok(Some(self.0.clone()))
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn idle_remote_answer_continues_original_owner_once_and_preserves_other_question() {
    let model = Model::start(vec![Answer::ToolCalls(vec![
        ("ask_user", json!({"question":"Which deployment target?","why":"Deployment needs a target.","recommended_answer":"staging","options":["staging","production"]}).to_string()),
        ("ask_user", json!({"question":"Which log format?","why":"Logs need a format.","recommended_answer":"JSON","options":["JSON","plain"]}).to_string()),
    ])]).await;
    model.keep_side_requests_off_the_script();
    configure(&model, false);
    let addr = serve().await;
    let work = tempfile::tempdir().unwrap();
    let mut original = Window::open(addr, true).await;
    let chat = original
        .request(
            "session/new",
            json!({"cwd": work.path(), "mcpServers": [], "_meta": {"client": "goose-desktop"}}),
        )
        .await["sessionId"]
        .as_str()
        .unwrap()
        .to_string();
    let prompt = original
        .prompt(&chat, "Ask both configuration questions", None)
        .await;
    assert_eq!(
        original.response(prompt).await.unwrap()["stopReason"],
        "end_turn"
    );
    eventually("ask_user ended the model turn", || async {
        user_turns_running() == 0
    })
    .await;
    original.close().await;

    let manager = Arc::new(SessionManager::instance());
    let registry = OwnerRunRegistry::for_store(Paths::data_dir(), manager.clone());
    let catalog = manager.work_catalog().await.unwrap();
    let record = catalog.lookup_local_session(&chat).await.unwrap();
    assert_eq!(record.state, "waiting_input");
    assert!(record.current_run_id.is_none());
    let work_id = Uuid::parse_str(&record.work_id).unwrap();
    let generation = Uuid::parse_str(&catalog.watermark().await.unwrap().generation).unwrap();
    let inputs = registry.pending_inputs(work_id).await.unwrap();
    assert_eq!(inputs.len(), 2);
    let input = inputs
        .iter()
        .find(|input| input.prompt == "Which deployment target?")
        .unwrap();
    assert!(input.run_id.is_none());
    let item_id = match &input.payload {
        PendingInputPayload::Question {
            item_id,
            origin_run_id,
            ..
        } => {
            assert!(origin_run_id.is_some());
            item_id.clone()
        }
        _ => panic!("expected typed question"),
    };
    let owner = Uuid::new_v4();
    registry.bind_owner(owner).unwrap();
    let receiver = ReceiverBinding {
        owner_node_id: owner,
        runtime_id: Uuid::new_v4(),
        store_generation: generation,
        host_lease_id: Uuid::new_v4(),
        host_revision: 1,
    };
    let authority = Arc::new(Authority(AuthorizationLease {
        active: true,
        account_id: Uuid::new_v4(),
        device_id: Uuid::new_v4(),
        device_revision: 1,
        role: DeviceRole::Controller,
        owner_node_id: owner,
        scopes: vec![ControllerScope::InputAnswer],
        token_expires_at: chrono::Utc::now() + chrono::Duration::hours(1),
        lease_seconds: 30,
        host_revision: 1,
        store_generation: generation,
        host_lease_id: receiver.host_lease_id,
    }));
    let device = authorize(
        authority.as_ref(),
        "test-controller",
        &receiver,
        ControllerScope::InputAnswer,
    )
    .await
    .unwrap();
    let authorization = DispatchAuthorization::new(
        authority,
        "test-controller".into(),
        receiver,
        ControllerScope::InputAnswer,
        device.clone(),
    );
    let command = WorkCommand {
        command_id: Uuid::new_v4(),
        work: WorkRef {
            owner_node_id: owner,
            kind: WorkKind::Chat,
            work_id,
        },
        expected_store_generation: generation,
        expected_revision: record.revision.try_into().unwrap(),
        expected_run_id: None,
        action: ActionPayload::AnswerInput {
            input_id: input.input_id,
            option_id: "choice_0".into(),
            value: None,
        },
    };
    model.script(vec![Answer::Finish("I will deploy to staging.")]);
    let accepted = registry
        .accept_command(&device, command.clone(), authorization.clone())
        .await
        .unwrap();
    eventually("remote question continuation delivered", || async {
        matches!(
            registry
                .receipt(&device, accepted.command_id)
                .await
                .unwrap()
                .state,
            CommandState::Delivered
        )
    })
    .await;
    let replay = registry
        .accept_command(&device, command, authorization)
        .await
        .unwrap();
    assert_eq!(replay.command_id, accepted.command_id);
    eventually("remote owner continuation finished", || async {
        registry.current_run(&chat).await.is_none()
    })
    .await;
    let final_session = manager.get_session(&chat, true).await.unwrap();
    let questions = NeedsYouState::from_extension_data(&final_session.extension_data).unwrap();
    let answered = questions
        .items
        .iter()
        .find(|item| item.id == item_id)
        .unwrap();
    assert_eq!(answered.status, NeedsYouStatus::Answered);
    assert_eq!(answered.answer.as_deref(), Some("staging"));
    assert!(answered.answer_delivered_at.is_some());
    assert_eq!(
        questions
            .items
            .iter()
            .filter(|item| item.status == NeedsYouStatus::Open)
            .count(),
        1
    );
    let messages = final_session.conversation.unwrap();
    assert_eq!(
        messages
            .messages()
            .iter()
            .filter(|message| message.as_concat_text()
                == "Answer to your question \"Which deployment target?\": staging")
            .count(),
        1
    );
    assert!(messages
        .messages()
        .iter()
        .any(|message| message.as_concat_text() == "I will deploy to staging."));
    assert_eq!(model.requests.lock().unwrap().len(), 2);
    assert!(model
        .requests
        .lock()
        .unwrap()
        .last()
        .unwrap()
        .contains("staging"));
    assert!(!model
        .requests
        .lock()
        .unwrap()
        .last()
        .unwrap()
        .contains("closed as superseded"));
}
