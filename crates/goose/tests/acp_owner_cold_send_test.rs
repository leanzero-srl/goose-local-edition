//! Cold controller dispatch must not invalidate itself while restoring saved runtime state.
#[path = "acp_ws/mod.rs"]
mod acp_ws;

use acp_ws::{configure, eventually, serve, Answer, Model, Window};
use async_trait::async_trait;
use goose::agents::ExtensionConfig;
use goose::config::{paths::Paths, GooseMode};
use goose::session::{
    EnabledExtensionsState, ExtensionData, ExtensionState, SessionManager, SessionType,
};
use goose::work_control::OwnerRunRegistry;
use goose_providers::model::ModelConfig;
use leanzero_link::controller_auth::{
    authorize, AuthorizationError, ControllerAuthority, DispatchAuthorization, ReceiverBinding,
};
use leanzero_link::controller_wire::*;
use std::sync::Arc;
use tokio::sync::{Mutex, Notify};
use uuid::Uuid;

struct Authority {
    lease: AuthorizationLease,
    barrier: Mutex<Option<(Arc<Notify>, Arc<Notify>)>>,
}
#[async_trait]
impl ControllerAuthority for Authority {
    async fn introspect(
        &self,
        _: &str,
        _: &ReceiverBinding,
        _: ControllerScope,
    ) -> Result<Option<AuthorizationLease>, AuthorizationError> {
        if let Some((entered, resume)) = self.barrier.lock().await.take() {
            entered.notify_one();
            resume.notified().await;
        }
        Ok(Some(self.lease.clone()))
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cold_send_preserves_saved_revision_and_rejects_real_changes() {
    let model = Model::start(vec![Answer::Finish("Cold phone connection works.")]).await;
    model.keep_side_requests_off_the_script();
    configure(&model, false);
    let addr = serve().await;
    let observer = Window::open(addr, true).await;
    let manager = Arc::new(SessionManager::instance());
    let registry = OwnerRunRegistry::for_store(Paths::data_dir(), manager.clone());
    let catalog = manager.work_catalog().await.unwrap();
    let generation = Uuid::parse_str(&catalog.watermark().await.unwrap().generation).unwrap();
    let owner = Uuid::new_v4();
    registry.bind_owner(owner).unwrap();
    let receiver = ReceiverBinding {
        owner_node_id: owner,
        runtime_id: Uuid::new_v4(),
        store_generation: generation,
        host_lease_id: Uuid::new_v4(),
        host_revision: 1,
    };
    let authority = Arc::new(Authority {
        lease: AuthorizationLease {
            active: true,
            account_id: Uuid::new_v4(),
            device_id: Uuid::new_v4(),
            device_revision: 1,
            role: DeviceRole::Controller,
            owner_node_id: owner,
            scopes: vec![ControllerScope::WorkSend],
            token_expires_at: chrono::Utc::now() + chrono::Duration::hours(1),
            lease_seconds: 30,
            host_revision: 1,
            store_generation: generation,
            host_lease_id: receiver.host_lease_id,
        },
        barrier: Mutex::new(None),
    });
    let device = authorize(
        authority.as_ref(),
        "test-controller",
        &receiver,
        ControllerScope::WorkSend,
    )
    .await
    .unwrap();
    let normalized = goose::providers::get_from_registry("openai")
        .await
        .unwrap()
        .normalize_model_config(ModelConfig::new("gpt-4o"))
        .unwrap();
    assert!(
        normalized.context_limit.is_some(),
        "normalization control must change missing context"
    );

    for scenario in [
        "unchanged",
        "external_change",
        "normalization_change",
        "fallback_change",
    ] {
        let directory = tempfile::tempdir().unwrap();
        let session = manager
            .create_session(
                directory.path().to_path_buf(),
                scenario.into(),
                SessionType::User,
                GooseMode::Auto,
            )
            .await
            .unwrap();
        let mut extension_data = ExtensionData::new();
        EnabledExtensionsState::new(vec![
            ExtensionConfig::Frontend {
                name: "saved-frontend".into(),
                description: "No external process".into(),
                tools: vec![],
                instructions: None,
                bundled: None,
                available_tools: vec![],
            },
            ExtensionConfig::Platform {
                name: "missing-cold-restore-fixture".into(),
                description: "Expected load failure".into(),
                display_name: None,
                bundled: None,
                available_tools: vec![],
            },
        ])
        .to_extension_data(&mut extension_data)
        .unwrap();
        let config = if scenario == "normalization_change" {
            ModelConfig::new("gpt-4o")
        } else {
            normalized.clone()
        };
        manager
            .update(&session.id)
            .provider_name(if scenario == "fallback_change" {
                "missing-cold-provider-fixture"
            } else {
                "openai"
            })
            .model_config(config)
            .extension_data(extension_data.clone())
            .apply()
            .await
            .unwrap();
        let before = catalog.lookup_local_session(&session.id).await.unwrap();
        let command = WorkCommand {
            command_id: Uuid::new_v4(),
            work: WorkRef {
                owner_node_id: owner,
                kind: WorkKind::Chat,
                work_id: Uuid::parse_str(&before.work_id).unwrap(),
            },
            expected_store_generation: generation,
            expected_revision: before.revision.try_into().unwrap(),
            expected_run_id: None,
            action: ActionPayload::Send {
                content: vec![ControllerContent::Text {
                    text: "The original reviewed instruction".into(),
                }],
            },
        };
        let authorization = DispatchAuthorization::new(
            authority.clone(),
            "test-controller".into(),
            receiver.clone(),
            ControllerScope::WorkSend,
            device.clone(),
        );
        let entered = Arc::new(Notify::new());
        let resume = Arc::new(Notify::new());
        *authority.barrier.lock().await = Some((entered.clone(), resume.clone()));
        let accepted = registry
            .accept_command(&device, command.clone(), authorization)
            .await
            .unwrap();
        assert!(matches!(accepted.state, CommandState::Accepted));
        tokio::time::timeout(std::time::Duration::from_secs(15), entered.notified())
            .await
            .unwrap();
        let prepared = catalog.lookup_local_session(&session.id).await.unwrap();
        let saved = manager.get_session(&session.id, false).await.unwrap();
        assert_eq!(
            serde_json::to_value(&saved.extension_data).unwrap(),
            serde_json::to_value(&extension_data).unwrap(),
            "failed extension and original order survive {scenario}"
        );
        if matches!(scenario, "normalization_change" | "fallback_change") {
            assert!(prepared.revision > before.revision);
        } else {
            assert_eq!(
                prepared.revision, before.revision,
                "cold prepare itself must not change revision"
            );
        }
        if scenario == "external_change" {
            manager
                .update(&session.id)
                .user_provided_name("Concurrent owner edit")
                .apply()
                .await
                .unwrap();
        }
        resume.notify_one();
        eventually("cold dispatch settles", || async {
            matches!(
                registry
                    .receipt(&device, command.command_id)
                    .await
                    .unwrap()
                    .state,
                CommandState::Delivered | CommandState::Rejected
            )
        })
        .await;
        let receipt = registry.receipt(&device, command.command_id).await.unwrap();
        if scenario == "unchanged" {
            assert!(
                matches!(receipt.state, CommandState::Delivered),
                "{receipt:?}"
            );
            eventually("cold owner answer completes", || async {
                registry.current_run(&session.id).await.is_none()
            })
            .await;
            let saved = manager.get_session(&session.id, true).await.unwrap();
            let messages = saved.conversation.unwrap();
            assert_eq!(
                messages
                    .messages()
                    .iter()
                    .filter(|message| message.id == receipt.message_id)
                    .count(),
                1
            );
            assert!(messages
                .messages()
                .iter()
                .any(|message| message.as_concat_text() == "Cold phone connection works."));
        } else {
            assert!(
                matches!(receipt.state, CommandState::Rejected),
                "{receipt:?}"
            );
            assert!(receipt.reason.unwrap().starts_with("stale_revision:"));
            let saved = manager.get_session(&session.id, true).await.unwrap();
            assert!(saved.conversation.unwrap().messages().is_empty());
            if scenario == "external_change" {
                assert_eq!(saved.name, "Concurrent owner edit");
            }
        }
    }
    // Restoring a captured snapshot cannot overwrite an edit committed after that read.
    let directory = tempfile::tempdir().unwrap();
    let session = manager
        .create_session(
            directory.path().to_path_buf(),
            "snapshot race".into(),
            SessionType::User,
            GooseMode::Auto,
        )
        .await
        .unwrap();
    manager
        .update(&session.id)
        .provider_name("openai")
        .model_config(normalized.clone())
        .apply()
        .await
        .unwrap();
    let snapshot = manager.get_session(&session.id, false).await.unwrap();
    let changed_model = ModelConfig::new("external-owner-selected-model");
    manager
        .update(&session.id)
        .model_config(changed_model.clone())
        .apply()
        .await
        .unwrap();
    let edited = catalog.lookup_local_session(&session.id).await.unwrap();
    let runtime = goose::agents::Agent::with_config(goose::agents::AgentConfig::new(
        manager.clone(),
        goose::config::permission::PermissionManager::instance(),
        None,
        GooseMode::Auto,
        true,
        goose::agents::GoosePlatform::GooseDesktop,
    ));
    assert!(!runtime
        .restore_provider_from_session(&snapshot)
        .await
        .unwrap());
    let after = manager.get_session(&session.id, false).await.unwrap();
    assert_eq!(
        serde_json::to_value(after.model_config).unwrap(),
        serde_json::to_value(Some(changed_model)).unwrap()
    );
    assert_eq!(
        catalog
            .lookup_local_session(&session.id)
            .await
            .unwrap()
            .revision,
        edited.revision
    );
    assert_eq!(
        model.requests.lock().unwrap().len(),
        1,
        "only unchanged command may reach model fixture"
    );
    observer.close().await;
}
