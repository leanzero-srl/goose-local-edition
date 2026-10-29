//! Management shares the owner's session database and the desktop's real service paths.
mod nodes;
mod providers;
mod receipts;
#[cfg(test)]
mod tests;

use super::OwnerRunRegistry;
use async_trait::async_trait;
use axum::http::StatusCode;
use leanzero_link::{
    controller::ControllerError,
    controller_auth::{AuthenticatedDevice, DispatchAuthorization},
    controller_management::{
        ManagedNodes, ManagedProviders, ManagementAction, ManagementBackend, ManagementCommand,
        ManagementReceipt,
    },
    controller_wire::ControllerScope,
    state::{MlxControl, MlxOp},
};
use serde_json::{json, Value};
use sqlx::{Pool, Sqlite};
use std::sync::{Arc, LazyLock};
use tokio::sync::Mutex;
use uuid::Uuid;

static OPERATIONS: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));
static RUNTIME: LazyLock<Uuid> = LazyLock::new(Uuid::new_v4);

#[derive(Clone)]
pub struct GooseManagement {
    pool: Pool<Sqlite>,
    owner: Uuid,
    account: Uuid,
    generation: Uuid,
    mlx: Arc<dyn MlxControl>,
    enabled: Arc<dyn Fn() -> bool + Send + Sync>,
}
impl GooseManagement {
    pub async fn new(
        runs: &OwnerRunRegistry,
        owner: Uuid,
        account: Uuid,
        mlx: Arc<dyn MlxControl>,
        enabled: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<Arc<Self>, ControllerError> {
        let pool = runs.pool().await.map_err(store_error)?.clone();
        let generation: String =
            sqlx::query_scalar("SELECT generation FROM controller_store WHERE singleton=1")
                .fetch_one(&pool)
                .await
                .map_err(store_error)?;
        let service = Arc::new(Self {
            pool,
            owner,
            account,
            generation: Uuid::parse_str(&generation).map_err(store_error)?,
            mlx,
            enabled,
        });
        service.ensure_table().await?;
        Ok(service)
    }
    fn authenticate(
        &self,
        device: &AuthenticatedDevice,
        scope: ControllerScope,
    ) -> Result<(), ControllerError> {
        device.check_fresh()?;
        if device.owner_node_id() != self.owner || device.account_id() != self.account {
            return Err(error(
                StatusCode::FORBIDDEN,
                "wrong_owner",
                "device belongs to another owner account",
            ));
        }
        if !device.scopes().contains(&scope)
            || (scope != ControllerScope::CatalogRead && !self.scopes().contains(&scope))
        {
            return Err(error(
                StatusCode::FORBIDDEN,
                "scope_denied",
                "host management operation is not enabled",
            ));
        }
        Ok(())
    }
    async fn fence(&self) -> Result<(), ControllerError> {
        let generation: String =
            sqlx::query_scalar("SELECT generation FROM controller_store WHERE singleton=1")
                .fetch_one(&self.pool)
                .await
                .map_err(store_error)?;
        if generation != self.generation.to_string() {
            return Err(error(
                StatusCode::CONFLICT,
                "store_generation_mismatch",
                "owner store changed; refresh host capabilities",
            ));
        }
        Ok(())
    }
    async fn mlx(&self, op: MlxOp, body: Value) -> Result<Value, ControllerError> {
        self.mlx.dispatch(op, body).await.map_err(|_| {
            error(
                StatusCode::CONFLICT,
                "model_operation_refused",
                "host could not perform this model operation; inspect its current download state",
            )
        })
    }
    async fn execute(
        &self,
        action: ManagementAction,
        authorization: &DispatchAuthorization,
    ) -> Result<Option<String>, ControllerError> {
        match action {
            action
            @ (ManagementAction::NodeCreate { .. } | ManagementAction::NodeRemove { .. }) => {
                self.execute_node(action, authorization).await
            }
            ManagementAction::Download { repo_id } => self
                .mlx(MlxOp::Download, json!({"repoId":repo_id}))
                .await
                .map(|_| None),
            ManagementAction::DownloadCancel { repo_id } => self
                .mlx(MlxOp::DownloadCancel, json!({"repoId":repo_id}))
                .await
                .map(|_| None),
            ManagementAction::DownloadPause { repo_id } => self
                .mlx(MlxOp::DownloadPause, json!({"repoId":repo_id}))
                .await
                .map(|_| None),
            ManagementAction::DownloadResume { repo_id } => self
                .mlx(MlxOp::DownloadResume, json!({"repoId":repo_id}))
                .await
                .map(|_| None),
            other => providers::execute(other).await,
        }
    }
}
#[async_trait]
impl ManagementBackend for GooseManagement {
    fn scopes(&self) -> Vec<ControllerScope> {
        let mut scopes = vec![ControllerScope::ModelsRead, ControllerScope::ProvidersRead];
        if (self.enabled)() {
            scopes.extend([
                ControllerScope::ModelsDownload,
                ControllerScope::ModelsManage,
                ControllerScope::ProvidersManage,
            ]);
        }
        scopes
    }
    async fn nodes(&self, device: &AuthenticatedDevice) -> Result<ManagedNodes, ControllerError> {
        self.authenticate(device, ControllerScope::ModelsRead)?;
        self.fence().await?;
        let mut inventory = self.node_inventory().await?;
        if !device.scopes().contains(&ControllerScope::ModelsManage) {
            inventory.capabilities.clear();
        }
        Ok(inventory)
    }
    async fn models(&self, device: &AuthenticatedDevice) -> Result<Value, ControllerError> {
        self.authenticate(device, ControllerScope::ModelsRead)?;
        self.fence().await?;
        self.mlx(MlxOp::ModelsList, json!({})).await
    }
    async fn download(
        &self,
        device: &AuthenticatedDevice,
        repo_id: &str,
    ) -> Result<Value, ControllerError> {
        self.authenticate(device, ControllerScope::ModelsRead)?;
        self.fence().await?;
        validate_repo(repo_id)?;
        let result = self
            .mlx(MlxOp::DownloadProgress, json!({"repoId":repo_id}))
            .await?;
        if result.get("progress").is_none_or(Value::is_null) {
            return Err(error(
                StatusCode::NOT_FOUND,
                "download_missing",
                "no download is tracked on this host; it may have restarted",
            ));
        }
        Ok(result)
    }
    async fn providers(
        &self,
        device: &AuthenticatedDevice,
    ) -> Result<ManagedProviders, ControllerError> {
        self.authenticate(device, ControllerScope::ProvidersRead)?;
        self.fence().await?;
        providers::list().await
    }
    async fn accept(
        &self,
        device: &AuthenticatedDevice,
        command: ManagementCommand,
        authorization: DispatchAuthorization,
    ) -> Result<ManagementReceipt, ControllerError> {
        self.accept_inner(device, command, authorization).await
    }
    async fn receipt(
        &self,
        device: &AuthenticatedDevice,
        id: Uuid,
    ) -> Result<ManagementReceipt, ControllerError> {
        self.read_receipt(device, id).await
    }
}
fn validate_repo(repo: &str) -> Result<(), ControllerError> {
    goose_sidecar::hf::validate_model_id(repo).map_err(|_| {
        error(
            StatusCode::BAD_REQUEST,
            "invalid_repository",
            "expected a Hugging Face owner/repository identifier",
        )
    })
}
fn error(status: StatusCode, code: &str, message: &str) -> ControllerError {
    ControllerError::new(status, code, message, false)
}
fn store_error(_: impl std::fmt::Display) -> ControllerError {
    ControllerError::unavailable("management receipt store is unavailable")
}
