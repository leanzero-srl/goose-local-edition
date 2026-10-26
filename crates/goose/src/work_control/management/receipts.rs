use super::*;
use chrono::Utc;
use leanzero_link::controller_wire::CommandState;
use sqlx::Row;

impl GooseManagement {
    pub(super) async fn ensure_table(&self) -> Result<(), ControllerError> {
        sqlx::query("CREATE TABLE IF NOT EXISTS controller_management_commands (account_id TEXT NOT NULL, device_id TEXT NOT NULL, command_id TEXT NOT NULL, owner_id TEXT NOT NULL, generation TEXT NOT NULL, digest TEXT NOT NULL, runtime_id TEXT NOT NULL, receipt_json TEXT NOT NULL, PRIMARY KEY(account_id,device_id,command_id))")
            .execute(&self.pool).await.map_err(store_error)?;
        Ok(())
    }
    pub(super) async fn accept_inner(
        &self,
        device: &AuthenticatedDevice,
        command: ManagementCommand,
        authorization: DispatchAuthorization,
    ) -> Result<ManagementReceipt, ControllerError> {
        self.authenticate(device, command.action.required_scope())?;
        if command.owner_node_id != self.owner {
            return Err(error(
                StatusCode::FORBIDDEN,
                "wrong_owner",
                "management command targets another host",
            ));
        }
        if command.expected_store_generation != self.generation {
            return Err(error(
                StatusCode::CONFLICT,
                "store_generation_mismatch",
                "owner store changed; refresh host capabilities",
            ));
        }
        if command.command_id.is_nil() || command.action.target().trim().is_empty() {
            return Err(error(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "command identity and target are required",
            ));
        }
        if let ManagementAction::Download { repo_id }
        | ManagementAction::DownloadCancel { repo_id }
        | ManagementAction::DownloadPause { repo_id }
        | ManagementAction::DownloadResume { repo_id } = &command.action
        {
            validate_repo(repo_id)?;
        }
        self.fence().await?;
        // Never retain credentials: canonical typed command bytes exist only in memory.
        let digest = blake3::hash(&serde_json::to_vec(&command).map_err(store_error)?)
            .to_hex()
            .to_string();
        let mut tx = self
            .pool
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(store_error)?;
        let generation: String =
            sqlx::query_scalar("SELECT generation FROM controller_store WHERE singleton=1")
                .fetch_one(&mut *tx)
                .await
                .map_err(store_error)?;
        if generation != self.generation.to_string() {
            return Err(error(
                StatusCode::CONFLICT,
                "store_generation_mismatch",
                "owner store changed; refresh host capabilities",
            ));
        }
        let prior = sqlx::query("SELECT digest,runtime_id,receipt_json FROM controller_management_commands WHERE account_id=? AND device_id=? AND command_id=?")
            .bind(self.account.to_string()).bind(device.device_id().to_string()).bind(command.command_id.to_string())
            .fetch_optional(&mut *tx).await.map_err(store_error)?;
        if let Some(prior) = prior {
            if prior.get::<String, _>("digest") != digest {
                return Err(error(
                    StatusCode::CONFLICT,
                    "command_id_conflict",
                    "command identity was already used for different contents",
                ));
            }
            let receipt = recovered(prior.get("receipt_json"), prior.get("runtime_id"))?;
            tx.commit().await.map_err(store_error)?;
            return Ok(receipt);
        }
        let receipt = ManagementReceipt {
            command_id: command.command_id,
            owner_node_id: self.owner,
            authenticated_device_id: device.device_id(),
            action: command.action.name().into(),
            state: CommandState::Accepted,
            target: command.action.target().into(),
            reason: None,
            updated_at: Utc::now(),
        };
        sqlx::query("INSERT INTO controller_management_commands(account_id,device_id,command_id,owner_id,generation,digest,runtime_id,receipt_json) VALUES (?,?,?,?,?,?,?,?)")
            .bind(self.account.to_string()).bind(device.device_id().to_string()).bind(command.command_id.to_string())
            .bind(self.owner.to_string()).bind(self.generation.to_string()).bind(digest).bind(RUNTIME.to_string())
            .bind(serde_json::to_string(&receipt).map_err(store_error)?).execute(&mut *tx).await.map_err(store_error)?;
        tx.commit().await.map_err(store_error)?;
        let service = self.clone();
        let accepted = receipt.clone();
        tokio::spawn(async move {
            // Disconnecting an observer cannot drop an accepted management operation.
            let _lock = OPERATIONS.lock().await;
            let mut receipt = accepted;
            let guard = async {
                service.fence().await?;
                let device = authorization.recheck().await?;
                service.authenticate(&device, command.action.required_scope())?;
                service.fence().await?;
                Ok::<(), ControllerError>(())
            }
            .await;
            if let Err(error) = guard {
                receipt.state = CommandState::Rejected;
                receipt.reason = Some(error.body.code);
            } else {
                receipt.state = CommandState::Dispatching;
                if service.save_receipt(&receipt).await.is_err() {
                    return;
                }
                match service.execute(command.action).await {
                    Ok(target) => {
                        receipt.state = CommandState::Delivered;
                        if let Some(target) = target {
                            receipt.target = target;
                        }
                    }
                    Err(error) => {
                        receipt.state = CommandState::Rejected;
                        receipt.reason = Some(error.body.code);
                    }
                }
            }
            if service.save_receipt(&receipt).await.is_err() {
                tracing::error!("management receipt completion could not be persisted");
            }
        });
        Ok(receipt)
    }
    async fn save_receipt(&self, receipt: &ManagementReceipt) -> Result<(), ControllerError> {
        let mut receipt = receipt.clone();
        receipt.updated_at = Utc::now();
        sqlx::query("UPDATE controller_management_commands SET receipt_json=? WHERE account_id=? AND device_id=? AND command_id=?")
            .bind(serde_json::to_string(&receipt).map_err(store_error)?).bind(self.account.to_string())
            .bind(receipt.authenticated_device_id.to_string()).bind(receipt.command_id.to_string())
            .execute(&self.pool).await.map_err(store_error)?;
        Ok(())
    }
    pub(super) async fn read_receipt(
        &self,
        device: &AuthenticatedDevice,
        id: Uuid,
    ) -> Result<ManagementReceipt, ControllerError> {
        self.authenticate(device, ControllerScope::CatalogRead)?;
        self.fence().await?;
        let row=sqlx::query("SELECT receipt_json,runtime_id FROM controller_management_commands WHERE account_id=? AND device_id=? AND command_id=? AND owner_id=? AND generation=?")
            .bind(self.account.to_string()).bind(device.device_id().to_string()).bind(id.to_string())
            .bind(self.owner.to_string()).bind(self.generation.to_string()).fetch_optional(&self.pool).await.map_err(store_error)?
            .ok_or_else(|| error(StatusCode::NOT_FOUND, "command_missing", "management command was not found for this device"))?;
        recovered(row.get("receipt_json"), row.get("runtime_id"))
    }
}
fn recovered(json: &str, runtime: &str) -> Result<ManagementReceipt, ControllerError> {
    let mut receipt: ManagementReceipt = serde_json::from_str(json).map_err(store_error)?;
    if runtime != RUNTIME.to_string()
        && matches!(
            receipt.state,
            CommandState::Accepted | CommandState::Dispatching
        )
    {
        receipt.state = CommandState::OutcomeUnknown;
        receipt.reason = Some("owner_restarted_without_delivery_proof".into());
    }
    Ok(receipt)
}
