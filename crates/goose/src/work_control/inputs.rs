use super::{conflict, unavailable, OwnerRunRegistry};
use async_trait::async_trait;
use leanzero_link::controller::ControllerError;
use leanzero_link::controller_wire::{InputOption, PendingInput};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InputAnswer {
    pub option_id: String,
    pub value: Option<String>,
}

#[async_trait]
pub trait InputHandler: Send + Sync {
    fn run_id(&self) -> &str;
    fn observe(
        &self,
        cx: &agent_client_protocol::ConnectionTo<agent_client_protocol::Client>,
        registry: Arc<OwnerRunRegistry>,
        input: PendingInput,
    );
    fn validate(&self, answer: &InputAnswer) -> anyhow::Result<()>;
    async fn answer(&self, answer: InputAnswer) -> anyhow::Result<()>;
}

impl OwnerRunRegistry {
    pub async fn pending_inputs(
        &self,
        work_id: Uuid,
    ) -> Result<Vec<PendingInput>, ControllerError> {
        self.ensure_store().await.map_err(unavailable)?;
        let rows: Vec<String> = sqlx::query_scalar(
            "SELECT input_json FROM owner_inputs WHERE work_id=? AND state='open' ORDER BY rowid",
        )
        .bind(work_id.to_string())
        .fetch_all(self.pool().await.map_err(unavailable)?)
        .await
        .map_err(unavailable)?;
        rows.into_iter()
            .map(|row| serde_json::from_str(&row).map_err(unavailable))
            .collect()
    }

    pub async fn register_input(
        self: &Arc<Self>,
        session_id: &str,
        request_id: &str,
        prompt: String,
        options: Vec<InputOption>,
        handler: Arc<dyn InputHandler>,
    ) -> anyhow::Result<PendingInput> {
        self.ensure_store().await?;
        let runs = self.runs.lock().await;
        let run = runs
            .get(session_id)
            .ok_or_else(|| anyhow::anyhow!("no active owner run"))?;
        anyhow::ensure!(
            run.run_id == handler.run_id(),
            "input belongs to a stale run"
        );
        let input = PendingInput {
            input_id: Uuid::new_v4(),
            run_id: Uuid::parse_str(&run.run_id)?,
            prompt,
            options,
        };
        let mut tx = self.pool().await?.begin_with("BEGIN IMMEDIATE").await?;
        sqlx::query("INSERT INTO owner_inputs(input_id,work_id,session_id,generation,run_id,request_id,input_json,state) VALUES (?,?,?,?,?,?,?,'open')")
            .bind(input.input_id.to_string()).bind(run.work_id.to_string()).bind(session_id).bind(&run.generation)
            .bind(&run.run_id).bind(request_id).bind(serde_json::to_string(&input)?).execute(&mut *tx).await?;
        sqlx::query("UPDATE work_identity SET state='waiting_input',event_kind='pending_input' WHERE work_id=? AND deleted=0")
            .bind(run.work_id.to_string()).execute(&mut *tx).await?;
        tx.commit().await?;
        self.input_handlers
            .lock()
            .await
            .insert(input.input_id, handler.clone());
        drop(runs);
        for cx in self.input_observers(session_id) {
            handler.observe(&cx, self.clone(), input.clone());
        }
        Ok(input)
    }

    pub async fn answer_input(
        &self,
        input_id: Uuid,
        expected_run: Uuid,
        answer: InputAnswer,
    ) -> Result<(), ControllerError> {
        let runs = self.runs.lock().await;
        self.answer_input_locked(&runs, input_id, expected_run, answer)
            .await
    }

    pub(super) async fn answer_input_locked(
        &self,
        runs: &std::collections::HashMap<String, super::ActivePromptRun>,
        input_id: Uuid,
        expected_run: Uuid,
        answer: InputAnswer,
    ) -> Result<(), ControllerError> {
        let pool = self.pool().await.map_err(unavailable)?;
        let row: Option<(String,String,String,String,String,String)> = sqlx::query_as("SELECT session_id,work_id,generation,run_id,input_json,state FROM owner_inputs WHERE input_id=?")
            .bind(input_id.to_string()).fetch_optional(pool).await.map_err(unavailable)?;
        let (session_id, work_id, generation, run_id, json, state) =
            row.ok_or_else(|| conflict("input_not_found", "pending input no longer exists"))?;
        if state != "open" {
            return Err(conflict(
                "input_already_answered",
                "pending input has already been consumed or interrupted",
            ));
        }
        if run_id != expected_run.to_string()
            || runs.get(&session_id).is_none_or(|run| run.run_id != run_id)
        {
            return Err(conflict(
                "stale_run",
                "pending input belongs to a different run",
            ));
        }
        let current_generation: String =
            sqlx::query_scalar("SELECT generation FROM controller_store WHERE singleton=1")
                .fetch_one(pool)
                .await
                .map_err(unavailable)?;
        if current_generation != generation {
            return Err(conflict(
                "store_generation_mismatch",
                "pending input belongs to a replaced store",
            ));
        }
        let input: PendingInput = serde_json::from_str(&json).map_err(unavailable)?;
        let option = input
            .options
            .iter()
            .find(|option| option.option_id == answer.option_id)
            .ok_or_else(|| {
                conflict(
                    "invalid_input_option",
                    "this answer is not one of the current choices",
                )
            })?;
        if answer.value.is_some() && !option.accepts_value {
            return Err(conflict(
                "invalid_input_value",
                "this choice does not accept a value",
            ));
        }
        let handler = self
            .input_handlers
            .lock()
            .await
            .get(&input_id)
            .cloned()
            .ok_or_else(|| {
                conflict("input_interrupted", "original request is no longer running")
            })?;
        handler
            .validate(&answer)
            .map_err(|error| conflict("invalid_input_value", error.to_string()))?;
        let changed = sqlx::query("UPDATE owner_inputs SET state='dispatching',answer_json=? WHERE input_id=? AND state='open'")
            .bind(serde_json::to_string(&answer).map_err(unavailable)?).bind(input_id.to_string()).execute(pool).await.map_err(unavailable)?;
        if changed.rows_affected() != 1 {
            return Err(conflict(
                "input_already_answered",
                "another viewer answered this input",
            ));
        }
        let delivered = handler.answer(answer).await;
        let mut tx = pool
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(unavailable)?;
        sqlx::query("UPDATE owner_inputs SET state=? WHERE input_id=?")
            .bind(if delivered.is_ok() {
                "answered"
            } else {
                "outcome_unknown"
            })
            .bind(input_id.to_string())
            .execute(&mut *tx)
            .await
            .map_err(unavailable)?;
        sqlx::query("UPDATE work_identity SET state=CASE WHEN EXISTS(SELECT 1 FROM owner_inputs WHERE work_id=? AND state='open') THEN 'waiting_input' ELSE 'running' END,event_kind='pending_input' WHERE work_id=? AND current_run_id=? AND deleted=0")
            .bind(&work_id).bind(&work_id).bind(&run_id).execute(&mut *tx).await.map_err(unavailable)?;
        tx.commit().await.map_err(unavailable)?;
        self.input_handlers.lock().await.remove(&input_id);
        delivered.map_err(unavailable)
    }
}
