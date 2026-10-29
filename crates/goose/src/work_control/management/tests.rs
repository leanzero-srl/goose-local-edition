use super::*;
use chrono::Utc;
use leanzero_link::controller_auth::{
    authorize, AuthorizationError, ControllerAuthority, ReceiverBinding,
};
use leanzero_link::controller_wire::{AuthorizationLease, CommandState, DeviceRole};
use leanzero_link::state::MlxControlError;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

struct Authority {
    lease: AuthorizationLease,
    active: AtomicBool,
}
#[async_trait]
impl ControllerAuthority for Authority {
    async fn introspect(
        &self,
        _: &str,
        _: &ReceiverBinding,
        _: ControllerScope,
    ) -> Result<Option<AuthorizationLease>, AuthorizationError> {
        let mut lease = self.lease.clone();
        lease.active = self.active.load(Ordering::SeqCst);
        Ok(Some(lease))
    }
}
struct DownloadProbe {
    pool: Pool<Sqlite>,
    calls: AtomicUsize,
    models_unavailable: AtomicBool,
    models_gate: std::sync::Mutex<Option<(Arc<tokio::sync::Notify>, Arc<tokio::sync::Notify>)>>,
}
#[async_trait]
impl MlxControl for DownloadProbe {
    async fn dispatch(&self, op: MlxOp, request: Value) -> Result<Value, MlxControlError> {
        match op {
            MlxOp::Download => {
                let stored: String = sqlx::query_scalar(
                    "SELECT receipt_json FROM controller_management_commands LIMIT 1",
                )
                .fetch_one(&self.pool)
                .await
                .unwrap();
                assert!(
                    stored.contains("dispatching"),
                    "mutation ran before durable dispatch receipt"
                );
                assert_eq!(request, json!({"repoId":"fixture/model"}));
                self.calls.fetch_add(1, Ordering::SeqCst);
                Ok(json!({}))
            }
            MlxOp::ModelsList => {
                if self.models_unavailable.load(Ordering::SeqCst) {
                    return Err(MlxControlError::Failed(
                        "private-storage-path-do-not-expose".into(),
                    ));
                }
                let gate = self.models_gate.lock().unwrap().clone();
                if let Some((started, release)) = gate {
                    started.notify_one();
                    release.notified().await;
                }
                Ok(
                    json!({"models":[{"id":"fixture-installed","complete":true},{"id":"fixture-partial","complete":false}]}),
                )
            }
            MlxOp::DownloadProgress => Ok(
                json!({"progress":{"state":"downloading","totalBytes":100,"downloadedBytes":40}}),
            ),
            _ => panic!("unexpected op"),
        }
    }
}
struct Fixture {
    service: GooseManagement,
    device: AuthenticatedDevice,
    authority: Arc<Authority>,
    binding: ReceiverBinding,
    probe: Arc<DownloadProbe>,
    enabled: Arc<AtomicBool>,
}
impl Fixture {
    async fn new() -> Self {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        let owner = Uuid::new_v4();
        let account = Uuid::new_v4();
        let generation = Uuid::new_v4();
        sqlx::query(
            "CREATE TABLE controller_store(singleton INTEGER PRIMARY KEY,generation TEXT NOT NULL)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO controller_store VALUES (1,?)")
            .bind(generation.to_string())
            .execute(&pool)
            .await
            .unwrap();
        let binding = ReceiverBinding {
            owner_node_id: owner,
            runtime_id: Uuid::new_v4(),
            store_generation: generation,
            host_lease_id: Uuid::new_v4(),
            host_revision: 1,
        };
        let authority = Arc::new(Authority {
            active: AtomicBool::new(true),
            lease: AuthorizationLease {
                active: true,
                account_id: account,
                device_id: Uuid::new_v4(),
                device_revision: 1,
                role: DeviceRole::Controller,
                owner_node_id: owner,
                scopes: vec![
                    ControllerScope::CatalogRead,
                    ControllerScope::ModelsRead,
                    ControllerScope::ModelsDownload,
                    ControllerScope::ModelsManage,
                    ControllerScope::ProvidersManage,
                    ControllerScope::ProvidersRead,
                ],
                token_expires_at: Utc::now() + chrono::Duration::minutes(10),
                lease_seconds: 30,
                host_revision: 1,
                store_generation: generation,
                host_lease_id: binding.host_lease_id,
            },
        });
        let device = authorize(
            authority.as_ref(),
            "fixture",
            &binding,
            ControllerScope::ModelsDownload,
        )
        .await
        .unwrap();
        let probe = Arc::new(DownloadProbe {
            pool: pool.clone(),
            calls: AtomicUsize::new(0),
            models_gate: std::sync::Mutex::new(None),
            models_unavailable: AtomicBool::new(false),
        });
        let enabled = Arc::new(AtomicBool::new(true));
        let flag = enabled.clone();
        let service = GooseManagement {
            pool,
            owner,
            account,
            generation,
            mlx: probe.clone(),
            enabled: Arc::new(move || flag.load(Ordering::SeqCst)),
        };
        service.ensure_table().await.unwrap();
        Self {
            service,
            device,
            authority,
            binding,
            probe,
            enabled,
        }
    }
    fn command(&self) -> ManagementCommand {
        ManagementCommand {
            command_id: Uuid::new_v4(),
            owner_node_id: self.service.owner,
            expected_store_generation: self.service.generation,
            action: ManagementAction::Download {
                repo_id: "fixture/model".into(),
            },
        }
    }
    fn auth(&self, scope: ControllerScope) -> DispatchAuthorization {
        DispatchAuthorization::new(
            self.authority.clone(),
            "fixture".into(),
            self.binding.clone(),
            scope,
            self.device.clone(),
        )
    }
    async fn settled(&self, id: Uuid) -> ManagementReceipt {
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                let receipt = self.service.receipt(&self.device, id).await.unwrap();
                if !matches!(
                    receipt.state,
                    CommandState::Accepted | CommandState::Dispatching
                ) {
                    return receipt;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap()
    }
}
#[tokio::test]
async fn management_records_before_dispatch_and_retry_never_duplicates() {
    let fixture = Fixture::new().await;
    let command = fixture.command();
    fixture
        .service
        .accept(
            &fixture.device,
            command.clone(),
            fixture.auth(ControllerScope::ModelsDownload),
        )
        .await
        .unwrap();
    let receipt = fixture.settled(command.command_id).await;
    assert!(matches!(receipt.state, CommandState::Delivered));
    fixture
        .service
        .accept(
            &fixture.device,
            command.clone(),
            fixture.auth(ControllerScope::ModelsDownload),
        )
        .await
        .unwrap();
    assert_eq!(fixture.probe.calls.load(Ordering::SeqCst), 1);
    let mut conflict = command;
    conflict.action = ManagementAction::Download {
        repo_id: "other/model".into(),
    };
    assert_eq!(
        fixture
            .service
            .accept(
                &fixture.device,
                conflict,
                fixture.auth(ControllerScope::ModelsDownload)
            )
            .await
            .unwrap_err()
            .body
            .code,
        "command_id_conflict"
    );
    assert_eq!(
        fixture
            .service
            .download(&fixture.device, "fixture/model")
            .await
            .unwrap()["progress"]["downloadedBytes"],
        40
    );
}
#[tokio::test]
async fn management_refuses_wrong_owner_generation_policy_and_revoked_dispatch() {
    let fixture = Fixture::new().await;
    let mut command = fixture.command();
    command.owner_node_id = Uuid::new_v4();
    assert_eq!(
        fixture
            .service
            .accept(
                &fixture.device,
                command,
                fixture.auth(ControllerScope::ModelsDownload)
            )
            .await
            .unwrap_err()
            .body
            .code,
        "wrong_owner"
    );
    let mut command = fixture.command();
    command.expected_store_generation = Uuid::new_v4();
    assert_eq!(
        fixture
            .service
            .accept(
                &fixture.device,
                command,
                fixture.auth(ControllerScope::ModelsDownload)
            )
            .await
            .unwrap_err()
            .body
            .code,
        "store_generation_mismatch"
    );
    fixture.enabled.store(false, Ordering::SeqCst);
    assert_eq!(
        fixture
            .service
            .accept(
                &fixture.device,
                fixture.command(),
                fixture.auth(ControllerScope::ModelsDownload)
            )
            .await
            .unwrap_err()
            .body
            .code,
        "scope_denied"
    );
    fixture.enabled.store(true, Ordering::SeqCst);
    fixture.authority.active.store(false, Ordering::SeqCst);
    let command = fixture.command();
    fixture
        .service
        .accept(
            &fixture.device,
            command.clone(),
            fixture.auth(ControllerScope::ModelsDownload),
        )
        .await
        .unwrap();
    assert!(matches!(
        fixture.settled(command.command_id).await.state,
        CommandState::Rejected
    ));
    assert_eq!(fixture.probe.calls.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn management_credentials_never_enter_receipt_store_and_restart_is_unknown() {
    let fixture = Fixture::new().await;
    let mut command = fixture.command();
    command.action = ManagementAction::ProviderConfigure {
        provider_id: "missing-fixture-provider".into(),
        fields: [("API_KEY".into(), "credential-must-never-be-stored".into())].into(),
        test_model: None,
    };
    fixture
        .service
        .accept(
            &fixture.device,
            command.clone(),
            fixture.auth(ControllerScope::ProvidersManage),
        )
        .await
        .unwrap();
    let receipt = fixture.settled(command.command_id).await;
    assert!(matches!(receipt.state, CommandState::Rejected));
    let row: (String, String) =
        sqlx::query_as("SELECT digest,receipt_json FROM controller_management_commands")
            .fetch_one(&fixture.service.pool)
            .await
            .unwrap();
    assert!(!row.0.contains("credential-must"));
    assert!(!row.1.contains("credential-must"));
    let mut accepted = receipt;
    accepted.state = CommandState::Dispatching;
    sqlx::query("UPDATE controller_management_commands SET runtime_id=?,receipt_json=?")
        .bind(Uuid::new_v4().to_string())
        .bind(serde_json::to_string(&accepted).unwrap())
        .execute(&fixture.service.pool)
        .await
        .unwrap();
    assert!(matches!(
        fixture
            .service
            .receipt(&fixture.device, command.command_id)
            .await
            .unwrap()
            .state,
        CommandState::OutcomeUnknown
    ));
    assert!(matches!(
        fixture
            .service
            .accept(
                &fixture.device,
                command,
                fixture.auth(ControllerScope::ProvidersManage)
            )
            .await
            .unwrap()
            .state,
        CommandState::OutcomeUnknown
    ));
}
#[tokio::test]
async fn management_real_download_executor_absence_is_explicit() {
    let mut fixture = Fixture::new().await;
    fixture.service.mlx = Arc::new(crate::acp::server::GoosedMlxControl::new());
    assert_eq!(
        fixture
            .service
            .download(&fixture.device, "fixture/not-downloaded-controller-test")
            .await
            .unwrap_err()
            .body
            .code,
        "download_missing"
    );
    assert_eq!(
        fixture
            .service
            .download(&fixture.device, "../escape")
            .await
            .unwrap_err()
            .body
            .code,
        "invalid_repository"
    );
}

#[test]
fn management_real_secret_store_exposes_presence_without_secret_fragments() {
    let root = tempfile::tempdir().unwrap();
    let config = crate::config::Config::new_with_file_secrets(
        root.path().join("config.yaml"),
        root.path().join("secrets.yaml"),
    )
    .unwrap();
    config
        .set_secret(
            "CONTROLLER_FIXTURE_TOKEN",
            &"secret-prefix-do-not-leak-suffix",
        )
        .unwrap();
    config
        .set_param("CONTROLLER_FIXTURE_ENDPOINT", "https://example.invalid/v1")
        .unwrap();
    let key = |name: &str, secret| crate::providers::base::ConfigKey {
        name: name.into(),
        required: true,
        secret,
        default: None,
        oauth_flow: false,
        device_code_flow: false,
        primary: false,
    };
    let secret = providers::field(&config, key("CONTROLLER_FIXTURE_TOKEN", true)).unwrap();
    assert!(secret.is_set);
    assert_eq!(secret.value, None);
    assert!(!serde_json::to_string(&secret)
        .unwrap()
        .contains("secret-prefix"));
    let endpoint = providers::field(&config, key("CONTROLLER_FIXTURE_ENDPOINT", false)).unwrap();
    assert_eq!(
        endpoint.value.as_deref(),
        Some("https://example.invalid/v1")
    );
    assert!(
        !providers::field(&config, key("CONTROLLER_FIXTURE_MISSING", true))
            .unwrap()
            .is_set
    );
}

#[tokio::test]
async fn management_real_custom_provider_uses_existing_store_and_registry() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().to_string_lossy().to_string();
    let _env = env_lock::lock_env([("GOOSE_PATH_ROOT", Some(path.as_str()))]);
    let id = providers::execute(ManagementAction::ProviderCreate {
        engine: "openai".into(),
        display_name: "Controller fixture".into(),
        api_url: "https://example.invalid/v1".into(),
        api_key: None,
        models: vec!["fixture-model".into()],
        requires_auth: false,
    })
    .await
    .unwrap()
    .unwrap();
    let loaded = crate::config::declarative_providers::load_provider(&id).unwrap();
    assert_eq!(loaded.config.models[0].name, "fixture-model");
    assert_eq!(loaded.config.base_url, "https://example.invalid/v1");
    assert!(crate::providers::get_from_registry(&id).await.is_ok());
    crate::config::declarative_providers::remove_custom_provider(&id).unwrap();
    crate::providers::refresh_custom_providers().await.unwrap();
}

#[tokio::test]
async fn management_node_create_read_remove_is_owner_scoped_and_idempotent() {
    use leanzero_link::controller_management::ManagedNodeKind;
    let root = tempfile::tempdir().unwrap();
    let path = root.path().to_string_lossy().to_string();
    let _env = env_lock::lock_env([("GOOSE_PATH_ROOT", Some(path.as_str()))]);
    let provider = providers::execute(ManagementAction::ProviderCreate {
        engine: "openai".into(),
        display_name: "Node management fixture".into(),
        api_url: "https://node-fixture.invalid".into(),
        api_key: None,
        models: vec!["fixture-model".into()],
        requires_auth: false,
    })
    .await
    .unwrap()
    .unwrap();
    let fixture = Fixture::new().await;
    let before = crate::nodes::read(crate::config::Config::global(), Ok("Fixture".into()))
        .unwrap()
        .config;
    let mut command = fixture.command();
    command.action = ManagementAction::NodeCreate {
        id: "remote-endpoint-proof".into(),
        name: "Remote endpoint proof".into(),
        kind: ManagedNodeKind::Endpoint,
        model: "fixture-model".into(),
        provider: Some(provider.clone()),
    };
    fixture
        .service
        .accept(
            &fixture.device,
            command.clone(),
            fixture.auth(ControllerScope::ModelsManage),
        )
        .await
        .unwrap();
    assert!(matches!(
        fixture.settled(command.command_id).await.state,
        CommandState::Delivered
    ));
    let repeated = fixture
        .service
        .accept(
            &fixture.device,
            command.clone(),
            fixture.auth(ControllerScope::ModelsManage),
        )
        .await
        .unwrap();
    assert!(matches!(repeated.state, CommandState::Delivered));
    let inventory = fixture.service.nodes(&fixture.device).await.unwrap();
    assert_eq!(inventory.local_models, vec!["fixture-installed"]);
    let node = inventory
        .nodes
        .iter()
        .find(|n| n.id == "remote-endpoint-proof")
        .unwrap();
    assert!(node.editable);
    assert_eq!(node.model.as_deref(), Some("fixture-model"));
    let after = crate::nodes::read(crate::config::Config::global(), Ok("Fixture".into()))
        .unwrap()
        .config;
    assert_eq!(after.defs.len(), before.defs.len() + 1);
    assert_eq!(after.for_new_chats, before.for_new_chats);
    assert_eq!(after.for_builds, before.for_builds);
    assert_eq!(after.strategies, before.strategies);
    assert_eq!(fixture.probe.calls.load(Ordering::SeqCst), 0);
    let mut remove = fixture.command();
    remove.action = ManagementAction::NodeRemove {
        id: node.id.clone(),
        expected_definition_revision: "old".into(),
    };
    fixture
        .service
        .accept(
            &fixture.device,
            remove.clone(),
            fixture.auth(ControllerScope::ModelsManage),
        )
        .await
        .unwrap();
    assert!(matches!(
        fixture.settled(remove.command_id).await.state,
        CommandState::Rejected
    ));
    fixture
        .probe
        .models_unavailable
        .store(true, Ordering::SeqCst);
    let unavailable = fixture.service.nodes(&fixture.device).await.unwrap();
    assert!(unavailable.local_models.is_empty() && unavailable.local_models_error.is_some());
    assert!(!unavailable.supported_kinds.contains(&ManagedNodeKind::Mlx));
    assert!(unavailable.nodes.iter().any(|n| n.id == node.id));
    assert!(!serde_json::to_string(&unavailable)
        .unwrap()
        .contains("private-storage-path"));
    remove.command_id = Uuid::new_v4();
    remove.action = ManagementAction::NodeRemove {
        id: node.id.clone(),
        expected_definition_revision: node.definition_revision.clone(),
    };
    fixture
        .service
        .accept(
            &fixture.device,
            remove.clone(),
            fixture.auth(ControllerScope::ModelsManage),
        )
        .await
        .unwrap();
    assert!(matches!(
        fixture.settled(remove.command_id).await.state,
        CommandState::Delivered
    ));
    assert!(!fixture
        .service
        .nodes(&fixture.device)
        .await
        .unwrap()
        .nodes
        .iter()
        .any(|n| n.id == node.id));
    fixture.enabled.store(false, Ordering::SeqCst);
    assert!(fixture
        .service
        .accept(
            &fixture.device,
            command,
            fixture.auth(ControllerScope::ModelsManage)
        )
        .await
        .is_err());
    assert!(fixture
        .service
        .nodes(&fixture.device)
        .await
        .unwrap()
        .capabilities
        .is_empty());
    crate::config::declarative_providers::remove_custom_provider(&provider).unwrap();
    crate::providers::refresh_custom_providers().await.unwrap();
}

#[tokio::test]
async fn management_node_revocation_and_host_toggle_during_model_preparation_prevent_write() {
    use leanzero_link::controller_management::ManagedNodeKind;
    let root = tempfile::tempdir().unwrap();
    let path = root.path().to_string_lossy().to_string();
    let _env = env_lock::lock_env([("GOOSE_PATH_ROOT", Some(path.as_str()))]);
    for revoked in [false, true] {
        let fixture = Fixture::new().await;
        let started = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        *fixture.probe.models_gate.lock().unwrap() = Some((started.clone(), release.clone()));
        let mut command = fixture.command();
        command.action = ManagementAction::NodeCreate {
            id: "denied-after-preparation".into(),
            name: "Must not exist".into(),
            kind: ManagedNodeKind::Mlx,
            model: "fixture-installed".into(),
            provider: None,
        };
        fixture
            .service
            .accept(
                &fixture.device,
                command.clone(),
                fixture.auth(ControllerScope::ModelsManage),
            )
            .await
            .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), started.notified())
            .await
            .unwrap();
        if revoked {
            fixture.authority.active.store(false, Ordering::SeqCst);
        } else {
            fixture.enabled.store(false, Ordering::SeqCst);
        }
        release.notify_one();
        assert!(matches!(
            fixture.settled(command.command_id).await.state,
            CommandState::Rejected
        ));
        assert!(
            !crate::nodes::read(crate::config::Config::global(), Ok("Fixture".into()))
                .unwrap()
                .config
                .defs
                .iter()
                .any(|d| d.id == "denied-after-preparation")
        );
    }
}

#[tokio::test]
async fn management_node_inventory_does_not_advertise_writes_to_read_only_controller() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().to_string_lossy().to_string();
    let _env = env_lock::lock_env([("GOOSE_PATH_ROOT", Some(path.as_str()))]);
    let mut fixture = Fixture::new().await;
    Arc::get_mut(&mut fixture.authority)
        .unwrap()
        .lease
        .scopes
        .retain(|s| *s != ControllerScope::ModelsManage);
    let device = authorize(
        fixture.authority.as_ref(),
        "fixture",
        &fixture.binding,
        ControllerScope::ModelsRead,
    )
    .await
    .unwrap();
    assert!(fixture
        .service
        .nodes(&device)
        .await
        .unwrap()
        .capabilities
        .is_empty());
}
