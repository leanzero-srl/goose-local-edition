//! Original-owner execution. Connections observe work; they do not own its lifetime.

pub mod backend;
mod commands;
mod cursors;
mod inputs;
mod partials;
mod store;

use crate::agents::Agent;
use crate::execution::manager::AgentManager;
use crate::session::SessionManager;
use agent_client_protocol::schema::v1::{PromptRequest, PromptResponse, SessionNotification};
use agent_client_protocol::{Client, ConnectionTo};
use async_trait::async_trait;
use leanzero_link::controller::ControllerError;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock, Mutex as StdMutex, OnceLock, RwLock, Weak};
use tokio::sync::{Mutex, OnceCell};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub use backend::{ControllerBackendConfig, GooseControllerBackend};
pub use cursors::CursorPolicy;
pub use inputs::{InputAnswer, InputHandler};

pub(crate) struct ActivePromptRun {
    pub run_id: String,
    pub cancel_token: CancellationToken,
    pub agent: Arc<Agent>,
    manager: Arc<AgentManager>,
    work_id: Uuid,
    generation: String,
}

#[derive(Clone)]
pub struct RunTicket {
    pub session_id: String,
    pub run_id: Uuid,
    pub cancel_token: CancellationToken,
    pub message_id: Option<String>,
}

#[async_trait]
pub trait OwnerExecutor: Send + Sync {
    async fn prepare(&self, session_id: &str) -> anyhow::Result<(Arc<Agent>, Arc<AgentManager>)>;
    async fn execute(
        &self,
        args: PromptRequest,
        ticket: RunTicket,
    ) -> Result<PromptResponse, agent_client_protocol::Error>;
}

static REGISTRIES: LazyLock<StdMutex<HashMap<PathBuf, Weak<OwnerRunRegistry>>>> =
    LazyLock::new(|| StdMutex::new(HashMap::new()));

pub struct OwnerRunRegistry {
    pub(crate) runs: Arc<Mutex<HashMap<String, ActivePromptRun>>>,
    session_manager: Arc<SessionManager>,
    owner: RwLock<Option<Uuid>>,
    executor: RwLock<Option<Arc<dyn OwnerExecutor>>>,
    agent_manager: OnceLock<Arc<AgentManager>>,
    initialized: OnceCell<()>,
    input_handlers: Mutex<HashMap<Uuid, Arc<dyn InputHandler>>>,
    queued: Mutex<HashMap<String, Vec<commands::QueuedCommand>>>,
    observers: RwLock<HashMap<String, HashMap<Uuid, ConnectionTo<Client>>>>,
}

impl OwnerRunRegistry {
    pub fn for_store(data_dir: PathBuf, session_manager: Arc<SessionManager>) -> Arc<Self> {
        let key = std::fs::canonicalize(&data_dir).unwrap_or(data_dir);
        let mut registries = REGISTRIES.lock().expect("owner registry poisoned");
        if let Some(registry) = registries.get(&key).and_then(Weak::upgrade) {
            return registry;
        }
        let registry = Arc::new(Self {
            runs: Arc::new(Mutex::new(HashMap::new())),
            session_manager,
            owner: RwLock::new(None),
            executor: RwLock::new(None),
            agent_manager: OnceLock::new(),
            initialized: OnceCell::new(),
            input_handlers: Mutex::new(HashMap::new()),
            queued: Mutex::new(HashMap::new()),
            observers: RwLock::new(HashMap::new()),
        });
        registries.insert(key, Arc::downgrade(&registry));
        registry
    }

    pub fn bind_owner(&self, owner: Uuid) -> Result<(), ControllerError> {
        let mut bound = self.owner.write().expect("owner identity poisoned");
        if bound.is_some_and(|existing| existing != owner) {
            return Err(conflict(
                "wrong_owner",
                "this store already belongs to another owner",
            ));
        }
        *bound = Some(owner);
        Ok(())
    }

    pub fn bind_agent_manager(&self, manager: Arc<AgentManager>) -> Arc<AgentManager> {
        self.agent_manager.get_or_init(|| manager).clone()
    }

    pub fn bind_executor(&self, executor: Arc<dyn OwnerExecutor>) {
        *self.executor.write().expect("owner executor poisoned") = Some(executor);
    }

    pub fn has_executor(&self) -> bool {
        self.executor
            .read()
            .expect("owner executor poisoned")
            .is_some()
    }

    pub async fn attach_observer(
        self: &Arc<Self>,
        id: Uuid,
        session_id: &str,
        cx: ConnectionTo<Client>,
    ) {
        self.observers
            .write()
            .expect("owner observers poisoned")
            .entry(session_id.to_string())
            .or_default()
            .insert(id, cx.clone());
        let pending = {
            let handlers = self.input_handlers.lock().await;
            handlers
                .iter()
                .map(|(id, handler)| (*id, handler.clone()))
                .collect::<Vec<_>>()
        };
        for (input_id, handler) in pending {
            if let Ok(Some(json)) = async {
                Ok::<_, anyhow::Error>(sqlx::query_scalar::<_, String>("SELECT input_json FROM owner_inputs WHERE input_id=? AND session_id=? AND state='open'")
                    .bind(input_id.to_string()).bind(session_id).fetch_optional(self.pool().await?).await?)
            }.await {
                match serde_json::from_str(&json) {
                    Ok(input) => handler.observe(&cx, self.clone(), input),
                    Err(error) => tracing::error!(%error, "pending owner input is unreadable"),
                }
            }
        }
    }

    pub fn detach_observer(&self, id: Uuid) {
        self.observers
            .write()
            .expect("owner observers poisoned")
            .retain(|_, observers| {
                observers.remove(&id);
                !observers.is_empty()
            });
    }

    pub fn notify_session(&self, notification: SessionNotification) {
        if let Some(observers) = self
            .observers
            .write()
            .expect("owner observers poisoned")
            .get_mut(notification.session_id.0.as_ref())
        {
            observers.retain(|_, cx| cx.send_notification(notification.clone()).is_ok());
        }
    }

    pub(super) fn input_observers(&self, session_id: &str) -> Vec<ConnectionTo<Client>> {
        self.observers
            .read()
            .expect("owner observers poisoned")
            .get(session_id)
            .map(|observers| observers.values().cloned().collect())
            .unwrap_or_default()
    }

    pub async fn current_run(&self, session_id: &str) -> Option<Uuid> {
        self.runs
            .lock()
            .await
            .get(session_id)
            .and_then(|run| Uuid::parse_str(&run.run_id).ok())
    }

    pub async fn active_agent(&self, session_id: &str) -> Option<Arc<Agent>> {
        self.runs
            .lock()
            .await
            .get(session_id)
            .map(|run| run.agent.clone())
    }

    pub async fn start_existing(
        self: &Arc<Self>,
        args: PromptRequest,
    ) -> Result<
        (
            RunTicket,
            tokio::task::JoinHandle<Result<PromptResponse, agent_client_protocol::Error>>,
        ),
        ControllerError,
    > {
        self.ensure_store().await.map_err(unavailable)?;
        let executor = self
            .executor
            .read()
            .expect("owner executor poisoned")
            .clone()
            .ok_or_else(|| {
                ControllerError::unavailable("owner prompt executor is not initialized")
            })?;
        let session_id = args.session_id.0.to_string();
        let (agent, manager) = executor.prepare(&session_id).await.map_err(unavailable)?;
        let mut runs = self.runs.lock().await;
        if runs.contains_key(&session_id) || manager.is_session_busy(&session_id).await {
            return Err(conflict(
                "work_busy",
                "session already has an active owner run",
            ));
        }
        let ticket = self
            .reserve_locked(
                &mut runs,
                &session_id,
                Uuid::new_v4(),
                CancellationToken::new(),
                agent,
                manager,
                None,
            )
            .await
            .map_err(unavailable)?;
        let registry = self.clone();
        let running = ticket.clone();
        let task = tokio::spawn(async move {
            let result = executor.execute(args, running.clone()).await;
            registry
                .finish(&running.session_id, &running.run_id.to_string())
                .await;
            result
        });
        Ok((ticket, task))
    }

    pub(crate) async fn reserve(
        &self,
        session_id: &str,
        run_id: Uuid,
        token: CancellationToken,
        agent: Arc<Agent>,
        manager: Arc<AgentManager>,
        message_id: Option<String>,
    ) -> anyhow::Result<RunTicket> {
        self.ensure_store().await?;
        let mut runs = self.runs.lock().await;
        self.reserve_locked(
            &mut runs, session_id, run_id, token, agent, manager, message_id,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn reserve_locked(
        &self,
        runs: &mut HashMap<String, ActivePromptRun>,
        session_id: &str,
        run_id: Uuid,
        token: CancellationToken,
        agent: Arc<Agent>,
        manager: Arc<AgentManager>,
        message_id: Option<String>,
    ) -> anyhow::Result<RunTicket> {
        if runs.contains_key(session_id) {
            anyhow::bail!("session already has an active owner run");
        }
        let catalog = self.session_manager.work_catalog().await?;
        let record = catalog.lookup_local_session(session_id).await?;
        let generation = catalog.watermark().await?.generation;
        let work_id = Uuid::parse_str(&record.work_id)?;
        manager
            .try_register_connection_prompt(session_id, token.clone())
            .await?;
        if let Err(error) = catalog
            .record_runtime(&record.work_id, &generation, "running", Some(run_id))
            .await
        {
            manager.unregister_cancel_token(session_id).await;
            return Err(error);
        }
        runs.insert(
            session_id.to_owned(),
            ActivePromptRun {
                run_id: run_id.to_string(),
                cancel_token: token.clone(),
                agent,
                manager,
                work_id,
                generation,
            },
        );
        Ok(RunTicket {
            session_id: session_id.to_owned(),
            run_id,
            cancel_token: token,
            message_id,
        })
    }

    pub async fn finish(self: &Arc<Self>, session_id: &str, run_id: &str) {
        let mut runs = self.runs.lock().await;
        if runs.get(session_id).is_none_or(|run| run.run_id != run_id) {
            return;
        }
        let run = runs.remove(session_id).expect("checked run exists");
        run.manager.unregister_cancel_token(session_id).await;
        run.agent.discard_pending_steers(session_id).await;
        if let Err(error) = self.clear_partials(session_id, run_id).await {
            tracing::error!(session_id, %error, "owner partial cleanup could not be persisted");
        }
        if let Err(error) = self.finish_store(&run).await {
            tracing::error!(session_id, %error, "owner run completion could not be persisted");
        }
        drop(runs);
        self.input_handlers
            .lock()
            .await
            .retain(|_, handler| handler.run_id() != run_id);
        self.dispatch_next(session_id.to_owned(), run.run_id);
    }

    pub async fn steer(
        &self,
        session_id: &str,
        expected_run: &str,
        message: crate::conversation::message::Message,
    ) -> anyhow::Result<()> {
        let runs = self.runs.lock().await;
        let run = runs
            .get(session_id)
            .ok_or_else(|| anyhow::anyhow!("no active run"))?;
        anyhow::ensure!(run.run_id == expected_run, "stale_run");
        run.agent.steer(session_id, message).await;
        Ok(())
    }

    pub async fn stop(&self, session_id: &str, expected_run: Option<&str>) -> anyhow::Result<bool> {
        let runs = self.runs.lock().await;
        let Some(run) = runs.get(session_id) else {
            return Ok(false);
        };
        anyhow::ensure!(
            expected_run.is_none_or(|expected| expected == run.run_id),
            "stale_run"
        );
        run.cancel_token.cancel();
        Ok(true)
    }
}

fn conflict(code: &str, message: impl Into<String>) -> ControllerError {
    ControllerError::new(axum::http::StatusCode::CONFLICT, code, message, false)
}
fn unavailable(error: impl std::fmt::Display) -> ControllerError {
    ControllerError::unavailable(error.to_string())
}
fn unsupported(message: impl Into<String>) -> ControllerError {
    ControllerError::new(
        axum::http::StatusCode::UNPROCESSABLE_ENTITY,
        "unsupported_action",
        message,
        false,
    )
}

#[cfg(test)]
pub(crate) mod tests;
