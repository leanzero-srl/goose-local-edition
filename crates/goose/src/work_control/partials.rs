use super::{unavailable, OwnerRunRegistry};
use crate::conversation::message::{Message, MessageContent};
use crate::conversation::Conversation;
use crate::session::work_catalog::HistoryRecord;
use leanzero_link::controller::ControllerError;
use rmcp::model::Role;
use uuid::Uuid;

impl OwnerRunRegistry {
    pub(crate) async fn record_stream_message(
        &self,
        session_id: &str,
        run_id: &str,
        message: &Message,
    ) -> anyhow::Result<()> {
        if message.role != Role::Assistant
            || !message.is_user_visible()
            || message.metadata.harness_nudge
        {
            return Ok(());
        }
        let Some(message_id) = &message.id else {
            return Ok(());
        };
        let mut visible = message.clone();
        visible.content = visible
            .content
            .iter()
            .filter_map(|content| content.filter_for_audience(Role::User))
            .collect();
        if visible.content.is_empty() {
            return Ok(());
        }
        let runs = self.runs.lock().await;
        let run = runs
            .get(session_id)
            .ok_or_else(|| anyhow::anyhow!("stream event has no owner run"))?;
        anyhow::ensure!(run.run_id == run_id, "stream event belongs to an old run");
        let mut tx = self.pool().await?.begin_with("BEGIN IMMEDIATE").await?;
        let offset: i64 = sqlx::query_scalar("SELECT COALESCE((SELECT offset_bytes+text_bytes FROM owner_partial_events WHERE work_id=? AND run_id=? AND message_id=? AND revision>COALESCE((SELECT MAX(revision) FROM owner_partial_clears WHERE work_id=? AND run_id=?),0) ORDER BY revision DESC LIMIT 1),0)")
            .bind(run.work_id.to_string()).bind(run_id).bind(message_id).bind(run.work_id.to_string()).bind(run_id)
            .fetch_one(&mut *tx).await?;
        let text_bytes = visible
            .content
            .iter()
            .map(|content| match content {
                MessageContent::Text(text) => text.text.len(),
                _ => 0,
            })
            .sum::<usize>();
        sqlx::query("UPDATE work_identity SET event_kind='partial_replacement' WHERE work_id=? AND deleted=0 AND current_run_id=?")
            .bind(run.work_id.to_string()).bind(run_id).execute(&mut *tx).await?;
        let revision: i64 =
            sqlx::query_scalar("SELECT revision FROM work_identity WHERE work_id=? AND deleted=0")
                .bind(run.work_id.to_string())
                .fetch_one(&mut *tx)
                .await?;
        sqlx::query("INSERT INTO owner_partial_events(revision,work_id,run_id,message_id,message_json,offset_bytes,text_bytes) VALUES (?,?,?,?,?,?,?)")
            .bind(revision).bind(run.work_id.to_string()).bind(run_id).bind(message_id).bind(serde_json::to_string(&visible)?)
            .bind(offset).bind(i64::try_from(text_bytes)?).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }

    pub(crate) async fn clear_partials(
        &self,
        session_id: &str,
        run_id: &str,
    ) -> anyhow::Result<()> {
        let catalog = self.session_manager.work_catalog().await?;
        let work = catalog.lookup_local_session(session_id).await?;
        let mut tx = self.pool().await?.begin_with("BEGIN IMMEDIATE").await?;
        let ids: Vec<String> = sqlx::query_scalar("SELECT DISTINCT p.message_id FROM owner_partial_events p WHERE p.work_id=? AND p.run_id=? AND p.revision>COALESCE((SELECT MAX(revision) FROM owner_partial_clears WHERE work_id=p.work_id AND run_id=p.run_id),0) AND NOT EXISTS(SELECT 1 FROM messages m WHERE m.session_id=? AND m.message_id=p.message_id)")
            .bind(&work.work_id).bind(run_id).bind(session_id).fetch_all(&mut *tx).await?;
        if !ids.is_empty() {
            sqlx::query("UPDATE work_identity SET event_kind='partial_discarded' WHERE work_id=? AND deleted=0")
                .bind(&work.work_id).execute(&mut *tx).await?;
            let revision: i64 =
                sqlx::query_scalar("SELECT revision FROM work_identity WHERE work_id=?")
                    .bind(&work.work_id)
                    .fetch_one(&mut *tx)
                    .await?;
            sqlx::query("INSERT INTO owner_partial_clears(revision,work_id,run_id,message_ids_json) VALUES (?,?,?,?)")
                .bind(revision).bind(&work.work_id).bind(run_id).bind(serde_json::to_string(&ids)?).execute(&mut *tx).await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn partial_at(
        &self,
        work_id: Uuid,
        revision: i64,
    ) -> Result<Option<HistoryRecord>, ControllerError> {
        let json: Option<String> = sqlx::query_scalar(
            "SELECT message_json FROM owner_partial_events WHERE work_id=? AND revision=?",
        )
        .bind(work_id.to_string())
        .bind(revision)
        .fetch_optional(self.pool().await.map_err(unavailable)?)
        .await
        .map_err(unavailable)?;
        json.map(|json| record(serde_json::from_str(&json).map_err(unavailable)?, revision))
            .transpose()
    }

    pub async fn partial_offset_at(
        &self,
        work_id: Uuid,
        revision: i64,
    ) -> Result<u64, ControllerError> {
        let offset: i64 = sqlx::query_scalar(
            "SELECT offset_bytes FROM owner_partial_events WHERE work_id=? AND revision=?",
        )
        .bind(work_id.to_string())
        .bind(revision)
        .fetch_one(self.pool().await.map_err(unavailable)?)
        .await
        .map_err(unavailable)?;
        u64::try_from(offset).map_err(unavailable)
    }

    pub async fn partial_cleared_at(
        &self,
        work_id: Uuid,
        revision: i64,
    ) -> Result<Vec<String>, ControllerError> {
        let json: String = sqlx::query_scalar(
            "SELECT message_ids_json FROM owner_partial_clears WHERE work_id=? AND revision=?",
        )
        .bind(work_id.to_string())
        .bind(revision)
        .fetch_one(self.pool().await.map_err(unavailable)?)
        .await
        .map_err(unavailable)?;
        serde_json::from_str(&json).map_err(unavailable)
    }

    pub async fn current_partials_at(
        &self,
        work_id: Uuid,
        watermark: i64,
    ) -> Result<Vec<HistoryRecord>, ControllerError> {
        let rows: Vec<(i64,String)> = sqlx::query_as("SELECT p.revision,p.message_json FROM owner_partial_events p WHERE p.work_id=? AND p.revision<=? AND NOT EXISTS(SELECT 1 FROM owner_partial_clears c WHERE c.work_id=p.work_id AND c.run_id=p.run_id AND c.revision>=p.revision AND c.revision<=?) AND NOT EXISTS(SELECT 1 FROM controller_message_rows m WHERE m.work_id=p.work_id AND m.message_id=p.message_id AND m.valid_from<=?) AND EXISTS(SELECT 1 FROM catalog_versions v WHERE v.work_id=p.work_id AND v.revision=(SELECT MAX(revision) FROM catalog_versions WHERE work_id=p.work_id AND revision<=?) AND v.current_run_id=p.run_id) ORDER BY p.revision")
            .bind(work_id.to_string()).bind(watermark).bind(watermark).bind(watermark).bind(watermark)
            .fetch_all(self.pool().await.map_err(unavailable)?).await.map_err(unavailable)?;
        let mut messages = std::collections::BTreeMap::<String, (i64, Conversation)>::new();
        for (revision, json) in rows {
            let message: Message = serde_json::from_str(&json).map_err(unavailable)?;
            let id = message
                .id
                .clone()
                .ok_or_else(|| unavailable("partial has no message identity"))?;
            let item = messages
                .entry(id)
                .or_insert_with(|| (revision, Conversation::empty()));
            item.0 = revision;
            item.1.push(message);
        }
        messages
            .into_values()
            .map(|(revision, messages)| {
                record(
                    messages
                        .last()
                        .ok_or_else(|| unavailable("empty partial reconstruction"))?
                        .clone(),
                    revision,
                )
            })
            .collect()
    }
}

fn record(message: Message, revision: i64) -> Result<HistoryRecord, ControllerError> {
    Ok(HistoryRecord {
        row_id: revision,
        message_id: message
            .id
            .ok_or_else(|| unavailable("partial has no message identity"))?,
        revision,
        role: match message.role {
            Role::User => "user",
            Role::Assistant => "assistant",
        }
        .into(),
        content_json: serde_json::to_string(&message.content).map_err(unavailable)?,
        created_timestamp: message.created,
        metadata_json: Some(serde_json::to_string(&message.metadata).map_err(unavailable)?),
    })
}
