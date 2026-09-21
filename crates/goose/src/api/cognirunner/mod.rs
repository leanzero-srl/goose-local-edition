//! CogniRunner task mode: `POST /cognirunner/tasks` hands goose a prompt and a callback, goose
//! runs it as one ephemeral session (the same machinery as the OpenAI-compatible shim) and PUSHES
//! signed receipts to the callback as the turn happens — `started`, batched `text`, `tool`
//! started/finished, `question`, then `done` or `failed`. The caller never waits on a turn.
//!
//! - `POST /cognirunner/tasks` → `202 {taskId, sessionId}`
//! - `POST /cognirunner/tasks/{id}/messages {text}` → `agent.steer` (the ACP
//!   `_goose/unstable/session/steer` door, over REST for this route only) → `202`
//! - `POST /cognirunner/tasks/{id}/cancel` → the run's cancel token → `202`
//! - `GET /cognirunner/tasks/{id}` → `{status, seq, lastEventAt, sessionId}` for reconciliation
//!
//! Every push is a JSON ARRAY of envelopes `{taskId, threadId, seq, at, type, …}` (one element
//! per event; text is coalesced, tool events ride together), signed
//! `x-cognirunner-signature: sha256=<HMAC-SHA256(callbackSecret, raw body)>`, at most one push
//! per second per task, retried three times with backoff on a non-2xx, and never on the turn's
//! critical path: a callback that stays down loses receipts, not the turn.

pub mod batcher;
pub mod events;
pub mod registry;
pub mod signature;

use super::openai_compat::{
    apply_model, cleanup_session, keep_session, model_is_known, session_token_state,
    split_model_id, start_ephemeral_session, OpenAiError,
};
use super::AgentManagerSource;
use crate::agents::{AgentEvent, SessionConfig};
use crate::conversation::message::Message;
use crate::execution::manager::AgentManager;
use axum::{
    extract::{rejection::JsonRejection, Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use batcher::Batcher;
use chrono::{DateTime, Utc};
use events::{map_message, Mapped, TaskEvent, Usage};
use futures::StreamExt;
use registry::{TaskRecord, TaskRegistry, TaskStatus};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

pub const PUSH_MIN_INTERVAL: Duration = Duration::from_secs(1);
/// Three retries after the first attempt, with these waits between them.
const PUSH_BACKOFF: [Duration; 3] = [
    Duration::from_millis(500),
    Duration::from_millis(1500),
    Duration::from_millis(4000),
];
const PUSH_TIMEOUT: Duration = Duration::from_secs(15);
/// Events queued for the pusher; a callback that cannot keep up drops receipts, never the turn.
const EVENT_QUEUE: usize = 512;
const CONTEXT_PROMPT_KEY: &str = "cognirunner_context";
const RECIPE_PROMPT_KEY: &str = "recipe";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateTaskRequest {
    pub prompt: String,
    #[serde(default)]
    pub context: Option<String>,
    /// `provider/model`, `swarm` or `swarm-build` — the same ids `/v1/models` lists.
    pub model: String,
    pub callback_url: String,
    pub callback_secret: String,
    pub thread_id: String,
    /// Recipe instructions, applied as a system-prompt extension for this session.
    #[serde(default)]
    pub recipe: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SteerRequest {
    pub text: String,
}

/// One pushed event as it appears on the wire.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Envelope {
    pub task_id: String,
    pub thread_id: String,
    pub seq: u64,
    pub at: DateTime<Utc>,
    #[serde(flatten)]
    pub event: TaskEvent,
}

#[derive(Clone)]
pub struct CogniRunnerState {
    source: AgentManagerSource,
    registry: Arc<TaskRegistry>,
    http: reqwest::Client,
}

impl CogniRunnerState {
    pub fn new(source: AgentManagerSource) -> Self {
        Self {
            source,
            registry: Arc::new(TaskRegistry::default()),
            http: reqwest::Client::builder()
                .timeout(PUSH_TIMEOUT)
                .build()
                .expect("a default reqwest client builds"),
        }
    }
}

pub fn routes(source: AgentManagerSource) -> Router {
    Router::new()
        .route("/cognirunner/tasks", post(create_task))
        .route("/cognirunner/tasks/{id}", get(get_task))
        .route("/cognirunner/tasks/{id}/messages", post(steer_task))
        .route("/cognirunner/tasks/{id}/cancel", post(cancel_task))
        .with_state(CogniRunnerState::new(source))
}

fn task_json(task: &TaskRecord) -> Value {
    json!({
        "taskId": task.id,
        "sessionId": task.session_id,
        "threadId": task.thread_id,
        "status": task.status,
        "seq": task.seq,
        "createdAt": task.created_at,
        "lastEventAt": task.last_event_at,
    })
}

fn not_found() -> OpenAiError {
    OpenAiError::new(StatusCode::NOT_FOUND, "task not found")
}

async fn create_task(
    State(state): State<CogniRunnerState>,
    headers: HeaderMap,
    body: Result<Json<CreateTaskRequest>, JsonRejection>,
) -> Result<Response, OpenAiError> {
    let Json(request) = body.map_err(|e| OpenAiError::bad_request(e.body_text()))?;
    if request.prompt.trim().is_empty() {
        return Err(OpenAiError::bad_request("prompt must not be empty"));
    }
    if request.thread_id.trim().is_empty() {
        return Err(OpenAiError::bad_request("threadId must not be empty"));
    }
    if request.callback_secret.is_empty() {
        return Err(OpenAiError::bad_request("callbackSecret must not be empty"));
    }
    let callback_url = url::Url::parse(&request.callback_url)
        .ok()
        .filter(|u| matches!(u.scheme(), "http" | "https"))
        .ok_or_else(|| OpenAiError::bad_request("callbackUrl must be an http(s) URL"))?;
    let model_ref = split_model_id(&request.model).ok_or_else(OpenAiError::model_not_found)?;
    if !model_is_known(&model_ref).await {
        return Err(OpenAiError::model_not_found());
    }

    let keep = keep_session(&headers);
    let agents = state
        .source
        .manager()
        .await
        .map_err(|e| OpenAiError::internal(format!("No agent manager: {}", e)))?;
    let session_id = start_ephemeral_session(
        &agents,
        &format!("CogniRunner task · {}", request.thread_id.trim()),
    )
    .await?;
    if let Err(e) = apply_model(&agents, &session_id, &model_ref).await {
        cleanup_session(&agents, &session_id, keep).await;
        return Err(e);
    }

    let task_id = format!("task_{}", uuid::Uuid::new_v4());
    let record = TaskRecord::new(
        task_id.clone(),
        session_id.clone(),
        request.thread_id.trim().to_string(),
    );
    let cancel = record.cancel.clone();
    // The manager's token map is the busy set every reader consults (`busy_session_ids`,
    // `cancel_session`); a fresh session can never already hold one.
    if let Err(e) = agents
        .try_register_cancel_token(&session_id, cancel.clone())
        .await
    {
        cleanup_session(&agents, &session_id, keep).await;
        return Err(OpenAiError::internal(format!("session busy: {}", e)));
    }
    if state.registry.insert(record).is_err() {
        agents.unregister_cancel_token(&session_id).await;
        cleanup_session(&agents, &session_id, keep).await;
        return Err(OpenAiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            format!(
                "task capacity reached ({} tasks); retry when one finishes",
                registry::TASK_CAP
            ),
        ));
    }

    let run = TaskRun {
        state: state.clone(),
        agents,
        task_id: task_id.clone(),
        session_id: session_id.clone(),
        thread_id: request.thread_id.trim().to_string(),
        model: request.model.trim().to_string(),
        prompt: request.prompt,
        context: request.context,
        recipe: request.recipe,
        callback_url,
        callback_secret: request.callback_secret,
        keep,
        cancel,
    };
    drop(tokio::spawn(run.drive()));

    Ok((
        StatusCode::ACCEPTED,
        Json(json!({ "taskId": task_id, "sessionId": session_id })),
    )
        .into_response())
}

async fn get_task(
    State(state): State<CogniRunnerState>,
    Path(id): Path<String>,
) -> Result<Response, OpenAiError> {
    let task = state.registry.get(&id).ok_or_else(not_found)?;
    Ok(Json(task_json(&task)).into_response())
}

async fn steer_task(
    State(state): State<CogniRunnerState>,
    Path(id): Path<String>,
    body: Result<Json<SteerRequest>, JsonRejection>,
) -> Result<Response, OpenAiError> {
    let Json(request) = body.map_err(|e| OpenAiError::bad_request(e.body_text()))?;
    if request.text.trim().is_empty() {
        return Err(OpenAiError::bad_request("text must not be empty"));
    }
    let task = state.registry.get(&id).ok_or_else(not_found)?;
    if task.status.is_terminal() {
        return Err(OpenAiError::new(
            StatusCode::CONFLICT,
            format!("task is {}", status_name(task.status)),
        ));
    }
    let agents = state
        .source
        .manager()
        .await
        .map_err(|e| OpenAiError::internal(format!("No agent manager: {}", e)))?;
    let agent = agents
        .get_or_create_agent(task.session_id.clone())
        .await
        .map_err(|e| OpenAiError::internal(format!("No agent for session: {}", e)))?;
    let message_id = format!("steer_{}", uuid::Uuid::new_v4());
    let message = Message::user()
        .with_text(request.text.trim())
        .with_id(message_id.clone());
    agent.steer(&task.session_id, message).await;
    Ok((
        StatusCode::ACCEPTED,
        Json(json!({ "taskId": id, "messageId": message_id, "queued": true })),
    )
        .into_response())
}

async fn cancel_task(
    State(state): State<CogniRunnerState>,
    Path(id): Path<String>,
) -> Result<Response, OpenAiError> {
    let task = state.registry.get(&id).ok_or_else(not_found)?;
    if !task.status.is_terminal() {
        state.registry.set_status(&id, TaskStatus::Cancelled);
        // The manager's cancel door, the same one every other run on this process answers to.
        if let Ok(agents) = state.source.manager().await {
            if let Err(e) = agents.cancel_session(&task.session_id).await {
                tracing::debug!(task = %id, error = %e, "cognirunner: no registered token, cancelling directly");
                task.cancel.cancel();
            }
            if let Ok(agent) = agents.get_or_create_agent(task.session_id.clone()).await {
                agent.discard_pending_steers(&task.session_id).await;
            }
        } else {
            task.cancel.cancel();
        }
    }
    let task = state.registry.get(&id).ok_or_else(not_found)?;
    Ok((StatusCode::ACCEPTED, Json(task_json(&task))).into_response())
}

fn status_name(status: TaskStatus) -> &'static str {
    match status {
        TaskStatus::Running => "running",
        TaskStatus::Done => "done",
        TaskStatus::Failed => "failed",
        TaskStatus::Cancelled => "cancelled",
    }
}

struct TaskRun {
    state: CogniRunnerState,
    agents: Arc<AgentManager>,
    task_id: String,
    session_id: String,
    thread_id: String,
    model: String,
    prompt: String,
    context: Option<String>,
    recipe: Option<String>,
    callback_url: url::Url,
    callback_secret: String,
    keep: bool,
    cancel: CancellationToken,
}

impl TaskRun {
    async fn drive(self) {
        let (tx, rx) = mpsc::channel::<Mapped>(EVENT_QUEUE);
        let pusher = tokio::spawn(push_loop(
            self.state.clone(),
            self.task_id.clone(),
            self.thread_id.clone(),
            self.callback_url.clone(),
            self.callback_secret.clone(),
            rx,
        ));

        let emit = |event: Mapped| {
            if let Err(e) = tx.try_send(event) {
                tracing::warn!(task = %self.task_id, error = %e, "cognirunner: event queue full, receipt dropped");
            }
        };
        emit(Mapped::Event(TaskEvent::Started {
            model: self.model.clone(),
            session_id: self.session_id.clone(),
        }));

        let status = self.run_turn(&emit).await;
        drop(tx);
        self.state.registry.set_status(&self.task_id, status);
        self.agents.unregister_cancel_token(&self.session_id).await;
        cleanup_session(&self.agents, &self.session_id, self.keep).await;
        if let Err(e) = pusher.await {
            tracing::warn!(task = %self.task_id, error = %e, "cognirunner: pusher task failed");
        }
    }

    /// The turn: `agent.reply` under the task's cancel token, every event mapped and queued.
    /// Returns the status the task ends in; the terminal `done`/`failed` receipt is queued here.
    async fn run_turn(&self, emit: &impl Fn(Mapped)) -> TaskStatus {
        let failed = |error: String| {
            emit(Mapped::Event(TaskEvent::Failed { error }));
            TaskStatus::Failed
        };
        if self.cancel.is_cancelled() {
            return self.finish(emit, "cancelled").await;
        }
        let agent = match self
            .agents
            .get_or_create_agent(self.session_id.clone())
            .await
        {
            Ok(agent) => agent,
            Err(e) => return failed(format!("Failed to get agent: {}", e)),
        };
        if let Some(context) = self.context.as_deref().filter(|c| !c.trim().is_empty()) {
            agent
                .extend_system_prompt(CONTEXT_PROMPT_KEY.to_string(), context.to_string())
                .await;
        }
        if let Some(recipe) = self.recipe.as_deref().filter(|r| !r.trim().is_empty()) {
            agent
                .extend_system_prompt(RECIPE_PROMPT_KEY.to_string(), recipe.to_string())
                .await;
        }
        let session_config = SessionConfig {
            id: self.session_id.clone(),
            schedule_id: None,
            max_turns: None,
            retry_config: None,
        };
        let mut stream = match agent
            .reply(
                Message::user().with_text(&self.prompt),
                session_config,
                Some(self.cancel.clone()),
            )
            .await
        {
            Ok(stream) => stream,
            Err(e) => return failed(e.to_string()),
        };

        loop {
            let event = tokio::select! {
                biased;
                _ = self.cancel.cancelled() => break,
                next = stream.next() => match next {
                    Some(event) => event,
                    None => break,
                },
            };
            match event {
                Ok(AgentEvent::Message(message)) => {
                    for mapped in map_message(&message) {
                        let is_failure = matches!(mapped, Mapped::Event(TaskEvent::Failed { .. }));
                        emit(mapped);
                        if is_failure {
                            self.cancel.cancel();
                            return TaskStatus::Failed;
                        }
                    }
                }
                Ok(AgentEvent::Usage(_))
                | Ok(AgentEvent::HistoryReplaced(_))
                | Ok(AgentEvent::McpNotification(_)) => {}
                Err(e) => return failed(e.to_string()),
            }
        }
        drop(stream);
        let reason = if self.cancel.is_cancelled() {
            "cancelled"
        } else {
            "stop"
        };
        self.finish(emit, reason).await
    }

    async fn finish(&self, emit: &impl Fn(Mapped), reason: &str) -> TaskStatus {
        let token_state =
            session_token_state(self.agents.session_manager(), &self.session_id).await;
        emit(Mapped::Event(TaskEvent::Done {
            finish_reason: reason.to_string(),
            usage: Usage::from(&token_state),
        }));
        if reason == "cancelled" {
            TaskStatus::Cancelled
        } else {
            TaskStatus::Done
        }
    }
}

/// Drains the task's events into pushes under the batcher's rate rule; exits after the queue
/// closes and the last batch is out.
async fn push_loop(
    state: CogniRunnerState,
    task_id: String,
    thread_id: String,
    callback_url: url::Url,
    secret: String,
    mut rx: mpsc::Receiver<Mapped>,
) {
    let mut batcher = Batcher::new(PUSH_MIN_INTERVAL);
    let mut open = true;
    loop {
        let ready = batcher.ready_at(Instant::now());
        if !open && ready.is_none() {
            break;
        }
        tokio::select! {
            biased;
            received = rx.recv(), if open => match received {
                Some(Mapped::Text(text)) => batcher.push_text(text),
                Some(Mapped::Event(event)) => batcher.push(event),
                None => open = false,
            },
            _ = async {
                match ready {
                    Some(at) => tokio::time::sleep_until(tokio::time::Instant::from_std(at)).await,
                    None => std::future::pending::<()>().await,
                }
            } => {
                let batch = batcher.take(Instant::now());
                if !batch.is_empty() {
                    push_batch(&state, &task_id, &thread_id, &callback_url, &secret, batch).await;
                }
            }
        }
    }
}

async fn push_batch(
    state: &CogniRunnerState,
    task_id: &str,
    thread_id: &str,
    callback_url: &url::Url,
    secret: &str,
    batch: Vec<TaskEvent>,
) {
    let mut envelopes = Vec::with_capacity(batch.len());
    for event in batch {
        let at = Utc::now();
        let Some(seq) = state.registry.next_seq(task_id, at) else {
            return;
        };
        envelopes.push(Envelope {
            task_id: task_id.to_string(),
            thread_id: thread_id.to_string(),
            seq,
            at,
            event,
        });
    }
    let body = match serde_json::to_vec(&envelopes) {
        Ok(body) => body,
        Err(e) => {
            tracing::warn!(task = %task_id, error = %e, "cognirunner: cannot serialize receipts");
            return;
        }
    };
    let signature = signature::sign(secret, &body);
    for (attempt, backoff) in PUSH_BACKOFF
        .iter()
        .map(Some)
        .chain(std::iter::once(None))
        .enumerate()
    {
        let result = state
            .http
            .post(callback_url.clone())
            .header("content-type", "application/json")
            .header(signature::SIGNATURE_HEADER, &signature)
            .body(body.clone())
            .send()
            .await;
        match result {
            Ok(response) if response.status().is_success() => return,
            Ok(response) => {
                tracing::warn!(task = %task_id, attempt, status = %response.status(), "cognirunner: callback refused the push");
            }
            Err(e) => {
                tracing::warn!(task = %task_id, attempt, error = %e, "cognirunner: callback unreachable");
            }
        }
        if let Some(backoff) = backoff {
            tokio::time::sleep(*backoff).await;
        }
    }
    tracing::warn!(task = %task_id, events = envelopes.len(), "cognirunner: receipts dropped after retries");
}
