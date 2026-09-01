use axum::http::StatusCode;
use goose::builtin_extension::register_builtin_extensions;
use goose::execution::manager::AgentManager;
use goose::scheduler_trait::SchedulerTrait;
use goose::session::SessionManager;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

use crate::session_delta_tap::SessionDeltaMsg;
use crate::session_event_bus::SessionEventBus;
use goose::agents::ExtensionLoadResult;

type ExtensionLoadingTasks =
    Arc<Mutex<HashMap<String, Arc<Mutex<Option<JoinHandle<Vec<ExtensionLoadResult>>>>>>>>;

/// Buffer for the process-wide per-message delta tap. A lagged reader (the LeanZero Link
/// control service's local delta pump, the only consumer) drops the oldest deltas rather
/// than blocking the reply loop — the mesh mirror is best-effort and peers reconcile
/// session summaries via polling.
const SESSION_DELTA_TAP_CAPACITY: usize = 1024;

#[derive(Clone)]
pub struct AppState {
    pub(crate) agent_manager: Arc<AgentManager>,
    pub recipe_file_hash_map: Arc<Mutex<HashMap<String, PathBuf>>>,
    recipe_session_tracker: Arc<Mutex<HashSet<String>>>,
    pub extension_loading_tasks: ExtensionLoadingTasks,
    session_buses: Arc<Mutex<HashMap<String, Arc<SessionEventBus>>>>,
    /// Fan-out of every session's reply-loop `MessageEvent`s, tapped ADDITIVELY beside
    /// the per-session bus for cross-device mirroring. Absent-receiver sends are ignored
    /// (`broadcast::Sender::send` returns `Err` with no subscribers), so the reply loop
    /// never blocks or errors on it.
    session_delta_tap: tokio::sync::broadcast::Sender<SessionDeltaMsg>,
}

impl AppState {
    pub async fn new(_tls: bool) -> anyhow::Result<Arc<AppState>> {
        register_builtin_extensions(goose_mcp::BUILTIN_EXTENSIONS.clone());

        let agent_manager = AgentManager::instance().await?;
        let (session_delta_tap, _) = tokio::sync::broadcast::channel(SESSION_DELTA_TAP_CAPACITY);
        Ok(Arc::new(Self {
            agent_manager,
            recipe_file_hash_map: Arc::new(Mutex::new(HashMap::new())),
            recipe_session_tracker: Arc::new(Mutex::new(HashSet::new())),
            extension_loading_tasks: Arc::new(Mutex::new(HashMap::new())),
            session_buses: Arc::new(Mutex::new(HashMap::new())),
            session_delta_tap,
        }))
    }

    /// A sender handle for the process-wide per-message delta tap. The reply loop sends
    /// each published `MessageEvent` here (in addition to the per-session bus); the
    /// LeanZero Link boot path wraps a receiver as a `DeltaSource`.
    pub fn session_delta_tap(&self) -> tokio::sync::broadcast::Sender<SessionDeltaMsg> {
        self.session_delta_tap.clone()
    }

    pub async fn set_extension_loading_task(
        &self,
        session_id: String,
        task: JoinHandle<Vec<ExtensionLoadResult>>,
    ) {
        let mut tasks = self.extension_loading_tasks.lock().await;
        tasks.insert(session_id, Arc::new(Mutex::new(Some(task))));
    }

    pub async fn has_extension_loading_task(&self, session_id: &str) -> bool {
        let tasks = self.extension_loading_tasks.lock().await;
        tasks.contains_key(session_id)
    }

    pub async fn take_extension_loading_task(
        &self,
        session_id: &str,
    ) -> Result<Option<Vec<ExtensionLoadResult>>, tokio::task::JoinError> {
        let task_holder = {
            let tasks = self.extension_loading_tasks.lock().await;
            tasks.get(session_id).cloned()
        };

        if let Some(holder) = task_holder {
            let mut task = holder.lock().await;
            if let Some(handle) = task.as_mut() {
                // Keep the per-session task locked and discoverable while awaiting so
                // concurrent routes cannot mutate extensions before background loading finishes.
                match handle.await {
                    Ok(results) => {
                        task.take();
                        return Ok(Some(results));
                    }
                    Err(e) => {
                        task.take();
                        tracing::warn!("Background extension loading task failed: {}", e);
                        return Err(e);
                    }
                }
            }
        }
        Ok(None)
    }

    pub async fn remove_extension_loading_task(&self, session_id: &str) {
        let mut tasks = self.extension_loading_tasks.lock().await;
        tasks.remove(session_id);
    }

    pub fn scheduler(&self) -> Arc<dyn SchedulerTrait> {
        self.agent_manager.scheduler()
    }

    pub fn session_manager(&self) -> &SessionManager {
        self.agent_manager.session_manager()
    }

    pub async fn set_recipe_file_hash_map(&self, hash_map: HashMap<String, PathBuf>) {
        let mut map = self.recipe_file_hash_map.lock().await;
        *map = hash_map;
    }

    pub async fn mark_recipe_run_if_absent(&self, session_id: &str) -> bool {
        let mut sessions = self.recipe_session_tracker.lock().await;
        if sessions.contains(session_id) {
            false
        } else {
            sessions.insert(session_id.to_string());
            true
        }
    }

    pub async fn get_or_create_event_bus(&self, session_id: &str) -> Arc<SessionEventBus> {
        let mut buses = self.session_buses.lock().await;
        buses
            .entry(session_id.to_string())
            .or_insert_with(|| Arc::new(SessionEventBus::new()))
            .clone()
    }

    /// Get an existing event bus for a session without creating one.
    pub async fn get_event_bus(&self, session_id: &str) -> Option<Arc<SessionEventBus>> {
        let buses = self.session_buses.lock().await;
        buses.get(session_id).cloned()
    }

    pub async fn get_agent(&self, session_id: String) -> anyhow::Result<Arc<goose::agents::Agent>> {
        self.agent_manager.get_or_create_agent(session_id).await
    }

    pub async fn get_agent_for_route(
        &self,
        session_id: String,
    ) -> Result<Arc<goose::agents::Agent>, StatusCode> {
        self.get_agent(session_id).await.map_err(|e| {
            tracing::error!("Failed to get agent: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })
    }
}
