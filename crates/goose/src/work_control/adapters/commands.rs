use super::{agent_alive, error, read_json_optional, WorkAdapters};
use crate::{
    session::work_catalog::{CatalogRecord, WorkCatalog},
    work_control::{conflict, OwnerRunRegistry},
};
use leanzero_link::{
    controller::ControllerError,
    controller_auth::{AuthenticatedDevice, DispatchAuthorization},
    controller_wire::*,
};
use serde_json::{json, Value};
use std::{path::PathBuf, sync::Arc};
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

impl WorkAdapters {
    pub async fn accept(
        &self,
        catalog: &WorkCatalog,
        runs: &Arc<OwnerRunRegistry>,
        device: &AuthenticatedDevice,
        command: WorkCommand,
        auth: DispatchAuthorization,
    ) -> Result<CommandReceipt, ControllerError> {
        let _guard = self.control.lock().await;
        auth.recheck().await?;
        let canonical = serde_json::to_string(&command).map_err(error)?;
        let pool = runs.pool().await.map_err(error)?;
        let prior: Option<(String,String)> = sqlx::query_as("SELECT command_json,receipt_json FROM owner_commands WHERE account_id=? AND device_id=? AND command_id=?")
            .bind(device.account_id().to_string()).bind(device.device_id().to_string()).bind(command.command_id.to_string()).fetch_optional(pool).await.map_err(error)?;
        if let Some((previous, receipt)) = prior {
            if previous != canonical {
                return Err(conflict(
                    "command_id_conflict",
                    "command ID already has different contents",
                ));
            }
            return serde_json::from_str(&receipt).map_err(error);
        }
        self.refresh(catalog).await?;
        let record = catalog
            .lookup(
                &command.work.work_id.to_string(),
                &command.expected_store_generation.to_string(),
            )
            .await
            .map_err(error)?;
        if record.revision as u64 != command.expected_revision {
            return Err(conflict(
                "stale_revision",
                "work changed; refresh before issuing this command",
            ));
        }
        if record.current_run_id.as_deref()
            != command.expected_run_id.map(|id| id.to_string()).as_deref()
            || command.expected_run_id.is_none()
        {
            return Err(conflict(
                "stale_run",
                "command does not name the current owner run",
            ));
        }
        if !self
            .capabilities(&record)
            .contains(&command.action.action())
        {
            return Err(crate::work_control::unsupported(
                "action is unavailable for this work",
            ));
        }
        if let ActionPayload::Note { text } = &command.action {
            if text.trim().is_empty() {
                return Err(ControllerError::new(
                    axum::http::StatusCode::BAD_REQUEST,
                    "invalid_request",
                    "note is empty",
                    false,
                ));
            }
        }
        let mut receipt = CommandReceipt {
            command_id: command.command_id,
            work: command.work.clone(),
            authenticated_device_id: device.device_id(),
            revision: command.expected_revision,
            state: CommandState::Dispatching,
            run_id: command.expected_run_id,
            message_id: None,
            input_id: None,
            checkpoint_id: None,
            reason: None,
            updated_at: chrono::Utc::now(),
        };
        sqlx::query("INSERT INTO owner_commands(account_id,device_id,command_id,work_id,generation,session_id,command_json,receipt_json,state,run_id) VALUES (?,?,?,?,?,?,?,?,?,?)")
            .bind(device.account_id().to_string()).bind(device.device_id().to_string()).bind(command.command_id.to_string())
            .bind(command.work.work_id.to_string()).bind(command.expected_store_generation.to_string()).bind(&record.local_locator)
            .bind(canonical).bind(serde_json::to_string(&receipt).map_err(error)?).bind("dispatching").bind(record.current_run_id.as_deref())
            .execute(pool).await.map_err(error)?;
        // A lost reply after this point is reconciled, never replayed as a second side effect.
        if let Err(e) = auth.recheck().await {
            receipt.state = CommandState::Rejected;
            receipt.reason = Some(e.to_string());
            save_receipt(runs, device.account_id(), &receipt).await?;
            return Ok(receipt);
        }
        let dispatched = self
            .dispatch(catalog, &record, &command, device.device_id())
            .await;
        match dispatched {
            Ok((state, checkpoint, reason)) => {
                receipt.state = state;
                receipt.checkpoint_id = Some(checkpoint);
                receipt.reason = reason;
            }
            Err(e) => {
                receipt.state = CommandState::OutcomeUnknown;
                receipt.reason = Some(format!("{}: {}", e.body.code, e.body.message));
            }
        }
        save_receipt(runs, device.account_id(), &receipt).await?;
        Ok(receipt)
    }

    async fn dispatch(
        &self,
        catalog: &WorkCatalog,
        record: &CatalogRecord,
        command: &WorkCommand,
        device: Uuid,
    ) -> Result<(CommandState, String, Option<String>), ControllerError> {
        match record.kind.as_str() {
            "shell_job" => {
                let view = self
                    .jobs
                    .cancel(&record.local_locator)
                    .await
                    .map_err(|e| error(e.reason))?;
                let done = view.brief.state.is_terminal();
                Ok((
                    if done {
                        CommandState::Delivered
                    } else {
                        CommandState::Waiting
                    },
                    command.command_id.to_string(),
                    Some(
                        if done {
                            "shell_job_stopped"
                        } else {
                            "shell_job_cancelling"
                        }
                        .into(),
                    ),
                ))
            }
            "benchmark" => {
                let desktop = self
                    .desktop
                    .as_ref()
                    .ok_or_else(|| error("benchmark desktop owner is unavailable"))?;
                let run = record
                    .current_run_id
                    .as_deref()
                    .ok_or_else(|| error("benchmark has no current run"))?;
                let mut params: Value = serde_json::from_str(
                    &catalog
                        .external_run_key(&record.work_id, run)
                        .await
                        .map_err(error)?,
                )
                .map_err(error)?;
                params["command_id"] = json!(command.command_id);
                match &command.action {
                    ActionPayload::Note { text } => {
                        params["action"] = json!("note");
                        params["text"] = json!(text);
                    }
                    ActionPayload::Stop {} => params["action"] = json!("stop"),
                    _ => return Err(error("unsupported benchmark action")),
                }
                let result = desktop.call("benchmark.command", params).await?;
                let state = match result["state"].as_str() {
                    Some("delivered") => CommandState::Delivered,
                    Some("waiting") => CommandState::Waiting,
                    _ => return Err(error("desktop returned no delivery state")),
                };
                let checkpoint = result["checkpoint_id"]
                    .as_str()
                    .ok_or_else(|| error("desktop returned no checkpoint"))?
                    .to_owned();
                Ok((
                    state,
                    checkpoint,
                    result["reason"].as_str().map(str::to_owned),
                ))
            }
            "agent_work" => {
                let dir = PathBuf::from(&record.local_locator);
                let root = dir.join(".swarm/agent");
                let state = read_json_optional(&root.join("state.json"))
                    .await?
                    .ok_or_else(|| error("Agent Work state is missing"))?;
                let expected = catalog
                    .external_run_key(
                        &record.work_id,
                        record
                            .current_run_id
                            .as_deref()
                            .ok_or_else(|| error("Agent Work run is absent"))?,
                    )
                    .await
                    .map_err(error)?;
                if state["run_id"].as_str() != Some(expected.as_str()) || !agent_alive(&dir).await?
                {
                    return Err(conflict("stale_run", "Agent Work owner run changed"));
                }
                let checkpoint = command.command_id.to_string();
                match &command.action {
                    ActionPayload::Note { text } => {
                        let inbox = root.join("inbox");
                        tokio::fs::create_dir_all(&inbox).await.map_err(error)?;
                        let temporary = inbox.join(format!("{checkpoint}.tmp"));
                        let mut file = tokio::fs::OpenOptions::new()
                            .write(true)
                            .create_new(true)
                            .open(&temporary)
                            .await
                            .map_err(error)?;
                        file.write_all(serde_json::to_string(&json!({"text":text,"command_id":command.command_id,"device_id":device,"human_assisted":true})).map_err(error)?.as_bytes()).await.map_err(error)?;
                        file.sync_all().await.map_err(error)?;
                        tokio::fs::rename(temporary, inbox.join(format!("{checkpoint}.json")))
                            .await
                            .map_err(error)?;
                    }
                    ActionPayload::Resume {} => {
                        match tokio::fs::remove_file(root.join("paused")).await {
                            Ok(()) => {}
                            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                            Err(e) => return Err(error(e)),
                        }
                    }
                    action => {
                        let flag = match action {
                            ActionPayload::Stop {} => "stop",
                            ActionPayload::Pause {} => "paused",
                            ActionPayload::Tick {} => "tick-now",
                            _ => return Err(error("unsupported Agent Work action")),
                        };
                        tokio::fs::write(root.join(flag), checkpoint.as_bytes())
                            .await
                            .map_err(error)?;
                    }
                }
                Ok((
                    CommandState::Waiting,
                    checkpoint,
                    Some("waiting_for_agent_checkpoint".into()),
                ))
            }
            _ => Err(error("unknown work adapter")),
        }
    }

    pub async fn reconcile(
        &self,
        runs: &OwnerRunRegistry,
        catalog: &WorkCatalog,
        account: Uuid,
        mut receipt: CommandReceipt,
    ) -> Result<CommandReceipt, ControllerError> {
        if !matches!(receipt.state, CommandState::Waiting) {
            return Ok(receipt);
        }
        let record = catalog
            .lookup(
                &receipt.work.work_id.to_string(),
                &catalog.watermark().await.map_err(error)?.generation,
            )
            .await
            .map_err(error)?;
        let delivered = match record.kind.as_str() {
            "shell_job" => self
                .jobs
                .view(&record.local_locator)
                .is_some_and(|v| v.brief.state.is_terminal()),
            "agent_work" => {
                let root = PathBuf::from(&record.local_locator).join(".swarm/agent");
                let command_json:String=sqlx::query_scalar("SELECT command_json FROM owner_commands WHERE account_id=? AND device_id=? AND command_id=?")
                    .bind(account.to_string()).bind(receipt.authenticated_device_id.to_string()).bind(receipt.command_id.to_string())
                    .fetch_one(runs.pool().await.map_err(error)?).await.map_err(error)?;
                let command: WorkCommand = serde_json::from_str(&command_json).map_err(error)?;
                let run_key = catalog
                    .external_run_key(
                        &record.work_id,
                        &receipt
                            .run_id
                            .ok_or_else(|| error("Agent Work receipt has no run"))?
                            .to_string(),
                    )
                    .await
                    .map_err(error)?;
                let state = read_json_optional(&root.join("state.json")).await?;
                let same_run = state
                    .as_ref()
                    .is_some_and(|state| state["run_id"].as_str() == Some(run_key.as_str()));
                let status = state.as_ref().and_then(|state| state["status"].as_str());
                match command.action {
                    ActionPayload::Note { .. } => tokio::fs::try_exists(
                        root.join("inbox/consumed")
                            .join(format!("{}.json", receipt.command_id)),
                    )
                    .await
                    .map_err(error)?,
                    ActionPayload::Pause {} => same_run && status == Some("paused"),
                    ActionPayload::Resume {} => {
                        same_run
                            && matches!(status, Some("idle" | "waiting" | "ticking" | "holding"))
                    }
                    ActionPayload::Stop {} => same_run && status == Some("stopped"),
                    ActionPayload::Tick {} => {
                        same_run
                            && !tokio::fs::try_exists(root.join("tick-now"))
                                .await
                                .map_err(error)?
                    }
                    _ => false,
                }
            }
            _ => false,
        };
        if delivered {
            receipt.state = CommandState::Delivered;
            receipt.reason = Some("owner_checkpoint_observed".into());
            save_receipt(runs, account, &receipt).await?;
        }
        Ok(receipt)
    }
}

async fn save_receipt(
    runs: &OwnerRunRegistry,
    account: Uuid,
    receipt: &CommandReceipt,
) -> Result<(), ControllerError> {
    let state = serde_json::to_value(receipt.state).map_err(error)?;
    sqlx::query("UPDATE owner_commands SET state=?,receipt_json=? WHERE account_id=? AND device_id=? AND command_id=?")
        .bind(state.as_str().ok_or_else(||error("receipt state is invalid"))?).bind(serde_json::to_string(receipt).map_err(error)?)
        .bind(account.to_string()).bind(receipt.authenticated_device_id.to_string()).bind(receipt.command_id.to_string())
        .execute(runs.pool().await.map_err(error)?).await.map_err(error)?;
    Ok(())
}
