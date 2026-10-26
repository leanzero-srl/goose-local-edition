//! Read-only projection of the durable session catalog. Loading a remote view never loads an Agent.

pub use super::cursors::CursorPolicy;
use super::cursors::{invalid_cursor, Cursor, CursorSigner, Position};
use super::OwnerRunRegistry;
use crate::conversation::message::{
    Message, MessageContent, MessageMetadata, SystemNotificationType,
};
use crate::session::work_catalog::{
    self, CatalogError, CatalogRecord, HistoryRecord, StoreWatermark, WorkCatalog,
};
use async_trait::async_trait;
use axum::http::StatusCode;
use chrono::{DateTime, Utc};
use futures::{future::BoxFuture, stream::BoxStream};
use leanzero_link::controller::{ControllerBackend, ControllerError};
use leanzero_link::controller_auth::{AuthenticatedDevice, DispatchAuthorization};
use leanzero_link::controller_wire::*;
use leanzero_link::load::NodeLoad;
use rmcp::model::Role;
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicI64, Ordering},
        Arc,
    },
};
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

pub type ResourceSampler =
    Arc<dyn Fn() -> BoxFuture<'static, Result<NodeLoad, ControllerError>> + Send + Sync>;

pub struct ControllerBackendConfig {
    pub host: HostDescriptor,
    pub account_id: Uuid,
    pub cursor_key_path: PathBuf,
    pub policy: CursorPolicy,
}

#[derive(Clone)]
pub struct GooseControllerBackend(Arc<BackendState>);

struct BackendState {
    host: HostDescriptor,
    account_id: Uuid,
    catalog: WorkCatalog,
    generation: Uuid,
    high_water: AtomicI64,
    runs: Arc<OwnerRunRegistry>,
    resources: Option<ResourceSampler>,
    signer: CursorSigner,
    policy: CursorPolicy,
}

impl GooseControllerBackend {
    pub async fn new(
        config: ControllerBackendConfig,
        catalog: WorkCatalog,
        runs: Arc<OwnerRunRegistry>,
        resources: Option<ResourceSampler>,
    ) -> Result<Self, ControllerError> {
        if config.host.device_role != DeviceRole::Host
            || config.host.protocol_version != CONTROLLER_PROTOCOL_VERSION
            || config.host.availability != HostAvailability::Online
            || config.host.owner_node_id.is_nil()
            || config.account_id.is_nil()
        {
            return Err(ControllerError::unavailable(
                "controller requires a registered online owner",
            ));
        }
        runs.ensure_store().await.map_err(store_error)?;
        let watermark = catalog.watermark().await.map_err(store_error)?;
        let generation = parse_uuid(&watermark.generation)?;
        runs.bind_owner(config.host.owner_node_id)?;
        let signer = CursorSigner::open(
            &config.cursor_key_path,
            config.host.owner_node_id,
            config.account_id,
            &config.policy,
        )?;
        Ok(Self(Arc::new(BackendState {
            host: config.host,
            account_id: config.account_id,
            catalog,
            generation,
            high_water: AtomicI64::new(watermark.revision),
            runs,
            resources,
            signer,
            policy: config.policy,
        })))
    }

    fn authenticate(
        &self,
        device: &AuthenticatedDevice,
        scope: ControllerScope,
    ) -> Result<(), ControllerError> {
        device.check_fresh()?;
        if device.owner_node_id() != self.0.host.owner_node_id
            || device.account_id() != self.0.account_id
        {
            return Err(ControllerError::new(
                StatusCode::FORBIDDEN,
                "wrong_owner",
                "device does not belong to this owner account",
                false,
            ));
        }
        if !device.scopes().contains(&scope) || !self.allowed_scopes().contains(&scope) {
            return Err(ControllerError::new(
                StatusCode::FORBIDDEN,
                "scope_denied",
                "controller scope is not enabled",
                false,
            ));
        }
        Ok(())
    }

    async fn fence(&self) -> Result<StoreWatermark, ControllerError> {
        let watermark = self
            .0
            .catalog
            .check_external_fence(
                &self.0.generation.to_string(),
                self.0.high_water.load(Ordering::Acquire),
            )
            .await
            .map_err(store_error)?;
        self.0
            .high_water
            .fetch_max(watermark.revision, Ordering::AcqRel);
        Ok(watermark)
    }

    fn work_path(&self, work: &WorkRef, generation: Uuid) -> Result<(), ControllerError> {
        if work.owner_node_id != self.0.host.owner_node_id {
            return Err(ControllerError::new(
                StatusCode::FORBIDDEN,
                "wrong_owner",
                "work belongs to another owner",
                false,
            ));
        }
        if generation != self.0.generation {
            return Err(snapshot_required("store generation changed"));
        }
        if work.kind != WorkKind::Chat {
            return Err(unsupported("this work kind has no controller adapter"));
        }
        Ok(())
    }

    fn watermark(&self, revision: i64) -> Result<CatalogCursor, ControllerError> {
        Ok(CatalogCursor {
            owner_node_id: self.0.host.owner_node_id,
            store_generation: self.0.generation,
            revision: revision
                .try_into()
                .map_err(|_| ControllerError::unavailable("negative journal revision"))?,
        })
    }

    fn new_token(
        &self,
        device: &AuthenticatedDevice,
        revision: i64,
        position: Position,
    ) -> Result<String, ControllerError> {
        self.0.signer.encode(&self.0.signer.new_cursor(
            device,
            self.0.generation,
            revision,
            position,
        )?)
    }

    fn decode(&self, device: &AuthenticatedDevice, token: &str) -> Result<Cursor, ControllerError> {
        let cursor = self.0.signer.decode(device, token)?;
        if cursor.generation != self.0.generation {
            return Err(snapshot_required("store generation changed"));
        }
        Ok(cursor)
    }

    async fn visible_work(
        &self,
        work: &WorkRef,
        watermark: &StoreWatermark,
    ) -> Result<CatalogRecord, ControllerError> {
        let record = self
            .0
            .catalog
            .lookup_at(&work.work_id.to_string(), watermark)
            .await
            .map_err(store_error)?;
        if !visible(&record) {
            return Err(not_found());
        }
        Ok(record)
    }

    fn entry(&self, record: &CatalogRecord) -> Result<CatalogEntry, ControllerError> {
        if record.kind != "chat" {
            return Err(ControllerError::unavailable("unsupported stored work kind"));
        }
        let state = match record.state.as_str() {
            "idle" => WorkState::Idle,
            "running" => WorkState::Running,
            "waiting_input" => WorkState::WaitingInput,
            "paused" => WorkState::Paused,
            "finished" => WorkState::Finished,
            "failed" => WorkState::Failed,
            "interrupted" => WorkState::Interrupted,
            "deleted" => WorkState::Deleted,
            _ => return Err(ControllerError::unavailable("invalid stored work state")),
        };
        let current_run_id = record
            .current_run_id
            .as_deref()
            .map(parse_uuid)
            .transpose()?;
        let capabilities = if self.0.runs.has_executor() && visible(record) {
            if current_run_id.is_some() {
                let mut actions = vec![WorkAction::Steer, WorkAction::Stop];
                if state == WorkState::WaitingInput {
                    actions.push(WorkAction::AnswerInput);
                }
                actions
            } else {
                vec![WorkAction::Send]
            }
        } else {
            Vec::new()
        };
        Ok(CatalogEntry {
            work: WorkRef {
                owner_node_id: self.0.host.owner_node_id,
                kind: WorkKind::Chat,
                work_id: parse_uuid(&record.work_id)?,
            },
            store_generation: self.0.generation,
            revision: self.watermark(record.revision)?.revision,
            title: record.title.clone(),
            working_dir: (!record.working_dir.is_empty()).then(|| record.working_dir.clone()),
            state,
            current_run_id,
            capabilities,
            parent: None,
            related: Vec::new(),
            updated_at: DateTime::parse_from_rfc3339(&record.updated_at)
                .map_err(|_| ControllerError::unavailable("invalid catalog timestamp"))?
                .with_timezone(&Utc),
            last_event: None,
            archived: record.archived,
        })
    }

    async fn change(
        &self,
        device: &AuthenticatedDevice,
        record: CatalogRecord,
    ) -> Result<Option<CatalogChange>, ControllerError> {
        let entry = if visible(&record) {
            Some(self.entry(&record)?)
        } else {
            let previous = self
                .0
                .catalog
                .lookup_at(
                    &record.work_id,
                    &StoreWatermark {
                        generation: self.0.generation.to_string(),
                        revision: record.revision - 1,
                    },
                )
                .await;
            match previous {
                Ok(previous) if visible(&previous) => {}
                Ok(_) => return Ok(None),
                Err(error)
                    if error.downcast_ref::<CatalogError>()
                        == Some(&CatalogError::WorkNotFound) =>
                {
                    return Ok(None)
                }
                Err(error) => return Err(store_error(error)),
            }
            None
        };
        Ok(Some(CatalogChange {
            resume_cursor: self.new_token(device, record.revision, Position::CatalogResume)?,
            cursor: self.watermark(record.revision)?,
            kind: if entry.is_some() {
                CatalogChangeKind::Upsert
            } else {
                CatalogChangeKind::Tombstone
            },
            work: WorkRef {
                owner_node_id: self.0.host.owner_node_id,
                kind: WorkKind::Chat,
                work_id: parse_uuid(&record.work_id)?,
            },
            entry,
        }))
    }

    async fn changes_at(
        &self,
        device: &AuthenticatedDevice,
        watermark: &StoreWatermark,
    ) -> Result<CatalogChanges, ControllerError> {
        self.fence().await?;
        let page = self
            .0
            .catalog
            .changes(watermark, self.0.policy.page_size)
            .await
            .map_err(store_error)?;
        let mut changes = Vec::new();
        for record in page.records {
            if let Some(change) = self.change(device, record).await? {
                changes.push(change);
            }
        }
        Ok(CatalogChanges {
            resume_cursor: self.new_token(device, page.cursor.revision, Position::CatalogResume)?,
            cursor: self.watermark(page.cursor.revision)?,
            changes,
            has_more: page.has_more,
        })
    }

    fn frame(
        &self,
        device: &AuthenticatedDevice,
        work: &WorkRef,
        record: &CatalogRecord,
        kind: &str,
        payload: Value,
    ) -> Result<WorkFrame, ControllerError> {
        let revision = self.watermark(record.revision)?.revision;
        Ok(WorkFrame {
            resume_cursor: self.new_token(
                device,
                record.revision,
                Position::WorkResume {
                    work_id: work.work_id,
                },
            )?,
            work: work.clone(),
            store_generation: self.0.generation,
            stream_epoch: self.0.generation,
            sequence: revision,
            work_revision: revision,
            run_id: record
                .current_run_id
                .as_deref()
                .map(parse_uuid)
                .transpose()?,
            kind: kind.into(),
            payload,
        })
    }

    async fn work_event(
        &self,
        device: &AuthenticatedDevice,
        work: &WorkRef,
        event: work_catalog::WorkEventRecord,
    ) -> Result<Option<WorkFrame>, ControllerError> {
        let watermark = StoreWatermark {
            generation: self.0.generation.to_string(),
            revision: event.revision,
        };
        let record = self
            .0
            .catalog
            .lookup_at(&event.work_id, &watermark)
            .await
            .map_err(store_error)?;
        if !visible(&record) {
            return Ok(Some(self.frame(
                device,
                work,
                &record,
                "tombstone",
                Value::Null,
            )?));
        }
        match event.event_kind.as_str() {
            "partial_replacement" => {
                let partial = self
                    .0
                    .runs
                    .partial_at(work.work_id, event.revision)
                    .await?
                    .ok_or_else(|| {
                        ControllerError::unavailable("partial event has no durable content")
                    })?;
                let Some(item) = visible_message(&partial)? else {
                    return Ok(None);
                };
                let offset_bytes = self
                    .0
                    .runs
                    .partial_offset_at(work.work_id, event.revision)
                    .await?;
                Ok(Some(self.frame(
                    device,
                    work,
                    &record,
                    "partial_replacement",
                    json!({
                        "message_id": item.message_id, "message_revision": item.revision,
                        "offset_bytes": offset_bytes, "content": item.message,
                    }),
                )?))
            }
            "partial_discarded" => {
                let message_ids = self
                    .0
                    .runs
                    .partial_cleared_at(work.work_id, event.revision)
                    .await?;
                Ok(Some(self.frame(
                    device,
                    work,
                    &record,
                    "partial_discarded",
                    json!({"message_ids": message_ids}),
                )?))
            }

            "message_upsert" | "message_deleted" => {
                let row_id = event.row_id.ok_or_else(|| {
                    ControllerError::unavailable("message event has no row identity")
                })?;
                let previous = self
                    .0
                    .catalog
                    .message_at(
                        &event.work_id,
                        &StoreWatermark {
                            generation: watermark.generation.clone(),
                            revision: event.revision - 1,
                        },
                        row_id,
                    )
                    .await
                    .map_err(store_error)?;
                let previous = previous
                    .as_ref()
                    .map(visible_message)
                    .transpose()?
                    .flatten();
                let current = self
                    .0
                    .catalog
                    .message_at(&event.work_id, &watermark, row_id)
                    .await
                    .map_err(store_error)?;
                let current = current.as_ref().map(visible_message).transpose()?.flatten();
                if let Some(item) = current {
                    let previous_message_id = previous
                        .filter(|old| old.message_id != item.message_id)
                        .map(|old| old.message_id);
                    Ok(Some(self.frame(
                        device,
                        work,
                        &record,
                        "message_upsert",
                        json!({"item": item, "previous_message_id": previous_message_id}),
                    )?))
                } else if let Some(item) = previous {
                    Ok(Some(self.frame(
                        device,
                        work,
                        &record,
                        "message_deleted",
                        json!({"message_id": item.message_id}),
                    )?))
                } else {
                    Ok(None)
                }
            }
            _ => Ok(Some(self.frame(
                device,
                work,
                &record,
                &event.event_kind,
                json!({"entry": self.entry(&record)?}),
            )?)),
        }
    }
}

#[async_trait]
impl ControllerBackend for GooseControllerBackend {
    fn host(&self) -> HostDescriptor {
        let mut host = self.0.host.clone();
        host.capabilities = self.allowed_scopes();
        host
    }

    fn allowed_scopes(&self) -> Vec<ControllerScope> {
        let mut scopes = vec![ControllerScope::CatalogRead, ControllerScope::HistoryRead];
        if self.0.runs.has_executor() {
            scopes.extend([
                ControllerScope::WorkSend,
                ControllerScope::WorkSteer,
                ControllerScope::WorkStop,
                ControllerScope::InputAnswer,
            ]);
        }
        if self.0.resources.is_some() {
            scopes.push(ControllerScope::ResourcesRead);
        }
        scopes
    }

    async fn catalog(
        &self,
        device: &AuthenticatedDevice,
        page_token: Option<&str>,
    ) -> Result<CatalogPage, ControllerError> {
        self.authenticate(device, ControllerScope::CatalogRead)?;
        let current = self.fence().await?;
        let token = page_token
            .map(|token| self.decode(device, token))
            .transpose()?;
        let cursor = match &token {
            Some(Cursor {
                position: Position::CatalogPage { after_work_id },
                generation,
                revision,
                ..
            }) => Some(work_catalog::CatalogPageCursor {
                watermark: StoreWatermark {
                    generation: generation.to_string(),
                    revision: *revision,
                },
                after_work_id: after_work_id.clone(),
            }),
            Some(_) => return Err(invalid_cursor()),
            None => Some(work_catalog::CatalogPageCursor {
                watermark: current,
                after_work_id: String::new(),
            }),
        };
        let page = self
            .0
            .catalog
            .catalog_page(cursor.as_ref(), self.0.policy.page_size)
            .await
            .map_err(store_error)?;
        let entries = page
            .records
            .iter()
            .filter(|record| visible(record))
            .map(|record| self.entry(record))
            .collect::<Result<Vec<_>, _>>()?;
        let next_page_token = page
            .next
            .map(|next| {
                let position = Position::CatalogPage {
                    after_work_id: next.after_work_id,
                };
                let cursor = match &token {
                    Some(cursor) => {
                        let mut cursor = cursor.clone();
                        cursor.position = position;
                        cursor
                    }
                    None => self.0.signer.new_cursor(
                        device,
                        self.0.generation,
                        page.watermark.revision,
                        position,
                    )?,
                };
                self.0.signer.encode(&cursor)
            })
            .transpose()?;
        Ok(CatalogPage {
            entries,
            watermark: self.watermark(page.watermark.revision)?,
            next_page_token,
            resume_cursor: self.new_token(
                device,
                page.watermark.revision,
                Position::CatalogResume,
            )?,
        })
    }

    async fn changes(
        &self,
        device: &AuthenticatedDevice,
        cursor: &str,
    ) -> Result<CatalogChanges, ControllerError> {
        self.authenticate(device, ControllerScope::CatalogRead)?;
        let cursor = self.decode(device, cursor)?;
        if cursor.position != Position::CatalogResume {
            return Err(invalid_cursor());
        }
        self.changes_at(
            device,
            &StoreWatermark {
                generation: cursor.generation.to_string(),
                revision: cursor.revision,
            },
        )
        .await
    }

    async fn catalog_stream(
        &self,
        device: &AuthenticatedDevice,
        cursor: &str,
    ) -> Result<BoxStream<'static, Result<CatalogChange, ControllerError>>, ControllerError> {
        self.authenticate(device, ControllerScope::CatalogRead)?;
        let cursor = self.decode(device, cursor)?;
        if cursor.position != Position::CatalogResume {
            return Err(invalid_cursor());
        }
        self.fence().await?;
        let mut watermark = StoreWatermark {
            generation: cursor.generation.to_string(),
            revision: cursor.revision,
        };
        // Validate future cursors before upgrading a connection, including on an empty catalog.
        self.0
            .catalog
            .changes(&watermark, 1)
            .await
            .map_err(store_error)?;
        let mut subscription = self
            .0
            .catalog
            .subscribe(self.0.policy.journal_poll_interval)
            .await
            .map_err(store_error)?;
        let backend = self.clone();
        let device = device.clone();
        Ok(buffered_stream(
            self.0.policy.stream_buffer,
            move |sender| {
                Box::pin(async move {
                    loop {
                        let page = backend.changes_at(&device, &watermark).await?;
                        watermark.revision = page
                            .cursor
                            .revision
                            .try_into()
                            .map_err(|_| invalid_cursor())?;
                        for change in page.changes {
                            send_frame(&sender, change)?;
                        }
                        if page.has_more {
                            continue;
                        }
                        tokio::select! {
                            _ = sender.closed() => return Ok(()),
                            changed = subscription.changed() => { changed.map_err(store_error)?; }
                        }
                    }
                })
            },
        ))
    }

    async fn snapshot(
        &self,
        device: &AuthenticatedDevice,
        work: &WorkRef,
        generation: Uuid,
    ) -> Result<WorkSnapshot, ControllerError> {
        self.authenticate(device, ControllerScope::HistoryRead)?;
        self.work_path(work, generation)?;
        let watermark = self.fence().await?;
        let record = self.visible_work(work, &watermark).await?;
        let pending_inputs = self.0.runs.pending_inputs(work.work_id).await?;
        let mut live_messages = Vec::new();
        for partial in self
            .0
            .runs
            .current_partials_at(work.work_id, watermark.revision)
            .await?
        {
            if let Some(item) = visible_message(&partial)? {
                live_messages.push(item);
            }
        }
        Ok(WorkSnapshot {
            live_messages,
            entry: self.entry(&record)?,
            pending_inputs,
            history_watermark: self.watermark(watermark.revision)?,
            work_cursor: self.new_token(
                device,
                watermark.revision,
                Position::WorkResume {
                    work_id: work.work_id,
                },
            )?,
        })
    }

    async fn history(
        &self,
        device: &AuthenticatedDevice,
        work: &WorkRef,
        generation: Uuid,
        page_token: Option<&str>,
    ) -> Result<HistoryPage, ControllerError> {
        self.authenticate(device, ControllerScope::HistoryRead)?;
        self.work_path(work, generation)?;
        let current = self.fence().await?;
        self.visible_work(work, &current).await?;
        let token = page_token
            .map(|token| self.decode(device, token))
            .transpose()?;
        let cursor = match &token {
            Some(Cursor {
                position:
                    Position::HistoryPage {
                        work_id,
                        after_timestamp,
                        after_row_id,
                    },
                generation,
                revision,
                ..
            }) if *work_id == work.work_id => Some(work_catalog::HistoryPageCursor {
                watermark: StoreWatermark {
                    generation: generation.to_string(),
                    revision: *revision,
                },
                work_id: work_id.to_string(),
                after_timestamp: *after_timestamp,
                after_row_id: *after_row_id,
            }),
            Some(_) => return Err(invalid_cursor()),
            None => None,
        };
        let page = self
            .0
            .catalog
            .history_page(
                &work.work_id.to_string(),
                &generation.to_string(),
                cursor.as_ref(),
                self.0.policy.page_size,
            )
            .await
            .map_err(store_error)?;
        let mut items = Vec::new();
        for record in &page.records {
            if let Some(item) = visible_message(record)? {
                items.push(item);
            }
        }
        let next_page_token = page
            .next
            .map(|next| {
                let position = Position::HistoryPage {
                    work_id: work.work_id,
                    after_timestamp: next.after_timestamp,
                    after_row_id: next.after_row_id,
                };
                let cursor = match &token {
                    Some(cursor) => {
                        let mut cursor = cursor.clone();
                        cursor.position = position;
                        cursor
                    }
                    None => self.0.signer.new_cursor(
                        device,
                        self.0.generation,
                        page.watermark.revision,
                        position,
                    )?,
                };
                self.0.signer.encode(&cursor)
            })
            .transpose()?;
        Ok(HistoryPage {
            work: work.clone(),
            store_generation: generation,
            revision: self.watermark(page.watermark.revision)?.revision,
            items,
            next_page_token,
            work_cursor: self.new_token(
                device,
                page.watermark.revision,
                Position::WorkResume {
                    work_id: work.work_id,
                },
            )?,
        })
    }

    async fn work_stream(
        &self,
        device: &AuthenticatedDevice,
        work: &WorkRef,
        cursor: &str,
    ) -> Result<BoxStream<'static, Result<WorkFrame, ControllerError>>, ControllerError> {
        self.authenticate(device, ControllerScope::HistoryRead)?;
        let cursor = self.decode(device, cursor)?;
        self.work_path(work, cursor.generation)?;
        if cursor.position
            != (Position::WorkResume {
                work_id: work.work_id,
            })
        {
            return Err(invalid_cursor());
        }
        let current = self.fence().await?;
        self.visible_work(work, &current).await?;
        let mut watermark = StoreWatermark {
            generation: cursor.generation.to_string(),
            revision: cursor.revision,
        };
        self.0
            .catalog
            .work_events(&work.work_id.to_string(), &watermark, 1)
            .await
            .map_err(store_error)?;
        let mut subscription = self
            .0
            .catalog
            .subscribe(self.0.policy.journal_poll_interval)
            .await
            .map_err(store_error)?;
        let backend = self.clone();
        let device = device.clone();
        let work = work.clone();
        Ok(buffered_stream(
            self.0.policy.stream_buffer,
            move |sender| {
                Box::pin(async move {
                    loop {
                        let current = backend.fence().await?;
                        let latest = backend
                            .0
                            .catalog
                            .lookup_at(&work.work_id.to_string(), &current)
                            .await
                            .map_err(store_error)?;
                        if !visible(&latest) {
                            send_frame(
                                &sender,
                                backend.frame(&device, &work, &latest, "tombstone", Value::Null)?,
                            )?;
                            return Ok(());
                        }
                        let events = backend
                            .0
                            .catalog
                            .work_events(
                                &work.work_id.to_string(),
                                &watermark,
                                backend.0.policy.page_size,
                            )
                            .await
                            .map_err(store_error)?;
                        if !events.is_empty() {
                            for event in events {
                                watermark.revision = event.revision;
                                if let Some(frame) =
                                    backend.work_event(&device, &work, event).await?
                                {
                                    let terminal = frame.kind == "tombstone";
                                    send_frame(&sender, frame)?;
                                    if terminal {
                                        return Ok(());
                                    }
                                }
                            }
                            continue;
                        }
                        watermark.revision = current.revision;
                        tokio::select! {
                            _ = sender.closed() => return Ok(()),
                            changed = subscription.changed() => { changed.map_err(store_error)?; }
                        }
                    }
                })
            },
        ))
    }

    async fn accept_command(
        &self,
        device: &AuthenticatedDevice,
        command: WorkCommand,
        dispatch_auth: DispatchAuthorization,
    ) -> Result<CommandReceipt, ControllerError> {
        self.authenticate(device, command.action.action().required_scope())?;
        self.work_path(&command.work, command.expected_store_generation)?;
        let watermark = self.fence().await?;
        self.visible_work(&command.work, &watermark).await?;
        self.0
            .runs
            .accept_command(device, command, dispatch_auth)
            .await
    }

    async fn receipt(
        &self,
        device: &AuthenticatedDevice,
        command_id: Uuid,
    ) -> Result<CommandReceipt, ControllerError> {
        self.authenticate(device, ControllerScope::CatalogRead)?;
        self.fence().await?;
        self.0.runs.receipt(device, command_id).await
    }

    async fn resources(
        &self,
        device: &AuthenticatedDevice,
    ) -> Result<ResourceSnapshot, ControllerError> {
        self.authenticate(device, ControllerScope::ResourcesRead)?;
        let sampler = self
            .0
            .resources
            .as_ref()
            .ok_or_else(|| unsupported("resource sampler is not installed"))?;
        let load = sampler().await?;
        let sampled_at = load.sampled_at.ok_or_else(|| {
            ControllerError::unavailable("resource sample has no measurement timestamp")
        })?;
        Ok(ResourceSnapshot { load, sampled_at })
    }
}

fn visible(record: &CatalogRecord) -> bool {
    !record.deleted
        && record.kind == "chat"
        && matches!(record.session_type.as_str(), "user" | "scheduled" | "acp")
}

fn visible_message(record: &HistoryRecord) -> Result<Option<HistoryItem>, ControllerError> {
    if !matches!(record.role.as_str(), "user" | "assistant") {
        return Ok(None);
    }
    let metadata = match &record.metadata_json {
        Some(metadata) => serde_json::from_str::<MessageMetadata>(metadata)
            .map_err(|_| ControllerError::unavailable("stored message metadata is invalid"))?,
        // Pre-metadata schema rows were public transcript messages. Malformed present metadata
        // is an error; it must never take this legacy absence path.
        None => MessageMetadata::default(),
    };
    if !metadata.user_visible || metadata.harness_nudge {
        return Ok(None);
    }
    let mut message: Message = serde_json::from_value(json!({
        "id": record.message_id, "role": record.role, "created": record.created_timestamp,
        "content": serde_json::from_str::<Value>(&record.content_json).map_err(|_| ControllerError::unavailable("stored message content is invalid"))?,
        "metadata": metadata,
    })).map_err(|_| ControllerError::unavailable("stored message is invalid"))?;
    message.content = message.content.iter().filter_map(|content| content.filter_for_audience(Role::User)).filter(|content| !matches!(content, MessageContent::SystemNotification(notification) if notification.notification_type == SystemNotificationType::ThinkingMessage)).collect();
    if message.content.is_empty() {
        return Ok(None);
    }
    let mut value = serde_json::to_value(&message)
        .map_err(|_| ControllerError::unavailable("message serialization failed"))?;
    value["metadata"] = json!({"userVisible": true, "agentVisible": metadata.agent_visible, "steer": metadata.steer});
    if let Some(content) = value.get_mut("content").and_then(Value::as_array_mut) {
        for item in content {
            if let Some(item) = item.as_object_mut() {
                item.remove("metadata");
                if item.get("type").and_then(Value::as_str) == Some("systemNotification") {
                    item.remove("data");
                }
            }
        }
    }
    strip_transport_metadata(&mut value);
    Ok(Some(HistoryItem {
        message_id: record.message_id.clone(),
        revision: record
            .revision
            .try_into()
            .map_err(|_| ControllerError::unavailable("negative message revision"))?,
        message: value,
    }))
}

fn strip_transport_metadata(value: &mut Value) {
    match value {
        Value::Object(object) => {
            object.remove("_meta");
            for value in object.values_mut() {
                strip_transport_metadata(value);
            }
        }
        Value::Array(values) => {
            for value in values {
                strip_transport_metadata(value);
            }
        }
        _ => {}
    }
}

fn buffered_stream<T: Send + 'static>(
    capacity: usize,
    produce: impl FnOnce(mpsc::Sender<T>) -> BoxFuture<'static, Result<(), ControllerError>>,
) -> BoxStream<'static, Result<T, ControllerError>> {
    let (sender, mut receiver) = mpsc::channel(capacity);
    let (done_sender, done_receiver) = oneshot::channel();
    let producer = produce(sender);
    tokio::spawn(async move {
        let _ = done_sender.send(producer.await);
    });
    Box::pin(async_stream::stream! {
        while let Some(frame) = receiver.recv().await { yield Ok(frame); }
        match done_receiver.await {
            Ok(Ok(())) => {}, Ok(Err(error)) => yield Err(error),
            Err(_) => yield Err(ControllerError::unavailable("controller stream producer stopped")),
        }
    })
}

fn send_frame<T>(sender: &mpsc::Sender<T>, frame: T) -> Result<(), ControllerError> {
    sender.try_send(frame).map_err(|error| match error {
        mpsc::error::TrySendError::Full(_) => ControllerError::new(
            StatusCode::GONE,
            "stream_gap",
            "viewer fell behind; resume from the last cursor or request a new snapshot",
            true,
        ),
        mpsc::error::TrySendError::Closed(_) => ControllerError::new(
            StatusCode::GONE,
            "observer_detached",
            "viewer disconnected",
            false,
        ),
    })
}

fn parse_uuid(value: &str) -> Result<Uuid, ControllerError> {
    Uuid::parse_str(value)
        .map_err(|_| ControllerError::unavailable("invalid persisted controller identity"))
}
fn not_found() -> ControllerError {
    ControllerError::new(
        StatusCode::NOT_FOUND,
        "work_not_found",
        "work is not available",
        false,
    )
}
fn unsupported(message: &str) -> ControllerError {
    ControllerError::new(
        StatusCode::UNPROCESSABLE_ENTITY,
        "unsupported_action",
        message,
        false,
    )
}
fn snapshot_required(message: &str) -> ControllerError {
    ControllerError::new(StatusCode::GONE, "snapshot_required", message, false)
}
fn store_error(error: anyhow::Error) -> ControllerError {
    match error.downcast_ref::<CatalogError>() {
        Some(CatalogError::GenerationMismatch) => snapshot_required("store generation changed"),
        Some(CatalogError::CursorAhead) => ControllerError::new(
            StatusCode::CONFLICT,
            "cursor_ahead",
            "cursor is ahead of this store",
            false,
        ),
        Some(CatalogError::RollbackDetected) => {
            ControllerError::unavailable("store revision rolled back")
        }
        Some(CatalogError::WorkNotFound) => not_found(),
        Some(CatalogError::InvalidCursor | CatalogError::InvalidPageSize) => invalid_cursor(),
        None => ControllerError::unavailable("controller catalog read failed"),
    }
}
