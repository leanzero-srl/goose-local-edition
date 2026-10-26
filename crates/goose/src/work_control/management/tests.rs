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
