//! An OpenAI-compatible surface over goose: `GET /v1/models` and `POST /v1/chat/completions`.
//!
//! Any OpenAI client (a Forge app, an SDK, `curl`) can drive a goose node without knowing goose's
//! session/reply protocol. A completion is one ephemeral goose session: the request's messages become
//! the session's conversation, the requested `provider/model` (or `swarm` / `swarm-build`) is applied
//! the same way `POST /agent/update_provider` does, the reply runs to completion with goose's own
//! extensions (tools are goose's, never the caller's), and the session is deleted afterwards.
//!
//! ONE implementation, two hosts: goosed mounts these routes over its `AppState`'s agent manager,
//! and `goose serve` — the engine the desktop actually runs — mounts them over the manager its
//! [`crate::acp::server_factory::AcpServer`] builds. Both go through [`AgentManagerSource`].

use super::AgentManagerSource;
use crate::agents::{AgentEvent, SessionConfig};
use crate::config::paths::Paths;
use crate::config::{resolve_extensions_for_new_session, Config};
use crate::conversation::message::{Message, TokenState};
use crate::conversation::Conversation;
use crate::execution::manager::AgentManager;
use crate::providers::configured::{check_provider_configured, resolve_model_info};
use crate::providers::{create, providers as get_providers};
use crate::session::session_manager::SessionType;
use crate::session::{EnabledExtensionsState, ExtensionState, SessionManager};
use axum::{
    body::Body,
    extract::{rejection::JsonRejection, State},
    http::{self, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use futures::StreamExt;
use rmcp::model::Role;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::sync::CancellationToken;

const SWARM_PROVIDER: &str = "swarm";
const SWARM_CHAT_MODEL: &str = "swarm";
const SWARM_BUILD_MODEL: &str = "swarm-build";
const OWNED_BY: &str = "goose";
const SYSTEM_PROMPT_KEY: &str = "openai_compat";
const JSON_ONLY_INSTRUCTION: &str = "Respond with ONLY a valid JSON object.";
pub const KEEP_SESSION_HEADER: &str = "x-goose-keep-session";
const WORKING_DIR_ENV: &str = "GOOSE_OPENAI_COMPAT_WORKING_DIR";

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<OpenAiMessage>,
    #[serde(default)]
    pub stream: bool,
    /// Accepted and ignored: goose's own MCP tools run inside the turn.
    #[serde(default)]
    #[schema(value_type = Object)]
    pub tools: Option<Value>,
    #[serde(default)]
    #[schema(value_type = Object)]
    pub response_format: Option<Value>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
    #[serde(default)]
    pub temperature: Option<f64>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct OpenAiMessage {
    pub role: String,
    #[serde(default)]
    #[schema(value_type = Object)]
    pub content: Option<Value>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ModelObject {
    pub id: String,
    pub object: &'static str,
    pub owned_by: &'static str,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ModelList {
    pub object: &'static str,
    pub data: Vec<ModelObject>,
}

/// The OpenAI error envelope, so a client's error handling sees the shape it expects.
pub struct OpenAiError {
    status: StatusCode,
    message: String,
}

impl OpenAiError {
    pub fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, message)
    }

    pub fn model_not_found() -> Self {
        Self::new(StatusCode::NOT_FOUND, "model not found")
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, message)
    }

    pub fn body(&self) -> Value {
        let kind = match self.status {
            StatusCode::BAD_REQUEST => "invalid_request_error",
            StatusCode::NOT_FOUND => "not_found_error",
            StatusCode::UNAUTHORIZED => "authentication_error",
            _ => "server_error",
        };
        json!({ "error": { "message": self.message, "type": kind, "code": self.status.as_u16() } })
    }
}

impl IntoResponse for OpenAiError {
    fn into_response(self) -> Response {
        if self.status.is_server_error() {
            tracing::error!(status = %self.status, message = %self.message, "openai_compat error");
        } else {
            tracing::warn!(status = %self.status, message = %self.message, "openai_compat error");
        }
        (self.status, Json(self.body())).into_response()
    }
}

/// `provider/model` split on the FIRST `/` (model names may themselves contain `/`, e.g.
/// OpenRouter's `org/model`). The two swarm ids map to the swarm provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelRef {
    pub provider: String,
    pub model: String,
}

pub fn split_model_id(id: &str) -> Option<ModelRef> {
    let id = id.trim();
    if id == SWARM_CHAT_MODEL || id == SWARM_BUILD_MODEL {
        return Some(ModelRef {
            provider: SWARM_PROVIDER.to_string(),
            model: id.to_string(),
        });
    }
    let (provider, model) = id.split_once('/')?;
    if provider.is_empty() || model.is_empty() {
        return None;
    }
    Some(ModelRef {
        provider: provider.to_string(),
        model: model.to_string(),
    })
}

/// The goose side of an OpenAI request: prior turns, the turn to answer, and the system text.
#[derive(Debug, Default)]
pub struct Translated {
    pub system: Option<String>,
    pub history: Vec<Message>,
    pub user_message: Option<Message>,
}

fn content_text(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(parts) => {
            let mut out = Vec::new();
            for part in parts {
                let kind = part.get("type").and_then(Value::as_str).unwrap_or("");
                match kind {
                    "text" | "input_text" => {
                        if let Some(text) = part.get("text").and_then(Value::as_str) {
                            out.push(text.to_string());
                        }
                    }
                    "image_url" | "input_image" | "image" => {
                        tracing::warn!(
                            "openai_compat: dropping image part (images are not supported)"
                        );
                    }
                    other => {
                        tracing::warn!(
                            part_type = other,
                            "openai_compat: dropping unknown content part"
                        );
                    }
                }
            }
            out.join("\n")
        }
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// `messages[]` → goose. System/developer turns are joined into one system text; the last user
/// turn is the one the agent answers; everything before it is history. `json_object`/`json_schema`
/// response formats append the JSON-only instruction to the system text.
pub fn translate_messages(
    messages: &[OpenAiMessage],
    response_format: Option<&Value>,
) -> Translated {
    let mut system_parts: Vec<String> = Vec::new();
    let mut turns: Vec<Message> = Vec::new();

    for m in messages {
        let text = m.content.as_ref().map(content_text).unwrap_or_default();
        match m.role.as_str() {
            "system" | "developer" => {
                if !text.is_empty() {
                    system_parts.push(text);
                }
            }
            "user" => turns.push(Message::user().with_text(text)),
            "assistant" => turns.push(Message::assistant().with_text(text)),
            "tool" | "function" => {
                tracing::warn!(
                    "openai_compat: dropping tool-result message (tools are goose's own)"
                );
            }
            other => {
                tracing::warn!(
                    role = other,
                    "openai_compat: dropping message with unknown role"
                );
            }
        }
    }

    let wants_json = response_format
        .and_then(|f| f.get("type"))
        .and_then(Value::as_str)
        .map(|t| t == "json_object" || t == "json_schema")
        .unwrap_or(false);
    if wants_json {
        system_parts.push(JSON_ONLY_INSTRUCTION.to_string());
    }

    let last_user = turns.iter().rposition(|m| m.role == Role::User);
    let (history, user_message) = match last_user {
        Some(idx) => {
            let mut history = turns;
            let mut tail = history.split_off(idx);
            let user = tail.remove(0);
            if !tail.is_empty() {
                tracing::warn!(
                    dropped = tail.len(),
                    "openai_compat: dropping assistant turns after the last user turn"
                );
            }
            (history, Some(user))
        }
        None => (turns, None),
    };

    Translated {
        system: if system_parts.is_empty() {
            None
        } else {
            Some(system_parts.join("\n\n"))
        },
        history,
        user_message,
    }
}

async fn list_models() -> Vec<String> {
    let config = Config::global();
    let mut ids = Vec::new();
    for (metadata, provider_type) in get_providers().await {
        if metadata.name == SWARM_PROVIDER || !check_provider_configured(&metadata, provider_type) {
            continue;
        }
        let mut names: Vec<String> = metadata
            .known_models
            .iter()
            .map(|m| m.name.clone())
            .collect();
        if let Some(saved) = crate::config::get_provider_entry(config, &metadata.name)
            .map(|e| e.model)
            .filter(|m| !m.is_empty())
        {
            if !names.contains(&saved) {
                names.insert(0, saved);
            }
        }
        for name in names {
            let id = format!("{}/{}", metadata.name, name);
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
    }
    ids.push(SWARM_CHAT_MODEL.to_string());
    ids.push(SWARM_BUILD_MODEL.to_string());
    ids
}

#[utoipa::path(
    get,
    path = "/v1/models",
    responses(
        (status = 200, description = "OpenAI-style model list", body = ModelList),
        (status = 401, description = "Unauthorized - invalid secret key")
    )
)]
async fn models() -> Json<ModelList> {
    let data = list_models()
        .await
        .into_iter()
        .map(|id| ModelObject {
            id,
            object: "model",
            owned_by: OWNED_BY,
        })
        .collect();
    Json(ModelList {
        object: "list",
        data,
    })
}

pub async fn model_is_known(model: &ModelRef) -> bool {
    get_providers()
        .await
        .into_iter()
        .any(|(metadata, provider_type)| {
            metadata.name == model.provider && check_provider_configured(&metadata, provider_type)
        })
}

pub fn working_dir() -> PathBuf {
    std::env::var(WORKING_DIR_ENV)
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(Paths::data_dir)
}

/// The same steps `POST /agent/start` takes for a fresh session, minus recipes and the background
/// extension load (a completion needs its tools before the turn starts, so the load is awaited).
pub async fn start_ephemeral_session(
    agents: &AgentManager,
    name: &str,
) -> Result<String, OpenAiError> {
    let manager = agents.session_manager();
    let config = Config::global();
    let mode = config.get_goose_mode().unwrap_or_default();
    let working_dir = working_dir();
    if let Err(e) = std::fs::create_dir_all(&working_dir) {
        tracing::warn!(dir = %working_dir.display(), error = %e, "openai_compat: cannot create working dir");
    }

    let session = manager
        .create_session(
            working_dir.clone(),
            name.to_string(),
            SessionType::User,
            mode,
        )
        .await
        .map_err(|e| OpenAiError::internal(format!("Failed to create session: {}", e)))?;

    let mut extensions = resolve_extensions_for_new_session(None, None);
    extensions.extend(crate::plugins::mcp_servers::enabled_plugin_mcp_servers(
        Some(&working_dir),
    ));
    let mut extension_data = session.extension_data.clone();
    if EnabledExtensionsState::new(extensions)
        .to_extension_data(&mut extension_data)
        .is_ok()
    {
        if let Err(e) = manager
            .update(&session.id)
            .extension_data(extension_data)
            .apply()
            .await
        {
            tracing::warn!(error = %e, "openai_compat: failed to save extension state");
        }
    }

    let session = manager
        .get_session(&session.id, false)
        .await
        .map_err(|e| OpenAiError::internal(format!("Failed to read session: {}", e)))?;
    let agent = agents
        .get_or_create_agent(session.id.clone())
        .await
        .map_err(|e| OpenAiError::internal(format!("Failed to create agent: {}", e)))?;
    let results = agent.load_extensions_from_session(&session).await;
    for r in results.iter().filter(|r| !r.success) {
        tracing::warn!(extension = %r.name, error = ?r.error, "openai_compat: extension failed to load");
    }
    Ok(session.id)
}

/// The same steps `POST /agent/update_provider` takes.
pub async fn apply_model(
    agents: &AgentManager,
    session_id: &str,
    model: &ModelRef,
) -> Result<(), OpenAiError> {
    let agent = agents
        .get_or_create_agent(session_id.to_string())
        .await
        .map_err(|e| OpenAiError::internal(format!("No agent for session: {}", e)))?;

    let mut model_config =
        crate::model_config::model_config_from_user_config(&model.provider, &model.model)
            .map_err(|e| OpenAiError::bad_request(format!("Invalid model config: {}", e)))?;
    let info = resolve_model_info(&model.provider, &model.model)
        .await
        .map_err(|e| OpenAiError::bad_request(e.to_string()))?;
    model_config.reasoning = Some(info.reasoning);

    let extensions =
        EnabledExtensionsState::for_session(agents.session_manager(), session_id, Config::global())
            .await;
    let provider = create(&model.provider, extensions).await.map_err(|e| {
        OpenAiError::bad_request(format!(
            "Failed to create {} provider: {}",
            model.provider, e
        ))
    })?;
    agent
        .update_provider(provider, model_config, session_id)
        .await
        .map_err(|e| OpenAiError::internal(format!("Failed to update provider: {}", e)))?;
    let mode = agent.goose_mode().await;
    agent
        .update_goose_mode(mode, session_id)
        .await
        .map_err(|e| OpenAiError::internal(format!("Failed to propagate mode: {}", e)))?;
    Ok(())
}

pub async fn cleanup_session(agents: &AgentManager, session_id: &str, keep: bool) {
    if let Err(e) = agents.remove_session_if_loaded(session_id).await {
        tracing::warn!(session_id, error = %e, "openai_compat: failed to unload agent");
    }
    if keep {
        return;
    }
    if let Err(e) = agents.session_manager().delete_session(session_id).await {
        tracing::warn!(session_id, error = %e, "openai_compat: failed to delete session");
    }
}

pub fn keep_session(headers: &HeaderMap) -> bool {
    headers
        .get(KEEP_SESSION_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(|v| matches!(v.trim(), "1" | "true" | "yes"))
        .unwrap_or(false)
}

/// The session's token counters, or zeros when the row cannot be read (a receipt never fails a
/// turn that already happened).
pub async fn session_token_state(session_manager: &SessionManager, session_id: &str) -> TokenState {
    session_manager
        .get_session(session_id, false)
        .await
        .map(|session| TokenState::from(&session))
        .inspect_err(|e| {
            tracing::warn!(
                "Failed to fetch session token state for {}: {}",
                session_id,
                e
            );
        })
        .unwrap_or_default()
}

pub fn usage_json(token_state: &TokenState) -> Value {
    json!({
        "prompt_tokens": token_state.input_tokens.max(0),
        "completion_tokens": token_state.output_tokens.max(0),
        "total_tokens": token_state.total_tokens.max(0),
    })
}

fn completion_json(id: &str, created: i64, model: &str, text: &str, usage: Value) -> Value {
    json!({
        "id": id,
        "object": "chat.completion",
        "created": created,
        "model": model,
        "choices": [{
            "index": 0,
            "message": { "role": "assistant", "content": text },
            "finish_reason": "stop"
        }],
        "usage": usage
    })
}

fn chunk_json(id: &str, created: i64, model: &str, delta: Value, finish: Option<&str>) -> Value {
    json!({
        "id": id,
        "object": "chat.completion.chunk",
        "created": created,
        "model": model,
        "choices": [{ "index": 0, "delta": delta, "finish_reason": finish }]
    })
}

enum TurnEvent {
    /// Assistant text with the message id it belongs to: goose streams one reply as many
    /// `Message` events sharing an id, so consecutive texts with the same id are deltas of one
    /// message and glue together; a new id is a new message.
    Text {
        id: Option<String>,
        text: String,
    },
    Error(String),
    Done(TokenState),
}

/// Folds streamed assistant texts into one completion body.
#[derive(Default)]
pub struct TextAccumulator {
    text: String,
    last_id: Option<String>,
}

impl TextAccumulator {
    pub fn push(&mut self, id: Option<String>, delta: &str) {
        let same_message = self.last_id.is_none() || id.is_none() || self.last_id == id;
        if !same_message && !self.text.is_empty() {
            self.text.push_str("\n\n");
        }
        self.text.push_str(delta);
        if id.is_some() {
            self.last_id = id;
        }
    }

    pub fn finish(self) -> String {
        self.text.trim().to_string()
    }
}

/// The agent turns a provider failure into an assistant message and ends the turn normally
/// (`crates/goose/src/agents/agent.rs`, the two `provider_errored` arms) — the desktop shows it as
/// a chat bubble. An OpenAI client must see an error, not a 200 with `finish_reason: stop` and a
/// stack trace as content, so those two fixed sign-offs are recognised and re-raised as errors.
const PROVIDER_ERROR_SIGN_OFFS: [&str; 2] = [
    "Please retry if you think this is a transient or recoverable error.",
    "Please resend your message to try again.",
];

pub fn is_provider_error_message(text: &str) -> bool {
    let text = text.trim_end();
    PROVIDER_ERROR_SIGN_OFFS
        .iter()
        .any(|sign_off| text.ends_with(sign_off))
}

/// Runs one turn on the session and forwards assistant text as it arrives.
async fn run_turn(
    agents: Arc<AgentManager>,
    session_id: String,
    translated: Translated,
    tx: mpsc::Sender<TurnEvent>,
) {
    let agent = match agents.get_or_create_agent(session_id.clone()).await {
        Ok(a) => a,
        Err(e) => {
            let _ = tx
                .send(TurnEvent::Error(format!("Failed to get agent: {}", e)))
                .await;
            return;
        }
    };
    if let Some(system) = translated.system {
        agent
            .extend_system_prompt(SYSTEM_PROMPT_KEY.to_string(), system)
            .await;
    }
    if !translated.history.is_empty() {
        let conv = Conversation::new_unvalidated(translated.history);
        if let Err(e) = agents
            .session_manager()
            .replace_conversation(&session_id, &conv)
            .await
        {
            tracing::warn!(error = %e, "openai_compat: failed to seed conversation history");
        }
    }
    let user_message = translated
        .user_message
        .unwrap_or_else(|| Message::user().with_text(""));

    let session = match agents
        .session_manager()
        .get_session(&session_id, false)
        .await
    {
        Ok(s) => s,
        Err(e) => {
            let _ = tx
                .send(TurnEvent::Error(format!("Failed to read session: {}", e)))
                .await;
            return;
        }
    };
    let session_config = SessionConfig {
        id: session_id.clone(),
        schedule_id: session.schedule_id.clone(),
        max_turns: None,
        retry_config: None,
    };
    let cancel = CancellationToken::new();
    let mut stream = match agent
        .reply(user_message, session_config, Some(cancel.clone()))
        .await
    {
        Ok(s) => s,
        Err(e) => {
            let _ = tx.send(TurnEvent::Error(e.to_string())).await;
            return;
        }
    };

    while let Some(event) = stream.next().await {
        match event {
            Ok(AgentEvent::Message(message)) => {
                if message.role == Role::Assistant {
                    let text = message.as_concat_text();
                    if text.is_empty() {
                        continue;
                    }
                    if is_provider_error_message(&text) {
                        let _ = tx.send(TurnEvent::Error(text)).await;
                        cancel.cancel();
                        return;
                    }
                    let event = TurnEvent::Text {
                        id: message.id.clone(),
                        text,
                    };
                    if tx.send(event).await.is_err() {
                        cancel.cancel();
                        return;
                    }
                }
            }
            Ok(AgentEvent::Usage(_))
            | Ok(AgentEvent::HistoryReplaced(_))
            | Ok(AgentEvent::McpNotification(_)) => {}
            Err(e) => {
                let _ = tx.send(TurnEvent::Error(e.to_string())).await;
                return;
            }
        }
    }
    let token_state = session_token_state(agents.session_manager(), &session_id).await;
    let _ = tx.send(TurnEvent::Done(token_state)).await;
}

#[utoipa::path(
    post,
    path = "/v1/chat/completions",
    request_body = ChatCompletionRequest,
    responses(
        (status = 200, description = "OpenAI-style chat completion (JSON, or SSE when stream=true)"),
        (status = 400, description = "Bad request"),
        (status = 401, description = "Unauthorized - invalid secret key"),
        (status = 404, description = "Unknown model"),
        (status = 500, description = "Internal server error")
    )
)]
async fn chat_completions(
    State(source): State<AgentManagerSource>,
    headers: HeaderMap,
    body: Result<Json<ChatCompletionRequest>, JsonRejection>,
) -> Result<Response, OpenAiError> {
    let Json(request) = body.map_err(|e| OpenAiError::bad_request(e.body_text()))?;
    if request.tools.is_some() {
        tracing::debug!("openai_compat: request carried tools; ignored (goose's own tools run)");
    }
    if request.temperature.is_some() || request.max_tokens.is_some() {
        tracing::debug!("openai_compat: temperature/max_tokens are ignored");
    }
    let model_ref = split_model_id(&request.model).ok_or_else(OpenAiError::model_not_found)?;
    if !model_is_known(&model_ref).await {
        return Err(OpenAiError::model_not_found());
    }
    let translated = translate_messages(&request.messages, request.response_format.as_ref());
    if translated.user_message.is_none() {
        return Err(OpenAiError::bad_request(
            "messages must contain at least one user message",
        ));
    }

    let keep = keep_session(&headers);
    let agents = source
        .manager()
        .await
        .map_err(|e| OpenAiError::internal(format!("No agent manager: {}", e)))?;
    let session_id = start_ephemeral_session(&agents, "OpenAI-compatible request").await?;
    if let Err(e) = apply_model(&agents, &session_id, &model_ref).await {
        cleanup_session(&agents, &session_id, keep).await;
        return Err(e);
    }

    let id = format!("chatcmpl-{}", uuid::Uuid::new_v4());
    let created = chrono::Utc::now().timestamp();
    let model_name = request.model.clone();

    let (tx, mut rx) = mpsc::channel::<TurnEvent>(64);
    let turn_agents = agents.clone();
    let turn_session = session_id.clone();
    // Listed as an external client's work for exactly the turn's life (the desktop's MLX tile and
    // tray attribute engine requests by it; see providers::mlx_serving).
    let serving = crate::providers::mlx_serving::register(
        crate::providers::mlx_serving::ServingVia::OpenaiApi,
        Some(session_id.clone()),
        &model_ref.provider,
        &model_ref.model,
        None,
        None,
    );
    drop(tokio::spawn(async move {
        let _serving = serving;
        run_turn(turn_agents, turn_session, translated, tx).await
    }));

    if request.stream {
        let (sse_tx, sse_rx) = mpsc::channel::<String>(64);
        let agents_for_cleanup = agents.clone();
        drop(tokio::spawn(async move {
            let role_chunk = chunk_json(
                &id,
                created,
                &model_name,
                json!({ "role": "assistant", "content": "" }),
                None,
            );
            let _ = sse_tx.send(format!("data: {}\n\n", role_chunk)).await;
            let mut ping = tokio::time::interval(Duration::from_secs(15));
            ping.tick().await;
            loop {
                tokio::select! {
                    _ = ping.tick() => {
                        if sse_tx.send(": ping\n\n".to_string()).await.is_err() {
                            break;
                        }
                    }
                    event = rx.recv() => match event {
                        Some(TurnEvent::Text { text, .. }) => {
                            let chunk = chunk_json(&id, created, &model_name, json!({ "content": text }), None);
                            if sse_tx.send(format!("data: {}\n\n", chunk)).await.is_err() {
                                break;
                            }
                        }
                        Some(TurnEvent::Error(message)) => {
                            let err = OpenAiError::internal(message).body();
                            let _ = sse_tx.send(format!("data: {}\n\n", err)).await;
                            let _ = sse_tx.send("data: [DONE]\n\n".to_string()).await;
                            break;
                        }
                        Some(TurnEvent::Done(token_state)) => {
                            let mut last = chunk_json(&id, created, &model_name, json!({}), Some("stop"));
                            last["usage"] = usage_json(&token_state);
                            let _ = sse_tx.send(format!("data: {}\n\n", last)).await;
                            let _ = sse_tx.send("data: [DONE]\n\n".to_string()).await;
                            break;
                        }
                        None => {
                            let _ = sse_tx.send("data: [DONE]\n\n".to_string()).await;
                            break;
                        }
                    }
                }
            }
            cleanup_session(&agents_for_cleanup, &session_id, keep).await;
        }));
        let body =
            Body::from_stream(ReceiverStream::new(sse_rx).map(Ok::<_, std::convert::Infallible>));
        return http::Response::builder()
            .status(StatusCode::OK)
            .header("Content-Type", "text/event-stream")
            .header("Cache-Control", "no-cache")
            .header("Connection", "keep-alive")
            .body(body)
            .map_err(|e| OpenAiError::internal(e.to_string()));
    }

    let mut text = TextAccumulator::default();
    let mut outcome: Result<TokenState, OpenAiError> = Ok(TokenState::default());
    while let Some(event) = rx.recv().await {
        match event {
            TurnEvent::Text { id, text: delta } => text.push(id, &delta),
            TurnEvent::Error(message) => {
                outcome = Err(OpenAiError::internal(message));
                break;
            }
            TurnEvent::Done(token_state) => {
                outcome = Ok(token_state);
                break;
            }
        }
    }
    cleanup_session(&agents, &session_id, keep).await;
    let token_state = outcome?;
    let body = completion_json(
        &id,
        created,
        &model_name,
        &text.finish(),
        usage_json(&token_state),
    );
    Ok(Json(body).into_response())
}

pub fn routes(source: AgentManagerSource) -> Router {
    Router::new()
        .route("/v1/models", get(models))
        .route("/v1/chat/completions", post(chat_completions))
        .with_state(source)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(role: &str, content: Value) -> OpenAiMessage {
        OpenAiMessage {
            role: role.to_string(),
            content: Some(content),
        }
    }

    #[test]
    fn split_model_id_uses_the_first_slash() {
        assert_eq!(
            split_model_id("openrouter/anthropic/claude-sonnet-4"),
            Some(ModelRef {
                provider: "openrouter".into(),
                model: "anthropic/claude-sonnet-4".into()
            })
        );
        assert_eq!(
            split_model_id("lmstudio/qwen3-coder"),
            Some(ModelRef {
                provider: "lmstudio".into(),
                model: "qwen3-coder".into()
            })
        );
    }

    #[test]
    fn split_model_id_maps_swarm_ids_to_the_swarm_provider() {
        assert_eq!(
            split_model_id("swarm"),
            Some(ModelRef {
                provider: "swarm".into(),
                model: "swarm".into()
            })
        );
        assert_eq!(
            split_model_id("swarm-build"),
            Some(ModelRef {
                provider: "swarm".into(),
                model: "swarm-build".into()
            })
        );
    }

    #[test]
    fn split_model_id_rejects_ids_without_a_provider() {
        assert_eq!(split_model_id("gpt-5.4-mini"), None);
        assert_eq!(split_model_id("/model"), None);
        assert_eq!(split_model_id("provider/"), None);
        assert_eq!(split_model_id(""), None);
    }

    #[test]
    fn translate_string_and_parts_content() {
        let messages = vec![
            msg("system", json!("You are terse.")),
            msg("user", json!("hello")),
            msg("assistant", json!("hi")),
            msg(
                "user",
                json!([
                    {"type": "text", "text": "look at this"},
                    {"type": "image_url", "image_url": {"url": "data:image/png;base64,AAAA"}},
                    {"type": "text", "text": "and that"}
                ]),
            ),
        ];
        let t = translate_messages(&messages, None);
        assert_eq!(t.system.as_deref(), Some("You are terse."));
        assert_eq!(t.history.len(), 2);
        assert_eq!(t.history[0].role, Role::User);
        assert_eq!(t.history[0].as_concat_text(), "hello");
        assert_eq!(t.history[1].role, Role::Assistant);
        assert_eq!(t.history[1].as_concat_text(), "hi");
        let user = t.user_message.expect("last user turn");
        assert_eq!(user.role, Role::User);
        assert_eq!(user.as_concat_text(), "look at this\nand that");
    }

    #[test]
    fn translate_appends_json_instruction_for_json_response_format() {
        let messages = vec![msg("user", json!("give me json"))];
        let t = translate_messages(&messages, Some(&json!({"type": "json_object"})));
        assert_eq!(t.system.as_deref(), Some(JSON_ONLY_INSTRUCTION));

        let messages = vec![
            msg("system", json!("Be brief.")),
            msg("user", json!("give me json")),
        ];
        let t = translate_messages(&messages, Some(&json!({"type": "json_schema"})));
        assert_eq!(
            t.system.as_deref(),
            Some(format!("Be brief.\n\n{}", JSON_ONLY_INSTRUCTION).as_str())
        );

        let t = translate_messages(&messages, Some(&json!({"type": "text"})));
        assert_eq!(t.system.as_deref(), Some("Be brief."));
    }

    #[test]
    fn translate_without_a_user_turn_yields_no_user_message() {
        let messages = vec![msg("system", json!("x")), msg("assistant", json!("y"))];
        let t = translate_messages(&messages, None);
        assert!(t.user_message.is_none());
        assert_eq!(t.history.len(), 1);
    }

    #[test]
    fn translate_drops_tool_messages_and_trailing_assistant_turns() {
        let messages = vec![
            msg("user", json!("a")),
            msg("tool", json!("tool output")),
            msg("assistant", json!("partial")),
        ];
        let t = translate_messages(&messages, None);
        assert!(t.history.is_empty());
        assert_eq!(t.user_message.unwrap().as_concat_text(), "a");
    }

    #[test]
    fn a_provider_failure_the_agent_phrased_as_a_message_is_an_error() {
        assert!(is_provider_error_message(
            "Ran into this error: Server error: boom.\n\nPlease retry if you think this is a transient or recoverable error."
        ));
        assert!(is_provider_error_message(
            "Rate limited.\n\nPlease resend your message to try again.\n"
        ));
        assert!(!is_provider_error_message("The sky is blue."));
        assert!(!is_provider_error_message(
            "Please retry if you think this is a transient or recoverable error. Anyway, blue."
        ));
    }

    #[test]
    fn streamed_deltas_of_one_message_glue_and_a_new_message_starts_a_paragraph() {
        let mut acc = TextAccumulator::default();
        acc.push(Some("m1".into()), "A");
        acc.push(Some("m1".into()), " clear");
        acc.push(Some("m1".into()), " sky.");
        acc.push(Some("m2".into()), "Second message.");
        assert_eq!(acc.finish(), "A clear sky.\n\nSecond message.");

        let mut acc = TextAccumulator::default();
        acc.push(None, "\n\nP");
        acc.push(None, "ong");
        assert_eq!(acc.finish(), "Pong");
    }

    #[test]
    fn completion_shape_matches_openai() {
        let body = completion_json(
            "chatcmpl-1",
            7,
            "lmstudio/m",
            "hi",
            usage_json(&TokenState::default()),
        );
        assert_eq!(body["object"], "chat.completion");
        assert_eq!(body["choices"][0]["message"]["role"], "assistant");
        assert_eq!(body["choices"][0]["message"]["content"], "hi");
        assert_eq!(body["choices"][0]["finish_reason"], "stop");
        assert_eq!(body["usage"]["total_tokens"], 0);
        let chunk = chunk_json("chatcmpl-1", 7, "m", json!({"content": "x"}), Some("stop"));
        assert_eq!(chunk["object"], "chat.completion.chunk");
        assert_eq!(chunk["choices"][0]["finish_reason"], "stop");
    }
}
