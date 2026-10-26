use super::{conflict, unavailable, InputAnswer, InputHandler, OwnerRunRegistry};
use leanzero_link::controller::ControllerError;
use leanzero_link::controller_wire::{InputOption, PendingInput, PendingInputPayload};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum PreflightPhase {
    Starting,
    WaitingInput,
    Dispatching,
    Interrupted,
}

pub(super) struct Preflight {
    pub setup_id: Uuid,
    pub work_id: Uuid,
    pub phase: PreflightPhase,
}

pub(crate) struct SetupTicket {
    registry: Arc<OwnerRunRegistry>,
    session_id: String,
    setup_id: Uuid,
    finished: bool,
}

impl SetupTicket {
    pub async fn finish(mut self, success: bool) -> anyhow::Result<()> {
        self.registry
            .finish_preflight(
                &self.session_id,
                self.setup_id,
                if success { "ready" } else { "failed" },
            )
            .await?;
        self.finished = true;
        Ok(())
    }
}

impl Drop for SetupTicket {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        let registry = self.registry.clone();
        let session_id = self.session_id.clone();
        let setup_id = self.setup_id;
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                if let Err(error) = registry
                    .finish_preflight(&session_id, setup_id, "interrupted")
                    .await
                {
                    tracing::error!(%error, "interrupted owner setup could not be persisted");
                }
            });
        }
    }
}

impl OwnerRunRegistry {
    pub(crate) fn preflight_state(&self, session_id: &str) -> Option<PreflightPhase> {
        self.preflights
            .read()
            .expect("owner preflights poisoned")
            .get(session_id)
            .map(|setup| setup.phase)
    }

    pub(crate) async fn begin_preflight(
        self: &Arc<Self>,
        session_id: &str,
    ) -> anyhow::Result<SetupTicket> {
        self.ensure_store().await?;
        let runs = self.runs.lock().await;
        anyhow::ensure!(
            !runs.contains_key(session_id),
            "session already has an owner run"
        );
        let catalog = self.session_manager.work_catalog().await?;
        let work = catalog.lookup_local_session(session_id).await?;
        let generation = catalog.watermark().await?.generation;
        let setup_id = Uuid::new_v4();
        let work_id = Uuid::parse_str(&work.work_id)?;
        anyhow::ensure!(
            self.preflights
                .read()
                .expect("owner preflights poisoned")
                .get(session_id)
                .is_none_or(|setup| setup.work_id != work_id),
            "session setup is already pending"
        );
        let mut tx = self.pool().await?.begin_with("BEGIN IMMEDIATE").await?;
        sqlx::query("INSERT INTO owner_preflights(setup_id,session_id,work_id,generation,state) VALUES (?,?,?,?,'starting')")
            .bind(setup_id.to_string()).bind(session_id).bind(work.work_id).bind(generation).execute(&mut *tx).await?;
        sqlx::query("UPDATE work_identity SET state='waiting_input',event_kind='pending_input' WHERE work_id=? AND deleted=0")
            .bind(work_id.to_string()).execute(&mut *tx).await?;
        tx.commit().await?;
        self.preflights
            .write()
            .expect("owner preflights poisoned")
            .insert(
                session_id.to_owned(),
                Preflight {
                    setup_id,
                    work_id,
                    phase: PreflightPhase::Starting,
                },
            );
        Ok(SetupTicket {
            registry: self.clone(),
            session_id: session_id.to_owned(),
            setup_id,
            finished: false,
        })
    }

    pub(crate) async fn register_preflight_input(
        self: &Arc<Self>,
        session_id: &str,
        prompt: String,
        schema: serde_json::Value,
        handler: Arc<dyn InputHandler>,
    ) -> anyhow::Result<PendingInput> {
        anyhow::ensure!(
            handler.run_id().is_none(),
            "recipe setup is not a model run"
        );
        let runs = self.runs.lock().await;
        let setup_id = self
            .preflights
            .read()
            .expect("owner preflights poisoned")
            .get(session_id)
            .filter(|setup| setup.phase == PreflightPhase::Starting)
            .map(|setup| setup.setup_id)
            .ok_or_else(|| anyhow::anyhow!("original recipe setup is not waiting for input"))?;
        let input = PendingInput {
            input_id: Uuid::new_v4(),
            run_id: None,
            prompt,
            options: vec![
                InputOption {
                    option_id: "accept".into(),
                    label: "Submit".into(),
                    accepts_value: true,
                },
                InputOption {
                    option_id: "cancel".into(),
                    label: "Cancel".into(),
                    accepts_value: false,
                },
            ],
            payload: PendingInputPayload::Form { schema },
        };
        let mut tx = self.pool().await?.begin_with("BEGIN IMMEDIATE").await?;
        sqlx::query("UPDATE owner_preflights SET state='waiting_input',input_id=?,input_json=? WHERE setup_id=? AND state='starting'")
            .bind(input.input_id.to_string()).bind(serde_json::to_string(&input)?).bind(setup_id.to_string()).execute(&mut *tx).await?;
        sqlx::query("UPDATE work_identity SET event_kind='pending_input' WHERE local_locator=? AND deleted=0")
            .bind(session_id).execute(&mut *tx).await?;
        tx.commit().await?;
        self.input_handlers
            .lock()
            .await
            .insert(input.input_id, handler.clone());
        self.preflights
            .write()
            .expect("owner preflights poisoned")
            .get_mut(session_id)
            .expect("setup locked")
            .phase = PreflightPhase::WaitingInput;
        drop(runs);
        for cx in self.input_observers(session_id) {
            handler.observe(&cx, self.clone(), input.clone());
        }
        Ok(input)
    }

    pub(crate) async fn answer_preflight(
        &self,
        input_id: Uuid,
        answer: InputAnswer,
    ) -> Result<(), ControllerError> {
        let _runs = self.runs.lock().await;
        self.answer_preflight_locked(input_id, None, answer).await
    }

    pub(super) async fn answer_preflight_locked(
        &self,
        input_id: Uuid,
        expected_work: Option<Uuid>,
        answer: InputAnswer,
    ) -> Result<(), ControllerError> {
        let pool = self.pool().await.map_err(unavailable)?;
        let row:Option<(String,String,String,String,String,String)>=sqlx::query_as("SELECT setup_id,session_id,work_id,generation,input_json,state FROM owner_preflights WHERE input_id=?")
            .bind(input_id.to_string()).fetch_optional(pool).await.map_err(unavailable)?;
        let (setup_id, session_id, work_id, generation, json, state) =
            row.ok_or_else(|| conflict("input_not_found", "recipe input no longer exists"))?;
        if state != "waiting_input" {
            return Err(conflict(
                "input_already_answered",
                "recipe input has already been consumed or interrupted",
            ));
        }
        if expected_work.is_some_and(|work| work.to_string() != work_id) {
            return Err(conflict(
                "input_not_found",
                "recipe input belongs to another work item",
            ));
        }
        let current = self
            .session_manager
            .work_catalog()
            .await
            .map_err(unavailable)?
            .lookup(&work_id, &generation)
            .await
            .map_err(unavailable)?;
        if current.local_locator != session_id
            || self
                .preflights
                .read()
                .expect("owner preflights poisoned")
                .get(&session_id)
                .is_none_or(|setup| {
                    setup.setup_id.to_string() != setup_id
                        || setup.phase != PreflightPhase::WaitingInput
                })
        {
            return Err(conflict(
                "input_interrupted",
                "original recipe setup is no longer running",
            ));
        }
        let input: PendingInput = serde_json::from_str(&json).map_err(unavailable)?;
        let option = input
            .options
            .iter()
            .find(|option| option.option_id == answer.option_id)
            .ok_or_else(|| conflict("invalid_input_option", "unknown recipe input option"))?;
        if !option.accepts_value && answer.value.is_some() {
            return Err(conflict(
                "invalid_input_value",
                "this choice does not accept values",
            ));
        }
        let handler = self
            .input_handlers
            .lock()
            .await
            .get(&input_id)
            .cloned()
            .ok_or_else(|| conflict("input_interrupted", "original recipe callback is gone"))?;
        handler
            .validate(&answer)
            .map_err(|error| conflict("invalid_input_value", error.to_string()))?;
        let changed=sqlx::query("UPDATE owner_preflights SET state='dispatching',answer_json=? WHERE input_id=? AND state='waiting_input'")
            .bind(serde_json::to_string(&answer).map_err(unavailable)?).bind(input_id.to_string()).execute(pool).await.map_err(unavailable)?;
        if changed.rows_affected() != 1 {
            return Err(conflict(
                "input_already_answered",
                "another viewer answered the recipe input",
            ));
        }
        self.preflights
            .write()
            .expect("owner preflights poisoned")
            .get_mut(&session_id)
            .expect("setup locked")
            .phase = PreflightPhase::Dispatching;
        let delivered = handler.answer(answer).await;
        sqlx::query("UPDATE owner_preflights SET state=? WHERE setup_id=?")
            .bind(if delivered.is_ok() {
                "answered"
            } else {
                "interrupted"
            })
            .bind(&setup_id)
            .execute(pool)
            .await
            .map_err(unavailable)?;
        sqlx::query(
            "UPDATE work_identity SET event_kind='pending_input' WHERE work_id=? AND deleted=0",
        )
        .bind(&work_id)
        .execute(pool)
        .await
        .map_err(unavailable)?;
        self.input_handlers.lock().await.remove(&input_id);
        delivered.map_err(unavailable)
    }

    pub(super) async fn is_preflight_input(&self, input_id: Uuid) -> Result<bool, ControllerError> {
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM owner_preflights WHERE input_id=?)")
            .bind(input_id.to_string())
            .fetch_one(self.pool().await.map_err(unavailable)?)
            .await
            .map_err(unavailable)
    }

    async fn finish_preflight(
        &self,
        session_id: &str,
        setup_id: Uuid,
        state: &str,
    ) -> anyhow::Result<()> {
        let _runs = self.runs.lock().await;
        let input: Option<String> =
            sqlx::query_scalar("SELECT input_id FROM owner_preflights WHERE setup_id=?")
                .bind(setup_id.to_string())
                .fetch_one(self.pool().await?)
                .await?;
        let mut tx = self.pool().await?.begin_with("BEGIN IMMEDIATE").await?;
        sqlx::query("UPDATE owner_preflights SET state=? WHERE setup_id=?")
            .bind(state)
            .bind(setup_id.to_string())
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE work_identity SET state=?,event_kind='pending_input' WHERE work_id=(SELECT work_id FROM owner_preflights WHERE setup_id=?) AND deleted=0 AND current_run_id IS NULL")
            .bind(if state=="ready" {"idle"}else{"interrupted"}).bind(setup_id.to_string()).execute(&mut *tx).await?;
        tx.commit().await?;
        if let Some(input_id) = input {
            self.input_handlers
                .lock()
                .await
                .remove(&Uuid::parse_str(&input_id)?);
        }
        let mut setups = self.preflights.write().expect("owner preflights poisoned");
        if setups
            .get(session_id)
            .is_some_and(|setup| setup.setup_id == setup_id)
        {
            if state == "interrupted" {
                setups.get_mut(session_id).expect("checked setup").phase =
                    PreflightPhase::Interrupted;
            } else {
                setups.remove(session_id);
            }
        }
        Ok(())
    }
}
