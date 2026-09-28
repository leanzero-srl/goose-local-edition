//! Q-357: one compaction of a conversation, as the chat shows it. Every entry point — the reply's
//! start, a turn's tool loop, the recovery from a refused request, and `/compact` — runs through
//! here, so manual and automatic compaction send the same events: a progress status while the
//! engine reads the conversation and the model writes (`StatusMessage::Progress{compaction}`), then
//! a stored notice for how it ended (`StatusMessage::Notice{compaction}`: compacted, a question
//! about the person's note, or failed).
//!
//! The swarm's workers keep the compaction their golden run measured, events and all: the
//! transcript request, the old inline lines, no kept block, no card.

use std::time::Instant;

use async_stream::stream;
use futures::stream::BoxStream;
use goose_providers::model::ModelConfig;
use goose_sdk_types::custom_notifications::{
    CompactionNoteVerdict, CompactionStage, CompactionStatus, CompactionTriggerKind,
};
use rmcp::model::Tool;
use tracing::{error, warn};

use super::agent::COMPACTION_THINKING_TEXT;
use super::{Agent, AgentEvent};
use crate::agents::platform_extensions::ledger::LedgerFile;
use crate::config::Config;
use crate::context_mgmt::pillars::{LedgerRead, NoteVerdict};
use crate::context_mgmt::state::{CompactionState, LastCompaction};
use crate::context_mgmt::{
    self, ChatCompacted, ChatCompaction, CompactionTrigger, NoteOutcome, SummaryRequest,
    WritingProgress, DEFAULT_COMPACTION_THRESHOLD, WRITTEN_PARTS,
};
use crate::conversation::message::{Message, SystemNotificationContent, SystemNotificationType};
use crate::conversation::Conversation;

/// The `kind` in a compaction notice's data.
pub const COMPACTION_NOTICE_KIND: &str = "compaction";

pub(crate) enum CompactionRun {
    /// Automatic, before the reply to the person's new message.
    AtReplyStart,
    /// Automatic, between the calls of a turn: the frame the loop holds.
    MidTurn {
        tools: Vec<Tool>,
        system_prompt: String,
    },
    /// The chat's own request was refused as too long.
    Recovery,
    /// The person asked (`/compact [note]`).
    Manual { note: Option<String> },
}

pub(crate) enum CompactionStep {
    Event(AgentEvent),
    Done(CompactionEnd),
}

pub(crate) enum CompactionEnd {
    Compacted(Conversation),
    /// The model asked about the note; the conversation is unchanged.
    Asked,
    /// Nothing was compacted. `legacy`: the message a swarm worker's call site shows, as it did
    /// before the card (a chat's card has already said it).
    Failed {
        legacy: Option<Message>,
    },
}

/// The compaction status a system notification carries, when it is one.
pub fn compaction_of(notification: &SystemNotificationContent) -> Option<CompactionStatus> {
    let data = notification.data.as_ref()?;
    if data.get("kind").and_then(serde_json::Value::as_str) != Some(COMPACTION_NOTICE_KIND) {
        return None;
    }
    match serde_json::from_value(data.clone()) {
        Ok(status) => Some(status),
        Err(e) => {
            warn!("a compaction notice's data does not parse ({e}); shown as its text alone");
            None
        }
    }
}

fn status_notification(status: &CompactionStatus) -> Message {
    let kind = match status.stage {
        CompactionStage::Reading | CompactionStage::Writing => {
            SystemNotificationType::ThinkingMessage
        }
        _ => SystemNotificationType::InlineMessage,
    };
    let mut data = serde_json::to_value(status).expect("a compaction status serializes");
    data["kind"] = COMPACTION_NOTICE_KIND.into();
    let inline = kind == SystemNotificationType::InlineMessage;
    let message =
        Message::assistant().with_system_notification_with_data(kind, status_line(status), data);
    if inline {
        message.user_only()
    } else {
        message
    }
}

/// What a client without the card shows: the status in one line.
pub fn status_line(status: &CompactionStatus) -> String {
    use crate::turn_outcome::{compact_tokens, elapsed_words};
    let tokens = |n: Option<u64>| n.map(compact_tokens);
    match status.stage {
        CompactionStage::Reading => match tokens(status.tokens_before) {
            Some(before) => format!("Compacting the conversation · reading {before} tokens"),
            None => "Compacting the conversation".to_string(),
        },
        CompactionStage::Writing => {
            let mut line = "Compacting the conversation · writing the summary".to_string();
            if let Some(written) = tokens(status.written_tokens) {
                line.push_str(&format!(" · {written} tokens written"));
            }
            if !status.parts.is_empty() {
                line.push_str(&format!(
                    " · part {} of {}",
                    status.parts.len(),
                    status.parts_total
                ));
            }
            line
        }
        CompactionStage::Done => {
            let mut line = match (tokens(status.tokens_before), tokens(status.tokens_after)) {
                (Some(before), Some(after)) => {
                    format!("Conversation compacted · {before} → {after} tokens")
                }
                _ => "Conversation compacted".to_string(),
            };
            line.push_str(&format!(" · {}", elapsed_words(status.elapsed_ms)));
            match (status.note_verdict, status.said.as_deref()) {
                (Some(CompactionNoteVerdict::Concern), Some(said)) => line.push_str(&format!(
                    ". Compacted following your note. goose noted: “{said}”"
                )),
                (Some(CompactionNoteVerdict::Missing), _) => line.push_str(
                    ". goose didn't say whether your note was clear — it was followed as written.",
                ),
                (Some(CompactionNoteVerdict::NotSent), _) => line.push_str(
                    ". The conversation was too long to send your note with it; it is kept word \
                     for word in what goose keeps.",
                ),
                _ => {}
            }
            if let Some(warning) = &status.warning {
                line.push_str(&format!(". {warning}"));
            }
            line
        }
        CompactionStage::Question => format!(
            "goose asks about your note: “{}”",
            status.said.as_deref().unwrap_or_default()
        ),
        CompactionStage::Failed => format!(
            "Compaction failed: {}. Your conversation is unchanged.",
            status.error.as_deref().unwrap_or_default()
        ),
    }
}

impl Agent {
    /// One compaction of `conversation`, reported as the chat shows it; the last step says how it
    /// ended. The compacted conversation is stored and the session's usage updated before the end.
    pub(crate) fn run_compaction<'a>(
        &'a self,
        session_id: &'a str,
        schedule_id: Option<String>,
        model_config: ModelConfig,
        conversation: Conversation,
        run: CompactionRun,
    ) -> BoxStream<'a, CompactionStep> {
        if self.is_swarm_worker() {
            return self.run_worker_compaction(
                session_id,
                schedule_id,
                model_config,
                conversation,
                run,
            );
        }
        Box::pin(stream! {
            let started = Instant::now();
            let sessions = self.config.session_manager.clone();
            let manual = matches!(run, CompactionRun::Manual { .. });
            let trigger_kind = match run {
                CompactionRun::Manual { .. } => CompactionTriggerKind::Manual,
                CompactionRun::Recovery => CompactionTriggerKind::Recovery,
                _ => CompactionTriggerKind::Auto,
            };
            let mut status = CompactionStatus {
                stage: CompactionStage::Reading,
                trigger: trigger_kind,
                tokens_before: None,
                tokens_after: None,
                written_tokens: None,
                parts: Vec::new(),
                parts_total: WRITTEN_PARTS.len() as u32,
                elapsed_ms: 0,
                writing_ms: None,
                note: None,
                note_verdict: None,
                said: None,
                error: None,
                warning: None,
            };

            let prepared = self
                .prepare_chat_compaction(session_id, &model_config, &run, &mut status)
                .await;
            let (state, chat, request) = match prepared {
                Ok(prepared) => prepared,
                Err(e) => {
                    status.stage = CompactionStage::Failed;
                    status.error = Some(e.to_string());
                    status.elapsed_ms = started.elapsed().as_millis() as u64;
                    for event in self.store_notice(session_id, &status).await {
                        yield CompactionStep::Event(event);
                    }
                    yield CompactionStep::Done(CompactionEnd::Failed { legacy: None });
                    return;
                }
            };
            yield CompactionStep::Event(AgentEvent::Message(status_notification(&status)));

            let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<WritingProgress>();
            let chat = ChatCompaction {
                progress: Some(std::sync::Arc::new(move |progress| {
                    // The card stopped listening once the compaction ended; nothing is lost.
                    let _ = sender.send(progress);
                })),
                ..chat
            };
            let provider = match self.provider().await {
                Ok(provider) => provider,
                Err(e) => {
                    status.stage = CompactionStage::Failed;
                    status.error = Some(e.to_string());
                    for event in self.store_notice(session_id, &status).await {
                        yield CompactionStep::Event(event);
                    }
                    yield CompactionStep::Done(CompactionEnd::Failed { legacy: None });
                    return;
                }
            };
            let compaction = context_mgmt::compact_chat(
                provider.as_ref(),
                &model_config,
                session_id,
                &conversation,
                manual,
                &request,
                &chat,
            );
            tokio::pin!(compaction);
            let mut first_output: Option<Instant> = None;
            let mut wrote = |progress: WritingProgress, status: &mut CompactionStatus| {
                let now = Instant::now();
                let first = *first_output.get_or_insert(now);
                status.stage = CompactionStage::Writing;
                status.written_tokens = progress.written_tokens;
                status.parts = progress.parts;
                status.elapsed_ms = started.elapsed().as_millis() as u64;
                status.writing_ms = Some(now.duration_since(first).as_millis() as u64);
                AgentEvent::Message(status_notification(status))
            };
            let outcome = loop {
                enum Next {
                    Wrote(WritingProgress),
                    Ended(anyhow::Result<ChatCompacted>),
                }
                let next = tokio::select! {
                    biased;
                    Some(progress) = receiver.recv() => Next::Wrote(progress),
                    outcome = &mut compaction => Next::Ended(outcome),
                };
                match next {
                    Next::Wrote(progress) => {
                        yield CompactionStep::Event(wrote(progress, &mut status));
                    }
                    Next::Ended(outcome) => break outcome,
                }
            };
            // An answer that arrived in one poll leaves its progress queued: the last of it is what
            // was written, and the card shows it before the end.
            let mut last_progress = None;
            while let Ok(progress) = receiver.try_recv() {
                last_progress = Some(progress);
            }
            if let Some(progress) = last_progress {
                yield CompactionStep::Event(wrote(progress, &mut status));
            }
            status.elapsed_ms = started.elapsed().as_millis() as u64;

            match outcome {
                Ok(ChatCompacted::Compacted { conversation: compacted, usage, note, .. }) => {
                    let stored = async {
                        sessions.replace_conversation(session_id, &compacted).await?;
                        self.update_session_metrics(session_id, schedule_id, &usage, true)
                            .await?;
                        anyhow::Ok(
                            sessions
                                .get_session(session_id, false)
                                .await?
                                .usage
                                .total_tokens,
                        )
                    }
                    .await;
                    let tokens_after = match stored {
                        Ok(tokens) => tokens,
                        Err(e) => {
                            status.stage = CompactionStage::Failed;
                            status.error = Some(e.to_string());
                            for event in self.store_notice(session_id, &status).await {
                                yield CompactionStep::Event(event);
                            }
                            yield CompactionStep::Done(CompactionEnd::Failed { legacy: None });
                            return;
                        }
                    };
                    status.stage = CompactionStage::Done;
                    status.tokens_after = tokens_after.map(|t| t.max(0) as u64);
                    (status.note_verdict, status.said) = match note {
                        NoteOutcome::NoNote => (None, None),
                        NoteOutcome::NotSent => (Some(CompactionNoteVerdict::NotSent), None),
                        NoteOutcome::Read(NoteVerdict::Ok) => (Some(CompactionNoteVerdict::Ok), None),
                        NoteOutcome::Read(NoteVerdict::Missing) => {
                            (Some(CompactionNoteVerdict::Missing), None)
                        }
                        NoteOutcome::Read(NoteVerdict::Concern(said)) => {
                            (Some(CompactionNoteVerdict::Concern), Some(said))
                        }
                        NoteOutcome::Read(NoteVerdict::Question(said)) => {
                            (Some(CompactionNoteVerdict::Question), Some(said))
                        }
                    };
                    let last = LastCompaction {
                        at: chrono::Utc::now(),
                        trigger: status.trigger,
                        tokens_before: status.tokens_before,
                        tokens_after: status.tokens_after,
                        elapsed_ms: status.elapsed_ms,
                        note_verdict: status.note_verdict,
                        said: status.said.clone(),
                    };
                    if let Err(e) = state.after_compaction(last).save(&sessions, session_id).await {
                        status.warning = Some(format!(
                            "goose could not record this compaction for the chat ({e})"
                        ));
                    }
                    yield CompactionStep::Event(AgentEvent::HistoryReplaced(compacted.clone()));
                    for event in self.store_notice(session_id, &status).await {
                        yield CompactionStep::Event(event);
                    }
                    yield CompactionStep::Done(CompactionEnd::Compacted(compacted));
                }
                Ok(ChatCompacted::Asked { question }) => {
                    status.stage = CompactionStage::Question;
                    status.note_verdict = Some(CompactionNoteVerdict::Question);
                    status.said = Some(question);
                    for event in self.store_notice(session_id, &status).await {
                        yield CompactionStep::Event(event);
                    }
                    yield CompactionStep::Done(CompactionEnd::Asked);
                }
                Err(e) => {
                    if matches!(run, CompactionRun::Recovery) {
                        #[cfg(feature = "telemetry")]
                        crate::posthog::emit_error("compaction_failed", &e.to_string());
                        error!("Compaction failed: {}", e);
                    } else {
                        warn!("compaction failed: {e}");
                    }
                    status.stage = CompactionStage::Failed;
                    status.error = Some(e.to_string());
                    for event in self.store_notice(session_id, &status).await {
                        yield CompactionStep::Event(event);
                    }
                    yield CompactionStep::Done(CompactionEnd::Failed { legacy: None });
                }
            }
        })
    }

    /// Everything a chat's compaction reads besides the conversation: the session, the person's
    /// settings (a typed `/compact <note>` saved as the note), the trigger's numbers, the ledger,
    /// the goal, the kept block's budget, and the summary request.
    async fn prepare_chat_compaction(
        &self,
        session_id: &str,
        model_config: &ModelConfig,
        run: &CompactionRun,
        status: &mut CompactionStatus,
    ) -> anyhow::Result<(CompactionState, ChatCompaction, SummaryRequest)> {
        let sessions = self.config.session_manager.clone();
        let session = sessions.get_session(session_id, false).await?;
        let mut state = match CompactionState::of(&session.extension_data) {
            Ok(state) => state,
            Err(e) => {
                status.warning = Some(format!(
                    "goose could not read the note saved for this chat ({e}) and compacted \
                     without it"
                ));
                CompactionState::default()
            }
        };
        if let CompactionRun::Manual { note: Some(typed) } = run {
            if !typed.trim().is_empty() {
                state.note = Some(typed.trim().to_string());
                state.follow_as_written = false;
                state.save(&sessions, session_id).await?;
            }
        }
        let note = state.note().map(str::to_string);
        let may_ask = matches!(run, CompactionRun::Manual { .. })
            && note.is_some()
            && !state.follow_as_written;
        status.note = note.clone();
        status.tokens_before = session.usage.total_tokens.map(|t| t.max(0) as u64);

        let provider = self.provider().await?;
        let limit = context_mgmt::effective_context_limit(provider.as_ref(), model_config).await;
        let trigger = match run {
            CompactionRun::Manual { .. } => CompactionTrigger::Manual,
            CompactionRun::Recovery => CompactionTrigger::Recovery,
            CompactionRun::AtReplyStart | CompactionRun::MidTurn { .. } => {
                CompactionTrigger::Auto {
                    used: status.tokens_before.map(|t| t as usize),
                    limit,
                    threshold: Config::global()
                        .get_param::<f64>("GOOSE_AUTO_COMPACT_THRESHOLD")
                        .unwrap_or(DEFAULT_COMPACTION_THRESHOLD),
                }
            }
        };
        let ledger = match LedgerFile::for_chat(
            &session.working_dir,
            session_id,
            dirs::home_dir().as_deref(),
        )
        .read_all()
        {
            Ok(entries) => LedgerRead::Entries(entries),
            Err(e) => LedgerRead::Unreadable(e.to_string()),
        };
        let request = match run {
            CompactionRun::Manual { .. } | CompactionRun::AtReplyStart => {
                self.summary_request_for_next_reply(&session).await?
            }
            CompactionRun::MidTurn {
                tools,
                system_prompt,
            } => self.summary_request_extending(tools, system_prompt).await,
            CompactionRun::Recovery => SummaryRequest::Transcript,
        };
        let chat = ChatCompaction {
            trigger,
            note,
            may_ask,
            pins: state.pins.clone(),
            goal: self.get_goal().await,
            ledger,
            kept_budget_chars: context_mgmt::kept_budget_chars(limit),
            progress: None,
        };
        Ok((state, chat, request))
    }

    /// Stores how a compaction ended where the chat shows it (user-only, so a reload shows the card
    /// again), and returns the event that shows it now. A notice the store refused is still shown.
    async fn store_notice(&self, session_id: &str, status: &CompactionStatus) -> Vec<AgentEvent> {
        let notice = status_notification(status);
        if let Err(e) = self
            .config
            .session_manager
            .add_message(session_id, &notice)
            .await
        {
            warn!("the compaction notice was not stored ({e}); it is shown but a reload loses it");
        }
        vec![AgentEvent::Message(notice)]
    }

    /// The swarm workers' compaction as their golden run measured it: the transcript request, the
    /// old inline lines, the answer stored as written.
    fn run_worker_compaction<'a>(
        &'a self,
        session_id: &'a str,
        schedule_id: Option<String>,
        model_config: ModelConfig,
        conversation: Conversation,
        run: CompactionRun,
    ) -> BoxStream<'a, CompactionStep> {
        Box::pin(stream! {
            let inline = |text: String| {
                AgentEvent::Message(
                    Message::assistant()
                        .with_system_notification(SystemNotificationType::InlineMessage, text),
                )
            };
            let before = match &run {
                CompactionRun::AtReplyStart => {
                    let threshold = Config::global()
                        .get_param::<f64>("GOOSE_AUTO_COMPACT_THRESHOLD")
                        .unwrap_or(DEFAULT_COMPACTION_THRESHOLD);
                    Some(format!(
                        "Exceeded auto-compact threshold of {}%. Performing auto-compaction...",
                        (threshold * 100.0) as u32
                    ))
                }
                CompactionRun::Recovery => {
                    Some("Context limit reached. Compacting to continue conversation...".to_string())
                }
                CompactionRun::MidTurn { .. } => {
                    Some("Context near the cap — compacting to stay lean...".to_string())
                }
                CompactionRun::Manual { .. } => None,
            };
            if let Some(before) = before {
                yield CompactionStep::Event(inline(before));
                yield CompactionStep::Event(AgentEvent::Message(
                    Message::assistant().with_system_notification(
                        SystemNotificationType::ThinkingMessage,
                        COMPACTION_THINKING_TEXT,
                    ),
                ));
            }
            let provider = match self.provider().await {
                Ok(provider) => provider,
                Err(e) => {
                    yield CompactionStep::Done(CompactionEnd::Failed {
                        legacy: worker_failure(&run, &e),
                    });
                    return;
                }
            };
            let manual = matches!(run, CompactionRun::Manual { .. });
            let result = context_mgmt::compact_messages(
                provider.as_ref(),
                &model_config,
                session_id,
                &conversation,
                manual,
                &SummaryRequest::Transcript,
            )
            .await;
            let stored = match result {
                Ok((compacted, usage)) => async {
                    self.config
                        .session_manager
                        .replace_conversation(session_id, &compacted)
                        .await?;
                    self.update_session_metrics(session_id, schedule_id, &usage, true)
                        .await?;
                    anyhow::Ok(compacted)
                }
                .await,
                Err(e) => Err(e),
            };
            match stored {
                Ok(compacted) => {
                    yield CompactionStep::Event(AgentEvent::HistoryReplaced(compacted.clone()));
                    if matches!(run, CompactionRun::AtReplyStart) {
                        yield CompactionStep::Event(inline("Compaction complete".to_string()));
                    }
                    yield CompactionStep::Done(CompactionEnd::Compacted(compacted));
                }
                Err(e) => {
                    if matches!(run, CompactionRun::Recovery) {
                        #[cfg(feature = "telemetry")]
                        crate::posthog::emit_error("compaction_failed", &e.to_string());
                        error!("Compaction failed: {}", e);
                    }
                    yield CompactionStep::Done(CompactionEnd::Failed {
                        legacy: worker_failure(&run, &e),
                    });
                }
            }
        })
    }
}

/// `/compact [note]` (and its aliases): `Some(note)` for the command, the note when it has one.
pub(crate) fn compact_command_note(message_text: &str) -> Option<Option<String>> {
    let parsed = crate::agents::execute_commands::parse_slash_command(message_text)?;
    (parsed.command == "compact")
        .then(|| Some(parsed.params_str.trim().to_string()).filter(|note| !note.is_empty()))
}

impl Agent {
    /// The reply to `/compact [note]`: the command stays in the chat (the person's, never the
    /// agent's), then the compaction's events. No turn follows.
    pub(crate) async fn manual_compaction_reply(
        &self,
        user_message: Message,
        session_config: crate::agents::types::SessionConfig,
        note: Option<String>,
    ) -> anyhow::Result<BoxStream<'_, anyhow::Result<AgentEvent>>> {
        use futures::StreamExt;
        let sessions = self.config.session_manager.clone();
        sessions
            .add_message(
                &session_config.id,
                &user_message.clone().with_visibility(true, false),
            )
            .await?;
        let conversation = sessions
            .get_session(&session_config.id, true)
            .await?
            .conversation
            .ok_or_else(|| anyhow::anyhow!("Session has no conversation"))?;
        let model_config = self.model_config_for_session(&session_config.id).await?;
        Ok(Box::pin(async_stream::try_stream! {
            yield AgentEvent::Message(user_message);
            let mut compaction = self.run_compaction(
                &session_config.id,
                session_config.schedule_id.clone(),
                model_config,
                conversation,
                CompactionRun::Manual { note },
            );
            while let Some(step) = compaction.next().await {
                match step {
                    CompactionStep::Event(event) => yield event,
                    CompactionStep::Done(CompactionEnd::Failed { legacy: Some(message) }) => {
                        yield AgentEvent::Message(message);
                    }
                    CompactionStep::Done(_) => {}
                }
            }
        }))
    }
}

/// What a swarm worker's call site showed when its compaction failed.
fn worker_failure(run: &CompactionRun, e: &anyhow::Error) -> Option<Message> {
    let text = format!(
        "Ran into this error trying to compact: {e}.\n\nPlease try again or create a new session"
    );
    match run {
        CompactionRun::AtReplyStart | CompactionRun::Manual { .. } => {
            Some(Message::assistant().with_text(text))
        }
        CompactionRun::Recovery => Some(Message::assistant().with_text(text).user_only()),
        CompactionRun::MidTurn { .. } => {
            warn!("per-turn proactive compaction failed: {e}");
            None
        }
    }
}
