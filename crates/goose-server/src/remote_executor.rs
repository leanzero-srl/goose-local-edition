//! goose-server's implementation of the LeanZero Link [`RemoteExecutor`] seam.
//!
//! `crates/goose` hosts the control service + `LinkManager`, but the reply machinery that
//! actually runs a goose prompt (create/resolve a session, register a request, spawn the
//! agent task, stream `MessageEvent`s to the bus AND the mesh delta tap) lives HERE, in
//! goose-server's routes over its [`AppState`]. So goose-server implements the executor
//! and injects it into `goose` at boot via `set_executor` (mirroring the delta-tap
//! injection). Once wired, a same-account peer's `POST /v1/swarm/execute` drives a real
//! run on this machine and its per-message deltas mirror back over `/v1/swarm/stream` —
//! there is NO separate result channel.
//!
//! SECURITY: this runs goose (shell/file tools) on this machine for a remote caller. The
//! gates that make that safe (tailnet membership + the account node_token bearer + the
//! receive-side idle guard + `allow_remote_execution`) all live on the control route in
//! `leanzero-link`; this type is reached only after they pass. It never re-checks or
//! weakens them.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use goose::acp::server::{ExecuteAccepted, ExecuteError, ExecuteRequest, RemoteExecutor};
use goose::config::GooseMode;
use goose::conversation::message::Message;
use goose::session::session_manager::SessionType;

use crate::routes::session_events::{spawn_reply_task, ReplyDispatchError};
use crate::state::AppState;

/// Runs remote prompts on this node by reusing goose-server's reply plumbing.
pub struct GoosedRemoteExecutor {
    state: Arc<AppState>,
}

impl GoosedRemoteExecutor {
    pub fn new(state: Arc<AppState>) -> Self {
        Self { state }
    }

    /// Default working directory for a remote-created session when the request names none:
    /// the node's `$HOME` (the link workspace), falling back to the process cwd. Documented
    /// so the companion app / dispatcher can rely on it.
    fn default_working_dir() -> PathBuf {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."))
    }
}

#[async_trait]
impl RemoteExecutor for GoosedRemoteExecutor {
    async fn execute(&self, req: ExecuteRequest) -> Result<ExecuteAccepted, ExecuteError> {
        if req.prompt.trim().is_empty() {
            return Err(ExecuteError::BadRequest(
                "prompt must not be empty".to_string(),
            ));
        }

        // Resolve the target session: reuse a named one (it must exist — a missing id is a
        // loud BadRequest, never a silent create under the wrong id) or create a fresh one
        // in the requested (or default) working dir.
        let session_id = match req.session_id.clone() {
            Some(id) => {
                self.state
                    .session_manager()
                    .get_session(&id, false)
                    .await
                    .map_err(|_| ExecuteError::BadRequest(format!("session {id} not found")))?;
                id
            }
            None => {
                let working_dir = req
                    .working_dir
                    .clone()
                    .map(PathBuf::from)
                    .unwrap_or_else(Self::default_working_dir);
                let session = self
                    .state
                    .session_manager()
                    .create_session(
                        working_dir,
                        remote_session_name(&req.prompt),
                        SessionType::User,
                        GooseMode::default(),
                    )
                    .await
                    .map_err(|e| ExecuteError::Internal(format!("creating a session: {e}")))?;
                session.id
            }
        };

        // Drive the SAME run path `POST /sessions/{id}/reply` uses; return immediately
        // (async) — the deltas mirror themselves over the mesh. An already-active session
        // maps to Busy so the caller (or the dispatcher's idle guard) can pick another node.
        let request_id = uuid::Uuid::new_v4().to_string();
        let message = Message::user().with_text(&req.prompt);
        spawn_reply_task(
            self.state.clone(),
            session_id.clone(),
            request_id,
            message,
            None,
        )
        .await
        .map_err(|e| match e {
            ReplyDispatchError::AlreadyActive => ExecuteError::Busy,
        })?;

        Ok(ExecuteAccepted { session_id })
    }
}

/// A short, human-readable session name from the prompt (for the mirror index the UI and
/// companion app show).
fn remote_session_name(prompt: &str) -> String {
    let snippet: String = prompt.trim().chars().take(48).collect();
    if snippet.is_empty() {
        "LeanZero Link remote".to_string()
    } else {
        format!("Link: {snippet}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Cancel any in-flight run so the fire-and-forget reply task never actually reaches a
    /// model in the test (the assertions are about session creation + dispatch, not output).
    async fn cancel_all(state: &AppState, session_id: &str) {
        if let Some(bus) = state.get_event_bus(session_id).await {
            for rid in bus.active_request_ids().await {
                bus.cancel_request(&rid).await;
            }
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn execute_creates_a_session_with_the_working_dir_and_dispatches_a_run() {
        let state = AppState::new(true).await.unwrap();
        let executor = GoosedRemoteExecutor::new(state.clone());

        let working_dir = std::env::temp_dir().join("leanzero-link-remote-exec-test");
        let accepted = executor
            .execute(ExecuteRequest {
                prompt: "say hello".to_string(),
                working_dir: Some(working_dir.to_string_lossy().into_owned()),
                session_id: None,
            })
            .await
            .expect("execute accepts and returns a session id");

        // The session exists with the requested working dir — created, not faked.
        let session = state
            .session_manager()
            .get_session(&accepted.session_id, false)
            .await
            .expect("the remote-created session exists");
        assert_eq!(session.working_dir, working_dir);
        assert!(session.name.starts_with("Link:"), "name: {}", session.name);

        // A run was dispatched: the reply plumbing allocated the session's event bus. We
        // assert this durable fact rather than the in-flight request set, which the
        // background task (no model wired in this test) may already have drained.
        assert!(
            state.get_event_bus(&accepted.session_id).await.is_some(),
            "a reply run was dispatched onto the session's event bus"
        );

        cancel_all(&state, &accepted.session_id).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn execute_rejects_an_empty_prompt() {
        let state = AppState::new(true).await.unwrap();
        let executor = GoosedRemoteExecutor::new(state);
        let err = executor
            .execute(ExecuteRequest {
                prompt: "   ".to_string(),
                working_dir: None,
                session_id: None,
            })
            .await
            .expect_err("an empty prompt is a loud BadRequest");
        assert!(matches!(err, ExecuteError::BadRequest(_)), "got {err:?}");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn execute_rejects_a_missing_session_id() {
        let state = AppState::new(true).await.unwrap();
        let executor = GoosedRemoteExecutor::new(state);
        let err = executor
            .execute(ExecuteRequest {
                prompt: "hi".to_string(),
                working_dir: None,
                session_id: Some("does-not-exist-zzzz".to_string()),
            })
            .await
            .expect_err("a missing session id is a loud BadRequest, never a silent create");
        assert!(matches!(err, ExecuteError::BadRequest(_)), "got {err:?}");
    }
}
