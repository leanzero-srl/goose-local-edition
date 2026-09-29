//! How a session's LAST turn ended, durable in its `extension_data` (`turn_outcome.v0`), so a
//! session whose answer was cut ("The split across your Macs stopped mid-answer") reads FAILED in
//! every list instead of "8h ago", and one the person STOPPED reads Stopped (Q-169). Written at the
//! end of every turn of a non-swarm agent, and by the ACP prompt when the person stops it: a later
//! turn that completes clears it.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use anyhow::Result;
use chrono::{DateTime, Utc};
use goose_providers::conversation::token_usage::ProviderUsage;
use goose_providers::formats::openai::{ToolFormingEvent, ToolFormingObserver};
use rmcp::model::Role;
use serde::{Deserialize, Serialize};

use crate::conversation::message::{
    Message, MessageContent, SystemNotificationContent, SystemNotificationType,
};
use crate::conversation::Conversation;
use crate::session::extension_data::ExtensionState;
use crate::session::{SessionManager, SessionType};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TurnOutcomeState {
    pub failed: bool,
    pub at: DateTime<Utc>,
    /// The failure as the person saw it in the chat.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// The person stopped the turn (Q-169: a stopped turn left only the user's message, and the
    /// sidebar row read "15m ago").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stopped: Option<Stopped>,
}

impl ExtensionState for TurnOutcomeState {
    const EXTENSION_NAME: &'static str = "turn_outcome";
    const VERSION: &'static str = "v0";
}

/// A turn the person stopped: how long it ran and what the model had written by then.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stopped {
    pub elapsed_ms: u64,
    /// Output tokens the provider reported for the turn's finished calls, plus the stopped call's
    /// streamed output counted by goose's tokenizer (a stopped stream never reports its usage).
    /// `None` only when the tokenizer could not be built.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct FailedSession {
    pub session_id: String,
    pub session_name: String,
    pub working_dir: PathBuf,
    pub at: DateTime<Utc>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone)]
pub struct StoppedSession {
    pub session_id: String,
    pub session_name: String,
    pub working_dir: PathBuf,
    pub at: DateTime<Utc>,
    pub stopped: Stopped,
}

/// A turn that ended on a failure; `shown` is the failure text the chat showed, when it had one.
#[derive(Debug, Clone, PartialEq)]
pub struct Failure {
    pub shown: Option<String>,
}

pub async fn record(
    session_manager: &SessionManager,
    session_id: &str,
    failure: Option<Failure>,
) -> Result<()> {
    let state = TurnOutcomeState {
        failed: failure.is_some(),
        at: Utc::now(),
        reason: failure.and_then(|failure| failure.shown),
        stopped: None,
    };
    session_manager
        .set_extension_state(session_id, &state)
        .await
}

/// The person stopped the turn: recorded as the session's last outcome, and the chat line that
/// says so stored in the conversation (user-only) so a reload shows it where the answer would be.
pub async fn record_stopped(
    session_manager: &SessionManager,
    session_id: &str,
    stopped: Stopped,
) -> Result<Message> {
    let state = TurnOutcomeState {
        failed: false,
        at: Utc::now(),
        reason: None,
        stopped: Some(stopped),
    };
    session_manager
        .set_extension_state(session_id, &state)
        .await?;
    let notice = stopped_notice(stopped);
    session_manager.add_message(session_id, &notice).await?;
    Ok(notice)
}

const STOPPED_NOTICE_KIND: &str = "turn_stopped";

/// "You stopped this answer after 6 min · 1.9k tokens" — what a client without its own rendering
/// shows; the desktop renders the same numbers from the notice's data.
pub fn stopped_line(stopped: Stopped) -> String {
    let elapsed = elapsed_words(stopped.elapsed_ms);
    match stopped.output_tokens {
        Some(0) => {
            format!("You stopped this answer after {elapsed}, before the model wrote anything")
        }
        Some(tokens) => format!(
            "You stopped this answer after {elapsed} · {} tokens",
            compact_tokens(tokens)
        ),
        None => format!("You stopped this answer after {elapsed}"),
    }
}

pub(crate) fn elapsed_words(elapsed_ms: u64) -> String {
    let seconds = elapsed_ms / 1000;
    match seconds {
        0..=59 => format!("{seconds} s"),
        60..=3599 => format!("{} min", seconds / 60),
        _ => format!("{} h {} min", seconds / 3600, (seconds % 3600) / 60),
    }
}

/// The desktop's `compactTokens`: "850", "1.9k", "24k".
pub(crate) fn compact_tokens(tokens: u64) -> String {
    match tokens {
        0..=999 => tokens.to_string(),
        1000..=9999 => format!("{:.1}k", tokens as f64 / 1000.0),
        _ => format!("{}k", (tokens as f64 / 1000.0).round() as u64),
    }
}

fn stopped_notice(stopped: Stopped) -> Message {
    let mut data =
        serde_json::json!({ "kind": STOPPED_NOTICE_KIND, "elapsedMs": stopped.elapsed_ms });
    if let Some(tokens) = stopped.output_tokens {
        data["outputTokens"] = tokens.into();
    }
    Message::assistant()
        .with_system_notification_with_data(
            SystemNotificationType::InlineMessage,
            stopped_line(stopped),
            data,
        )
        .user_only()
}

/// The stopped turn a stored or live notice records, when it is one.
pub fn stopped_of(notification: &SystemNotificationContent) -> Option<Stopped> {
    if notification.notification_type != SystemNotificationType::InlineMessage {
        return None;
    }
    let data = notification.data.as_ref()?;
    if data.get("kind").and_then(serde_json::Value::as_str) != Some(STOPPED_NOTICE_KIND) {
        return None;
    }
    Some(Stopped {
        elapsed_ms: data.get("elapsedMs")?.as_u64()?,
        output_tokens: data.get("outputTokens").and_then(serde_json::Value::as_u64),
    })
}

/// What a turn produced, measured while it streams, for the line a stopped turn leaves: its wall
/// time, the output tokens its finished calls reported, and the output the call in flight has
/// streamed so far (text, reasoning, and forming tool-call arguments) — that call never reports
/// usage once the person stops it.
pub struct TurnMeter {
    started: Instant,
    reported_output_tokens: u64,
    unreported_output: Arc<Mutex<String>>,
}

impl TurnMeter {
    pub fn start() -> Self {
        Self {
            started: Instant::now(),
            reported_output_tokens: 0,
            unreported_output: Arc::default(),
        }
    }

    /// A finished call's reported usage covers everything it streamed.
    pub fn on_usage(&mut self, usage: &ProviderUsage) {
        let Some(output) = usage
            .usage
            .output_tokens
            .and_then(|n| u64::try_from(n).ok())
        else {
            return;
        };
        self.reported_output_tokens += output;
        self.unreported_output
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
    }

    pub fn on_output(&self, text: &str) {
        self.unreported_output
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push_str(text);
    }

    /// Forming tool calls are not yielded until they finish; their argument fragments arrive here.
    pub fn tool_forming_observer(&self) -> ToolFormingObserver {
        let unreported = self.unreported_output.clone();
        Arc::new(move |event| {
            if let ToolFormingEvent::ArgsDelta { delta, .. } = event {
                unreported
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push_str(&delta);
            }
        })
    }

    pub async fn stopped(&self) -> Stopped {
        let elapsed_ms = u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX);
        let unreported = self
            .unreported_output
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let output_tokens = match crate::token_counter::create_token_counter().await {
            Ok(counter) => {
                Some(self.reported_output_tokens + counter.count_tokens(&unreported) as u64)
            }
            Err(error) => {
                tracing::warn!(%error, "a stopped turn's streamed output could not be counted");
                None
            }
        };
        Stopped {
            elapsed_ms,
            output_tokens,
        }
    }
}

/// The words a reply streamed that the agent has not stored yet (Q-519). The agent stores a
/// provider call's messages when that call's iteration ends; a stop — the in-window Stop, or the
/// window closing with the answer live — drops the reply stream first, so the words the person
/// watched stream died with it and the chat reopened on their prompt alone. Holds the call in
/// flight's text and reasoning. A message the model reads that is not its own (a tool's result, a
/// steer, a continuation) belongs to the same iteration, so the agent stores it with the call's
/// words; the next call's first words are what prove that iteration stored.
#[derive(Default)]
pub struct StreamedReply {
    in_flight: Conversation,
    iteration_ended: bool,
}

impl StreamedReply {
    pub fn on_message(&mut self, message: &Message) {
        if !message.is_agent_visible() {
            return;
        }
        if message.role != Role::Assistant {
            self.iteration_ended = true;
            return;
        }
        let words: Vec<MessageContent> = message
            .content
            .iter()
            .filter(|content| {
                matches!(
                    content,
                    MessageContent::Text(_) | MessageContent::Thinking(_)
                )
            })
            .cloned()
            .collect();
        if words.is_empty() {
            return;
        }
        if std::mem::take(&mut self.iteration_ended) {
            self.in_flight = Conversation::default();
        }
        let mut streamed = message.clone();
        streamed.content = words;
        self.in_flight.push(streamed);
    }

    /// Stores the streamed messages no stored message already carries (the same id and the same
    /// words — the net for an iteration that ended with no message between it and the next call).
    /// Reasoning with no text is shown and never sent back: a provider drops unsigned reasoning, and
    /// an assistant turn left empty is one a strict provider rejects.
    pub async fn store(self, session_manager: &SessionManager, session_id: &str) -> Result<()> {
        if self.in_flight.is_empty() {
            return Ok(());
        }
        let stored = session_manager
            .get_session(session_id, true)
            .await?
            .conversation
            .ok_or_else(|| anyhow::anyhow!("session {session_id} has no conversation"))?;
        for message in self.in_flight.messages() {
            let already_stored = stored.messages().iter().any(|row| {
                row.role == Role::Assistant
                    && row.id.is_some()
                    && row.id == message.id
                    && words_of(row) == words_of(message)
            });
            if already_stored {
                continue;
            }
            let has_text = message
                .content
                .iter()
                .any(|content| matches!(content, MessageContent::Text(_)));
            let message = if has_text {
                message.clone()
            } else {
                message.clone().user_only()
            };
            session_manager.add_message(session_id, &message).await?;
        }
        Ok(())
    }
}

fn words_of(message: &Message) -> String {
    message
        .content
        .iter()
        .filter_map(|content| match content {
            MessageContent::Text(text) => Some(text.text.as_str()),
            MessageContent::Thinking(thinking) => Some(thinking.thinking.as_str()),
            _ => None,
        })
        .collect()
}

/// User and scheduled sessions whose last turn failed.
pub async fn failed_sessions(session_manager: &SessionManager) -> Result<Vec<FailedSession>> {
    Ok(session_manager
        .sessions_with_extension_state::<TurnOutcomeState>()
        .await?
        .into_iter()
        .filter(|row| {
            row.state.failed
                && matches!(row.session_type, SessionType::User | SessionType::Scheduled)
        })
        .map(|row| FailedSession {
            session_id: row.session_id,
            session_name: row.name,
            working_dir: row.working_dir,
            at: row.state.at,
            reason: row.state.reason,
        })
        .collect())
}

/// User and scheduled sessions whose last turn the person stopped.
pub async fn stopped_sessions(session_manager: &SessionManager) -> Result<Vec<StoppedSession>> {
    Ok(session_manager
        .sessions_with_extension_state::<TurnOutcomeState>()
        .await?
        .into_iter()
        .filter(|row| matches!(row.session_type, SessionType::User | SessionType::Scheduled))
        .filter_map(|row| {
            Some(StoppedSession {
                stopped: row.state.stopped?,
                session_id: row.session_id,
                session_name: row.name,
                working_dir: row.working_dir,
                at: row.state.at,
            })
        })
        .collect())
}
