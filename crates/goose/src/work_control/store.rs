use super::{ActivePromptRun, OwnerRunRegistry};
use anyhow::Result;
use leanzero_link::controller_wire::{CommandReceipt, CommandState};
use sqlx::{Pool, Sqlite};

impl OwnerRunRegistry {
    pub(super) async fn pool(&self) -> Result<&Pool<Sqlite>> {
        self.session_manager.storage().pool().await
    }

    pub(crate) async fn ensure_store(&self) -> Result<()> {
        self.initialized.get_or_try_init(|| async {
            let pool = self.pool().await?;
            let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
            for statement in [
                "CREATE TABLE IF NOT EXISTS owner_commands (account_id TEXT NOT NULL, device_id TEXT NOT NULL, command_id TEXT NOT NULL, work_id TEXT NOT NULL, generation TEXT NOT NULL, session_id TEXT NOT NULL, command_json TEXT NOT NULL, receipt_json TEXT NOT NULL, state TEXT NOT NULL, message_id TEXT, run_id TEXT, PRIMARY KEY(account_id,device_id,command_id))",
                "CREATE TABLE IF NOT EXISTS owner_inputs (input_id TEXT PRIMARY KEY, work_id TEXT NOT NULL, session_id TEXT NOT NULL, generation TEXT NOT NULL, run_id TEXT NOT NULL, request_id TEXT NOT NULL, input_json TEXT NOT NULL, state TEXT NOT NULL, answer_json TEXT, UNIQUE(work_id,run_id,request_id))",
                "CREATE INDEX IF NOT EXISTS owner_inputs_work ON owner_inputs(work_id,state)",
                "CREATE TABLE IF NOT EXISTS owner_preflights (setup_id TEXT PRIMARY KEY, session_id TEXT NOT NULL, work_id TEXT NOT NULL, generation TEXT NOT NULL, state TEXT NOT NULL, input_id TEXT UNIQUE, input_json TEXT, answer_json TEXT)",
                "CREATE INDEX IF NOT EXISTS owner_preflights_work ON owner_preflights(work_id,state)",
                "CREATE TABLE IF NOT EXISTS owner_partial_events (revision INTEGER PRIMARY KEY, work_id TEXT NOT NULL, run_id TEXT NOT NULL, message_id TEXT NOT NULL, message_json TEXT NOT NULL, offset_bytes INTEGER NOT NULL, text_bytes INTEGER NOT NULL)",
                "CREATE INDEX IF NOT EXISTS owner_partial_by_message ON owner_partial_events(work_id,run_id,message_id,revision)",
                "CREATE TABLE IF NOT EXISTS owner_partial_clears (revision INTEGER PRIMARY KEY, work_id TEXT NOT NULL, run_id TEXT NOT NULL, message_ids_json TEXT NOT NULL)",
                "CREATE INDEX IF NOT EXISTS owner_partial_clear_work ON owner_partial_clears(work_id,run_id,revision)",
                "CREATE TABLE IF NOT EXISTS owner_run_completions (work_id TEXT NOT NULL, generation TEXT NOT NULL, run_id TEXT PRIMARY KEY, completed_at TEXT NOT NULL)",
            ] { sqlx::query(statement).execute(&mut *tx).await?; }
            // A process restart has no authority to replay a queued action and no live reply to
            // consume an approval. Message identity is the sole proof a send reached the agent.
            let interrupted: Vec<(String,String,String,String,Option<String>)> = sqlx::query_as(
                "SELECT account_id, device_id, command_id, receipt_json, message_id FROM owner_commands WHERE state IN ('accepted','waiting','dispatching')")
                .fetch_all(&mut *tx).await?;
            for (account, device, id, json, message_id) in interrupted {
                let mut receipt: CommandReceipt = serde_json::from_str(&json)?;
                let persisted: bool = if let Some(message_id) = message_id {
                    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM messages m JOIN owner_commands c ON c.session_id = m.session_id WHERE c.account_id=? AND c.device_id=? AND c.command_id=? AND m.message_id=?)")
                        .bind(&account).bind(&device).bind(&id).bind(message_id).fetch_one(&mut *tx).await?
                } else { false };
                receipt.state = if persisted { CommandState::Delivered } else { CommandState::OutcomeUnknown };
                receipt.reason = Some(if persisted { "reconciled_persisted_message" } else { "owner_restarted_without_delivery_proof" }.into());
                receipt.updated_at = chrono::Utc::now();
                sqlx::query("UPDATE owner_commands SET state=?, receipt_json=? WHERE account_id=? AND device_id=? AND command_id=?")
                    .bind(if persisted { "delivered" } else { "outcome_unknown" }).bind(serde_json::to_string(&receipt)?)
                    .bind(account).bind(device).bind(id).execute(&mut *tx).await?;
            }
            sqlx::query("UPDATE owner_inputs SET state='interrupted' WHERE state IN ('open','dispatching')").execute(&mut *tx).await?;
            sqlx::query("UPDATE work_identity SET state='interrupted', current_run_id=NULL, event_kind='owner_restarted' WHERE deleted=0 AND current_run_id IS NOT NULL").execute(&mut *tx).await?;
            sqlx::query("UPDATE owner_preflights SET state='interrupted' WHERE state IN ('starting','waiting_input','dispatching','answered')").execute(&mut *tx).await?;
            sqlx::query("UPDATE work_identity SET state='interrupted',event_kind='owner_restarted' WHERE deleted=0 AND work_id IN (SELECT work_id FROM owner_preflights WHERE state='interrupted')").execute(&mut *tx).await?;
            let setups: Vec<(String,String,String)> = sqlx::query_as("SELECT p.session_id,p.setup_id,p.work_id FROM owner_preflights p JOIN work_identity w ON w.work_id=p.work_id WHERE p.state='interrupted' AND w.deleted=0")
                .fetch_all(&mut *tx).await?;
            tx.commit().await?;
            let mut preflights=self.preflights.write().expect("owner preflights poisoned");
            for (session_id,setup_id,work_id) in setups {
                preflights.insert(session_id,super::preflights::Preflight { setup_id:uuid::Uuid::parse_str(&setup_id)?,work_id:uuid::Uuid::parse_str(&work_id)?,phase:super::PreflightPhase::Interrupted });
            }
            Ok::<(), anyhow::Error>(())
        }).await?;
        Ok(())
    }

    pub(super) async fn finish_store(&self, run: &ActivePromptRun) -> Result<()> {
        let mut tx = self.pool().await?.begin_with("BEGIN IMMEDIATE").await?;
        sqlx::query("INSERT OR IGNORE INTO owner_run_completions(work_id,generation,run_id,completed_at) VALUES (?,?,?,?)")
            .bind(run.work_id.to_string()).bind(&run.generation).bind(&run.run_id).bind(chrono::Utc::now().to_rfc3339()).execute(&mut *tx).await?;
        sqlx::query("UPDATE owner_inputs SET state='interrupted' WHERE work_id=? AND run_id=? AND state='open'")
            .bind(run.work_id.to_string()).bind(&run.run_id).execute(&mut *tx).await?;
        sqlx::query("UPDATE work_identity SET state=CASE WHEN EXISTS(SELECT 1 FROM sessions s, json_each(s.extension_data, '$.\"needs_you.v0\".items') item WHERE s.id=work_identity.local_locator AND json_extract(item.value,'$.status')='open') THEN 'waiting_input' ELSE 'idle' END, current_run_id=NULL, event_kind='pending_input' WHERE work_id=? AND current_run_id=? AND deleted=0 AND (SELECT generation FROM controller_store WHERE singleton=1)=?")
            .bind(run.work_id.to_string()).bind(&run.run_id).bind(&run.generation).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }
}
