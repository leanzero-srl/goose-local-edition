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
use goose::acp::server::{
    job_log_text, job_run_failure, ExecuteAccepted, ExecuteError, ExecuteLog, ExecuteRequest,
    RemoteExecutor, RunEnd,
};
use goose::config::{Config, GooseMode};
use goose::conversation::message::Message;
use goose::session::session_manager::SessionType;

use crate::routes::reply::MessageEvent;
use crate::routes::session_events::{spawn_reply_task, ReplyDispatchError};
use crate::session_event_bus::{SessionEvent, SessionEventBus};
use crate::state::AppState;
use tokio::sync::broadcast;

/// Runs remote prompts on this node by reusing goose-server's reply plumbing.
pub struct GoosedRemoteExecutor {
    state: Arc<AppState>,
}

impl GoosedRemoteExecutor {
    pub fn new(state: Arc<AppState>) -> Self {
        Self { state }
    }

    /// The working directory a remote-created session runs in: the request's, or `$HOME`
    /// (the link workspace) when it names none. A requested path must be ABSOLUTE and an
    /// EXISTING DIRECTORY on this node — a peer's cwd is not this machine's, and a session
    /// bound to a path that does not exist here would run its shell tools somewhere the
    /// caller never chose. Anything else is a loud `BadRequest`.
    fn resolve_working_dir(requested: Option<&str>) -> Result<PathBuf, ExecuteError> {
        let Some(raw) = requested else {
            return Self::default_working_dir();
        };
        let path = PathBuf::from(raw);
        if !path.is_absolute() {
            return Err(ExecuteError::BadRequest(format!(
                "working_dir must be an absolute path on the target node, got '{raw}'"
            )));
        }
        match std::fs::metadata(&path) {
            Ok(meta) if meta.is_dir() => Ok(path),
            Ok(_) => Err(ExecuteError::BadRequest(format!(
                "working_dir '{raw}' exists on the target node but is not a directory"
            ))),
            Err(error) => Err(ExecuteError::BadRequest(format!(
                "working_dir '{raw}' is not usable on the target node: {error}"
            ))),
        }
    }

    /// `$HOME` only. A node with no home directory has nowhere sanctioned to place a
    /// remote session, and that is an error — never the process cwd or `"."`, which would
    /// bind the session to wherever goosed happened to be started from.
    fn default_working_dir() -> Result<PathBuf, ExecuteError> {
        std::env::var_os("HOME").map(PathBuf::from).ok_or_else(|| {
            ExecuteError::Internal(
                "no $HOME on this node to place a remote-created session in".to_string(),
            )
        })
    }

    /// The mode a remote-created session runs under: this RECEIVING node's own configured
    /// mode, read exactly as the local new-session path reads it (`new_session.rs`), so a
    /// user on Approve/Chat is never handed an Auto session by a peer.
    fn receiver_goose_mode() -> GooseMode {
        Config::global().get_goose_mode().unwrap_or_default()
    }
}

#[async_trait]
impl RemoteExecutor for GoosedRemoteExecutor {
    async fn execute(&self, req: ExecuteRequest) -> Result<ExecuteAccepted, ExecuteError> {
        self.execute_with_log(req, None).await
    }

    /// The same run, with the session's reply events for THIS request streamed into `log`
    /// (a prompt job's log file) as they are published, so the job log tails the answer.
    async fn execute_logged(
        &self,
        req: ExecuteRequest,
        log: Arc<dyn ExecuteLog>,
    ) -> Result<ExecuteAccepted, ExecuteError> {
        self.execute_with_log(req, Some(log)).await
    }
}

impl GoosedRemoteExecutor {
    async fn execute_with_log(
        &self,
        req: ExecuteRequest,
        log: Option<Arc<dyn ExecuteLog>>,
    ) -> Result<ExecuteAccepted, ExecuteError> {
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
                let working_dir = Self::resolve_working_dir(req.working_dir.as_deref())?;
                let session = self
                    .state
                    .session_manager()
                    .create_session(
                        working_dir,
                        remote_session_name(&req.prompt),
                        SessionType::User,
                        Self::receiver_goose_mode(),
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
        // Subscribe BEFORE the run spawns so not one of its events can be missed; the replay
        // part is older runs' events and is dropped.
        let tap = match log {
            Some(log) => {
                let bus = self.state.get_or_create_event_bus(&session_id).await;
                let (_, _, rx) = bus.subscribe(None).await.map_err(|e| {
                    ExecuteError::Internal(format!("subscribing the job log: {e:?}"))
                })?;
                Some((bus, rx, log))
            }
            None => None,
        };
        spawn_reply_task(
            self.state.clone(),
            session_id.clone(),
            request_id.clone(),
            message,
            None,
        )
        .await
        .map_err(|e| match e {
            ReplyDispatchError::AlreadyActive => ExecuteError::Busy,
        })?;
        if let Some((bus, rx, log)) = tap {
            tokio::spawn(pump_job_log(bus, rx, request_id, log, JOB_LOG_IDLE_CHECK));
        }

        Ok(ExecuteAccepted { session_id })
    }
}

/// How long the job-log pump waits on a quiet bus before it checks the run is still active
/// (a run that died before its Finish must not leave the pump parked forever).
const JOB_LOG_IDLE_CHECK: std::time::Duration = std::time::Duration::from_secs(2);

/// Copies one request's reply events from the session bus into a job log (D4): messages via
/// the shared [`job_log_text`] formatter, errors as `[error: …]`, the end as
/// `[finished: reason]`. Stops at Finish, when the bus closes, or when the request is no
/// longer active on the bus after a quiet `idle_check`. It also reports how the run ended
/// ([`ExecuteLog::finish`], D13): a provider-error notice or an Error event makes it Failed even
/// when a normal Finish follows; a run that vanished with no failure seen reports nothing
/// (the job reads `lost`, never a fabricated Done).
async fn pump_job_log(
    bus: Arc<SessionEventBus>,
    mut rx: broadcast::Receiver<SessionEvent>,
    request_id: String,
    log: Arc<dyn ExecuteLog>,
    idle_check: std::time::Duration,
) {
    let mut failure: Option<String> = None;
    let report_failure = |log: &Arc<dyn ExecuteLog>, failure: Option<String>| {
        if let Some(why) = failure {
            log.finish(RunEnd::Failed(why));
        }
    };
    loop {
        let event = match tokio::time::timeout(idle_check, rx.recv()).await {
            Err(_) => {
                if bus.active_request_ids().await.contains(&request_id) {
                    continue;
                }
                log.append("\n[finished: ended]\n");
                report_failure(&log, failure);
                return;
            }
            Ok(Err(broadcast::error::RecvError::Lagged(n))) => {
                log.append(&format!("\n[log: {n} events skipped]\n"));
                continue;
            }
            Ok(Err(broadcast::error::RecvError::Closed)) => {
                report_failure(&log, failure);
                return;
            }
            Ok(Ok(event)) => event,
        };
        if event.request_id.as_deref() != Some(request_id.as_str()) {
            continue;
        }
        match event.event {
            MessageEvent::Message { message, .. } => {
                if let Some(why) = job_run_failure(&message) {
                    failure = Some(why);
                }
                let text = job_log_text(&message);
                if !text.is_empty() {
                    log.append(&text);
                }
            }
            MessageEvent::Error { error } => {
                log.append(&format!("\n[error: {error}]\n"));
                failure = Some(error);
            }
            MessageEvent::Finish { reason, .. } => {
                log.append(&format!("\n[finished: {reason}]\n"));
                log.finish(match failure {
                    _ if reason == "cancelled" => RunEnd::Cancelled,
                    Some(why) => RunEnd::Failed(why),
                    None => RunEnd::Completed,
                });
                return;
            }
            _ => {}
        }
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

    #[derive(Default)]
    struct MemLog(std::sync::Mutex<String>, std::sync::Mutex<Vec<RunEnd>>);
    impl ExecuteLog for MemLog {
        fn append(&self, text: &str) {
            self.0.lock().unwrap().push_str(text);
        }
        fn finish(&self, end: RunEnd) {
            self.1.lock().unwrap().push(end);
        }
    }

    /// Runs the pump over `events` (all for request "ours") and answers what it reported.
    async fn pump_ends(events: Vec<MessageEvent>) -> Vec<RunEnd> {
        let bus = Arc::new(SessionEventBus::new());
        let (_, _, rx) = bus.subscribe(None).await.unwrap();
        let log = Arc::new(MemLog::default());
        let pump = tokio::spawn(pump_job_log(
            bus.clone(),
            rx,
            "ours".to_string(),
            log.clone(),
            std::time::Duration::from_millis(100),
        ));
        for e in events {
            bus.publish(Some("ours".into()), e).await;
        }
        tokio::time::timeout(std::time::Duration::from_secs(5), pump)
            .await
            .expect("the pump ends")
            .unwrap();
        let ends = log.1.lock().unwrap().clone();
        ends
    }

    fn finish(reason: &str) -> MessageEvent {
        MessageEvent::Finish {
            reason: reason.into(),
            token_state: token_state(),
        }
    }

    /// D13 (3.0.130 live): a turn whose provider failed ends with a notice message and a
    /// NORMAL Finish; the job must read Failed with the provider's words, not Done.
    #[tokio::test]
    async fn the_pump_reports_a_provider_failure_even_when_a_normal_finish_follows() {
        use goose::conversation::message::ProviderErrorNotice;
        let notice = Message::assistant()
            .with_text("I could not reach a model.")
            .with_provider_error(ProviderErrorNotice {
                class: "execution".into(),
                transient: false,
                said: String::new(),
                detail: "Execution error: swarm chat: no node can serve this turn".into(),
            });
        let ends = pump_ends(vec![
            MessageEvent::Message {
                message: notice,
                token_state: token_state(),
            },
            finish("stop"),
        ])
        .await;
        assert_eq!(
            ends,
            vec![RunEnd::Failed(
                "Execution error: swarm chat: no node can serve this turn".into()
            )]
        );
    }

    #[tokio::test]
    async fn the_pump_reports_completed_cancelled_and_errors() {
        let ok = MessageEvent::Message {
            message: Message::assistant().with_text("hi"),
            token_state: token_state(),
        };
        assert_eq!(
            pump_ends(vec![ok, finish("stop")]).await,
            vec![RunEnd::Completed]
        );
        assert_eq!(
            pump_ends(vec![finish("cancelled")]).await,
            vec![RunEnd::Cancelled]
        );
        let err = || MessageEvent::Error {
            error: "boom".into(),
        };
        assert_eq!(
            pump_ends(vec![err(), finish("stop")]).await,
            vec![RunEnd::Failed("boom".into())]
        );
        // A run that vanished after an error (no Finish) is still Failed; one that vanished
        // with no failure seen reports nothing, so the job reads lost.
        assert_eq!(
            pump_ends(vec![err()]).await,
            vec![RunEnd::Failed("boom".into())]
        );
        assert_eq!(pump_ends(vec![]).await, vec![]);
    }

    fn token_state() -> goose::conversation::message::TokenState {
        Default::default()
    }

    /// D4: the pump copies only OUR request's events, in order, and stops at Finish.
    #[tokio::test]
    async fn the_job_log_pump_copies_this_requests_events_in_order_and_stops_at_finish() {
        let bus = Arc::new(SessionEventBus::new());
        let (_, _, rx) = bus.subscribe(None).await.unwrap();
        let log = Arc::new(MemLog::default());
        let pump = tokio::spawn(pump_job_log(
            bus.clone(),
            rx,
            "ours".to_string(),
            log.clone(),
            std::time::Duration::from_secs(5),
        ));
        let msg = |text: &str| MessageEvent::Message {
            message: Message::assistant().with_text(text),
            token_state: token_state(),
        };
        bus.publish(Some("ours".into()), msg("Hel")).await;
        bus.publish(Some("other".into()), msg("NOT OURS")).await;
        bus.publish(Some("ours".into()), msg("lo.")).await;
        bus.publish(
            Some("ours".into()),
            MessageEvent::Error {
                error: "boom".into(),
            },
        )
        .await;
        bus.publish(
            Some("ours".into()),
            MessageEvent::Finish {
                reason: "stop".into(),
                token_state: token_state(),
            },
        )
        .await;
        tokio::time::timeout(std::time::Duration::from_secs(5), pump)
            .await
            .expect("the pump stops at Finish")
            .unwrap();
        assert_eq!(
            log.0.lock().unwrap().as_str(),
            "Hello.\n[error: boom]\n\n[finished: stop]\n"
        );
    }

    /// A run that ends without a Finish (an early error return) does not park the pump.
    #[tokio::test]
    async fn the_job_log_pump_ends_when_the_request_is_gone_without_a_finish() {
        let bus = Arc::new(SessionEventBus::new());
        let (_, _, rx) = bus.subscribe(None).await.unwrap();
        let log = Arc::new(MemLog::default());
        let pump = tokio::spawn(pump_job_log(
            bus.clone(),
            rx,
            "ours".to_string(),
            log.clone(),
            std::time::Duration::from_millis(100),
        ));
        bus.publish(
            Some("ours".into()),
            MessageEvent::Error {
                error: "no agent".into(),
            },
        )
        .await;
        tokio::time::timeout(std::time::Duration::from_secs(5), pump)
            .await
            .expect("the pump notices the request is gone")
            .unwrap();
        assert_eq!(
            log.0.lock().unwrap().as_str(),
            "\n[error: no agent]\n\n[finished: ended]\n"
        );
    }

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
        std::fs::create_dir_all(&working_dir).unwrap();
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
        assert_eq!(
            session.goose_mode,
            GoosedRemoteExecutor::receiver_goose_mode(),
            "a remote-created session runs under the RECEIVER's configured mode"
        );

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

    /// R-H3: a peer's working_dir is validated on THIS node — relative, missing, or a file
    /// is a loud BadRequest; nothing is created and nothing runs.
    #[tokio::test(flavor = "multi_thread")]
    async fn execute_rejects_a_working_dir_that_is_not_an_existing_directory_here() {
        let state = AppState::new(true).await.unwrap();
        let executor = GoosedRemoteExecutor::new(state);

        let file = std::env::temp_dir().join("leanzero-link-remote-exec-not-a-dir");
        std::fs::write(&file, b"x").unwrap();
        let missing = std::env::temp_dir().join(format!(
            "leanzero-link-remote-exec-missing-{}",
            uuid::Uuid::new_v4()
        ));

        for (label, dir) in [
            ("relative", "relative/path".to_string()),
            ("missing", missing.to_string_lossy().into_owned()),
            ("a file", file.to_string_lossy().into_owned()),
        ] {
            let err = executor
                .execute(ExecuteRequest {
                    prompt: "hi".to_string(),
                    working_dir: Some(dir),
                    session_id: None,
                })
                .await
                .expect_err(label);
            assert!(
                matches!(err, ExecuteError::BadRequest(_)),
                "{label}: got {err:?}"
            );
        }
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
