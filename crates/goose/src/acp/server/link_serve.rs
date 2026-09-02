//! LeanZero Link under `goose serve` — the injection the shipped desktop actually runs.
//!
//! `goosed agent` injects goose-server's executor, delta tap and MLX control at boot
//! (`crates/goose-server/src/commands/agent.rs`). The desktop, however, runs
//! `goose serve --platform desktop` (`crates/goose-cli/src/cli.rs` `handle_serve_command`),
//! which has no goose-server, no `AppState` and no per-session event bus — so until this
//! module existed it injected NOTHING and the packaged app's control service answered
//! `501` on every `POST /v1/swarm/execute` and `/v1/swarm/mlx/*`. This module is the
//! goose-crate implementation of the same seams over the ACP server's own machinery.
//!
//! ## Which AgentManager a remote prompt runs on
//! Under `goose serve` every ACP connection builds its own `GooseAcpAgent`, and with it
//! its own `AgentManager` + `SessionManager` (`server.rs` `GooseAcpAgent::new`). The link
//! layer's `SwarmStateSource` — the busy set the receive-side idle guard consults — is
//! built from the agent that first touched `leanzeroLink/*` (`link.rs`
//! `build_link_manager`). A remote run MUST register its cancel token on THAT manager,
//! or the guard never sees it and a busy node keeps accepting work. So `build_link_manager`
//! binds its managers here ([`bind_run_managers`]) and [`ServeRemoteExecutor`] resolves
//! them at execute time. It never constructs a manager of its own: a third one would be
//! invisible to the guard, and `AgentManager::instance()` would start a second cron
//! scheduler over the same `schedule.json` beside the one `AcpServer` already runs.
//!
//! ## What a remote run is
//! [`ServeRemoteExecutor::execute`] creates the session the way `new_session.rs` creates a
//! desktop session with no recipe and no client MCP servers — the RECEIVER's configured
//! `GooseMode`, a validated absolute+existing `working_dir`, the default provider/model
//! and the enabled extensions written onto the row — registers the run's cancel token at
//! the same door `on_prompt` uses (`AgentManager::try_register_cancel_token`) BEFORE the
//! reply task spawns, and returns `ExecuteAccepted { session_id }` at once. The reply then
//! runs on a spawned task exactly as `on_prompt` runs one: `agent.reply` under the token,
//! the stream dropped promptly on cancel. There is no ACP client on the other end, so a
//! session in Approve/SmartApprove mode parks at its first tool confirmation until the
//! owner opens it locally — the receiver's mode is honored, never silently widened.

use super::*;

use std::pin::Pin;
use std::sync::{LazyLock, Mutex as StdMutex, OnceLock, RwLock as StdRwLock};
use std::task::{Context, Poll};

use futures::stream::{BoxStream, Stream};
use rmcp::model::ServerNotification;
use serde::Serialize;
use tokio::sync::{broadcast, mpsc};
use tokio_stream::wrappers::ReceiverStream;

use crate::agents::AgentEvent;
use crate::config::extensions::get_enabled_extensions_with_config;
use crate::config::ConfigError;
use crate::conversation::message::TokenState;

/// The managers a remote prompt runs on: the pair the link layer's `SwarmStateSource` was
/// built from, so the run's cancel token lands in the busy set the idle guard reads.
#[derive(Clone)]
pub(super) struct RunManagers {
    pub(super) agent_manager: Arc<AgentManager>,
    pub(super) session_manager: Arc<SessionManager>,
}

static RUN_MANAGERS: StdRwLock<Option<RunManagers>> = StdRwLock::new(None);

/// Bind the managers the LeanZero Link source reads. Called by `build_link_manager` each
/// time it builds a source, so a rebuilt manager (an identity or setting change) re-points
/// the executor at the managers its control service now consults.
pub(super) fn bind_run_managers(
    agent_manager: Arc<AgentManager>,
    session_manager: Arc<SessionManager>,
) {
    let mut slot = RUN_MANAGERS.write().unwrap();
    let rebound = slot
        .as_ref()
        .is_some_and(|bound| !Arc::ptr_eq(&bound.agent_manager, &agent_manager));
    *slot = Some(RunManagers {
        agent_manager,
        session_manager,
    });
    info!(
        rebound,
        "leanzeroLink: remote prompts run on the managers the link source reads"
    );
}

fn bound_run_managers() -> Option<RunManagers> {
    RUN_MANAGERS.read().unwrap().clone()
}

/// Runs a same-account peer's prompt on this node over the ACP server's own agent
/// machinery. Stateless beyond its construction inputs; the managers come from the slot
/// [`bind_run_managers`] fills.
pub struct ServeRemoteExecutor {
    config: &'static Config,
    builtins: Vec<String>,
}

impl ServeRemoteExecutor {
    /// `config` is the receiver's own configuration (`Config::global()` at boot) — the
    /// source of the mode, provider, model and enabled extensions a remote session gets.
    /// `builtins` are the serve command's builtin extensions, the same list every desktop
    /// session receives.
    pub fn new(config: &'static Config, builtins: Vec<String>) -> Self {
        Self { config, builtins }
    }

    /// The working directory a remote-created session runs in: the request's, or `$HOME`
    /// (the link workspace) when it names none. A requested path must be ABSOLUTE and an
    /// EXISTING DIRECTORY on this node — a peer's cwd is not this machine's, and a session
    /// bound to a path that does not exist here would run its shell tools somewhere the
    /// caller never chose. Anything else is a loud `BadRequest`. (The same rule as
    /// goose-server's `remote_executor.rs`.)
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
    /// bind the session to wherever `goose serve` happened to be started from.
    fn default_working_dir() -> Result<PathBuf, ExecuteError> {
        std::env::var_os("HOME").map(PathBuf::from).ok_or_else(|| {
            ExecuteError::Internal(
                "no $HOME on this node to place a remote-created session in".to_string(),
            )
        })
    }

    /// The mode a remote-created session runs under: this RECEIVING node's own configured
    /// mode. `NotFound` means the owner never set one, and `GooseMode::default()` (Auto)
    /// is then exactly what their own desktop sessions get (`new_session.rs`). Any other
    /// failure — an unparseable value — refuses the run: a peer never gets a mode this
    /// node's owner cannot read back.
    fn receiver_goose_mode(config: &Config) -> Result<GooseMode, ExecuteError> {
        match config.get_goose_mode() {
            Ok(mode) => Ok(mode),
            Err(ConfigError::NotFound(_)) => Ok(GooseMode::default()),
            Err(error) => Err(ExecuteError::Internal(format!(
                "this node's GOOSE_MODE is unreadable, refusing to run a remote prompt: {error}"
            ))),
        }
    }

    /// Create and configure the session a fresh remote prompt runs in, the way
    /// `handle_new_session` does for a desktop client with no recipe and no client MCP
    /// servers: the receiver's mode on the row, then the default provider + model and the
    /// enabled extensions written onto it — the facts `AgentManager::get_or_create_agent`
    /// restores the agent from. Provider, model and mode are resolved BEFORE the row is
    /// created, so a node with nothing configured refuses loudly and leaves no
    /// half-configured session behind; a configuration failure after creation deletes
    /// the row (as `cleanup_failed_new_session` does).
    async fn create_remote_session(
        &self,
        managers: &RunManagers,
        prompt: &str,
        working_dir: PathBuf,
    ) -> Result<String, ExecuteError> {
        let goose_mode = Self::receiver_goose_mode(self.config)?;
        let (provider_name, model_config) =
            super::resolve_default_provider_model_config(self.config).map_err(|error| {
                ExecuteError::Internal(format!(
                    "this node has no default provider/model to run a remote prompt with: {}",
                    acp_error_text(&error)
                ))
            })?;

        let session = managers
            .session_manager
            .create_session(
                working_dir,
                remote_session_name(prompt),
                SessionType::User,
                goose_mode,
            )
            .await
            .map_err(|error| ExecuteError::Internal(format!("creating a session: {error}")))?;

        let configured = async {
            let extension_data = self.initial_extension_data(&session)?;
            managers
                .session_manager
                .update(&session.id)
                .provider_name(provider_name)
                .model_config(model_config)
                .extension_data(extension_data)
                .apply()
                .await
                .map_err(|error| {
                    ExecuteError::Internal(format!("configuring session {}: {error}", session.id))
                })
        }
        .await;

        if let Err(error) = configured {
            if let Err(cleanup) = managers.session_manager.delete_session(&session.id).await {
                warn!(
                    session_id = %session.id,
                    %cleanup,
                    "leanzeroLink: failed to delete a remote session whose configuration failed"
                );
            }
            return Err(error);
        }
        Ok(session.id)
    }

    /// The extensions a remote session starts with — `initial_session_extensions`' default
    /// branch (`server.rs`): the serve builtins, the user's enabled extensions, and the
    /// working directory's plugin MCP servers.
    fn initial_extension_data(&self, session: &Session) -> Result<ExtensionData, ExecuteError> {
        let mut extensions = Vec::new();
        for builtin in &self.builtins {
            super::push_or_replace_extension(
                &mut extensions,
                super::builtin_to_extension_config(builtin),
            );
        }
        for extension in get_enabled_extensions_with_config(self.config) {
            super::push_or_replace_extension(&mut extensions, extension);
        }
        for extension in
            crate::plugins::mcp_servers::enabled_plugin_mcp_servers(Some(&session.working_dir))
        {
            super::push_or_replace_extension(&mut extensions, extension);
        }
        let mut extension_data = session.extension_data.clone();
        EnabledExtensionsState::new(extensions)
            .to_extension_data(&mut extension_data)
            .map_err(|error| {
                ExecuteError::Internal(format!("initializing session extensions: {error}"))
            })?;
        Ok(extension_data)
    }
}

#[async_trait::async_trait]
impl RemoteExecutor for ServeRemoteExecutor {
    async fn execute(&self, req: ExecuteRequest) -> Result<ExecuteAccepted, ExecuteError> {
        if req.prompt.trim().is_empty() {
            return Err(ExecuteError::BadRequest(
                "prompt must not be empty".to_string(),
            ));
        }

        // The control service that routes `/execute` here lives inside a LinkManager, and
        // every LinkManager is built from an ACP agent's source, which binds these — so an
        // empty slot is a boot-order defect, reported as itself.
        let managers = bound_run_managers().ok_or_else(|| {
            ExecuteError::Internal(
                "no ACP agent has built the LeanZero Link on this node yet, so there is no \
                 session manager to run a remote prompt on"
                    .to_string(),
            )
        })?;

        // Reuse a named session (it must exist — a missing id is a loud BadRequest, never
        // a silent create under the wrong id) or create a fresh one.
        let session_id = match req.session_id.clone() {
            Some(id) => {
                managers
                    .session_manager
                    .get_session(&id, false)
                    .await
                    .map_err(|_| ExecuteError::BadRequest(format!("session {id} not found")))?;
                id
            }
            None => {
                let working_dir = Self::resolve_working_dir(req.working_dir.as_deref())?;
                self.create_remote_session(&managers, &req.prompt, working_dir)
                    .await?
            }
        };

        // The door. The manager's token map is the busy set every reader consults — the
        // link source's `local_node`, the idle guard, `cancel_session`. Registered before
        // the task spawns, so the node reads Busy from the moment the 202 leaves; a token
        // already present means another door (an ACP prompt run, a subagent) holds the
        // session: refuse as Busy, never run two replies on one session.
        let cancel_token = CancellationToken::new();
        if let Err(error) = managers
            .agent_manager
            .try_register_cancel_token(&session_id, cancel_token.clone())
            .await
        {
            warn!(%session_id, %error, "leanzeroLink: remote prompt refused, the session is busy in another run");
            return Err(ExecuteError::Busy);
        }

        tokio::spawn(run_remote_reply(
            managers,
            session_id.clone(),
            req.prompt,
            cancel_token,
        ));

        Ok(ExecuteAccepted { session_id })
    }
}

/// Releases the door's token when the reply task ends by ANY path — the net goose-server's
/// `spawn_reply_task` carries as `AgentBusyGuard`. A token left behind would report this
/// node Busy forever and refuse every peer. Disarmed after the inline release on the
/// normal path so a spawned late removal can never strip a NEWER run's token.
struct BusyDoorGuard {
    agent_manager: Arc<AgentManager>,
    session_id: String,
    armed: bool,
}

impl BusyDoorGuard {
    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for BusyDoorGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let agent_manager = self.agent_manager.clone();
        let session_id = self.session_id.clone();
        tokio::spawn(async move {
            agent_manager.unregister_cancel_token(&session_id).await;
        });
    }
}

async fn run_remote_reply(
    managers: RunManagers,
    session_id: String,
    prompt: String,
    cancel_token: CancellationToken,
) {
    let mut guard = BusyDoorGuard {
        agent_manager: managers.agent_manager.clone(),
        session_id: session_id.clone(),
        armed: true,
    };
    drive_remote_reply(&managers, &session_id, prompt, &cancel_token).await;
    // Release inline on every non-panic exit so the next reply on this session is not
    // refused by a token the guard's spawned drop has not removed yet; the guard stays as
    // the panic net.
    managers
        .agent_manager
        .unregister_cancel_token(&session_id)
        .await;
    guard.disarm();
}

/// The reply itself, as `on_prompt` runs one (`server.rs`): the session's agent from the
/// manager, `agent.reply` under the run's cancel token, the stream drained and dropped
/// promptly on cancel so an in-flight provider future (and any child it spawned with
/// kill_on_drop) is torn down. The agent persists every message through
/// `session_manager.add_message` itself; nothing is forwarded to an ACP client because
/// there is none.
async fn drive_remote_reply(
    managers: &RunManagers,
    session_id: &str,
    prompt: String,
    cancel_token: &CancellationToken,
) {
    // A cancel that raced the 202: nothing is built and no model is called — the early
    // exit `on_prompt` takes before it streams.
    if cancel_token.is_cancelled() {
        info!(%session_id, "leanzeroLink: remote prompt cancelled before it started");
        return;
    }

    let agent = match managers
        .agent_manager
        .get_or_create_agent(session_id.to_string())
        .await
    {
        Ok(agent) => agent,
        Err(error) => {
            error!(%session_id, %error, "leanzeroLink: remote prompt could not get the session agent");
            publish_tap(session_id, || {
                TapEvent::Error(format!("Failed to get session agent: {error}"))
            });
            return;
        }
    };
    if cancel_token.is_cancelled() {
        info!(%session_id, "leanzeroLink: remote prompt cancelled before its reply started");
        return;
    }

    let session_config = SessionConfig {
        id: session_id.to_string(),
        schedule_id: None,
        max_turns: None,
        retry_config: None,
    };
    let stream = match agent
        .reply(
            Message::user().with_text(&prompt),
            session_config,
            Some(cancel_token.clone()),
        )
        .await
    {
        Ok(stream) => stream,
        Err(error) => {
            error!(%session_id, %error, "leanzeroLink: remote prompt failed to start its reply stream");
            publish_tap(session_id, || TapEvent::Error(error.to_string()));
            return;
        }
    };
    // The mirror: every event fans onto the process-wide tap, and the session's delta
    // sequence closes with Finish when the stream ends or is dropped.
    let mut stream = tapped_reply(session_id, stream);

    loop {
        let event = tokio::select! {
            biased;
            _ = cancel_token.cancelled() => {
                info!(%session_id, "leanzeroLink: remote prompt cancelled");
                break;
            }
            maybe_event = stream.next() => match maybe_event {
                Some(event) => event,
                None => break,
            },
        };
        if let Err(error) = event {
            error!(%session_id, %error, "leanzeroLink: remote prompt's reply stream failed");
            break;
        }
    }
    drop(stream);
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

fn acp_error_text(error: &agent_client_protocol::Error) -> String {
    error
        .data
        .as_ref()
        .and_then(|value| value.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| error.message.to_string())
}

// ---------------------------------------------------------------------------
// The per-message mirror: one process-wide tap that every reply door feeds.
// ---------------------------------------------------------------------------

/// One reply-loop event as the mirror sees it. `Unclassified` names an `AgentEvent`
/// that has no per-message delta class (`Usage`, `HistoryReplaced`) so the pump can log
/// the gap as `delta_unclassified{kind}`; a notification is classified on the pump side,
/// where its variant is inspected.
#[derive(Clone, Debug)]
pub(super) enum TapEvent {
    Message(Message),
    Notification {
        request_id: String,
        message: ServerNotification,
    },
    Error(String),
    Finish {
        reason: &'static str,
    },
    Unclassified(&'static str),
}

/// One tapped event with the ORIGIN node's per-session delta sequence stamped on it.
#[derive(Clone, Debug)]
pub(super) struct TappedMsg {
    pub(super) session_id: String,
    pub(super) seq: u64,
    pub(super) event: TapEvent,
}

/// goose-server's `SESSION_DELTA_TAP_CAPACITY`: a pump that falls this far behind drops
/// the oldest deltas (peers reconcile through their session poll) rather than blocking
/// a reply.
const REPLY_TAP_CAPACITY: usize = 1024;

static REPLY_TAP: OnceLock<broadcast::Sender<TappedMsg>> = OnceLock::new();
static DELTA_SEQ: LazyLock<StdMutex<HashMap<String, u64>>> =
    LazyLock::new(|| StdMutex::new(HashMap::new()));

fn reply_tap() -> &'static broadcast::Sender<TappedMsg> {
    REPLY_TAP.get_or_init(|| broadcast::channel(REPLY_TAP_CAPACITY).0)
}

/// The origin's per-session delta sequence, minted at publish like goose-server's
/// `bus.publish` so every subscriber sees the same `seq` for the same event. It advances
/// for every tapped event whether or not a mirror is listening, so a peer that attaches
/// mid-session sees the numbering it would have seen from the start.
fn next_seq(session_id: &str) -> u64 {
    let mut seqs = DELTA_SEQ.lock().unwrap();
    let seq = seqs.entry(session_id.to_string()).or_insert(0);
    *seq += 1;
    *seq
}

/// Fan one reply event onto the tap. Non-fallible and non-blocking: with no subscriber
/// (mesh not connected) the event is not even built — the reply path never pays for a
/// mirror nobody reads, and never fails because of one.
fn publish_tap(session_id: &str, event: impl FnOnce() -> TapEvent) {
    let seq = next_seq(session_id);
    let tap = reply_tap();
    if tap.receiver_count() == 0 {
        return;
    }
    let _ = tap.send(TappedMsg {
        session_id: session_id.to_string(),
        seq,
        event: event(),
    });
}

fn tap_agent_event(session_id: &str, event: &AgentEvent) {
    publish_tap(session_id, || match event {
        AgentEvent::Message(message) => TapEvent::Message(message.clone()),
        AgentEvent::McpNotification((request_id, notification)) => TapEvent::Notification {
            request_id: request_id.clone(),
            message: notification.clone(),
        },
        AgentEvent::Usage(_) => TapEvent::Unclassified("usage"),
        AgentEvent::HistoryReplaced(_) => TapEvent::Unclassified("history_replaced"),
    });
}

/// The reply stream of either door, wrapped so every event it yields is mirrored and the
/// session's delta sequence closes with a `Finish` — on natural end (`stop`), or when the
/// stream is dropped before that (`cancelled`: the Stop button, a cancel token). An `Err`
/// item mirrors as `Error`, and the `Finish` that follows says `stop`, as goose-server's
/// reply loop ends every run.
pub(super) struct TappedReply<S> {
    inner: S,
    session_id: String,
    errored: bool,
    finished: bool,
}

pub(super) fn tapped_reply<S>(session_id: &str, inner: S) -> TappedReply<S> {
    TappedReply {
        inner,
        session_id: session_id.to_string(),
        errored: false,
        finished: false,
    }
}

impl<S> TappedReply<S> {
    fn finish(&mut self, reason: &'static str) {
        if self.finished {
            return;
        }
        self.finished = true;
        publish_tap(&self.session_id, || TapEvent::Finish { reason });
    }
}

impl<S> Stream for TappedReply<S>
where
    S: Stream<Item = anyhow::Result<AgentEvent>> + Unpin,
{
    type Item = S::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        let polled = Pin::new(&mut this.inner).poll_next(cx);
        match &polled {
            Poll::Ready(Some(Ok(event))) => tap_agent_event(&this.session_id, event),
            Poll::Ready(Some(Err(error))) => {
                this.errored = true;
                let text = error.to_string();
                publish_tap(&this.session_id, || TapEvent::Error(text));
            }
            Poll::Ready(None) => this.finish("stop"),
            Poll::Pending => {}
        }
        polled
    }
}

impl<S> Drop for TappedReply<S> {
    fn drop(&mut self) {
        let reason = if self.errored { "stop" } else { "cancelled" };
        self.finish(reason);
    }
}

/// goose-server's `MessageEvent` wire shape (`routes/reply.rs`), reproduced field for
/// field so a `SessionDelta.payload` from a `goose serve` node parses exactly like one
/// from a `goosed agent` node — a consumer never learns which produced it.
#[derive(Serialize)]
#[serde(tag = "type")]
enum WireDelta<'a> {
    Message {
        message: &'a Message,
        token_state: TokenState,
    },
    Error {
        error: &'a str,
    },
    Finish {
        reason: &'a str,
        token_state: TokenState,
    },
    Notification {
        request_id: &'a str,
        message: &'a ServerNotification,
    },
}

/// The per-message [`DeltaSource`] for `goose serve`: each `subscribe` pumps the tap into
/// classified [`DeltaInput`]s. The pump exits when its receiver (the control service's
/// local delta stream) is dropped, so nothing leaks past a disconnect.
pub struct ServeDeltaSource {
    session_manager: Arc<SessionManager>,
}

impl ServeDeltaSource {
    /// `session_manager` reads the token counters a `Message`/`Finish` payload carries —
    /// its own handle over the same store, as goose-server's `AppState` has its own.
    pub fn new(session_manager: Arc<SessionManager>) -> Self {
        Self { session_manager }
    }
}

impl DeltaSource for ServeDeltaSource {
    fn subscribe(&self) -> BoxStream<'static, DeltaInput> {
        let mut rx = reply_tap().subscribe();
        let session_manager = self.session_manager.clone();
        let (out_tx, out_rx) = mpsc::channel::<DeltaInput>(256);
        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(msg) => {
                        if let Some(input) = to_delta_input(&session_manager, msg).await {
                            if out_tx.send(input).await.is_err() {
                                return;
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(dropped)) => {
                        warn!(
                            dropped,
                            "leanzeroLink delta tap: pump lagged and lost deltas; peers reconcile via their session poll"
                        );
                    }
                    Err(broadcast::error::RecvError::Closed) => return,
                }
            }
        });
        Box::pin(ReceiverStream::new(out_rx))
    }
}

/// The wire class of a tool notification, exactly as goose's own ACP layer surfaces them
/// (`tool_notifications.rs`) and goose-server's tap classifies them: logging, progress and
/// `platform_event` are tool-call updates; anything else is not tool-related.
fn tool_update_kind(notification: &ServerNotification) -> Option<SessionDeltaKind> {
    match notification {
        ServerNotification::LoggingMessageNotification(_)
        | ServerNotification::ProgressNotification(_) => Some(SessionDeltaKind::ToolUpdate),
        ServerNotification::CustomNotification(n) if n.method == "platform_event" => {
            Some(SessionDeltaKind::ToolUpdate)
        }
        _ => None,
    }
}

fn notification_kind(notification: &ServerNotification) -> String {
    let kind = match notification {
        ServerNotification::CancelledNotification(_) => "cancelled",
        ServerNotification::ProgressNotification(_) => "progress",
        ServerNotification::LoggingMessageNotification(_) => "logging_message",
        ServerNotification::ResourceUpdatedNotification(_) => "resource_updated",
        ServerNotification::ResourceListChangedNotification(_) => "resource_list_changed",
        ServerNotification::ToolListChangedNotification(_) => "tool_list_changed",
        ServerNotification::PromptListChangedNotification(_) => "prompt_list_changed",
        ServerNotification::ElicitationCompletionNotification(_) => "elicitation_completion",
        ServerNotification::CustomNotification(n) => {
            return format!("notification:custom:{}", n.method)
        }
    };
    format!("notification:{kind}")
}

/// The session's token counters as goose-server's `get_token_state` reads them. The delta
/// is never dropped over this read: on a failure the counters ride as zeros and the
/// failure is logged — the payload's `message` is the fact, `token_state` decorates it
/// (goose-server's tap carries the identical `unwrap_or_default` there).
async fn token_state(session_manager: &SessionManager, session_id: &str) -> TokenState {
    match session_manager.get_session(session_id, false).await {
        Ok(session) => TokenState::from(&session),
        Err(error) => {
            warn!(
                %session_id,
                %error,
                "leanzeroLink delta tap: session token state unreadable; the delta carries zero counters"
            );
            TokenState::default()
        }
    }
}

fn unclassified(session_id: &str, seq: u64, kind: &str) {
    info!(
        %session_id,
        seq,
        kind,
        "leanzeroLink delta tap: delta_unclassified — this reply event has no per-message delta class and is not mirrored; peers see its effect through their session poll"
    );
}

async fn to_delta_input(session_manager: &SessionManager, msg: TappedMsg) -> Option<DeltaInput> {
    let TappedMsg {
        session_id,
        seq,
        event,
    } = msg;
    let (kind, payload) = match &event {
        TapEvent::Message(message) => (
            SessionDeltaKind::Message,
            WireDelta::Message {
                message,
                token_state: token_state(session_manager, &session_id).await,
            },
        ),
        TapEvent::Error(error) => (
            SessionDeltaKind::Error,
            WireDelta::Error {
                error: error.as_str(),
            },
        ),
        TapEvent::Finish { reason } => (
            SessionDeltaKind::Finish,
            WireDelta::Finish {
                reason,
                token_state: token_state(session_manager, &session_id).await,
            },
        ),
        TapEvent::Notification {
            request_id,
            message,
        } => match tool_update_kind(message) {
            Some(kind) => (
                kind,
                WireDelta::Notification {
                    request_id: request_id.as_str(),
                    message,
                },
            ),
            None => {
                unclassified(&session_id, seq, &notification_kind(message));
                return None;
            }
        },
        TapEvent::Unclassified(kind) => {
            unclassified(&session_id, seq, kind);
            return None;
        }
    };
    match serde_json::to_value(&payload) {
        Ok(payload) => Some(DeltaInput {
            session_id,
            seq,
            kind,
            payload,
        }),
        Err(error) => {
            warn!(%session_id, seq, %error, "leanzeroLink delta tap: delta failed to serialize; dropped");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::{AgentConfig, GoosePlatform};
    use crate::config::permission::PermissionManager;
    use std::time::Duration;

    /// The bound-managers slot is process-wide; the executor tests take turns on it.
    static BIND_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    async fn temp_managers() -> (tempfile::TempDir, RunManagers) {
        let temp = tempfile::TempDir::new().unwrap();
        let session_manager = Arc::new(SessionManager::new(temp.path().to_path_buf()));
        let agent_config = AgentConfig::new(
            session_manager.clone(),
            PermissionManager::instance(),
            None,
            GooseMode::default(),
            false,
            GoosePlatform::GooseDesktop,
        );
        let agent_manager = Arc::new(AgentManager::new(agent_config, Some(100)).await.unwrap());
        (
            temp,
            RunManagers {
                agent_manager,
                session_manager,
            },
        )
    }

    /// A receiver whose config names a provider and a model — the two facts a remote
    /// session must inherit. Leaked to `'static` because that is the executor's contract
    /// (`Config::global()` in production); the files live in `dir` for the test's span.
    fn receiver_config(dir: &Path) -> &'static Config {
        let config_path = dir.join("config.yaml");
        let secrets_path = dir.join("secrets.yaml");
        std::fs::write(&config_path, "").unwrap();
        std::fs::write(&secrets_path, "").unwrap();
        let config = Config::new_with_file_secrets(&config_path, &secrets_path).unwrap();
        config.set_param("GOOSE_PROVIDER", "openai").unwrap();
        config.set_param("GOOSE_MODEL", "gpt-4o").unwrap();
        Box::leak(Box::new(config))
    }

    fn executor(config: &'static Config) -> ServeRemoteExecutor {
        ServeRemoteExecutor::new(config, vec!["developer".to_string()])
    }

    async fn wait_until_idle(agent_manager: &AgentManager, session_id: &str) {
        for _ in 0..200 {
            if !agent_manager.is_session_busy(session_id).await {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("session {session_id} never released its busy token");
    }

    #[tokio::test]
    async fn execute_creates_a_session_like_new_session_does_and_holds_the_busy_door() {
        let _turn = BIND_LOCK.lock().await;
        let (temp, managers) = temp_managers().await;
        bind_run_managers(
            managers.agent_manager.clone(),
            managers.session_manager.clone(),
        );
        let config = receiver_config(temp.path());
        let executor = executor(config);

        let working_dir = temp.path().join("work");
        std::fs::create_dir_all(&working_dir).unwrap();
        let accepted = executor
            .execute(ExecuteRequest {
                prompt: "say hello".to_string(),
                working_dir: Some(working_dir.to_string_lossy().into_owned()),
                session_id: None,
            })
            .await
            .expect("execute accepts and returns a session id");

        // Busy at the door, before the spawned task has run at all — the 202 and the
        // Busy report cannot disagree. Cancelling here makes the task take its
        // no-model early exit, so the test never builds a provider.
        assert!(
            managers
                .agent_manager
                .is_session_busy(&accepted.session_id)
                .await
        );
        managers
            .agent_manager
            .cancel_session(&accepted.session_id)
            .await
            .unwrap();

        // The session exists with everything `new_session.rs` writes — created, not faked.
        let session = managers
            .session_manager
            .get_session(&accepted.session_id, false)
            .await
            .expect("the remote-created session exists");
        assert_eq!(session.working_dir, working_dir);
        assert!(session.name.starts_with("Link:"), "name: {}", session.name);
        assert_eq!(session.session_type, SessionType::User);
        assert_eq!(
            session.goose_mode,
            ServeRemoteExecutor::receiver_goose_mode(config).unwrap(),
            "a remote-created session runs under the RECEIVER's configured mode"
        );
        assert_eq!(
            session.provider_name.as_deref(),
            Some(config.get_goose_provider().unwrap().as_str()),
            "the receiver's default provider is on the row for the agent to restore from"
        );
        assert_eq!(
            session.model_config.map(|m| m.model_name),
            Some(config.get_goose_model().unwrap()),
        );
        let extensions = <EnabledExtensionsState as ExtensionState>::from_extension_data(
            &session.extension_data,
        )
        .expect("the enabled-extensions state is written like new_session writes it");
        assert!(
            extensions
                .extensions
                .iter()
                .any(|e| e.name() == "developer"),
            "the serve builtins are on the row: {:?}",
            extensions
                .extensions
                .iter()
                .map(|e| e.name())
                .collect::<Vec<_>>()
        );

        // The door releases when the run ends.
        wait_until_idle(&managers.agent_manager, &accepted.session_id).await;
    }

    #[tokio::test]
    async fn execute_rejects_an_empty_prompt_before_touching_anything() {
        let executor = executor(receiver_config(tempfile::TempDir::new().unwrap().path()));
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

    #[tokio::test]
    async fn execute_without_a_bound_link_source_is_a_loud_internal_error() {
        let _turn = BIND_LOCK.lock().await;
        *RUN_MANAGERS.write().unwrap() = None;
        let temp = tempfile::TempDir::new().unwrap();
        let executor = executor(receiver_config(temp.path()));
        let err = executor
            .execute(ExecuteRequest {
                prompt: "hi".to_string(),
                working_dir: None,
                session_id: None,
            })
            .await
            .expect_err("no managers bound means no run, reported as itself");
        match err {
            ExecuteError::Internal(text) => assert!(text.contains("no ACP agent"), "{text}"),
            other => panic!("got {other:?}"),
        }
    }

    /// A peer's working_dir is validated on THIS node — relative, missing, or a file is a
    /// loud BadRequest; nothing is created and nothing runs.
    #[tokio::test]
    async fn execute_rejects_a_working_dir_that_is_not_an_existing_directory_here() {
        let _turn = BIND_LOCK.lock().await;
        let (temp, managers) = temp_managers().await;
        bind_run_managers(
            managers.agent_manager.clone(),
            managers.session_manager.clone(),
        );
        let executor = executor(receiver_config(temp.path()));

        let file = temp.path().join("not-a-dir");
        std::fs::write(&file, b"x").unwrap();
        let missing = temp.path().join("missing");
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
        assert!(
            managers
                .session_manager
                .list_sessions()
                .await
                .unwrap()
                .is_empty(),
            "a refused working_dir creates nothing"
        );
    }

    #[tokio::test]
    async fn execute_rejects_a_missing_session_id_instead_of_creating_under_it() {
        let _turn = BIND_LOCK.lock().await;
        let (temp, managers) = temp_managers().await;
        bind_run_managers(
            managers.agent_manager.clone(),
            managers.session_manager.clone(),
        );
        let executor = executor(receiver_config(temp.path()));
        let err = executor
            .execute(ExecuteRequest {
                prompt: "hi".to_string(),
                working_dir: None,
                session_id: Some("does-not-exist-zzzz".to_string()),
            })
            .await
            .expect_err("a missing session id is a loud BadRequest");
        assert!(matches!(err, ExecuteError::BadRequest(_)), "got {err:?}");
    }

    /// A session already held by the other door (an ACP prompt run, a subagent) answers
    /// Busy — never a second reply on one session — and the other door's token survives.
    #[tokio::test]
    async fn execute_on_a_session_busy_in_another_run_is_busy() {
        let _turn = BIND_LOCK.lock().await;
        let (temp, managers) = temp_managers().await;
        bind_run_managers(
            managers.agent_manager.clone(),
            managers.session_manager.clone(),
        );
        let executor = executor(receiver_config(temp.path()));
        let session = managers
            .session_manager
            .create_session(
                temp.path().to_path_buf(),
                "Chat".to_string(),
                SessionType::User,
                GooseMode::default(),
            )
            .await
            .unwrap();
        managers
            .agent_manager
            .try_register_cancel_token(&session.id, CancellationToken::new())
            .await
            .unwrap();

        let err = executor
            .execute(ExecuteRequest {
                prompt: "hi".to_string(),
                working_dir: None,
                session_id: Some(session.id.clone()),
            })
            .await
            .expect_err("the session is busy in another run");
        assert!(matches!(err, ExecuteError::Busy), "got {err:?}");
        assert!(managers.agent_manager.is_session_busy(&session.id).await);
    }

    #[test]
    fn remote_session_name_is_a_prompt_snippet() {
        assert_eq!(
            remote_session_name("  fix the build  "),
            "Link: fix the build"
        );
        assert_eq!(remote_session_name("   "), "LeanZero Link remote");
        let long = "x".repeat(100);
        assert_eq!(
            remote_session_name(&long).chars().count(),
            "Link: ".len() + 48
        );
    }

    fn logging_notification() -> ServerNotification {
        use rmcp::model::{LoggingLevel, LoggingMessageNotificationParam, Notification};
        ServerNotification::LoggingMessageNotification(Notification::new(
            LoggingMessageNotificationParam::new(
                LoggingLevel::Info,
                serde_json::json!({ "line": "x" }),
            ),
        ))
    }

    fn cancelled_notification() -> ServerNotification {
        use rmcp::model::{CancelledNotificationParam, Notification, NumberOrString};
        ServerNotification::CancelledNotification(Notification::new(CancelledNotificationParam {
            request_id: NumberOrString::String(Arc::from("r1")),
            reason: None,
        }))
    }

    /// Drain the subscriber stream for `session_id`'s deltas up to and including its
    /// Finish (other tests' sessions interleave on the same process-wide tap).
    async fn collect_deltas(
        stream: &mut BoxStream<'static, DeltaInput>,
        session_id: &str,
    ) -> Vec<DeltaInput> {
        let mut deltas = Vec::new();
        tokio::time::timeout(Duration::from_secs(5), async {
            while let Some(input) = stream.next().await {
                if input.session_id != session_id {
                    continue;
                }
                let done = input.kind == SessionDeltaKind::Finish;
                deltas.push(input);
                if done {
                    break;
                }
            }
        })
        .await
        .expect("the session's Finish delta arrives");
        deltas
    }

    fn assistant(text: &str) -> anyhow::Result<AgentEvent> {
        Ok(AgentEvent::Message(Message::assistant().with_text(text)))
    }

    #[tokio::test]
    async fn a_tapped_reply_mirrors_message_tool_update_and_finish_as_message_event_payloads() {
        let temp = tempfile::TempDir::new().unwrap();
        let session_manager = Arc::new(SessionManager::new(temp.path().to_path_buf()));
        let session = session_manager
            .create_session(
                temp.path().to_path_buf(),
                "Chat".to_string(),
                SessionType::User,
                GooseMode::default(),
            )
            .await
            .unwrap();
        let source = ServeDeltaSource::new(session_manager.clone());
        let mut deltas_stream = source.subscribe();

        let events = vec![
            assistant("mirror me"),
            Ok(AgentEvent::HistoryReplaced(
                crate::conversation::Conversation::new_unvalidated(Vec::new()),
            )),
            Ok(AgentEvent::McpNotification((
                "tool_1".to_string(),
                logging_notification(),
            ))),
            Ok(AgentEvent::McpNotification((
                "tool_1".to_string(),
                cancelled_notification(),
            ))),
        ];
        let mut reply = tapped_reply(&session.id, stream::iter(events));
        while reply.next().await.is_some() {}
        drop(reply);

        let deltas = collect_deltas(&mut deltas_stream, &session.id).await;
        let kinds: Vec<SessionDeltaKind> = deltas.iter().map(|d| d.kind).collect();
        assert_eq!(
            kinds,
            vec![
                SessionDeltaKind::Message,
                SessionDeltaKind::ToolUpdate,
                SessionDeltaKind::Finish
            ]
        );
        // The origin's per-session sequence: every tapped event consumes one — the
        // unclassified HistoryReplaced (2) and the non-tool notification (4) included.
        let seqs: Vec<u64> = deltas.iter().map(|d| d.seq).collect();
        assert_eq!(seqs, vec![1, 3, 5]);

        let message = &deltas[0].payload;
        assert_eq!(message["type"], "Message");
        assert_eq!(message["message"]["role"], "assistant");
        assert!(
            message["token_state"].is_object(),
            "goose-server's MessageEvent shape: {message}"
        );
        let update = &deltas[1].payload;
        assert_eq!(update["type"], "Notification");
        assert_eq!(update["request_id"], "tool_1");
        let finish = &deltas[2].payload;
        assert_eq!(finish["type"], "Finish");
        assert_eq!(finish["reason"], "stop");
        assert!(finish["token_state"].is_object());
    }

    #[tokio::test]
    async fn a_reply_dropped_before_its_end_closes_with_a_cancelled_finish() {
        let temp = tempfile::TempDir::new().unwrap();
        let source =
            ServeDeltaSource::new(Arc::new(SessionManager::new(temp.path().to_path_buf())));
        let mut deltas_stream = source.subscribe();
        let session_id = format!("s-{}", Uuid::new_v4());

        let mut reply = tapped_reply(
            &session_id,
            stream::iter(vec![assistant("one"), assistant("two")]),
        );
        assert!(reply.next().await.is_some());
        // The Stop button / a cancel token: the stream goes before its end.
        drop(reply);

        let deltas = collect_deltas(&mut deltas_stream, &session_id).await;
        let kinds: Vec<SessionDeltaKind> = deltas.iter().map(|d| d.kind).collect();
        assert_eq!(
            kinds,
            vec![SessionDeltaKind::Message, SessionDeltaKind::Finish]
        );
        assert_eq!(deltas[1].payload["reason"], "cancelled");
    }

    #[tokio::test]
    async fn a_failing_reply_mirrors_the_error_then_a_stop_finish() {
        let temp = tempfile::TempDir::new().unwrap();
        let source =
            ServeDeltaSource::new(Arc::new(SessionManager::new(temp.path().to_path_buf())));
        let mut deltas_stream = source.subscribe();
        let session_id = format!("s-{}", Uuid::new_v4());

        let events = vec![
            assistant("partial"),
            Err(anyhow::anyhow!("provider exploded")),
        ];
        let mut reply = tapped_reply(&session_id, stream::iter(events));
        // `on_prompt` breaks out of its loop on the first Err and drops the stream.
        while let Some(item) = reply.next().await {
            if item.is_err() {
                break;
            }
        }
        drop(reply);

        let deltas = collect_deltas(&mut deltas_stream, &session_id).await;
        let kinds: Vec<SessionDeltaKind> = deltas.iter().map(|d| d.kind).collect();
        assert_eq!(
            kinds,
            vec![
                SessionDeltaKind::Message,
                SessionDeltaKind::Error,
                SessionDeltaKind::Finish
            ]
        );
        assert_eq!(deltas[1].payload["error"], "provider exploded");
        assert_eq!(deltas[2].payload["reason"], "stop");
    }
}
