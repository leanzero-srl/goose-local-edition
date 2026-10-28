use super::{transport::OwnerTransport, ClientError, ClientResult, DesktopController};
use crate::{controller_wire::*, peer_dial::MeshProxy};
use chrono::{DateTime, Utc};
use reqwest::Method;
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, time::Duration};
use uuid::Uuid;

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct FleetCatalog {
    pub hosts: Vec<FleetHost>,
    pub updated_at: Option<DateTime<Utc>>,
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct FleetHost {
    pub host: HostDescriptor,
    pub entries: Vec<CatalogEntry>,
    pub stale: bool,
    pub error: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct PendingCommand {
    pub command: WorkCommand,
    pub receipt: Option<CommandReceipt>,
    pub device_id: Uuid,
}

impl DesktopController {
    pub async fn catalog(&self) -> FleetCatalog {
        self.cache.read().await.clone()
    }

    pub async fn sync(&self, proxy: Option<MeshProxy>) -> ClientResult<FleetCatalog> {
        let _guard = self.sync_lock.lock().await;
        let result = self.sync_inner(proxy).await;
        if let Err(error) = &result {
            self.record_error(error).await;
            *self.verified_at.write().await = None;
            for host in &mut self.cache.write().await.hosts {
                host.stale = true;
                host.error = Some(error.message.clone());
            }
        }
        result
    }

    async fn sync_inner(&self, proxy: Option<MeshProxy>) -> ClientResult<FleetCatalog> {
        let credentials = self.credentials().await?;
        let registered = self
            .worker
            .controller_hosts(&credentials.access_token)
            .await?;
        let mut owners = HashSet::new();
        if registered
            .hosts
            .iter()
            .any(|h| !owners.insert(h.owner_node_id) || h.device_role != DeviceRole::Host)
        {
            return Err(ClientError::invalid(
                "Authority returned duplicate or non-host owners",
            ));
        }
        let previous = self.cache.read().await.clone();
        let cursors = self.cursors.lock().await.clone();
        let results = futures::future::join_all(registered.hosts.into_iter().map(|host| {
            let old = previous
                .hosts
                .iter()
                .find(|entry| entry.host.owner_node_id == host.owner_node_id)
                .cloned();
            let cursor = cursors.get(&host.owner_node_id).cloned();
            let token = &credentials.access_token;
            async move {
                let mut entry = FleetHost {
                    host,
                    entries: old.map(|old| old.entries).unwrap_or_default(),
                    stale: true,
                    error: None,
                };
                let refreshed = async {
                    let proxy = proxy.ok_or_else(|| {
                        ClientError::unavailable("Connect this Mac to Link to reach its owners")
                    })?;
                    let transport = OwnerTransport::new(&entry.host, proxy)?;
                    if let Some(cursor) = cursor {
                        match update_catalog(
                            &transport,
                            &entry.host,
                            token,
                            entry.entries.clone(),
                            cursor,
                        )
                        .await
                        {
                            Err(error)
                                if error.status == 410 || error.code == "snapshot_required" =>
                            {
                                snapshot_catalog(&transport, &entry.host, token).await
                            }
                            result => result,
                        }
                    } else {
                        snapshot_catalog(&transport, &entry.host, token).await
                    }
                }
                .await;
                match refreshed {
                    Ok((entries, cursor)) => {
                        entry.entries = entries;
                        entry.stale = false;
                        (entry, Some(cursor))
                    }
                    Err(error) => {
                        entry.error = Some(error.message);
                        (entry, None)
                    }
                }
            }
        }))
        .await;
        let mut next_cursors = self.cursors.lock().await;
        next_cursors.retain(|owner, _| owners.contains(owner));
        let mut hosts = Vec::new();
        for (entry, cursor) in results {
            if let Some(cursor) = cursor {
                next_cursors.insert(entry.host.owner_node_id, cursor);
            }
            hosts.push(entry);
        }
        let result = FleetCatalog {
            hosts,
            updated_at: Some(Utc::now()),
        };
        // Refresh time is live freshness, not a durable catalog change.
        if previous.updated_at.is_none() || previous.hosts != result.hosts {
            crate::controller_host::save_json(&self.path.with_file_name("catalog.json"), &result)?;
        }
        *self.cache.write().await = result.clone();
        *self.last_error.write().await = None;
        *self.verified_at.write().await = proxy.map(|_| tokio::time::Instant::now());
        Ok(result)
    }

    async fn transport(
        &self,
        owner: Uuid,
        scope: ControllerScope,
        proxy: Option<MeshProxy>,
    ) -> ClientResult<(OwnerTransport, DeviceCredentials)> {
        let credentials = self.credentials().await?;
        if !credentials.device.scopes.contains(&scope) {
            return Err(ClientError::invalid(
                "This desktop device lacks the requested scope",
            ));
        }
        if self
            .verified_at
            .read()
            .await
            .is_none_or(|t| t.elapsed() >= Duration::from_secs(30))
        {
            return Err(ClientError::unavailable(
                "Refresh the verified host catalog before contacting this owner",
            ));
        }
        let cache = self.cache.read().await;
        let host = cache
            .hosts
            .iter()
            .find(|h| h.host.owner_node_id == owner)
            .ok_or_else(|| ClientError::invalid("The owner is not registered to this account"))?;
        if !host.host.capabilities.contains(&scope) {
            return Err(ClientError::unavailable(
                "The original Mac does not currently allow this operation",
            ));
        }
        Ok((
            OwnerTransport::new(
                &host.host,
                proxy.ok_or_else(|| {
                    ClientError::unavailable("The owned Link mesh is disconnected")
                })?,
            )?,
            credentials,
        ))
    }

    pub async fn open_work(
        &self,
        work: &WorkRef,
        generation: Uuid,
        proxy: Option<MeshProxy>,
    ) -> ClientResult<WorkSnapshot> {
        let (transport, credentials) = self
            .transport(work.owner_node_id, ControllerScope::HistoryRead, proxy)
            .await?;
        let snapshot: WorkSnapshot = transport
            .request(
                Method::GET,
                &work_path(work),
                &[("store_generation", generation.to_string())],
                &credentials.access_token,
                None,
            )
            .await?;
        if snapshot.entry.work != *work
            || snapshot.entry.store_generation != generation
            || snapshot.history_watermark.owner_node_id != work.owner_node_id
            || snapshot.history_watermark.store_generation != generation
            || snapshot.work_cursor.is_empty()
        {
            return Err(ClientError::invalid(
                "Owner snapshot changed work identity or generation",
            ));
        }
        Ok(snapshot)
    }

    pub async fn history(
        &self,
        work: &WorkRef,
        generation: Uuid,
        page_token: Option<&str>,
        proxy: Option<MeshProxy>,
    ) -> ClientResult<HistoryPage> {
        let (transport, credentials) = self
            .transport(work.owner_node_id, ControllerScope::HistoryRead, proxy)
            .await?;
        let mut query = vec![("store_generation", generation.to_string())];
        if let Some(token) = page_token {
            query.push(("page_token", token.to_owned()));
        }
        let page: HistoryPage = transport
            .request(
                Method::GET,
                &format!("{}/history", work_path(work)),
                &query,
                &credentials.access_token,
                None,
            )
            .await?;
        if page.work != *work || page.store_generation != generation || page.work_cursor.is_empty()
        {
            return Err(ClientError::invalid(
                "Owner history changed work identity or generation",
            ));
        }
        Ok(page)
    }

    pub async fn work_changes(
        &self,
        work: &WorkRef,
        generation: Uuid,
        cursor: &str,
        proxy: Option<MeshProxy>,
    ) -> ClientResult<WorkChanges> {
        let (transport, credentials) = self
            .transport(work.owner_node_id, ControllerScope::HistoryRead, proxy)
            .await?;
        let result: WorkChanges = transport
            .request(
                Method::GET,
                &format!("{}/changes", work_path(work)),
                &[
                    ("store_generation", generation.to_string()),
                    ("cursor", cursor.to_owned()),
                ],
                &credentials.access_token,
                None,
            )
            .await?;
        if result.resume_cursor.is_empty()
            || result.frames.iter().any(|f| {
                f.work != *work
                    || f.store_generation != generation
                    || f.stream_epoch != generation
                    || f.resume_cursor.is_empty()
            })
        {
            return Err(ClientError::invalid(
                "Owner replay changed selected work identity or generation",
            ));
        }
        Ok(result)
    }

    pub async fn command(
        &self,
        command: WorkCommand,
        proxy: Option<MeshProxy>,
    ) -> ClientResult<CommandReceipt> {
        let (transport, credentials) = self
            .transport(
                command.work.owner_node_id,
                command.action.action().required_scope(),
                proxy,
            )
            .await?;
        {
            let mut outbox = self.outbox.lock().await;
            let mut next = outbox.clone();
            if let Some(old) = next.iter().find(|old| {
                old.device_id == credentials.device.device_id
                    && old.command.command_id == command.command_id
            }) {
                if old.command != command {
                    return Err(ClientError::invalid(
                        "This command ID already has different contents",
                    ));
                }
            } else {
                next.push(PendingCommand {
                    command: command.clone(),
                    receipt: None,
                    device_id: credentials.device.device_id,
                });
            }
            crate::controller_host::save_json(&self.outbox_path, &next)?;
            *outbox = next;
        }
        let result: ClientResult<CommandReceipt> = transport
            .request(
                Method::POST,
                &format!("{}/commands", work_path(&command.work)),
                &[],
                &credentials.access_token,
                Some(
                    &serde_json::to_value(&command)
                        .map_err(|_| ClientError::invalid("Invalid typed command"))?,
                ),
            )
            .await;
        let receipt = match result {
            Err(error) if error.status == 409 && error.code == "stale_revision" => {
                // The owner checks existing receipts before revision and rejects stale
                // new commands before admission. Other failures cannot prove non-delivery.
                self.remove_unaccepted_command(&command, credentials.device.device_id)
                    .await?;
                return Err(error);
            }
            result => result?,
        };
        if receipt.command_id != command.command_id
            || receipt.work != command.work
            || receipt.authenticated_device_id != credentials.device.device_id
        {
            return Err(ClientError::invalid(
                "Owner command receipt changed identity",
            ));
        }
        self.save_receipt(receipt.clone()).await?;
        Ok(receipt)
    }

    async fn remove_unaccepted_command(
        &self,
        command: &WorkCommand,
        device_id: Uuid,
    ) -> ClientResult<()> {
        let mut outbox = self.outbox.lock().await;
        let mut next = outbox.clone();
        let index = next
            .iter()
            .position(|entry| entry.device_id == device_id && entry.command == *command)
            .filter(|index| next[*index].receipt.is_none())
            .ok_or_else(|| {
                ClientError::invalid("Cannot discard a command with an existing receipt")
            })?;
        next.remove(index);
        crate::controller_host::save_json(&self.outbox_path, &next)?;
        *outbox = next;
        Ok(())
    }

    pub async fn receipt(
        &self,
        owner: Uuid,
        command_id: Uuid,
        proxy: Option<MeshProxy>,
    ) -> ClientResult<CommandReceipt> {
        let (transport, credentials) = self
            .transport(owner, ControllerScope::CatalogRead, proxy)
            .await?;
        let receipt: CommandReceipt = transport
            .request(
                Method::GET,
                &format!("commands/{command_id}"),
                &[],
                &credentials.access_token,
                None,
            )
            .await?;
        if receipt.command_id != command_id
            || receipt.work.owner_node_id != owner
            || receipt.authenticated_device_id != credentials.device.device_id
        {
            return Err(ClientError::invalid(
                "Owner receipt changed command or device identity",
            ));
        }
        self.save_receipt(receipt.clone()).await?;
        Ok(receipt)
    }

    async fn save_receipt(&self, receipt: CommandReceipt) -> ClientResult<()> {
        let mut outbox = self.outbox.lock().await;
        if let Some(entry) = outbox.iter_mut().find(|p| {
            p.command.command_id == receipt.command_id
                && p.device_id == receipt.authenticated_device_id
        }) {
            entry.receipt = Some(receipt);
            crate::controller_host::save_json(&self.outbox_path, &*outbox)?;
        }
        Ok(())
    }

    pub async fn pending_commands(&self, work: &WorkRef) -> ClientResult<Vec<PendingCommand>> {
        let credentials = self.credentials().await?;
        Ok(self
            .outbox
            .lock()
            .await
            .iter()
            .filter(|p| {
                p.command.work == *work
                    && p.device_id == credentials.device.device_id
                    && p.receipt.as_ref().is_none_or(|r| {
                        matches!(
                            r.state,
                            CommandState::Accepted
                                | CommandState::Waiting
                                | CommandState::Dispatching
                                | CommandState::OutcomeUnknown
                        )
                    })
            })
            .cloned()
            .collect())
    }
}

fn work_path(work: &WorkRef) -> String {
    let kind = match work.kind {
        WorkKind::Chat => "chat",
        WorkKind::AgentWork => "agent_work",
        WorkKind::Benchmark => "benchmark",
        WorkKind::ShellJob => "shell_job",
    };
    format!("work/{kind}/{}", work.work_id)
}

async fn snapshot_catalog(
    transport: &OwnerTransport,
    host: &HostDescriptor,
    token: &str,
) -> ClientResult<(Vec<CatalogEntry>, String)> {
    let mut entries = Vec::new();
    let mut page_token = None;
    let mut watermark = None;
    let mut seen = HashSet::new();
    loop {
        let query = page_token
            .take()
            .map(|p| vec![("page_token", p)])
            .unwrap_or_default();
        let page: CatalogPage = transport
            .request(Method::GET, "catalog", &query, token, None)
            .await?;
        if page.watermark.owner_node_id != host.owner_node_id
            || watermark.as_ref().is_some_and(|w| w != &page.watermark)
            || page.resume_cursor.is_empty()
            || page.entries.iter().any(|e| {
                e.work.owner_node_id != host.owner_node_id
                    || e.store_generation != page.watermark.store_generation
                    || e.revision > page.watermark.revision
            })
        {
            return Err(ClientError::invalid(
                "Catalog snapshot changed its owner or signed watermark",
            ));
        }
        watermark = Some(page.watermark);
        entries.extend(page.entries);
        match page.next_page_token {
            None => return Ok((entries, page.resume_cursor)),
            Some(next) if seen.insert(next.clone()) => page_token = Some(next),
            Some(_) => return Err(ClientError::invalid("Owner repeated a catalog page token")),
        }
    }
}

async fn update_catalog(
    transport: &OwnerTransport,
    host: &HostDescriptor,
    token: &str,
    mut entries: Vec<CatalogEntry>,
    mut cursor: String,
) -> ClientResult<(Vec<CatalogEntry>, String)> {
    let mut seen = HashSet::new();
    loop {
        if !seen.insert(cursor.clone()) {
            return Err(ClientError::invalid(
                "Owner repeated a catalog replay token",
            ));
        }
        let page: CatalogChanges = transport
            .request(
                Method::GET,
                "catalog/changes",
                &[("cursor", cursor)],
                token,
                None,
            )
            .await?;
        if page.cursor.owner_node_id != host.owner_node_id || page.resume_cursor.is_empty() {
            return Err(ClientError::invalid("Catalog replay changed owner"));
        }
        for change in page.changes {
            if change.work.owner_node_id != host.owner_node_id
                || change.cursor.owner_node_id != host.owner_node_id
                || change.cursor.store_generation != page.cursor.store_generation
            {
                return Err(ClientError::invalid(
                    "Catalog change belongs to another owner or generation",
                ));
            }
            entries.retain(|entry| entry.work != change.work);
            if let Some(entry) = change.entry {
                if entry.work != change.work
                    || entry.store_generation != change.cursor.store_generation
                    || entry.revision != change.cursor.revision
                {
                    return Err(ClientError::invalid(
                        "Catalog change has a mismatched entry",
                    ));
                }
                entries.push(entry);
            } else if !matches!(change.kind, CatalogChangeKind::Tombstone) {
                return Err(ClientError::invalid("Catalog upsert has no entry"));
            }
        }
        cursor = page.resume_cursor;
        if !page.has_more {
            return Ok((entries, cursor));
        }
    }
}
