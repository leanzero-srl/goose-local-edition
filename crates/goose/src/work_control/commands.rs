use super::{
    command_revision::check_revision, conflict, unavailable, unsupported, InputAnswer,
    OwnerRunRegistry,
};
use crate::conversation::message::Message;
use agent_client_protocol::schema::v1::{ContentBlock, PromptRequest, SessionId, TextContent};
use leanzero_link::controller::ControllerError;
use leanzero_link::controller_auth::{AuthenticatedDevice, DispatchAuthorization};
use leanzero_link::controller_wire::*;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub(super) struct QueuedCommand {
    command: WorkCommand,
    account: Uuid,
    device: Uuid,
    auth: DispatchAuthorization,
    preceding_run: Uuid,
}

impl OwnerRunRegistry {
    pub async fn receipt(
        &self,
        device: &AuthenticatedDevice,
        id: Uuid,
    ) -> Result<CommandReceipt, ControllerError> {
        device.check_fresh()?;
        self.ensure_store().await.map_err(unavailable)?;
        self.read_receipt(device.account_id(), device.device_id(), id)
            .await?
            .ok_or_else(|| {
                ControllerError::new(
                    axum::http::StatusCode::NOT_FOUND,
                    "command_not_found",
                    "command is not visible to this device",
                    false,
                )
            })
    }

    async fn read_receipt(
        &self,
        account: Uuid,
        device: Uuid,
        id: Uuid,
    ) -> Result<Option<CommandReceipt>, ControllerError> {
        let json: Option<String> = sqlx::query_scalar("SELECT receipt_json FROM owner_commands WHERE account_id=? AND device_id=? AND command_id=?")
            .bind(account.to_string()).bind(device.to_string()).bind(id.to_string())
            .fetch_optional(self.pool().await.map_err(unavailable)?).await.map_err(unavailable)?;
        json.map(|json| serde_json::from_str(&json).map_err(unavailable))
            .transpose()
    }

    pub async fn accept_command(
        self: &Arc<Self>,
        device: &AuthenticatedDevice,
        command: WorkCommand,
        auth: DispatchAuthorization,
    ) -> Result<CommandReceipt, ControllerError> {
        device.check_fresh()?;
        self.ensure_store().await.map_err(unavailable)?;
        if self.owner.read().expect("owner identity poisoned").as_ref()
            != Some(&command.work.owner_node_id)
            || device.owner_node_id() != command.work.owner_node_id
        {
            return Err(conflict("wrong_owner", "command targets a different owner"));
        }
        if !device
            .scopes()
            .contains(&command.action.action().required_scope())
        {
            return Err(ControllerError::new(
                axum::http::StatusCode::FORBIDDEN,
                "authorization_denied",
                "device does not have this command scope",
                false,
            ));
        }
        validate_action(&command)?;
        let canonical = serde_json::to_string(&command).map_err(unavailable)?;
        let runs = self.runs.lock().await;
        let pool = self.pool().await.map_err(unavailable)?;
        let mut tx = pool
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(unavailable)?;
        let prior: Option<(String,String)> = sqlx::query_as("SELECT command_json,receipt_json FROM owner_commands WHERE account_id=? AND device_id=? AND command_id=?")
            .bind(device.account_id().to_string()).bind(device.device_id().to_string()).bind(command.command_id.to_string())
            .fetch_optional(&mut *tx).await.map_err(unavailable)?;
        if let Some((prior, receipt)) = prior {
            if prior != canonical {
                return Err(conflict(
                    "command_id_conflict",
                    "command ID was already used for different contents",
                ));
            }
            return serde_json::from_str(&receipt).map_err(unavailable);
        }
        let generation: String =
            sqlx::query_scalar("SELECT generation FROM controller_store WHERE singleton=1")
                .fetch_one(&mut *tx)
                .await
                .map_err(unavailable)?;
        if generation != command.expected_store_generation.to_string() {
            return Err(conflict(
                "store_generation_mismatch",
                "take a snapshot of this store generation",
            ));
        }
        let record: Option<(String,String,i64,Option<String>)> = sqlx::query_as("SELECT local_locator,kind,revision,current_run_id FROM work_identity WHERE work_id=? AND deleted=0")
            .bind(command.work.work_id.to_string()).fetch_optional(&mut *tx).await.map_err(unavailable)?;
        let (session_id, kind, revision, persisted_run) =
            record.ok_or_else(|| conflict("work_not_found", "work was deleted or replaced"))?;
        if self.preflight_state(&session_id).is_some()
            && !matches!(command.action, ActionPayload::AnswerInput { .. })
        {
            return Err(conflict(
                "setup_pending",
                "finish the original recipe setup before sending a prompt",
            ));
        }
        if kind != "chat" {
            return Err(unsupported(
                "only chat commands have an owner execution adapter",
            ));
        }
        check_revision(&mut tx, &command, revision).await?;
        let active = runs.get(&session_id).map(|run| run.run_id.as_str());
        let expected = command.expected_run_id.map(|run| run.to_string());
        if active != expected.as_deref() || persisted_run.as_deref() != active {
            return Err(conflict(
                "stale_run",
                "command does not name the current owner run",
            ));
        }
        if matches!(command.action, ActionPayload::Send { .. }) && !self.has_executor() {
            return Err(ControllerError::unavailable(
                "owner prompt executor is not initialized",
            ));
        }
        let queued = matches!(command.action, ActionPayload::Send { .. }) && active.is_some();
        let run_id = if matches!(command.action, ActionPayload::Send { .. }) {
            Some(Uuid::new_v4())
        } else {
            command.expected_run_id
        };
        let message_id = matches!(
            command.action,
            ActionPayload::Send { .. } | ActionPayload::Steer { .. }
        )
        .then(|| {
            format!(
                "controller_{}_{}_{}",
                device.account_id(),
                device.device_id(),
                command.command_id
            )
        });
        let receipt = CommandReceipt {
            command_id: command.command_id,
            work: command.work.clone(),
            authenticated_device_id: device.device_id(),
            revision: command.expected_revision,
            state: if queued {
                CommandState::Waiting
            } else {
                CommandState::Accepted
            },
            run_id,
            message_id,
            input_id: match command.action {
                ActionPayload::AnswerInput { input_id, .. } => Some(input_id),
                _ => None,
            },
            checkpoint_id: None,
            reason: None,
            updated_at: chrono::Utc::now(),
        };
        sqlx::query("INSERT INTO owner_commands(account_id,device_id,command_id,work_id,generation,session_id,command_json,receipt_json,state,message_id,run_id) VALUES (?,?,?,?,?,?,?,?,?,?,?)")
            .bind(device.account_id().to_string()).bind(device.device_id().to_string()).bind(command.command_id.to_string())
            .bind(command.work.work_id.to_string()).bind(generation).bind(&session_id).bind(canonical)
            .bind(serde_json::to_string(&receipt).map_err(unavailable)?).bind(if queued { "waiting" } else { "accepted" })
            .bind(&receipt.message_id).bind(run_id.map(|id| id.to_string())).execute(&mut *tx).await.map_err(unavailable)?;
        tx.commit().await.map_err(unavailable)?;
        if queued {
            self.queued
                .lock()
                .await
                .entry(session_id)
                .or_default()
                .push(QueuedCommand {
                    preceding_run: command.expected_run_id.expect("busy run checked"),
                    command,
                    account: device.account_id(),
                    device: device.device_id(),
                    auth,
                });
        } else {
            let registry = self.clone();
            let account = device.account_id();
            let device = device.device_id();
            tokio::spawn(async move {
                registry
                    .dispatch(command, account, device, auth, None)
                    .await;
            });
        }
        drop(runs);
        Ok(receipt)
    }

    pub(crate) async fn mark_prompt_delivered(
        &self,
        session_id: &str,
        message_id: &str,
    ) -> Result<(), ControllerError> {
        let rows: Vec<(String,String,String)> = sqlx::query_as("SELECT account_id,device_id,receipt_json FROM owner_commands WHERE session_id=? AND message_id=? AND state='dispatching' AND EXISTS(SELECT 1 FROM messages WHERE session_id=? AND message_id=?)")
            .bind(session_id).bind(message_id).bind(session_id).bind(message_id)
            .fetch_all(self.pool().await.map_err(unavailable)?).await.map_err(unavailable)?;
        for (account, device, json) in rows {
            self.update_receipt(
                Uuid::parse_str(&account).map_err(unavailable)?,
                Uuid::parse_str(&device).map_err(unavailable)?,
                serde_json::from_str(&json).map_err(unavailable)?,
                CommandState::Delivered,
                None,
            )
            .await?;
        }
        Ok(())
    }

    async fn update_receipt(
        &self,
        account: Uuid,
        device: Uuid,
        mut receipt: CommandReceipt,
        state: CommandState,
        reason: Option<String>,
    ) -> Result<(), ControllerError> {
        receipt.state = state;
        receipt.reason = reason;
        receipt.updated_at = chrono::Utc::now();
        let state = serde_json::to_value(state)
            .map_err(unavailable)?
            .as_str()
            .ok_or_else(|| unavailable("invalid command state"))?
            .to_string();
        let mut tx = self
            .pool()
            .await
            .map_err(unavailable)?
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(unavailable)?;
        sqlx::query("UPDATE owner_commands SET receipt_json=?,state=?,message_id=?,run_id=? WHERE account_id=? AND device_id=? AND command_id=?")
            .bind(serde_json::to_string(&receipt).map_err(unavailable)?).bind(state).bind(&receipt.message_id).bind(receipt.run_id.map(|id| id.to_string())).bind(account.to_string()).bind(device.to_string()).bind(receipt.command_id.to_string())
            .execute(&mut *tx).await.map_err(unavailable)?;
        sqlx::query(
            "UPDATE work_identity SET event_kind='command_receipt' WHERE work_id=? AND deleted=0",
        )
        .bind(receipt.work.work_id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(unavailable)?;
        tx.commit().await.map_err(unavailable)?;
        Ok(())
    }

    async fn dispatch(
        self: Arc<Self>,
        command: WorkCommand,
        account: Uuid,
        device: Uuid,
        auth: DispatchAuthorization,
        completed_run: Option<Uuid>,
    ) {
        let receipt = match self.read_receipt(account, device, command.command_id).await {
            Ok(Some(receipt)) => receipt,
            other => {
                tracing::error!(?other, "owner command receipt unavailable at dispatch");
                return;
            }
        };
        let result = self
            .dispatch_inner(&command, account, device, &auth, completed_run, &receipt)
            .await;
        if let Err(error) = result {
            if let Err(persist) = self
                .update_receipt(
                    account,
                    device,
                    receipt,
                    CommandState::Rejected,
                    Some(format!("{}: {}", error.body.code, error.body.message)),
                )
                .await
            {
                tracing::error!(%persist, "owner command rejection could not be persisted");
            }
            if let Some(completed_run) = completed_run {
                if let Ok(Some(session_id)) = async {
                    Ok::<_, anyhow::Error>(sqlx::query_scalar::<_, String>("SELECT session_id FROM owner_commands WHERE account_id=? AND device_id=? AND command_id=?")
                        .bind(account.to_string()).bind(device.to_string()).bind(command.command_id.to_string())
                        .fetch_optional(self.pool().await?).await?)
                }.await {
                    self.dispatch_next(session_id, completed_run.to_string());
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn dispatch_inner(
        self: &Arc<Self>,
        command: &WorkCommand,
        account: Uuid,
        device: Uuid,
        auth: &DispatchAuthorization,
        completed_run: Option<Uuid>,
        receipt: &CommandReceipt,
    ) -> Result<(), ControllerError> {
        let catalog = self
            .session_manager
            .work_catalog()
            .await
            .map_err(unavailable)?;
        let record = catalog
            .lookup(
                &command.work.work_id.to_string(),
                &command.expected_store_generation.to_string(),
            )
            .await
            .map_err(unavailable)?;
        let executor = self
            .executor
            .read()
            .expect("owner executor poisoned")
            .clone();
        if self.preflight_state(&record.local_locator).is_some()
            && !matches!(command.action, ActionPayload::AnswerInput { .. })
        {
            return Err(conflict(
                "setup_pending",
                "finish the original recipe setup before sending a prompt",
            ));
        }
        // Preparation can load extensions, but cannot run the model or mutate its transcript.
        // It happens before the lock so other work's stop/approval never waits for MCP startup.
        let question_continuation = match &command.action {
            ActionPayload::AnswerInput {
                input_id,
                option_id,
                ..
            } if command.expected_run_id.is_none() && option_id != "dismiss" => self
                .question_input(&record.local_locator, *input_id)
                .await?
                .is_some(),
            _ => false,
        };
        let prepared =
            if matches!(command.action, ActionPayload::Send { .. }) || question_continuation {
                let executor = executor.as_ref().ok_or_else(|| {
                    ControllerError::unavailable("owner prompt executor is not initialized")
                })?;
                Some(
                    executor
                        .prepare(&record.local_locator)
                        .await
                        .map_err(unavailable)?,
                )
            } else {
                None
            };
        let mut runs = self.runs.lock().await;
        let fresh = auth.recheck().await?;
        fresh.check_fresh()?;
        let record = catalog
            .lookup(
                &command.work.work_id.to_string(),
                &command.expected_store_generation.to_string(),
            )
            .await
            .map_err(unavailable)?;
        if completed_run.is_none() {
            let mut connection = self
                .pool()
                .await
                .map_err(unavailable)?
                .acquire()
                .await
                .map_err(unavailable)?;
            check_revision(&mut connection, command, record.revision).await?;
        }
        let active = runs
            .get(&record.local_locator)
            .map(|run| run.run_id.clone());
        let expected = command.expected_run_id.map(|run| run.to_string());
        if let Some(completed_run) = completed_run {
            let completed: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM owner_run_completions WHERE work_id=? AND generation=? AND run_id=?)")
                .bind(command.work.work_id.to_string()).bind(command.expected_store_generation.to_string()).bind(completed_run.to_string())
                .fetch_one(self.pool().await.map_err(unavailable)?).await.map_err(unavailable)?;
            if !completed || active.is_some() {
                return Err(conflict(
                    "stale_run",
                    "queued turn no longer follows its recorded run",
                ));
            }
        } else if active != expected || record.current_run_id != active {
            return Err(conflict("stale_run", "active run changed before dispatch"));
        }
        self.update_receipt(
            account,
            device,
            receipt.clone(),
            CommandState::Dispatching,
            None,
        )
        .await?;
        match &command.action {
            ActionPayload::Send { content } => {
                let (agent, manager) = prepared.expect("send prepared above");
                let run_id = receipt
                    .run_id
                    .ok_or_else(|| unavailable("send receipt has no run identity"))?;
                let ticket = self
                    .reserve_locked(
                        &mut runs,
                        &record.local_locator,
                        run_id,
                        CancellationToken::new(),
                        agent,
                        manager,
                        receipt.message_id.clone(),
                    )
                    .await
                    .map_err(unavailable)?;
                if let Some(completed_run) = completed_run {
                    if let Some(items) = self.queued.lock().await.get_mut(&record.local_locator) {
                        for item in items {
                            if item.preceding_run == completed_run {
                                item.preceding_run = run_id;
                            }
                        }
                    }
                }
                let args = PromptRequest::new(
                    SessionId::new(record.local_locator.clone()),
                    content_blocks(content)?,
                );
                let executor = executor.expect("send executor prepared above");
                self.spawn_command_prompt(executor, args, ticket, account, device, receipt.clone());
            }
            ActionPayload::Steer { content } => {
                let run = runs
                    .get(&record.local_locator)
                    .ok_or_else(|| conflict("stale_run", "run ended before steer"))?;
                let mut message = Message::user().with_id(
                    receipt
                        .message_id
                        .clone()
                        .ok_or_else(|| unavailable("steer has no message identity"))?,
                );
                for content in content {
                    if let ControllerContent::Text { text } = content {
                        message = message.with_text(text);
                    }
                }
                run.agent.steer(&record.local_locator, message).await;
                self.update_receipt(
                    account,
                    device,
                    receipt.clone(),
                    CommandState::Delivered,
                    None,
                )
                .await?;
            }
            ActionPayload::Stop {} => {
                let run = runs
                    .get(&record.local_locator)
                    .ok_or_else(|| conflict("stale_run", "run ended before stop"))?;
                run.cancel_token.cancel();
                self.update_receipt(
                    account,
                    device,
                    receipt.clone(),
                    CommandState::Delivered,
                    None,
                )
                .await?;
            }
            ActionPayload::AnswerInput {
                input_id,
                option_id,
                value,
            } => {
                if command.expected_run_id.is_none() {
                    if self.is_preflight_input(*input_id).await? {
                        self.answer_preflight_locked(
                            *input_id,
                            Some(command.work.work_id),
                            InputAnswer {
                                option_id: option_id.clone(),
                                value: value.clone(),
                            },
                        )
                        .await?;
                        self.update_receipt(
                            account,
                            device,
                            receipt.clone(),
                            CommandState::Delivered,
                            None,
                        )
                        .await?;
                        return Ok(());
                    }
                    if self
                        .question_input(&record.local_locator, *input_id)
                        .await?
                        .is_none()
                    {
                        return Err(conflict("input_not_found", "idle input no longer exists"));
                    }
                    let args = self
                        .resolve_question(
                            &record.local_locator,
                            command.work.work_id,
                            command.expected_store_generation,
                            *input_id,
                            &InputAnswer {
                                option_id: option_id.clone(),
                                value: value.clone(),
                            },
                        )
                        .await?;
                    if let Some(args) = args {
                        let (agent, manager) = prepared.ok_or_else(|| {
                            unavailable("question continuation has no prepared owner")
                        })?;
                        let mut receipt = receipt.clone();
                        let run_id = Uuid::new_v4();
                        receipt.run_id = Some(run_id);
                        receipt.message_id = Some(format!(
                            "controller_{account}_{device}_{}",
                            command.command_id
                        ));
                        self.update_receipt(
                            account,
                            device,
                            receipt.clone(),
                            CommandState::Dispatching,
                            None,
                        )
                        .await?;
                        let ticket = self
                            .reserve_locked(
                                &mut runs,
                                &record.local_locator,
                                run_id,
                                CancellationToken::new(),
                                agent,
                                manager,
                                receipt.message_id.clone(),
                            )
                            .await
                            .map_err(unavailable)?;
                        self.spawn_command_prompt(
                            executor.ok_or_else(|| {
                                unavailable("question continuation has no owner executor")
                            })?,
                            args,
                            ticket,
                            account,
                            device,
                            receipt,
                        );
                    } else {
                        self.update_receipt(
                            account,
                            device,
                            receipt.clone(),
                            CommandState::Delivered,
                            None,
                        )
                        .await?;
                    }
                    return Ok(());
                }
                self.answer_input_locked(
                    &runs,
                    *input_id,
                    command
                        .expected_run_id
                        .ok_or_else(|| conflict("stale_run", "answer needs an active run"))?,
                    InputAnswer {
                        option_id: option_id.clone(),
                        value: value.clone(),
                    },
                )
                .await?;
                self.update_receipt(
                    account,
                    device,
                    receipt.clone(),
                    CommandState::Delivered,
                    None,
                )
                .await?;
            }
            _ => return Err(unsupported("action has no execution adapter")),
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn spawn_command_prompt(
        self: &Arc<Self>,
        executor: Arc<dyn super::OwnerExecutor>,
        args: PromptRequest,
        ticket: super::RunTicket,
        account: Uuid,
        device: Uuid,
        receipt: CommandReceipt,
    ) {
        let registry = self.clone();
        // This task owns the same complete ACP turn used by local sends. The HTTP ACK
        // and every viewer may disappear while it continues polling the actual Agent.
        tokio::spawn(async move {
            let result = executor.execute(args, ticket.clone()).await;
            if let Err(error) = result {
                tracing::warn!(?error, run_id = %ticket.run_id, "controller owner turn failed");
            }
            registry
                .finish(&ticket.session_id, &ticket.run_id.to_string())
                .await;
            let proof: Result<bool, _> = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM messages WHERE session_id=? AND message_id=?)",
            )
            .bind(&ticket.session_id)
            .bind(&receipt.message_id)
            .fetch_one(match registry.pool().await {
                Ok(pool) => pool,
                Err(error) => {
                    tracing::error!(%error, "owner send reconciliation store unavailable");
                    return;
                }
            })
            .await;
            let (state, reason) = match proof {
                Ok(true) => (CommandState::Delivered, None),
                Ok(false) => (
                    CommandState::OutcomeUnknown,
                    Some("prompt ended without persisted message proof".into()),
                ),
                Err(error) => (CommandState::OutcomeUnknown, Some(error.to_string())),
            };
            if let Err(error) = registry
                .update_receipt(account, device, receipt, state, reason)
                .await
            {
                tracing::error!(%error, "owner send receipt could not be persisted");
            }
        });
    }

    pub(super) fn dispatch_next(self: &Arc<Self>, session_id: String, completed_run: String) {
        let registry = self.clone();
        tokio::spawn(async move {
            let completed_run = match Uuid::parse_str(&completed_run) {
                Ok(run) => run,
                Err(error) => {
                    tracing::error!(%error, "invalid completed owner run");
                    return;
                }
            };
            let next = {
                let mut queue = registry.queued.lock().await;
                let Some(items) = queue.get_mut(&session_id) else {
                    return;
                };
                let index = items
                    .iter()
                    .position(|item| item.preceding_run == completed_run);
                index.map(|index| items.remove(index))
            };
            if let Some(next) = next {
                registry
                    .dispatch(
                        next.command,
                        next.account,
                        next.device,
                        next.auth,
                        Some(completed_run),
                    )
                    .await;
            }
        });
    }
}

fn validate_action(command: &WorkCommand) -> Result<(), ControllerError> {
    if command.work.kind != WorkKind::Chat {
        return Err(unsupported("this work kind has no control adapter"));
    }
    match &command.action {
        ActionPayload::Send { content } | ActionPayload::Steer { content } => {
            content_blocks(content)?;
        }
        ActionPayload::Stop {} | ActionPayload::AnswerInput { .. } => {}
        _ => return Err(unsupported("this action has no control adapter")),
    }
    if matches!(
        command.action,
        ActionPayload::Steer { .. } | ActionPayload::Stop {}
    ) && command.expected_run_id.is_none()
    {
        return Err(conflict(
            "stale_run",
            "this action needs an exact active run",
        ));
    }
    Ok(())
}

fn content_blocks(content: &[ControllerContent]) -> Result<Vec<ContentBlock>, ControllerError> {
    if content.is_empty() {
        return Err(unsupported("send/steer needs content"));
    }
    content
        .iter()
        .map(|content| match content {
            ControllerContent::Text { text } => {
                Ok(ContentBlock::Text(TextContent::new(text.clone())))
            }
            ControllerContent::Resource { .. } => {
                Err(unsupported("owner resource upload is not configured"))
            }
        })
        .collect()
}
