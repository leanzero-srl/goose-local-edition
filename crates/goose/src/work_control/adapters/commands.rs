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
use sha2::{Digest, Sha256};
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
                params["device_id"] = json!(device);
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
                queue_agent_command(&root, &expected, command, device).await?;
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
            "benchmark" => {
                self.benchmark_note_delivered(catalog, &record, &receipt)
                    .await?
            }
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
                agent_command_consumed(&root, &run_key, &command, receipt.authenticated_device_id)
                    .await?
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

async fn queue_agent_command(
    root: &std::path::Path,
    run_id: &str,
    command: &WorkCommand,
    device: Uuid,
) -> Result<(), ControllerError> {
    let mut envelope = json!({
        "run_id": run_id, "command_id": command.command_id, "device_id": device,
        "action": command.action.action(), "queued_at": chrono::Utc::now(), "human_assisted": true,
    });
    let dir = match &command.action {
        ActionPayload::Note { text } => {
            envelope["text"] = json!(text);
            root.join("inbox")
        }
        ActionPayload::Pause {}
        | ActionPayload::Resume {}
        | ActionPayload::Stop {}
        | ActionPayload::Tick {} => root
            .join("control")
            .join(agent_run_directory_key(run_id))
            .join("requests"),
        _ => return Err(error("unsupported Agent Work action")),
    };
    tokio::fs::create_dir_all(&dir).await.map_err(error)?;
    let key = format!("{device}_{}", command.command_id);
    let temporary = dir.join(format!("{key}.tmp"));
    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .await
        .map_err(error)?;
    file.write_all(&serde_json::to_vec(&envelope).map_err(error)?)
        .await
        .map_err(error)?;
    file.sync_all().await.map_err(error)?;
    tokio::fs::rename(temporary, dir.join(format!("{key}.json")))
        .await
        .map_err(error)?;
    Ok(())
}

fn agent_run_directory_key(run_id: &str) -> String {
    Sha256::digest(run_id.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

async fn agent_command_consumed(
    root: &std::path::Path,
    run_id: &str,
    command: &WorkCommand,
    device: Uuid,
) -> Result<bool, ControllerError> {
    let ack = read_json_optional(
        &root
            .join("control/acks")
            .join(format!("{device}_{}.json", command.command_id)),
    )
    .await?;
    Ok(ack.is_some_and(|ack| {
        ack["run_id"].as_str() == Some(run_id)
            && ack["device_id"].as_str() == Some(device.to_string().as_str())
            && ack["command_id"].as_str() == Some(command.command_id.to_string().as_str())
            && ack["action"] == json!(command.action.action())
            && ack["state"].as_str() == Some("consumed")
    }))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn command(action: ActionPayload) -> WorkCommand {
        WorkCommand {
            command_id: Uuid::new_v4(),
            work: WorkRef {
                owner_node_id: Uuid::new_v4(),
                kind: WorkKind::AgentWork,
                work_id: Uuid::new_v4(),
            },
            expected_store_generation: Uuid::new_v4(),
            expected_revision: 1,
            expected_run_id: Some(Uuid::new_v4()),
            action,
        }
    }

    #[tokio::test]
    async fn remote_agent_queue_preserves_run_identity_for_every_action() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        tokio::fs::write(root.join("paused"), b"").await.unwrap();
        for action in [
            ActionPayload::Note {
                text: "note".into(),
            },
            ActionPayload::Pause {},
            ActionPayload::Resume {},
            ActionPayload::Stop {},
            ActionPayload::Tick {},
        ] {
            let command = command(action);
            let device = Uuid::new_v4();
            queue_agent_command(root, "prior-run", &command, device)
                .await
                .unwrap();
            let queue = if matches!(command.action, ActionPayload::Note { .. }) {
                root.join("inbox")
            } else {
                root.join("control")
                    .join(agent_run_directory_key("prior-run"))
                    .join("requests")
            };
            let envelope =
                read_json_optional(&queue.join(format!("{device}_{}.json", command.command_id)))
                    .await
                    .unwrap()
                    .unwrap();
            assert_eq!(envelope["run_id"], "prior-run");
            assert_eq!(envelope["device_id"], device.to_string());
            assert_eq!(envelope["command_id"], command.command_id.to_string());
            assert_eq!(envelope["action"], json!(command.action.action()));
            assert!(!agent_command_consumed(root, "prior-run", &command, device)
                .await
                .unwrap());
        }
        assert_eq!(
            tokio::fs::read(root.join("paused")).await.unwrap(),
            b"",
            "dispatching resume must not clear a later owner's pause"
        );
        assert!(!root.join("stop").exists());
        assert!(!root.join("tick-now").exists());
    }

    #[tokio::test]
    async fn remote_agent_receipt_requires_matching_run_command_action_and_consumption() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        tokio::fs::create_dir_all(root.join("control/acks"))
            .await
            .unwrap();
        tokio::fs::create_dir_all(root.join("inbox/consumed"))
            .await
            .unwrap();
        for action in [
            ActionPayload::Note {
                text: "note".into(),
            },
            ActionPayload::Pause {},
            ActionPayload::Resume {},
            ActionPayload::Stop {},
            ActionPayload::Tick {},
        ] {
            let command = command(action);
            let device = Uuid::new_v4();
            tokio::fs::write(
                root.join("state.json"),
                json!({"run_id":"run","status":"stopped"}).to_string(),
            )
            .await
            .unwrap();
            tokio::fs::write(
                root.join("inbox/consumed")
                    .join(format!("{device}_{}.json", command.command_id)),
                b"{}",
            )
            .await
            .unwrap();
            assert!(
                !agent_command_consumed(root, "run", &command, device)
                    .await
                    .unwrap(),
                "status, renamed note, and absent tick flag cannot prove delivery"
            );
            let ack = json!({"run_id":"run","device_id":device,"command_id":command.command_id,"action":command.action.action(),"state":"consumed"});
            let path = root
                .join("control/acks")
                .join(format!("{device}_{}.json", command.command_id));
            for (field, wrong) in [
                ("run_id", "other-run"),
                ("device_id", "other-device"),
                ("command_id", "other-command"),
                ("action", "send"),
                ("state", "queued"),
            ] {
                let mut wrong_ack = ack.clone();
                wrong_ack[field] = json!(wrong);
                tokio::fs::write(&path, wrong_ack.to_string())
                    .await
                    .unwrap();
                assert!(
                    !agent_command_consumed(root, "run", &command, device)
                        .await
                        .unwrap(),
                    "wrong {field} must not acknowledge this command"
                );
            }
            tokio::fs::write(&path, ack.to_string()).await.unwrap();
            assert!(agent_command_consumed(root, "run", &command, device)
                .await
                .unwrap());
            assert!(!agent_command_consumed(root, "later-run", &command, device)
                .await
                .unwrap());
        }
    }

    #[tokio::test]
    async fn remote_agent_reused_command_id_cannot_borrow_another_devices_ack() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        tokio::fs::create_dir_all(root.join("control/acks"))
            .await
            .unwrap();
        for action in [
            ActionPayload::Note {
                text: "first note".into(),
            },
            ActionPayload::Pause {},
        ] {
            let first = command(action);
            let mut second = first.clone();
            if matches!(second.action, ActionPayload::Note { .. }) {
                second.action = ActionPayload::Note {
                    text: "second note".into(),
                };
            }
            let device_a = Uuid::new_v4();
            let device_b = Uuid::new_v4();
            queue_agent_command(root, "run", &first, device_a)
                .await
                .unwrap();
            let ack_a = json!({"run_id":"run","device_id":device_a,"command_id":first.command_id,"action":first.action.action(),"state":"consumed"});
            tokio::fs::write(
                root.join("control/acks")
                    .join(format!("{device_a}_{}.json", first.command_id)),
                ack_a.to_string(),
            )
            .await
            .unwrap();
            queue_agent_command(root, "run", &second, device_b)
                .await
                .unwrap();
            assert!(agent_command_consumed(root, "run", &first, device_a)
                .await
                .unwrap());
            assert!(!agent_command_consumed(root, "run", &second, device_b)
                .await
                .unwrap());
            let queue = if matches!(first.action, ActionPayload::Note { .. }) {
                root.join("inbox")
            } else {
                root.join("control")
                    .join(agent_run_directory_key("run"))
                    .join("requests")
            };
            for device in [device_a, device_b] {
                let request =
                    read_json_optional(&queue.join(format!("{device}_{}.json", first.command_id)))
                        .await
                        .unwrap()
                        .unwrap();
                assert_eq!(request["device_id"], device.to_string());
                if matches!(first.action, ActionPayload::Note { .. }) {
                    assert_eq!(
                        request["text"],
                        if device == device_a {
                            "first note"
                        } else {
                            "second note"
                        }
                    );
                }
            }
        }
    }
}
