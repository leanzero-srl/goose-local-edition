use crate::routes::errors::ErrorResponse;
use crate::routes::reply::{get_token_state, track_tool_telemetry, MessageEvent};
use crate::session_delta_tap::SessionDeltaMsg;
use crate::session_event_bus::RequestGuard;
use crate::state::AppState;
use axum::{
    extract::{DefaultBodyLimit, Path, State},
    http::{self, HeaderMap},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use bytes::Bytes;
use futures::{stream::StreamExt, Stream};
use goose::agents::{AgentEvent, SessionConfig};
use goose::conversation::message::Message;
use goose::conversation::Conversation;
use goose::execution::manager::AgentManager;
use serde::{Deserialize, Serialize};
use std::{
    convert::Infallible,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};
use tokio::sync::mpsc;
use tokio::time::timeout;
use tokio_stream::wrappers::ReceiverStream;

// ── Request / Response types ────────────────────────────────────────────

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct SessionReplyRequest {
    /// Client-generated UUIDv7 identifying this request.
    pub request_id: String,
    pub user_message: Message,
    #[serde(default)]
    pub override_conversation: Option<Vec<Message>>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct SessionReplyResponse {
    pub request_id: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct CancelRequest {
    pub request_id: String,
}

// ── SSE Event Stream Response ───────────────────────────────────────────

/// An SSE response that includes `id:` lines for Last-Event-ID reconnection.
pub struct SseEventStream {
    rx: ReceiverStream<String>,
}

impl SseEventStream {
    fn new(rx: ReceiverStream<String>) -> Self {
        Self { rx }
    }
}

impl Stream for SseEventStream {
    type Item = Result<Bytes, Infallible>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.rx)
            .poll_next(cx)
            .map(|opt| opt.map(|s| Ok(Bytes::from(s))))
    }
}

impl IntoResponse for SseEventStream {
    fn into_response(self) -> axum::response::Response {
        let body = axum::body::Body::from_stream(self);
        http::Response::builder()
            .header("Content-Type", "text/event-stream")
            .header("Cache-Control", "no-cache")
            .header("Connection", "keep-alive")
            .body(body)
            .unwrap()
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────

fn format_sse_event(seq: u64, json: &str) -> String {
    format!("id: {}\ndata: {}\n\n", seq, json)
}

fn serialize_session_event(seq: u64, request_id: Option<&str>, event: &MessageEvent) -> String {
    // Build JSON payload: { request_id?: string, ...event_fields }
    // We flatten request_id into the event JSON.
    let mut event_json = serde_json::to_value(event).unwrap_or_else(
        |e| serde_json::json!({"type": "Error", "error": format!("Serialization error: {}", e)}),
    );

    if let Some(rid) = request_id {
        if let serde_json::Value::Object(ref mut map) = event_json {
            // Always insert chat_request_id for routing (the chat UUID that
            // the frontend registered its listener under).
            map.insert(
                "chat_request_id".to_string(),
                serde_json::Value::String(rid.to_string()),
            );
            // Also set request_id if the event doesn't already carry one
            // (e.g. Notification events have their own request_id for tool-call matching)
            map.entry("request_id")
                .or_insert_with(|| serde_json::Value::String(rid.to_string()));
        }
    }

    let json_str = serde_json::to_string(&event_json).unwrap_or_default();
    format_sse_event(seq, &json_str)
}

// ── GET /sessions/{id}/events ───────────────────────────────────────────

#[utoipa::path(
    get,
    path = "/sessions/{id}/events",
    params(
        ("id" = String, Path, description = "Session ID"),
    ),
    responses(
        (status = 200, description = "SSE event stream",
         body = MessageEvent,
         content_type = "text/event-stream"),
        (status = 404, description = "Session not found"),
    )
)]
pub async fn session_events(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
    headers: HeaderMap,
) -> Result<SseEventStream, axum::http::StatusCode> {
    // Validate the session exists before creating an event bus.
    state
        .session_manager()
        .get_session(&session_id, false)
        .await
        .map_err(|_| axum::http::StatusCode::NOT_FOUND)?;

    let last_event_id: Option<u64> = headers
        .get("Last-Event-ID")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok());

    let bus = state.get_or_create_event_bus(&session_id).await;

    let (replay, replay_max_seq, mut live_rx) = match bus.subscribe(last_event_id).await {
        Ok(result) => result,
        Err(_) => {
            // Client's Last-Event-ID has been evicted from the replay buffer.
            // Send a single error event so the client knows to reload.
            let (tx, rx) = mpsc::channel::<String>(1);
            let stream = ReceiverStream::new(rx);
            let seq = 0;
            let error_event = MessageEvent::Error {
                error: "Client too far behind — reload conversation".to_string(),
            };
            let frame = serialize_session_event(seq, None, &error_event);
            tokio::spawn(async move {
                let _ = tx.send(frame).await;
            });
            return Ok(SseEventStream::new(stream));
        }
    };

    let (tx, rx) = mpsc::channel::<String>(256);
    let stream = ReceiverStream::new(rx);
    let task_bus = bus.clone();

    tokio::spawn(async move {
        let bus = task_bus;

        // Notify the client about any in-flight requests BEFORE replay
        // so it can register event handlers before replayed events arrive.
        // Emitted without an SSE `id:` field so it doesn't regress the
        // client's Last-Event-ID cursor.
        let active_ids = bus.active_request_ids().await;
        if !active_ids.is_empty() {
            let event = MessageEvent::ActiveRequests {
                request_ids: active_ids,
            };
            let json_str = serde_json::to_string(&serde_json::to_value(&event).unwrap_or_default())
                .unwrap_or_default();
            let frame = format!("data: {}\n\n", json_str);
            if tx.send(frame).await.is_err() {
                return;
            }
        }

        // Send replayed events
        for event in &replay {
            let frame =
                serialize_session_event(event.seq, event.request_id.as_deref(), &event.event);
            if tx.send(frame).await.is_err() {
                return;
            }
        }

        // Send live events + heartbeat pings
        let mut heartbeat_interval = tokio::time::interval(Duration::from_millis(500));
        // Heartbeat uses a local counter — not stored in the replay buffer
        let mut heartbeat_seq = 0u64;

        loop {
            tokio::select! {
                _ = heartbeat_interval.tick() => {
                    // Send heartbeat directly without publishing to the bus,
                    // so pings don't evict real events from the replay buffer.
                    // Use a comment-style SSE id so it won't interfere with Last-Event-ID.
                    let frame = format!(": ping {}\n\n", heartbeat_seq);
                    heartbeat_seq += 1;
                    if tx.send(frame).await.is_err() {
                        return;
                    }
                }
                result = live_rx.recv() => {
                    match result {
                        Ok(event) => {
                            // Skip events already covered by replay to avoid duplicates
                            // at the replay/live handoff boundary.
                            if event.seq <= replay_max_seq {
                                continue;
                            }
                            let frame = serialize_session_event(
                                event.seq,
                                event.request_id.as_deref(),
                                &event.event,
                            );
                            if tx.send(frame).await.is_err() {
                                return;
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            tracing::warn!("SSE subscriber lagged by {} events, closing stream so client reconnects with Last-Event-ID", n);
                            // Close the stream so the client reconnects and
                            // replays missed events from the buffer.
                            return;
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                            return;
                        }
                    }
                }
            }
        }
    });

    Ok(SseEventStream::new(stream))
}

// ── POST /sessions/{id}/reply ───────────────────────────────────────────

#[utoipa::path(
    post,
    path = "/sessions/{id}/reply",
    params(
        ("id" = String, Path, description = "Session ID"),
    ),
    request_body = SessionReplyRequest,
    responses(
        (status = 200, description = "Request accepted",
         body = SessionReplyResponse),
        (status = 400, description = "Invalid request"),
        (status = 404, description = "Session not found"),
        (status = 424, description = "Agent not initialized"),
        (status = 500, description = "Internal server error"),
    )
)]
pub async fn session_reply(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
    Json(request): Json<SessionReplyRequest>,
) -> Result<Json<SessionReplyResponse>, ErrorResponse> {
    let request_id = request.request_id.clone();

    // Validate request_id is a valid UUID
    if uuid::Uuid::parse_str(&request_id).is_err() {
        return Err(ErrorResponse::bad_request(
            "request_id must be a valid UUID",
        ));
    }

    // Validate session exists before allocating a bus/registering work
    let session_data = state
        .session_manager()
        .get_session(&session_id, false)
        .await
        .map_err(|_| ErrorResponse::not_found(format!("Session {} not found", session_id)))?;

    tracing::info!(
        monotonic_counter.goose.session_starts = 1,
        session_type = "app",
        interface = "ui",
        "Session started"
    );

    if let Some(ref recipe) = session_data.recipe {
        if state.mark_recipe_run_if_absent(&session_id).await {
            tracing::info!(
                monotonic_counter.goose.recipe_runs = 1,
                recipe_name = %recipe.title,
                recipe_version = %recipe.version,
                session_type = "app",
                interface = "ui",
                "Recipe execution started"
            );
        }
    }

    let user_message = request.user_message;
    let override_conversation = request.override_conversation;

    // An elicitation response unblocks an in-flight tool call that is already
    // streaming on another request_id — don't register a new active request or
    // open a new SSE stream; route it to the agent's short-circuit path.
    let is_elicitation_response = user_message.content.iter().any(|c| {
        matches!(
            c,
            goose::conversation::message::MessageContent::ActionRequired(ar)
                if matches!(
                    ar.data,
                    goose::conversation::message::ActionRequiredData::ElicitationResponse { .. }
                )
        )
    });

    if is_elicitation_response {
        let agent = state.get_agent_for_route(session_id.clone()).await?;
        let session_config = goose::agents::types::SessionConfig {
            id: session_id.clone(),
            schedule_id: session_data.schedule_id.clone(),
            max_turns: None,
            retry_config: None,
        };
        let _ = agent
            .reply(user_message, session_config, None)
            .await
            .map_err(|e| ErrorResponse::internal(e.to_string()))?;
        return Ok(Json(SessionReplyResponse { request_id }));
    }

    if let Err(ReplyDispatchError::AlreadyActive) = spawn_reply_task(
        state.clone(),
        session_id.clone(),
        request_id.clone(),
        user_message,
        override_conversation,
    )
    .await
    {
        return Err(ErrorResponse::bad_request(
            "Session already has an active request. Cancel it first.",
        ));
    }

    Ok(Json(SessionReplyResponse { request_id }))
}

/// The dispatch outcome of [`spawn_reply_task`]: the session already had an in-flight
/// request, so nothing new was started (the HTTP route maps this to `400`, the LeanZero
/// Link executor maps it to `ExecuteError::Busy`).
#[derive(Debug)]
pub enum ReplyDispatchError {
    AlreadyActive,
}

/// Releases the session's [`AgentManager`] cancel token when the reply task ends by ANY
/// path — finish, error return, cancel, panic — the way [`RequestGuard`] releases the
/// bus request. The token map is the busy set the LeanZero Link idle guard reads; a
/// token left behind would report this node Busy forever and refuse every peer.
struct AgentBusyGuard {
    manager: Arc<AgentManager>,
    session_id: String,
}

impl Drop for AgentBusyGuard {
    fn drop(&mut self) {
        let manager = self.manager.clone();
        let session_id = self.session_id.clone();
        tokio::spawn(async move {
            manager.unregister_cancel_token(&session_id).await;
        });
    }
}

/// The shared reply-dispatch core: register `request_id` on the session's event bus and
/// spawn the agent reply task that streams every `MessageEvent` to the per-session bus AND
/// the process-wide delta tap (the LeanZero Link mesh mirror). Both
/// `POST /sessions/{id}/reply` and the LeanZero Link remote executor
/// (`GoosedRemoteExecutor::execute`) drive a run through THIS — neither reimplements the
/// agent loop. The session must already exist (both callers validate that first).
pub async fn spawn_reply_task(
    state: Arc<AppState>,
    session_id: String,
    request_id: String,
    user_message: Message,
    override_conversation: Option<Vec<Message>>,
) -> Result<(), ReplyDispatchError> {
    let session_start = std::time::Instant::now();

    let bus = state.get_or_create_event_bus(&session_id).await;

    let cancel_token = bus
        .try_register_request(request_id.clone())
        .await
        .map_err(|_| ReplyDispatchError::AlreadyActive)?;

    // Both doors into a reply — this one and the ACP `on_prompt` path — register the run
    // in the AgentManager's token map, the busy set the LeanZero Link idle guard and
    // `cancel_session` read. The bus registration above is per-request; this one is
    // per-session, and a token already present means another door (an ACP prompt run,
    // a subagent) holds the session: refuse as AlreadyActive and release the bus
    // request just taken, never run two replies on one session.
    if let Err(error) = state
        .agent_manager
        .try_register_cancel_token(&session_id, cancel_token.clone())
        .await
    {
        tracing::warn!(%session_id, %error, "reply refused: the session is busy in another run");
        bus.cleanup_request(&request_id).await;
        return Err(ReplyDispatchError::AlreadyActive);
    }

    let task_state = state.clone();
    let task_session_id = session_id.clone();
    let task_request_id = request_id.clone();
    let task_cancel = cancel_token.clone();
    let task_bus = bus.clone();

    drop(tokio::spawn(async move {
        let mut _guard = RequestGuard::new(task_bus.clone(), task_request_id.clone());
        let _busy_guard = AgentBusyGuard {
            manager: task_state.agent_manager.clone(),
            session_id: task_session_id.clone(),
        };

        let publish = |rid: Option<String>, event: MessageEvent| {
            let bus = task_bus.clone();
            let delta_tap = task_state.session_delta_tap();
            let session_id = task_session_id.clone();
            async move {
                // Per-session bus publish is unchanged — this returns the origin-scoped
                // seq the mesh mirror stamps onto SessionDelta.seq.
                let seq = bus.publish(rid, event.clone()).await;
                // ADDITIVE mesh tap: fan the same event out for cross-device mirroring.
                // Non-fallible — no subscriber (mesh not connected) or a lagged one yields
                // Err/drop, never blocking or failing the reply path.
                let _ = delta_tap.send(SessionDeltaMsg {
                    session_id,
                    seq,
                    event,
                });
            }
        };

        let agent = match task_state.get_agent(task_session_id.clone()).await {
            Ok(agent) => agent,
            Err(e) => {
                tracing::error!("Failed to get session agent: {}", e);
                publish(
                    Some(task_request_id.clone()),
                    MessageEvent::Error {
                        error: format!("Failed to get session agent: {}", e),
                    },
                )
                .await;
                return;
            }
        };

        let session = match task_state
            .session_manager()
            .get_session(&task_session_id, true)
            .await
        {
            Ok(metadata) => metadata,
            Err(e) => {
                tracing::error!("Failed to read session for {}: {}", task_session_id, e);
                publish(
                    Some(task_request_id.clone()),
                    MessageEvent::Error {
                        error: format!("Failed to read session: {}", e),
                    },
                )
                .await;
                return;
            }
        };

        let session_config = SessionConfig {
            id: task_session_id.clone(),
            schedule_id: session.schedule_id.clone(),
            max_turns: None,
            retry_config: None,
        };

        let mut all_messages = match override_conversation {
            Some(history) => {
                let conv = Conversation::new_unvalidated(history);
                if let Err(e) = task_state
                    .session_manager()
                    .replace_conversation(&task_session_id, &conv)
                    .await
                {
                    tracing::warn!(
                        "Failed to replace session conversation for {}: {}",
                        task_session_id,
                        e
                    );
                }
                conv
            }
            None => session.conversation.unwrap_or_default(),
        };
        all_messages.push(user_message.clone());

        let mut stream = match agent
            .reply(
                user_message.clone(),
                session_config,
                Some(task_cancel.clone()),
            )
            .await
        {
            Ok(stream) => stream,
            Err(e) => {
                tracing::error!("Failed to start reply stream: {:?}", e);
                publish(
                    Some(task_request_id.clone()),
                    MessageEvent::Error {
                        error: e.to_string(),
                    },
                )
                .await;
                return;
            }
        };

        loop {
            tokio::select! {
                _ = task_cancel.cancelled() => {
                    tracing::info!("Agent task cancelled for request {}", task_request_id);
                    break;
                }
                response = timeout(Duration::from_millis(500), stream.next()) => {
                    match response {
                        Ok(Some(Ok(AgentEvent::Message(message)))) => {
                            for content in &message.content {
                                track_tool_telemetry(content, all_messages.messages());
                            }
                            all_messages.push(message.clone());
                            let token_state = get_token_state(
                                task_state.session_manager(),
                                &task_session_id,
                            )
                            .await;
                            publish(
                                Some(task_request_id.clone()),
                                MessageEvent::Message {
                                    message,
                                    token_state,
                                },
                            )
                            .await;
                        }
                        Ok(Some(Ok(AgentEvent::Usage(_)))) => {}
                        Ok(Some(Ok(AgentEvent::HistoryReplaced(new_messages)))) => {
                            all_messages = new_messages.clone();
                            publish(
                                Some(task_request_id.clone()),
                                MessageEvent::UpdateConversation {
                                    conversation: new_messages,
                                },
                            )
                            .await;
                        }
                        Ok(Some(Ok(AgentEvent::McpNotification((notification_request_id, n))))) => {
                            publish(
                                Some(task_request_id.clone()),
                                MessageEvent::Notification {
                                    request_id: notification_request_id,
                                    message: n,
                                },
                            )
                            .await;
                        }
                        Ok(Some(Err(e))) => {
                            tracing::error!("Error processing message: {}", e);
                            publish(
                                Some(task_request_id.clone()),
                                MessageEvent::Error {
                                    error: e.to_string(),
                                },
                            )
                            .await;
                            break;
                        }
                        Ok(None) => {
                            break;
                        }
                        Err(_) => {
                            // Timeout — check if the bus still has subscribers
                            continue;
                        }
                    }
                }
            }
        }

        // Telemetry
        let session_duration = session_start.elapsed();

        if let Ok(session) = task_state
            .session_manager()
            .get_session(&task_session_id, true)
            .await
        {
            let total_tokens = session.usage.total_tokens.unwrap_or(0);
            tracing::info!(
                monotonic_counter.goose.session_completions = 1,
                session_type = "app",
                interface = "ui",
                exit_type = "normal",
                duration_ms = session_duration.as_millis() as u64,
                total_tokens = total_tokens,
                message_count = session.message_count,
                "Session completed"
            );

            tracing::info!(
                monotonic_counter.goose.session_duration_ms = session_duration.as_millis() as u64,
                session_type = "app",
                interface = "ui",
                "Session duration"
            );

            if total_tokens > 0 {
                tracing::info!(
                    monotonic_counter.goose.session_tokens = total_tokens,
                    session_type = "app",
                    interface = "ui",
                    "Session tokens"
                );
            }
        } else {
            tracing::info!(
                monotonic_counter.goose.session_completions = 1,
                session_type = "app",
                interface = "ui",
                exit_type = "normal",
                duration_ms = session_duration.as_millis() as u64,
                total_tokens = 0u64,
                message_count = all_messages.len(),
                "Session completed"
            );

            tracing::info!(
                monotonic_counter.goose.session_duration_ms = session_duration.as_millis() as u64,
                session_type = "app",
                interface = "ui",
                "Session duration"
            );
        }

        let final_token_state =
            get_token_state(task_state.session_manager(), &task_session_id).await;

        publish(
            Some(task_request_id.clone()),
            MessageEvent::Finish {
                reason: "stop".to_string(),
                token_state: final_token_state,
            },
        )
        .await;

        // Release the busy registration inline on the normal path so the next reply on
        // this session is not refused by a token the guard's spawned drop has not yet
        // removed; the guard stays as the net for every other exit.
        task_state
            .agent_manager
            .unregister_cancel_token(&task_session_id)
            .await;
        _guard.disarm();
        task_bus.cleanup_request(&task_request_id).await;
    }));

    Ok(())
}

// ── POST /sessions/{id}/cancel ──────────────────────────────────────────

#[utoipa::path(
    post,
    path = "/sessions/{id}/cancel",
    params(
        ("id" = String, Path, description = "Session ID"),
    ),
    request_body = CancelRequest,
    responses(
        (status = 200, description = "Cancellation accepted"),
    )
)]
pub async fn session_cancel(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
    Json(request): Json<CancelRequest>,
) -> axum::http::StatusCode {
    let bus = match state.get_event_bus(&session_id).await {
        Some(bus) => bus,
        None => return axum::http::StatusCode::NOT_FOUND,
    };
    bus.cancel_request(&request.request_id).await;
    axum::http::StatusCode::OK
}

// ── Route registration ──────────────────────────────────────────────────

pub fn routes(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/sessions/{id}/events", get(session_events))
        .route(
            "/sessions/{id}/reply",
            post(session_reply).layer(DefaultBodyLimit::max(50 * 1024 * 1024)),
        )
        .route("/sessions/{id}/cancel", post(session_cancel))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use goose::config::GooseMode;
    use goose::session::session_manager::SessionType;
    use tokio_util::sync::CancellationToken;

    /// The goose-server reply door: a session whose AgentManager token is held by the
    /// OTHER door (an ACP prompt run, a subagent) is refused as `AlreadyActive`, and the
    /// bus request taken for it is released — never two replies on one session.
    #[tokio::test(flavor = "multi_thread")]
    async fn spawn_reply_task_refuses_a_session_busy_in_another_run() {
        let state = AppState::new(true).await.unwrap();
        let session = state
            .session_manager()
            .create_session(
                std::env::temp_dir(),
                "busy elsewhere".to_string(),
                SessionType::User,
                GooseMode::default(),
            )
            .await
            .unwrap();
        state
            .agent_manager
            .try_register_cancel_token(&session.id, CancellationToken::new())
            .await
            .unwrap();

        let request_id = uuid::Uuid::new_v4().to_string();
        let outcome = spawn_reply_task(
            state.clone(),
            session.id.clone(),
            request_id.clone(),
            Message::user().with_text("hi"),
            None,
        )
        .await;
        assert!(matches!(outcome, Err(ReplyDispatchError::AlreadyActive)));

        let bus = state
            .get_event_bus(&session.id)
            .await
            .expect("the bus was allocated before the refusal");
        assert!(
            !bus.active_request_ids().await.contains(&request_id),
            "the refused request must not stay registered on the bus"
        );
        // The other door's token is untouched by the refusal.
        assert!(state.agent_manager.is_session_busy(&session.id).await);
        state
            .agent_manager
            .unregister_cancel_token(&session.id)
            .await;
    }

    /// The guard that rides the spawned reply task releases the busy registration on drop,
    /// whichever way the task ends.
    #[tokio::test(flavor = "multi_thread")]
    async fn agent_busy_guard_releases_the_token_on_drop() {
        let state = AppState::new(true).await.unwrap();
        let session_id = format!("guard-{}", uuid::Uuid::new_v4());
        state
            .agent_manager
            .try_register_cancel_token(&session_id, CancellationToken::new())
            .await
            .unwrap();
        assert!(state.agent_manager.is_session_busy(&session_id).await);

        drop(AgentBusyGuard {
            manager: state.agent_manager.clone(),
            session_id: session_id.clone(),
        });
        // The drop spawns the release onto the runtime; poll for it to land (a yield is
        // not enough when the spawned task is scheduled on a busy worker thread).
        for _ in 0..200 {
            if !state.agent_manager.is_session_busy(&session_id).await {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("the busy token was not released after the guard dropped");
    }
}
