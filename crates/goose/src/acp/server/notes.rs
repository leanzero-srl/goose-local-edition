//! Notes to another chat over ACP (Q-358 part 2, DESIGN-Q358-TRANSCRIPTS §2).
//!
//! - `notes/send` is THE PERSON'S CLICK: the only door through which a note leaves the chat that
//!   drafted it (`chat_notes::send`). "Steer it now" queues the note into the target's running turn
//!   (`Agent::steer`, it drains between tool calls); an idle target is offered the note as a turn of
//!   its own through `notes/deliverDue`, sent only to the windows that show it; no window shows it,
//!   and it waits until one does.
//! - A prompt carrying `_meta.goose.crossNote = {noteId, messageId}` is that turn: honoured only for
//!   a note waiting in this chat whose exact framing the prompt carries, taken once, never a slash
//!   command, and it supersedes none of the chat's open questions. A mark that is not honoured is
//!   REFUSED with its reason — the framing is goose's text, not the person's, so it is never sent on
//!   as if they had typed it.
//! - Before the person's next message in the sending chat, its model reads what became of each
//!   note (read there, dismissed there, gone) as an agent-only line, told once.

use std::sync::{Arc, Mutex as StdMutex};

use agent_client_protocol::schema::v1::{
    ContentBlock, ContentChunk, SessionId, SessionNotification, SessionUpdate, TextContent,
};
use agent_client_protocol::{Client, ConnectionTo};
use chrono::{DateTime, Utc};
use goose_sdk_types::custom_notifications::{
    NotesChangedNotification, NotesDeliverDueNotification,
};
use goose_sdk_types::custom_requests::{
    InboxNoteDto, InboxNoteStatus, NoteChatDto, NoteDeliveredHow, NoteDelivery, NoteDraftAction,
    NoteDraftDto, NoteDraftStatus, NoteInboxAction, NoteLiveState, NoteOutcomeDto,
    NoteOutcomeState, NoteResolution, NotesDraftRequest, NotesDraftResponse, NotesInboxRequest,
    NotesInboxResponse, NotesListRequest, NotesListResponse, NotesSendRequest, NotesSendResponse,
    NotesShowingRequest, NotesShowingResponse, NotesTargetsRequest, NotesTargetsResponse,
    NotesWaitingDto,
};
use tracing::warn;

use super::{message_update_meta, GooseAcpAgent};
use crate::chat_notes::{
    self, DeliveredHow, Delivery, DoorGuard, DraftNote, DraftStatus, InboxAction, InboxNote,
    InboxStatus, NoteChat, NoteDoor, NoteDue, Outcome, Steered, TargetResolution,
};
use crate::conversation::message::Message;
use crate::session::SessionManager;

fn invalid(error: impl std::fmt::Display) -> agent_client_protocol::Error {
    agent_client_protocol::Error::invalid_params().data(error.to_string())
}

fn internal(error: impl std::fmt::Display) -> agent_client_protocol::Error {
    agent_client_protocol::Error::internal_error().data(error.to_string())
}

/// One window's connection as the notes hub reaches it.
struct ConnectionNoteDoor {
    cx: ConnectionTo<Client>,
}

impl NoteDoor for ConnectionNoteDoor {
    fn deliver_due(&self, due: &NoteDue) {
        if let Err(error) = self.cx.send_notification(NotesDeliverDueNotification {
            session_id: due.session_id.clone(),
            note_id: due.note_id.clone(),
            message_id: due.message_id.clone(),
            prompt: due.prompt.clone(),
        }) {
            warn!(session_id = %due.session_id, note_id = %due.note_id, ?error, "notes: the note's offer did not reach this window; it is offered again when the chat's turn ends or a window shows it");
        }
    }

    fn changed(&self, session_ids: &[String]) {
        if let Err(error) = self.cx.send_notification(NotesChangedNotification {
            session_ids: session_ids.to_vec(),
        }) {
            warn!(
                ?session_ids,
                ?error,
                "notes: a notes change did not reach this window"
            );
        }
    }
}

/// The connection's notes door: open from `initialize` (for a client that hears goose's own
/// notifications) until the connection ends.
#[derive(Default)]
pub(super) struct NoteDoorSlot(StdMutex<Option<DoorGuard>>);

impl NoteDoorSlot {
    pub(super) fn open(&self, cx: &ConnectionTo<Client>, hears_goose: bool) {
        if !hears_goose {
            return;
        }
        let mut slot = self.0.lock().expect("notes door slot poisoned");
        if slot.is_none() {
            *slot =
                Some(chat_notes::hub().open_door(Arc::new(ConnectionNoteDoor { cx: cx.clone() })));
        }
    }

    pub(super) fn close(&self) {
        let guard = self.0.lock().expect("notes door slot poisoned").take();
        drop(guard);
    }

    fn set_showing(&self, session_id: &str, showing: bool) -> Result<(), String> {
        let slot = self.0.lock().expect("notes door slot poisoned");
        let Some(guard) = slot.as_ref() else {
            return Err(
                "this connection does not hear goose's notifications, so no note can be offered to it"
                    .to_string(),
            );
        };
        chat_notes::hub().set_showing(guard, session_id, showing);
        Ok(())
    }
}

/// `_meta.goose.crossNote = {noteId, messageId}`: `None` when the prompt carries no mark, `Err`
/// when it carries one that is not a note id with its message id.
pub(super) fn cross_note_id(
    meta: Option<&serde_json::Map<String, serde_json::Value>>,
) -> Option<Result<String, String>> {
    let mark = meta?.get("goose")?.get(chat_notes::CROSS_NOTE_META_KEY)?;
    let note_id = mark.get("noteId").and_then(|v| v.as_str());
    let message_id = mark.get("messageId").and_then(|v| v.as_str());
    Some(match (note_id, message_id) {
        (Some(note_id), Some(message_id))
            if message_id == format!("{}{note_id}", chat_notes::MESSAGE_ID_PREFIX) =>
        {
            Ok(note_id.to_string())
        }
        _ => Err(format!(
            "the note mark is not a note id with its message id: {mark}"
        )),
    })
}

fn rfc3339(at: DateTime<Utc>) -> String {
    at.to_rfc3339()
}

fn how_dto(how: DeliveredHow) -> NoteDeliveredHow {
    match how {
        DeliveredHow::Steered => NoteDeliveredHow::Steered,
        DeliveredHow::OwnTurn => NoteDeliveredHow::OwnTurn,
        DeliveredHow::WithYourMessage => NoteDeliveredHow::WithYourMessage,
    }
}

fn delivery_dto(delivery: Delivery) -> NoteDelivery {
    match delivery {
        Delivery::SteerNow => NoteDelivery::SteerNow,
        Delivery::LeaveThere => NoteDelivery::LeaveThere,
    }
}

fn inbox_dto(note: &InboxNote) -> InboxNoteDto {
    InboxNoteDto {
        id: note.id.clone(),
        from_session_id: note.from.session_id.clone(),
        from_name: note.from.name.clone(),
        from_working_dir: note.from.working_dir.to_string_lossy().to_string(),
        from_folder: chat_notes::home_relative(&note.from.working_dir),
        text: note.text.clone(),
        sent_at: rfc3339(note.sent_at),
        delivery: delivery_dto(note.delivery),
        status: match note.status {
            InboxStatus::Waiting => InboxNoteStatus::Waiting,
            InboxStatus::Steering => InboxNoteStatus::Steering,
            InboxStatus::WithNextMessage => InboxNoteStatus::WithNextMessage,
            InboxStatus::Delivered => InboxNoteStatus::Delivered,
            InboxStatus::Dismissed => InboxNoteStatus::Dismissed,
        },
        offer_when_idle: note.offer_when_idle,
        delivered_at: note.delivered_at.map(rfc3339),
        delivered_how: note.delivered_how.map(how_dto),
        dismissed_at: note.dismissed_at.map(rfc3339),
        message_id: note.message_id(),
        prompt: chat_notes::framing(note),
    }
}

fn live_state(session_id: &str) -> NoteLiveState {
    let hub = chat_notes::hub();
    if hub.is_running(session_id) {
        NoteLiveState::Working
    } else if hub.is_shown(session_id) {
        NoteLiveState::Idle
    } else {
        NoteLiveState::NotOpen
    }
}

/// A chat as the card names it, with where it stands now. The chat's name and folder are read
/// fresh; a chat that can no longer be read keeps the name the note recorded and says so.
async fn chat_dto(session_manager: &SessionManager, chat: &NoteChat) -> NoteChatDto {
    match session_manager.get_session(&chat.session_id, false).await {
        Ok(session) => NoteChatDto {
            session_id: session.id.clone(),
            name: session.name.clone(),
            working_dir: session.working_dir.to_string_lossy().to_string(),
            folder: chat_notes::home_relative(&session.working_dir),
            live: live_state(&session.id),
            last_active_at: rfc3339(session.last_message_at.unwrap_or(session.updated_at)),
        },
        Err(error) => {
            warn!(session_id = %chat.session_id, %error, "notes: a note's chat is unreadable; the card shows the name the note recorded");
            NoteChatDto {
                session_id: chat.session_id.clone(),
                name: chat.name.clone(),
                working_dir: chat.working_dir.to_string_lossy().to_string(),
                folder: chat_notes::home_relative(&chat.working_dir),
                live: NoteLiveState::NotOpen,
                last_active_at: String::new(),
            }
        }
    }
}

fn outcome_dto(outcome: &Outcome) -> NoteOutcomeDto {
    match outcome {
        Outcome::Waiting(note) => NoteOutcomeDto {
            state: match note.status {
                InboxStatus::Steering => NoteOutcomeState::Steering,
                InboxStatus::WithNextMessage => NoteOutcomeState::WithNextMessage,
                _ => NoteOutcomeState::Waiting,
            },
            ..Default::default()
        },
        Outcome::Delivered(note) => NoteOutcomeDto {
            state: NoteOutcomeState::Delivered,
            at: note.delivered_at.map(rfc3339),
            how: note.delivered_how.map(how_dto),
            reason: None,
        },
        Outcome::Dismissed(note) => NoteOutcomeDto {
            state: NoteOutcomeState::Dismissed,
            at: note.dismissed_at.map(rfc3339),
            how: None,
            reason: None,
        },
        Outcome::Gone(reason) => NoteOutcomeDto {
            state: NoteOutcomeState::Gone,
            reason: Some(reason.clone()),
            ..Default::default()
        },
    }
}

async fn draft_dto(session_manager: &SessionManager, draft: &DraftNote) -> NoteDraftDto {
    let target = match &draft.target {
        Some(chat) => Some(chat_dto(session_manager, chat).await),
        None => None,
    };
    let mut candidates = Vec::with_capacity(draft.candidates.len());
    for chat in &draft.candidates {
        candidates.push(chat_dto(session_manager, chat).await);
    }
    let outcome = match draft.status {
        DraftStatus::Sent => Some(outcome_dto(
            &chat_notes::outcome(session_manager, draft).await,
        )),
        _ => None,
    };
    NoteDraftDto {
        id: draft.id.clone(),
        to_query: draft.to_query.clone(),
        text: draft.text.clone(),
        created_at: rfc3339(draft.created_at),
        resolution: match draft.resolution {
            TargetResolution::TitleWords => NoteResolution::TitleWords,
            TargetResolution::LiveState => NoteResolution::LiveState,
            TargetResolution::Ambiguous => NoteResolution::Ambiguous,
            TargetResolution::NoMatch => NoteResolution::NoMatch,
            TargetResolution::PickedByPerson => NoteResolution::PickedByPerson,
        },
        target,
        candidates,
        status: match draft.status {
            DraftStatus::Draft => NoteDraftStatus::Draft,
            DraftStatus::Sent => NoteDraftStatus::Sent,
            DraftStatus::Cancelled => NoteDraftStatus::Cancelled,
        },
        delivery: draft.delivery.map(delivery_dto),
        sent_at: draft.sent_at.map(rfc3339),
        outcome,
    }
}

impl GooseAcpAgent {
    /// Open this connection's notes door once `initialize` has said what the client hears.
    pub(super) fn open_notes_door(&self, cx: &ConnectionTo<Client>) {
        self.notes_door
            .open(cx, self.supports_goose_custom_notifications());
    }

    pub(super) async fn on_notes_list(
        &self,
        req: NotesListRequest,
    ) -> Result<NotesListResponse, agent_client_protocol::Error> {
        let state = chat_notes::read_state(&self.session_manager, &req.session_id)
            .await
            .map_err(invalid)?;
        let mut drafts = Vec::with_capacity(state.drafts.len());
        for draft in &state.drafts {
            drafts.push(draft_dto(&self.session_manager, draft).await);
        }
        Ok(NotesListResponse {
            drafts,
            inbox: state.inbox.iter().map(inbox_dto).collect(),
        })
    }

    pub(super) async fn on_notes_targets(
        &self,
        req: NotesTargetsRequest,
    ) -> Result<NotesTargetsResponse, agent_client_protocol::Error> {
        let chats = chat_notes::target_chats(&self.session_manager, &req.session_id)
            .await
            .map_err(internal)?;
        Ok(NotesTargetsResponse {
            chats: chats
                .iter()
                .map(|chat| NoteChatDto {
                    session_id: chat.id.clone(),
                    name: chat.name.clone(),
                    working_dir: chat.working_dir.to_string_lossy().to_string(),
                    folder: chat_notes::home_relative(&chat.working_dir),
                    live: live_state(&chat.id),
                    last_active_at: rfc3339(chat.last_message_at.unwrap_or(chat.updated_at)),
                })
                .collect(),
        })
    }

    /// The person's click: the note leaves the draft for the target's inbox, then "Steer it now"
    /// steers it into a running turn or offers it to the windows showing an idle chat.
    pub(super) async fn on_notes_send(
        &self,
        req: NotesSendRequest,
    ) -> Result<NotesSendResponse, agent_client_protocol::Error> {
        let delivery = match req.delivery {
            NoteDelivery::SteerNow => Delivery::SteerNow,
            NoteDelivery::LeaveThere => Delivery::LeaveThere,
        };
        let (draft, note) = chat_notes::send(
            &self.session_manager,
            &req.session_id,
            &req.note_id,
            &req.text,
            delivery,
        )
        .await
        .map_err(invalid)?;
        let target = draft
            .target
            .clone()
            .expect("a sent draft has a target")
            .session_id;
        let hub = chat_notes::hub();
        if delivery == Delivery::SteerNow {
            match hub
                .steer(&self.session_manager, &target, &note.id)
                .await
                .map_err(internal)?
            {
                Steered::Queued { .. } => {}
                Steered::Idle => hub.offer_due(&self.session_manager, &target).await,
            }
        }
        hub.changed(&[req.session_id.clone(), target]);
        Ok(NotesSendResponse {
            draft: draft_dto(&self.session_manager, &draft).await,
        })
    }

    pub(super) async fn on_notes_draft(
        &self,
        req: NotesDraftRequest,
    ) -> Result<NotesDraftResponse, agent_client_protocol::Error> {
        let draft = match req.action {
            NoteDraftAction::Retarget { to_session_id } => {
                chat_notes::retarget(
                    &self.session_manager,
                    &req.session_id,
                    &req.note_id,
                    &to_session_id,
                )
                .await
            }
            NoteDraftAction::Cancel => {
                chat_notes::cancel(&self.session_manager, &req.session_id, &req.note_id).await
            }
        }
        .map_err(invalid)?;
        chat_notes::hub().changed(std::slice::from_ref(&req.session_id));
        Ok(NotesDraftResponse {
            draft: draft_dto(&self.session_manager, &draft).await,
        })
    }

    /// What the person in the target chat does with a note. "Steer this turn" on a chat whose turn
    /// has just ended cannot steer: the note is offered as its own turn instead, which is what the
    /// person asked for — goose reads it now.
    pub(super) async fn on_notes_inbox(
        &self,
        req: NotesInboxRequest,
    ) -> Result<NotesInboxResponse, agent_client_protocol::Error> {
        let hub = chat_notes::hub();
        let sm = &self.session_manager;
        let (session_id, note_id) = (req.session_id.as_str(), req.note_id.as_str());
        let offer_now = |action| async move {
            let note = chat_notes::act(sm, session_id, note_id, action)
                .await
                .map_err(invalid)?;
            hub.offer_due(sm, session_id).await;
            Ok::<_, agent_client_protocol::Error>(note)
        };
        let note = match req.action {
            NoteInboxAction::SteerThisTurn => {
                match hub.steer(sm, session_id, note_id).await.map_err(invalid)? {
                    Steered::Queued { .. } => {}
                    Steered::Idle => {
                        offer_now(InboxAction::AfterThisTurn).await?;
                    }
                }
                self.inbox_note(session_id, note_id).await?
            }
            NoteInboxAction::AfterThisTurn => offer_now(InboxAction::AfterThisTurn).await?,
            NoteInboxAction::AddToNextMessage => {
                chat_notes::act(sm, session_id, note_id, InboxAction::AddToNextMessage)
                    .await
                    .map_err(invalid)?
            }
            NoteInboxAction::Dismiss => {
                chat_notes::act(sm, session_id, note_id, InboxAction::Dismiss)
                    .await
                    .map_err(invalid)?
            }
        };
        hub.changed(&[session_id.to_string(), note.from.session_id.clone()]);
        Ok(NotesInboxResponse {
            note: inbox_dto(&note),
        })
    }

    async fn inbox_note(
        &self,
        session_id: &str,
        note_id: &str,
    ) -> Result<InboxNote, agent_client_protocol::Error> {
        chat_notes::read_state(&self.session_manager, session_id)
            .await
            .map_err(internal)?
            .inbox
            .into_iter()
            .find(|note| note.id == note_id)
            .ok_or_else(|| invalid(format!("no note {note_id} in this chat's inbox")))
    }

    pub(super) async fn on_notes_showing(
        &self,
        req: NotesShowingRequest,
    ) -> Result<NotesShowingResponse, agent_client_protocol::Error> {
        self.notes_door
            .set_showing(&req.session_id, req.showing)
            .map_err(invalid)?;
        if req.showing {
            chat_notes::hub()
                .offer_due(&self.session_manager, &req.session_id)
                .await;
        }
        Ok(NotesShowingResponse {})
    }

    /// For `session_activity`: every chat with notes waiting for the person there.
    pub(super) async fn notes_waiting(
        &self,
    ) -> Result<Vec<NotesWaitingDto>, agent_client_protocol::Error> {
        Ok(chat_notes::waiting(&self.session_manager)
            .await
            .map_err(internal)?
            .into_iter()
            .map(|(chat, notes)| {
                let oldest = notes.iter().min_by_key(|note| note.sent_at);
                NotesWaitingDto {
                    session_id: chat.session_id,
                    session_name: chat.name,
                    working_dir: chat.working_dir.to_string_lossy().to_string(),
                    count: notes.len() as u32,
                    from_name: oldest
                        .map(|note| note.from.name.clone())
                        .unwrap_or_default(),
                }
            })
            .collect())
    }

    /// `on_prompt`, before the run starts: the note a `crossNote` mark names, when it is this chat's
    /// waiting note and the prompt carries exactly its words. A mark that is not honoured refuses
    /// the prompt — nothing is started, nothing stored.
    pub(super) async fn cross_note_of_prompt(
        &self,
        session_id: &str,
        meta: Option<&serde_json::Map<String, serde_json::Value>>,
        message_text: &str,
    ) -> Result<Option<InboxNote>, agent_client_protocol::Error> {
        let Some(note_id) = cross_note_id(meta) else {
            return Ok(None);
        };
        let note_id = note_id.map_err(invalid)?;
        chat_notes::peek_own_turn(&self.session_manager, session_id, &note_id, message_text)
            .await
            .map(Some)
            .map_err(|error| invalid(format!("this note cannot start a turn here: {error}")))
    }

    /// The note's own turn is starting: taken once. Another window taking it first refuses this one.
    pub(super) async fn take_cross_note(
        &self,
        session_id: &str,
        note: &InboxNote,
    ) -> Result<(), agent_client_protocol::Error> {
        let taken = chat_notes::take_own_turn(&self.session_manager, session_id, &note.id)
            .await
            .map_err(|error| invalid(format!("this note cannot start a turn here: {error}")))?;
        chat_notes::hub().changed(&[session_id.to_string(), taken.from.session_id]);
        Ok(())
    }

    /// `on_prompt`, just before the reply stores the message: the notes the person chose to send
    /// with their next message go first (visible markers, sent to this window as they are stored),
    /// then — for this chat's own sent notes — what became of each, agent-only, told once. A failed
    /// read or write is logged and the person's message still goes.
    pub(super) async fn notes_before_message(
        &self,
        cx: &ConnectionTo<Client>,
        session_id: &SessionId,
        persons_message: bool,
    ) {
        let sid = session_id.0.to_string();
        if persons_message {
            match chat_notes::take_with_message(&self.session_manager, &sid).await {
                Ok(notes) => {
                    let mut senders = vec![sid.clone()];
                    for note in &notes {
                        let message = chat_notes::note_message(note);
                        if let Err(error) = self.session_manager.add_message(&sid, &message).await {
                            tracing::error!(session_id = %sid, note_id = %note.id, %error, "notes: a note sent with the person's message could not be stored; goose will not read it");
                            continue;
                        }
                        let chunk = ContentChunk::new(ContentBlock::Text(TextContent::new(
                            message.as_concat_text(),
                        )))
                        .meta(message_update_meta(
                            message.id.as_deref(),
                            message.created,
                            false,
                        ));
                        if let Err(error) = cx.send_notification(SessionNotification::new(
                            session_id.clone(),
                            SessionUpdate::UserMessageChunk(chunk),
                        )) {
                            warn!(session_id = %sid, ?error, "notes: the note's marker did not reach this window; it is stored in the chat");
                        }
                        senders.push(note.from.session_id.clone());
                    }
                    if !notes.is_empty() {
                        chat_notes::hub().changed(&senders);
                    }
                }
                Err(error) => {
                    tracing::error!(session_id = %sid, %error, "notes: the notes the person added to this message could not be read; they stay in the tray")
                }
            }
        }
        match chat_notes::take_outcomes_to_tell(&self.session_manager, &sid).await {
            Ok(lines) if !lines.is_empty() => {
                let line = Message::user()
                    .with_text(lines.join("\n"))
                    .with_visibility(false, true);
                if let Err(error) = self.session_manager.add_message(&sid, &line).await {
                    tracing::error!(session_id = %sid, %error, "notes: what became of this chat's notes could not be stored for its model");
                }
            }
            Ok(_) => {}
            Err(error) => {
                tracing::error!(session_id = %sid, %error, "notes: what became of this chat's notes could not be read")
            }
        }
    }

    /// The running turn stored a user message: a steered note's, when its id says so — delivered.
    pub(super) async fn note_drained(&self, session_id: &str, message_id: Option<&str>) {
        let Some(note_id) =
            message_id.and_then(|id| id.strip_prefix(chat_notes::MESSAGE_ID_PREFIX))
        else {
            return;
        };
        match chat_notes::mark_steered(&self.session_manager, session_id, note_id).await {
            Ok(note) => chat_notes::hub().changed(&[session_id.to_string(), note.from.session_id]),
            Err(error) => {
                tracing::error!(session_id, note_id, %error, "notes: a steered note reached the turn but could not be marked read")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_cross_note_mark_is_a_note_id_with_its_own_message_id() {
        assert!(cross_note_id(None).is_none());
        let unmarked = json!({"goose": {"loopTick": {}}});
        assert!(cross_note_id(unmarked.as_object()).is_none());
        let good =
            json!({"goose": {"crossNote": {"noteId": "nt_1", "messageId": "crossnote_nt_1"}}});
        assert_eq!(cross_note_id(good.as_object()).unwrap().unwrap(), "nt_1");
        for bad in [
            json!({"goose": {"crossNote": {"noteId": "nt_1", "messageId": "crossnote_nt_2"}}}),
            json!({"goose": {"crossNote": {"noteId": "nt_1"}}}),
            json!({"goose": {"crossNote": "nt_1"}}),
        ] {
            assert!(cross_note_id(bad.as_object()).unwrap().is_err(), "{bad}");
        }
    }

    /// HUMAN DIRECTION IS STRUCTURAL: `chat_notes::send` — the only writer of another chat's inbox —
    /// is called from this file's `notes/send` handler and from nowhere else. No tool, extension,
    /// loop or reviewer reaches it.
    #[test]
    fn only_the_persons_click_calls_send() {
        let needle = concat!("chat_notes::", "send(");
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut callers = Vec::new();
        let mut stack = vec![root.clone()];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|ext| ext == "rs") {
                    let count = std::fs::read_to_string(&path)
                        .unwrap()
                        .matches(needle)
                        .count();
                    if count > 0 {
                        callers.push((
                            path.strip_prefix(&root)
                                .unwrap()
                                .to_string_lossy()
                                .to_string(),
                            count,
                        ));
                    }
                }
            }
        }
        assert_eq!(callers, vec![("acp/server/notes.rs".to_string(), 1)]);
    }
}
