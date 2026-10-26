//! Recipe setup remains owned by the original process while every viewer disconnects.
#[path = "acp_ws/mod.rs"]
mod acp_ws;

use acp_ws::{configure, eventually, serve, Model, Window};
use async_trait::async_trait;
use goose::config::paths::Paths;
use goose::session::SessionManager;
use goose::work_control::OwnerRunRegistry;
use goose_sdk_types::custom_requests::REQUEST_RECIPE_PARAMS_METHOD;
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
async fn detached_recipe_form_uses_original_callback_once_without_a_model_run() {
    let model = Model::start(vec![]).await;
    configure(&model, false);
    let addr = serve().await;
    let cwd = tempfile::tempdir().unwrap();
    let capabilities =
        json!({"_meta":{"goose":{"customNotifications":true,"recipeParameterRequests":true}}});
    let recipe: goose::recipe::Recipe = serde_json::from_value(json!({
        "version":"1.0.0", "title":"Typed deployment", "description":"Configure a deployment",
        "prompt":"Deploy to {{ target }} with {{ replicas }} replicas. Enabled {{ enabled }}.",
        "extensions":[],
        "parameters":[
            {"key":"target","input_type":"select","requirement":"required","description":"Deployment target","options":["staging","production"]},
            {"key":"replicas","input_type":"number","requirement":"required","description":"Replica count"},
            {"key":"enabled","input_type":"boolean","requirement":"required","description":"Enable deployment"}
        ]
    })).unwrap();
    let mut original = Window::open_with_capabilities(addr, capabilities.clone()).await;
    original.send("session/new",json!({"cwd":cwd.path(),"mcpServers":[],"_meta":{"client":"goose-desktop","recipeDeeplink":goose::recipe_deeplink::encode(&recipe).unwrap()}})).await;
    let request = original
        .notification(REQUEST_RECIPE_PARAMS_METHOD, |_| true)
        .await;
    let chat = request["sessionId"].as_str().unwrap().to_owned();
    let manager = Arc::new(SessionManager::instance());
    let registry = OwnerRunRegistry::for_store(Paths::data_dir(), manager.clone());
    let catalog = manager.work_catalog().await.unwrap();
    let record = catalog.lookup_local_session(&chat).await.unwrap();
    let work_id = Uuid::parse_str(&record.work_id).unwrap();
    let generation = Uuid::parse_str(&catalog.watermark().await.unwrap().generation).unwrap();
    let inputs = registry.pending_inputs(work_id).await.unwrap();
    assert_eq!(inputs.len(), 1);
    let input = &inputs[0];
    assert!(input.run_id.is_none());
    let PendingInputPayload::Form { schema } = &input.payload else {
        panic!("recipe input must be a typed form")
    };
    assert_eq!(
        schema["properties"]["target"]["enum"],
        json!(["staging", "production"])
    );
    assert_eq!(schema["properties"]["replicas"]["type"], "number");
    assert_eq!(schema["properties"]["enabled"]["type"], "boolean");
    assert_eq!(schema["required"], json!(["target", "replicas", "enabled"]));
    original.close().await;
    assert!(registry.current_run(&chat).await.is_none());
    assert!(model.requests.lock().unwrap().is_empty());

    let mut observer = Window::open_with_capabilities(addr, capabilities).await;
    observer
        .request(
            "session/load",
            json!({"sessionId":chat,"cwd":cwd.path(),"mcpServers":[]}),
        )
        .await;
    let restored = observer
        .notification(REQUEST_RECIPE_PARAMS_METHOD, |_| true)
        .await;
    assert_eq!(restored, request);
    assert_eq!(
        registry.pending_inputs(work_id).await.unwrap()[0].input_id,
        input.input_id
    );
    observer.close().await;

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
        "recipe-controller",
        &receiver,
        ControllerScope::InputAnswer,
    )
    .await
    .unwrap();
    let authorization = DispatchAuthorization::new(
        authority,
        "recipe-controller".into(),
        receiver,
        ControllerScope::InputAnswer,
        device.clone(),
    );
    let invalid = [
        json!({"target":"unknown","replicas":2,"enabled":true}),
        json!({"target":"staging","enabled":true}),
        json!({"target":"staging","replicas":2,"enabled":"true"}),
    ];
    for values in invalid
        .into_iter()
        .chain([json!({"target":"staging","replicas":2,"enabled":true})])
    {
        let valid = values == json!({"target":"staging","replicas":2,"enabled":true});
        let record = catalog.lookup_local_session(&chat).await.unwrap();
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
                option_id: "accept".into(),
                value: Some(values.to_string()),
            },
        };
        let accepted = registry
            .accept_command(&device, command.clone(), authorization.clone())
            .await
            .unwrap();
        eventually("recipe command settled", || async {
            matches!(
                registry
                    .receipt(&device, accepted.command_id)
                    .await
                    .unwrap()
                    .state,
                CommandState::Delivered | CommandState::Rejected
            )
        })
        .await;
        let receipt = registry
            .receipt(&device, accepted.command_id)
            .await
            .unwrap();
        assert!(receipt.run_id.is_none());
        if valid {
            assert!(
                matches!(receipt.state, CommandState::Delivered),
                "{:?}",
                receipt.reason
            );
            let replay = registry
                .accept_command(&device, command, authorization.clone())
                .await
                .unwrap();
            assert!(matches!(replay.state, CommandState::Delivered));
        } else {
            assert!(matches!(receipt.state, CommandState::Rejected));
            assert!(receipt.reason.unwrap().starts_with("invalid_input_value:"));
            assert_eq!(
                registry.pending_inputs(work_id).await.unwrap()[0].input_id,
                input.input_id
            );
        }
    }
    eventually("original recipe setup completed", || async {
        catalog.lookup_local_session(&chat).await.unwrap().state == "idle"
    })
    .await;
    let session = manager.get_session(&chat, false).await.unwrap();
    assert_eq!(
        serde_json::to_value(session.user_recipe_values).unwrap(),
        json!({"target":"staging","replicas":"2","enabled":"true"})
    );
    assert!(registry.pending_inputs(work_id).await.unwrap().is_empty());
    assert!(registry.current_run(&chat).await.is_none());
    assert!(model.requests.lock().unwrap().is_empty());

    let record = catalog.lookup_local_session(&chat).await.unwrap();
    let duplicate = WorkCommand {
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
            option_id: "cancel".into(),
            value: None,
        },
    };
    let receipt = registry
        .accept_command(&device, duplicate, authorization)
        .await
        .unwrap();
    eventually("second form answer refused", || async {
        matches!(
            registry
                .receipt(&device, receipt.command_id)
                .await
                .unwrap()
                .state,
            CommandState::Rejected
        )
    })
    .await;
    assert!(registry
        .receipt(&device, receipt.command_id)
        .await
        .unwrap()
        .reason
        .unwrap()
        .starts_with("input_already_answered:"));
    assert_eq!(
        manager
            .get_session(&chat, false)
            .await
            .unwrap()
            .user_recipe_values
            .unwrap()["target"],
        "staging"
    );
}
