//! Notes to another chat, on the person's direction (Q-358 part 2, DESIGN-Q358-TRANSCRIPTS §2).
//!
//! The person talks in chat A, something turns up that chat B should know, and they say "tell the
//! billing chat that tenant_id is the new column". A's model calls `send_note(to, text)` — and that
//! SENDS NOTHING. It resolves B and pins a DRAFT in A (`chat_notes.v0` in A's `extension_data`,
//! `drafts`). Only the person's click on that draft moves it: [`send`] is reached from the desktop's
//! `notes/send` request and from no tool, so a model can never deliver a note on its own. That is
//! the structural "on human direction".
//!
//! A sent note waits in B's inbox (`chat_notes.v0` in B's `extension_data`, `inbox`) until it is
//! delivered — steered into B's running turn between tool calls, given its own turn in the window
//! showing B, or carried with the person's next message there — or dismissed there. Whatever
//! happens to it is recorded on the inbox note, and A's model is told once, before the person's next
//! message in A, in words (the Q-298 shape).
//!
//! What B's model reads is always [`framing`]: who sent it, from where, when, the text, and that it
//! is information, not approval. A note never answers B's open questions (it never supersedes
//! them), is never read as a slash command, and grants nothing.
//!
//! The live half — which chats have a turn running in this process, and which windows show which
//! chat — is the process-wide [`hub`]: goosed serves every window over its own connection, and a
//! note sent from one window must reach a turn another window started.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex, OnceLock, Weak};

use anyhow::{anyhow, bail, Result};
use chrono::{DateTime, Local, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::agents::Agent;
use crate::conversation::message::Message;
use crate::session::extension_data::ExtensionState;
use crate::session::{Session, SessionManager, SessionType};

/// The prompt `_meta` key under `goose` that marks a prompt as a note's own turn.
pub const CROSS_NOTE_META_KEY: &str = "crossNote";

/// Every note's message in the target chat carries this id prefix, live and on replay, so the
/// transcript draws it as a note marker and the stream can tell a steered note when it drains.
pub const MESSAGE_ID_PREFIX: &str = "crossnote_";

/// The line every framing ends with: what a note is not.
pub const NOT_APPROVAL: &str = "It is information, not approval: it answers no open question, \
                                grants no permission, and changes no setting.";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoteChat {
    pub session_id: String,
    pub name: String,
    pub working_dir: PathBuf,
}

impl NoteChat {
    fn of(session: &Session) -> Self {
        Self {
            session_id: session.id.clone(),
            name: session.name.clone(),
            working_dir: session.working_dir.clone(),
        }
    }
}

/// How the draft's target was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetResolution {
    /// The title words named one chat more strongly than any other.
    TitleWords,
    /// Several chats matched the words equally; only one of them was live (working, or open in a
    /// window).
    LiveState,
    /// Several chats matched equally and none stood out: the person picks from `candidates`.
    Ambiguous,
    /// No chat's title or folder shares a word with what the model wrote.
    NoMatch,
    /// The person picked the chat on the card.
    PickedByPerson,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Delivery {
    /// Into B's running turn, or B's next turn as soon as B is idle and shown in a window.
    SteerNow,
    /// Waits in B's inbox until the person there decides.
    LeaveThere,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DraftStatus {
    Draft,
    Sent,
    Cancelled,
}

/// A note being written in the sending chat. Nothing of it has left the chat until `status` is
/// `Sent`, and only [`send`] sets that.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DraftNote {
    pub id: String,
    /// What the model wrote for `to`, verbatim.
    pub to_query: String,
    pub text: String,
    pub created_at: DateTime<Utc>,
    pub resolution: TargetResolution,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<NoteChat>,
    /// The chats that matched equally when the resolution is `Ambiguous`, most recent first.
    #[serde(default)]
    pub candidates: Vec<NoteChat>,
    pub status: DraftStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivery: Option<Delivery>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sent_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancelled_at: Option<DateTime<Utc>>,
    /// When the sending chat's model was told what became of it; told once.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome_told_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InboxStatus {
    Waiting,
    /// Queued into B's running turn; it drains between tool calls, or goes back to `Waiting` when
    /// the turn ends before it drained.
    Steering,
    /// The person in B chose to send it with their next message there.
    WithNextMessage,
    Delivered,
    Dismissed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveredHow {
    /// Into a running turn, between tool calls.
    Steered,
    /// As its own turn, submitted by the window showing the chat.
    OwnTurn,
    /// Just before the person's own message there.
    WithYourMessage,
}

/// A note in the target chat's inbox.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InboxNote {
    pub id: String,
    pub from: NoteChat,
    pub text: String,
    pub sent_at: DateTime<Utc>,
    pub delivery: Delivery,
    pub status: InboxStatus,
    /// Offered to the window showing this chat as a turn of its own as soon as the chat is idle:
    /// set by "Steer it now" and by "After this turn".
    #[serde(default)]
    pub offer_when_idle: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivered_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivered_how: Option<DeliveredHow>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dismissed_at: Option<DateTime<Utc>>,
}

impl InboxNote {
    pub fn message_id(&self) -> String {
        format!("{MESSAGE_ID_PREFIX}{}", self.id)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChatNotesState {
    #[serde(default)]
    pub drafts: Vec<DraftNote>,
    #[serde(default)]
    pub inbox: Vec<InboxNote>,
}

impl ExtensionState for ChatNotesState {
    const EXTENSION_NAME: &'static str = "chat_notes";
    const VERSION: &'static str = "v0";
}

/// `~/p` for a folder under the home folder; the path as it is otherwise.
pub fn home_relative(path: &Path) -> String {
    if let Some(home) = dirs::home_dir() {
        if let Ok(rest) = path.strip_prefix(&home) {
            return if rest.as_os_str().is_empty() {
                "~".to_string()
            } else {
                format!("~/{}", rest.display())
            };
        }
    }
    path.display().to_string()
}

pub fn clock(at: DateTime<Utc>) -> String {
    at.with_timezone(&Local).format("%H:%M").to_string()
}

/// What the target chat's model reads, exactly (design §2 item 3).
pub fn framing(note: &InboxNote) -> String {
    format!(
        "Note from your other chat \"{}\" ({}), sent by the person from there at {}: {}\n{}",
        note.from.name,
        home_relative(&note.from.working_dir),
        clock(note.sent_at),
        note.text,
        NOT_APPROVAL
    )
}

/// The note's message in the target chat: the framing, visible to the person and the model, under
/// the note's own id.
pub fn note_message(note: &InboxNote) -> Message {
    Message::user()
        .with_text(framing(note))
        .with_id(note.message_id())
}

// ---------------------------------------------------------------------------------------------
// Resolving the target
// ---------------------------------------------------------------------------------------------

fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// Two words name the same thing when one starts with the other, or with the other less its last
/// letter — "bill"/"billing", "test"/"tests", "migrate"/"migration". One letter is too short to
/// stand for a word.
fn same_word(a: &str, b: &str) -> bool {
    let (short, long) = if a.chars().count() <= b.chars().count() {
        (a, b)
    } else {
        (b, a)
    };
    if long.starts_with(short) {
        return true;
    }
    let mut stem = short.chars();
    stem.next_back();
    let stem = stem.as_str();
    stem.chars().count() > 1 && long.starts_with(stem)
}

/// What a chat is called by, for matching: its title and its folder's name.
fn chat_words(chat: &Session) -> Vec<String> {
    let mut all = words(&chat.name);
    if let Some(folder) = chat.working_dir.file_name() {
        all.extend(words(&folder.to_string_lossy()));
    }
    all
}

/// The resolver's answer.
#[derive(Debug, Clone, PartialEq)]
pub struct Resolved {
    pub resolution: TargetResolution,
    pub target: Option<NoteChat>,
    pub candidates: Vec<NoteChat>,
}

fn last_active(chat: &Session) -> DateTime<Utc> {
    chat.last_message_at.unwrap_or(chat.updated_at)
}

/// Title words → recency → live state (design §2 item 1). Every word of `to` counts by how rare it
/// is among the chats, `ln(1 + chats / chats holding it)`: a word in one title of many weighs most,
/// "the" and "chat" little, and a word every title holds adds the same to all of them, so it tells
/// none apart. The chats with the top score are the candidates, most recently active first. One
/// candidate is the target; of several, the one live chat (goose working there, or open in a
/// window) is; otherwise the person picks. A score of nothing is `NoMatch`, never a guess.
pub fn resolve(to: &str, chats: &[Session], live: impl Fn(&str) -> bool) -> Resolved {
    let query: Vec<String> = {
        let mut seen = Vec::new();
        for word in words(to) {
            if !seen.contains(&word) {
                seen.push(word);
            }
        }
        seen
    };
    let per_chat: Vec<Vec<String>> = chats.iter().map(chat_words).collect();
    let total = chats.len() as f64;
    let weights: Vec<f64> = query
        .iter()
        .map(|word| {
            let holders = per_chat
                .iter()
                .filter(|chat| chat.iter().any(|w| same_word(w, word)))
                .count();
            if holders == 0 {
                0.0
            } else {
                (1.0 + total / holders as f64).ln()
            }
        })
        .collect();
    let scores: Vec<f64> = per_chat
        .iter()
        .map(|chat| {
            query
                .iter()
                .zip(&weights)
                .filter(|(word, _)| chat.iter().any(|w| same_word(w, word)))
                .map(|(_, weight)| weight)
                .sum()
        })
        .collect();
    let best = scores.iter().copied().fold(0.0_f64, f64::max);
    if best <= 0.0 {
        return Resolved {
            resolution: TargetResolution::NoMatch,
            target: None,
            candidates: Vec::new(),
        };
    }
    let mut top: Vec<&Session> = chats
        .iter()
        .zip(&scores)
        .filter(|(_, score)| **score >= best)
        .map(|(chat, _)| chat)
        .collect();
    top.sort_by_key(|chat| std::cmp::Reverse(last_active(chat)));
    if top.len() == 1 {
        return Resolved {
            resolution: TargetResolution::TitleWords,
            target: Some(NoteChat::of(top[0])),
            candidates: Vec::new(),
        };
    }
    let live_ones: Vec<&&Session> = top.iter().filter(|chat| live(&chat.id)).collect();
    if live_ones.len() == 1 {
        return Resolved {
            resolution: TargetResolution::LiveState,
            target: Some(NoteChat::of(live_ones[0])),
            candidates: Vec::new(),
        };
    }
    Resolved {
        resolution: TargetResolution::Ambiguous,
        target: None,
        candidates: top.into_iter().map(NoteChat::of).collect(),
    }
}

/// The chats a note may go to from `from`: the person's own chats, never the sender itself.
pub async fn target_chats(session_manager: &SessionManager, from: &str) -> Result<Vec<Session>> {
    let mut chats: Vec<Session> = session_manager
        .list_sessions_by_types(&[SessionType::User])
        .await?
        .into_iter()
        .filter(|chat| chat.id != from && chat.archived_at.is_none())
        .collect();
    chats.sort_by_key(|chat| std::cmp::Reverse(last_active(chat)));
    Ok(chats)
}

// ---------------------------------------------------------------------------------------------
// The draft, in the sending chat
// ---------------------------------------------------------------------------------------------

fn or_empty(value: Option<ChatNotesState>) -> ChatNotesState {
    value.unwrap_or_default()
}

fn find_draft<'a>(state: &'a mut ChatNotesState, note_id: &str) -> Result<&'a mut DraftNote> {
    state
        .drafts
        .iter_mut()
        .find(|draft| draft.id == note_id)
        .ok_or_else(|| anyhow!("no draft note {note_id} in this chat"))
}

fn find_inbox<'a>(state: &'a mut ChatNotesState, note_id: &str) -> Result<&'a mut InboxNote> {
    state
        .inbox
        .iter_mut()
        .find(|note| note.id == note_id)
        .ok_or_else(|| anyhow!("no note {note_id} in this chat's inbox"))
}

/// `send_note`: resolve the target and pin a draft in the sending chat. Writes the SENDING chat
/// only — never any other chat's inbox.
pub async fn draft(
    session_manager: &SessionManager,
    from: &str,
    to: &str,
    text: &str,
) -> Result<DraftNote> {
    let to = to.trim();
    let text = text.trim();
    if to.is_empty() {
        bail!("`to` must name the other chat");
    }
    if text.is_empty() {
        bail!("`text` must not be empty");
    }
    let chats = target_chats(session_manager, from).await?;
    let hub = hub();
    let resolved = resolve(to, &chats, |id| hub.is_running(id) || hub.is_shown(id));
    let note = DraftNote {
        id: format!("nt_{}", Uuid::new_v4().simple()),
        to_query: to.to_string(),
        text: text.to_string(),
        created_at: Utc::now(),
        resolution: resolved.resolution,
        target: resolved.target,
        candidates: resolved.candidates,
        status: DraftStatus::Draft,
        delivery: None,
        sent_at: None,
        cancelled_at: None,
        outcome_told_at: None,
    };
    session_manager
        .update_extension_state::<ChatNotesState, _>(from, |current| {
            let mut state = or_empty(current);
            state.drafts.push(note.clone());
            Ok((state, note))
        })
        .await
}

/// The person picked another chat on the draft card.
pub async fn retarget(
    session_manager: &SessionManager,
    from: &str,
    note_id: &str,
    to_session_id: &str,
) -> Result<DraftNote> {
    if to_session_id == from {
        bail!("a note cannot go to the chat it is written in");
    }
    let chat = session_manager.get_session(to_session_id, false).await?;
    if chat.session_type != SessionType::User {
        bail!("\"{}\" is not one of your chats", chat.name);
    }
    let target = NoteChat::of(&chat);
    session_manager
        .update_extension_state::<ChatNotesState, _>(from, |current| {
            let mut state = or_empty(current);
            let draft = find_draft(&mut state, note_id)?;
            if draft.status != DraftStatus::Draft {
                bail!("note {note_id} is no longer a draft");
            }
            draft.target = Some(target);
            draft.resolution = TargetResolution::PickedByPerson;
            draft.candidates.clear();
            let out = draft.clone();
            Ok((state, out))
        })
        .await
}

pub async fn cancel(
    session_manager: &SessionManager,
    from: &str,
    note_id: &str,
) -> Result<DraftNote> {
    session_manager
        .update_extension_state::<ChatNotesState, _>(from, |current| {
            let mut state = or_empty(current);
            let draft = find_draft(&mut state, note_id)?;
            if draft.status != DraftStatus::Draft {
                bail!("note {note_id} is no longer a draft");
            }
            draft.status = DraftStatus::Cancelled;
            draft.cancelled_at = Some(Utc::now());
            let out = draft.clone();
            Ok((state, out))
        })
        .await
}

/// THE PERSON'S CLICK. The only function that writes another chat's inbox; reached from the
/// desktop's `notes/send` request and from nothing a model can call. The draft is claimed first
/// (a second click finds it sent), the note written to the target's inbox, and the claim undone if
/// that write fails.
pub async fn send(
    session_manager: &SessionManager,
    from: &str,
    note_id: &str,
    text: &str,
    delivery: Delivery,
) -> Result<(DraftNote, InboxNote)> {
    let text = text.trim().to_string();
    if text.is_empty() {
        bail!("a note must not be empty");
    }
    let sender = session_manager.get_session(from, false).await?;
    let now = Utc::now();
    let draft = session_manager
        .update_extension_state::<ChatNotesState, _>(from, |current| {
            let mut state = or_empty(current);
            let draft = find_draft(&mut state, note_id)?;
            if draft.status != DraftStatus::Draft {
                bail!("note {note_id} was already sent or cancelled");
            }
            if draft.target.is_none() {
                bail!("note {note_id} has no chat to go to yet: pick one on the card");
            }
            draft.text = text.clone();
            draft.status = DraftStatus::Sent;
            draft.delivery = Some(delivery);
            draft.sent_at = Some(now);
            let out = draft.clone();
            Ok((state, out))
        })
        .await?;
    let target = draft
        .target
        .clone()
        .expect("a sent draft has a target: checked in the claim");
    let note = InboxNote {
        id: draft.id.clone(),
        from: NoteChat::of(&sender),
        text,
        sent_at: now,
        delivery,
        status: InboxStatus::Waiting,
        offer_when_idle: delivery == Delivery::SteerNow,
        delivered_at: None,
        delivered_how: None,
        dismissed_at: None,
    };
    let written = session_manager
        .update_extension_state::<ChatNotesState, _>(&target.session_id, |current| {
            let mut state = or_empty(current);
            state.inbox.push(note.clone());
            Ok((state, ()))
        })
        .await;
    if let Err(error) = written {
        session_manager
            .update_extension_state::<ChatNotesState, _>(from, |current| {
                let mut state = or_empty(current);
                let draft = find_draft(&mut state, note_id)?;
                draft.status = DraftStatus::Draft;
                draft.delivery = None;
                draft.sent_at = None;
                Ok((state, ()))
            })
            .await?;
        return Err(anyhow!(
            "the note could not be written to \"{}\": {error}",
            target.name
        ));
    }
    Ok((draft, note))
}

// ---------------------------------------------------------------------------------------------
// The inbox, in the target chat
// ---------------------------------------------------------------------------------------------

async fn update_inbox_note(
    session_manager: &SessionManager,
    session_id: &str,
    note_id: &str,
    change: impl FnOnce(&mut InboxNote) -> Result<()>,
) -> Result<InboxNote> {
    session_manager
        .update_extension_state::<ChatNotesState, _>(session_id, |current| {
            let mut state = or_empty(current);
            let note = find_inbox(&mut state, note_id)?;
            change(note)?;
            let out = note.clone();
            Ok((state, out))
        })
        .await
}

fn open(note: &InboxNote) -> Result<()> {
    match note.status {
        InboxStatus::Waiting => Ok(()),
        other => Err(anyhow!("note {} is not waiting (it is {other:?})", note.id)),
    }
}

fn delivered(note: &mut InboxNote, how: DeliveredHow) {
    note.status = InboxStatus::Delivered;
    note.delivered_how = Some(how);
    note.delivered_at = Some(Utc::now());
    note.offer_when_idle = false;
}

pub async fn read_state(
    session_manager: &SessionManager,
    session_id: &str,
) -> Result<ChatNotesState> {
    let session = session_manager.get_session(session_id, false).await?;
    match session
        .extension_data
        .get_extension_state(ChatNotesState::EXTENSION_NAME, ChatNotesState::VERSION)
    {
        None => Ok(ChatNotesState::default()),
        Some(value) => ChatNotesState::from_value(value),
    }
}

/// The note a prompt's `_meta.goose.crossNote` names, when it may be delivered as its own turn:
/// waiting in this chat, and the prompt carries exactly its framing. Reads, writes nothing.
pub async fn peek_own_turn(
    session_manager: &SessionManager,
    session_id: &str,
    note_id: &str,
    message_text: &str,
) -> Result<InboxNote> {
    let mut state = read_state(session_manager, session_id).await?;
    let note = find_inbox(&mut state, note_id)?;
    open(note)?;
    if message_text != framing(note) {
        bail!("the prompt does not carry note {note_id}'s words");
    }
    Ok(note.clone())
}

/// The note's own turn starts: delivered, once.
pub async fn take_own_turn(
    session_manager: &SessionManager,
    session_id: &str,
    note_id: &str,
) -> Result<InboxNote> {
    update_inbox_note(session_manager, session_id, note_id, |note| {
        open(note)?;
        delivered(note, DeliveredHow::OwnTurn);
        Ok(())
    })
    .await
}

/// A steered note drained into the running turn (its message reached the conversation).
pub async fn mark_steered(
    session_manager: &SessionManager,
    session_id: &str,
    note_id: &str,
) -> Result<InboxNote> {
    update_inbox_note(session_manager, session_id, note_id, |note| {
        if note.status != InboxStatus::Steering {
            bail!("note {} drained but was {:?}", note.id, note.status);
        }
        delivered(note, DeliveredHow::Steered);
        Ok(())
    })
    .await
}

/// The turn a note was steered into ended before it drained: it waits again and is offered as a
/// turn of its own. Returns the ids put back.
pub async fn requeue_undrained(
    session_manager: &SessionManager,
    session_id: &str,
) -> Result<Vec<String>> {
    session_manager
        .update_extension_state::<ChatNotesState, _>(session_id, |current| {
            let mut state = or_empty(current);
            let mut back = Vec::new();
            for note in state
                .inbox
                .iter_mut()
                .filter(|note| note.status == InboxStatus::Steering)
            {
                note.status = InboxStatus::Waiting;
                note.offer_when_idle = true;
                back.push(note.id.clone());
            }
            Ok((state, back))
        })
        .await
}

/// The person's plain message in this chat carries every note they chose to add to it; each is
/// delivered with it, oldest first.
pub async fn take_with_message(
    session_manager: &SessionManager,
    session_id: &str,
) -> Result<Vec<InboxNote>> {
    session_manager
        .update_extension_state::<ChatNotesState, _>(session_id, |current| {
            let mut state = or_empty(current);
            let mut taken = Vec::new();
            for note in state
                .inbox
                .iter_mut()
                .filter(|note| note.status == InboxStatus::WithNextMessage)
            {
                delivered(note, DeliveredHow::WithYourMessage);
                taken.push(note.clone());
            }
            taken.sort_by_key(|note| note.sent_at);
            Ok((state, taken))
        })
        .await
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InboxAction {
    /// Busy: as a turn of its own once this turn ends.
    AfterThisTurn,
    /// Idle: with the person's next message.
    AddToNextMessage,
    Dismiss,
}

pub async fn act(
    session_manager: &SessionManager,
    session_id: &str,
    note_id: &str,
    action: InboxAction,
) -> Result<InboxNote> {
    update_inbox_note(session_manager, session_id, note_id, |note| {
        match action {
            InboxAction::Dismiss => {
                if matches!(note.status, InboxStatus::Delivered | InboxStatus::Dismissed) {
                    bail!("note {} was already {:?}", note.id, note.status);
                }
                note.status = InboxStatus::Dismissed;
                note.dismissed_at = Some(Utc::now());
                note.offer_when_idle = false;
            }
            InboxAction::AfterThisTurn => {
                open(note)?;
                note.offer_when_idle = true;
            }
            InboxAction::AddToNextMessage => {
                open(note)?;
                note.status = InboxStatus::WithNextMessage;
                note.offer_when_idle = false;
            }
        }
        Ok(())
    })
    .await
}

/// Only [`Hub::steer`] calls this, under the runs lock, right before it queues the message.
async fn begin_steer(
    session_manager: &SessionManager,
    session_id: &str,
    note_id: &str,
) -> Result<InboxNote> {
    update_inbox_note(session_manager, session_id, note_id, |note| {
        open(note)?;
        note.status = InboxStatus::Steering;
        Ok(())
    })
    .await
}

/// The oldest note to offer as a turn of its own, when one waits for it.
pub async fn due(session_manager: &SessionManager, session_id: &str) -> Result<Option<InboxNote>> {
    let state = read_state(session_manager, session_id).await?;
    Ok(state
        .inbox
        .into_iter()
        .filter(|note| note.status == InboxStatus::Waiting && note.offer_when_idle)
        .min_by_key(|note| note.sent_at))
}

// ---------------------------------------------------------------------------------------------
// Telling the sender
// ---------------------------------------------------------------------------------------------

/// What became of a sent draft, read from the target's inbox.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Waiting(InboxNote),
    Delivered(InboxNote),
    Dismissed(InboxNote),
    /// The target chat, or the note in it, is gone (the chat was deleted).
    Gone(String),
}

pub async fn outcome(session_manager: &SessionManager, draft: &DraftNote) -> Outcome {
    let Some(target) = &draft.target else {
        return Outcome::Gone("the note has no target chat".to_string());
    };
    match read_state(session_manager, &target.session_id).await {
        Err(error) => Outcome::Gone(error.to_string()),
        Ok(state) => match state.inbox.into_iter().find(|note| note.id == draft.id) {
            None => Outcome::Gone(format!("the note is not in \"{}\"", target.name)),
            Some(note) => match note.status {
                InboxStatus::Delivered => Outcome::Delivered(note),
                InboxStatus::Dismissed => Outcome::Dismissed(note),
                _ => Outcome::Waiting(note),
            },
        },
    }
}

fn outcome_line(draft: &DraftNote, outcome: &Outcome) -> Option<String> {
    let name = draft
        .target
        .as_ref()
        .map(|target| target.name.as_str())
        .unwrap_or_default();
    let quoted = format!("\"{}\"", draft.text);
    match outcome {
        Outcome::Waiting(_) => None,
        Outcome::Delivered(note) => {
            let at = note.delivered_at.map(clock).unwrap_or_default();
            let how = match note.delivered_how {
                Some(DeliveredHow::Steered) => {
                    "it reached goose there during a running turn, between tool calls"
                }
                Some(DeliveredHow::OwnTurn) => "it started a turn there",
                Some(DeliveredHow::WithYourMessage) => {
                    "it went with the person's next message there"
                }
                None => "how is not recorded",
            };
            Some(format!(
                "Your note to the other chat \"{name}\" was read there in its turn at {at} ({how}). \
                 The note: {quoted}"
            ))
        }
        Outcome::Dismissed(note) => {
            let at = note.dismissed_at.map(clock).unwrap_or_default();
            Some(format!(
                "Your note to the other chat \"{name}\" was dismissed there at {at}; goose there \
                 never read it. The note: {quoted}"
            ))
        }
        Outcome::Gone(reason) => Some(format!(
            "Your note to the other chat \"{name}\" can no longer be followed: {reason}. The \
             note: {quoted}"
        )),
    }
}

/// The lines the sending chat's model reads before the person's next message there: each sent
/// note's outcome (read, dismissed, gone) and each draft the person cancelled, told once.
pub async fn take_outcomes_to_tell(
    session_manager: &SessionManager,
    from: &str,
) -> Result<Vec<String>> {
    let state = read_state(session_manager, from).await?;
    let mut lines: Vec<(String, String)> = Vec::new();
    for draft in state
        .drafts
        .iter()
        .filter(|draft| draft.outcome_told_at.is_none())
    {
        match draft.status {
            DraftStatus::Draft => {}
            DraftStatus::Cancelled => lines.push((
                draft.id.clone(),
                format!(
                    "The person cancelled your draft note (to \"{}\"); nothing was sent. The \
                     draft: \"{}\"",
                    draft.to_query, draft.text
                ),
            )),
            DraftStatus::Sent => {
                let outcome = outcome(session_manager, draft).await;
                if let Some(line) = outcome_line(draft, &outcome) {
                    lines.push((draft.id.clone(), line));
                }
            }
        }
    }
    if lines.is_empty() {
        return Ok(Vec::new());
    }
    let told: HashSet<String> = lines.iter().map(|(id, _)| id.clone()).collect();
    session_manager
        .update_extension_state::<ChatNotesState, _>(from, |current| {
            let mut state = or_empty(current);
            let now = Utc::now();
            for draft in state
                .drafts
                .iter_mut()
                .filter(|draft| told.contains(&draft.id) && draft.outcome_told_at.is_none())
            {
                draft.outcome_told_at = Some(now);
            }
            Ok((state, ()))
        })
        .await?;
    Ok(lines.into_iter().map(|(_, line)| line).collect())
}

/// Every chat with a note waiting for the person, for the lists: the chat and its waiting notes.
pub async fn waiting(session_manager: &SessionManager) -> Result<Vec<(NoteChat, Vec<InboxNote>)>> {
    Ok(session_manager
        .sessions_with_extension_state::<ChatNotesState>()
        .await?
        .into_iter()
        .filter_map(|row| {
            let notes: Vec<InboxNote> = row
                .state
                .inbox
                .into_iter()
                .filter(|note| {
                    matches!(
                        note.status,
                        InboxStatus::Waiting | InboxStatus::Steering | InboxStatus::WithNextMessage
                    )
                })
                .collect();
            if notes.is_empty() {
                return None;
            }
            Some((
                NoteChat {
                    session_id: row.session_id,
                    name: row.name,
                    working_dir: row.working_dir,
                },
                notes,
            ))
        })
        .collect())
}

// ---------------------------------------------------------------------------------------------
// The hub: live turns and the windows that show each chat, across every connection
// ---------------------------------------------------------------------------------------------

/// A note due as its own turn, offered to the windows showing its chat.
#[derive(Debug, Clone, PartialEq)]
pub struct NoteDue {
    pub session_id: String,
    pub note_id: String,
    pub message_id: String,
    pub prompt: String,
}

/// One window's connection, as the hub reaches it.
pub trait NoteDoor: Send + Sync {
    fn deliver_due(&self, due: &NoteDue);
    fn changed(&self, session_ids: &[String]);
}

/// Tells the window running a turn that a message was queued into it (its `queuedSteer` update).
pub type SteerAnnounce = Arc<dyn Fn(&str, &str) + Send + Sync>;

struct LiveRun {
    run_id: String,
    agent: Weak<Agent>,
    announce: SteerAnnounce,
}

struct Door {
    door: Arc<dyn NoteDoor>,
    showing: HashSet<String>,
}

#[derive(Default)]
pub struct Hub {
    next_door: AtomicU64,
    doors: StdMutex<HashMap<u64, Door>>,
    runs: tokio::sync::Mutex<HashMap<String, LiveRun>>,
    /// The same set as `runs`, readable without awaiting (resolution runs inside a sync closure).
    running: StdMutex<HashSet<String>>,
}

pub fn hub() -> &'static Hub {
    static HUB: OnceLock<Hub> = OnceLock::new();
    HUB.get_or_init(Hub::default)
}

/// A window's door, closed when this is dropped.
pub struct DoorGuard {
    id: u64,
}

impl Drop for DoorGuard {
    fn drop(&mut self) {
        hub()
            .doors
            .lock()
            .expect("notes hub doors poisoned")
            .remove(&self.id);
    }
}

/// What steering a note into a chat did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Steered {
    /// Queued into the running turn `run_id`.
    Queued { run_id: String },
    /// No turn runs there: nothing was queued.
    Idle,
}

impl Hub {
    pub fn open_door(&self, door: Arc<dyn NoteDoor>) -> DoorGuard {
        let id = self.next_door.fetch_add(1, Ordering::Relaxed);
        self.doors.lock().expect("notes hub doors poisoned").insert(
            id,
            Door {
                door,
                showing: HashSet::new(),
            },
        );
        DoorGuard { id }
    }

    /// The window behind `guard` shows (or stopped showing) `session_id`.
    pub fn set_showing(&self, guard: &DoorGuard, session_id: &str, showing: bool) {
        let mut doors = self.doors.lock().expect("notes hub doors poisoned");
        if let Some(door) = doors.get_mut(&guard.id) {
            if showing {
                door.showing.insert(session_id.to_string());
            } else {
                door.showing.remove(session_id);
            }
        }
    }

    pub fn is_shown(&self, session_id: &str) -> bool {
        self.doors
            .lock()
            .expect("notes hub doors poisoned")
            .values()
            .any(|door| door.showing.contains(session_id))
    }

    pub fn is_running(&self, session_id: &str) -> bool {
        self.running
            .lock()
            .expect("notes hub runs poisoned")
            .contains(session_id)
    }

    /// Tell every window that these chats' notes changed.
    pub fn changed(&self, session_ids: &[String]) {
        let doors: Vec<Arc<dyn NoteDoor>> = self
            .doors
            .lock()
            .expect("notes hub doors poisoned")
            .values()
            .map(|door| door.door.clone())
            .collect();
        for door in doors {
            door.changed(session_ids);
        }
    }

    pub async fn run_started(
        &self,
        session_id: &str,
        run_id: &str,
        agent: &Arc<Agent>,
        announce: SteerAnnounce,
    ) {
        let mut runs = self.runs.lock().await;
        runs.insert(
            session_id.to_string(),
            LiveRun {
                run_id: run_id.to_string(),
                agent: Arc::downgrade(agent),
                announce,
            },
        );
        self.running
            .lock()
            .expect("notes hub runs poisoned")
            .insert(session_id.to_string());
    }

    /// The run ended (however): any note steered into it that never drained waits again, and the
    /// chat — idle now — is offered its next due note.
    pub async fn run_ended(
        &self,
        session_manager: &SessionManager,
        session_id: &str,
        run_id: &str,
    ) {
        {
            let mut runs = self.runs.lock().await;
            if runs.get(session_id).map(|run| run.run_id.as_str()) != Some(run_id) {
                return;
            }
            let run = runs.remove(session_id).expect("checked above");
            // Whatever is still queued for the ended turn never reaches it: dropped here, under the
            // lock `steer` queues under, so a note steered in its last moment cannot also drain into
            // a later turn after it was put back to wait.
            if let Some(agent) = run.agent.upgrade() {
                agent.discard_pending_steers(session_id).await;
            }
            self.running
                .lock()
                .expect("notes hub runs poisoned")
                .remove(session_id);
            match requeue_undrained(session_manager, session_id).await {
                Ok(back) if !back.is_empty() => self.changed(&[session_id.to_string()]),
                Ok(_) => {}
                Err(error) => tracing::error!(
                    session_id,
                    %error,
                    "notes: a note steered into the turn that just ended could not be put back to wait; it reads as steering"
                ),
            }
        }
        self.offer_due(session_manager, session_id).await;
    }

    /// Queue the note into the chat's running turn, when one runs: the state is written first,
    /// under the runs lock, so the turn cannot end between the check and the queue.
    pub async fn steer(
        &self,
        session_manager: &SessionManager,
        session_id: &str,
        note_id: &str,
    ) -> Result<Steered> {
        let runs = self.runs.lock().await;
        let Some(run) = runs.get(session_id) else {
            return Ok(Steered::Idle);
        };
        let Some(agent) = run.agent.upgrade() else {
            return Ok(Steered::Idle);
        };
        let note = begin_steer(session_manager, session_id, note_id).await?;
        let message = note_message(&note);
        agent.steer(session_id, message).await;
        (run.announce)(&note.message_id(), &run.run_id);
        Ok(Steered::Queued {
            run_id: run.run_id.clone(),
        })
    }

    /// Offer the chat's oldest due note to every window showing it, when no turn runs there. No
    /// window shows it: nothing is sent, and it is offered when a window shows it.
    pub async fn offer_due(&self, session_manager: &SessionManager, session_id: &str) {
        if self.is_running(session_id) {
            return;
        }
        let note = match due(session_manager, session_id).await {
            Ok(Some(note)) => note,
            Ok(None) => return,
            Err(error) => {
                tracing::error!(session_id, %error, "notes: the chat's inbox could not be read to offer its due note");
                return;
            }
        };
        let due = NoteDue {
            session_id: session_id.to_string(),
            note_id: note.id.clone(),
            message_id: note.message_id(),
            prompt: framing(&note),
        };
        let doors: Vec<Arc<dyn NoteDoor>> = self
            .doors
            .lock()
            .expect("notes hub doors poisoned")
            .values()
            .filter(|door| door.showing.contains(session_id))
            .map(|door| door.door.clone())
            .collect();
        for door in doors {
            door.deliver_due(&due);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chat(id: &str, name: &str, dir: &str, minutes_ago: i64) -> Session {
        let at = Utc::now() - chrono::Duration::minutes(minutes_ago);
        Session {
            id: id.into(),
            working_dir: PathBuf::from(dir),
            name: name.into(),
            user_set_name: false,
            session_type: SessionType::User,
            created_at: at,
            updated_at: at,
            extension_data: Default::default(),
            usage: Default::default(),
            accumulated_usage: Default::default(),
            accumulated_cost: None,
            schedule_id: None,
            recipe: None,
            user_recipe_values: None,
            conversation: None,
            message_count: 0,
            last_message_at: Some(at),
            provider_name: None,
            model_config: None,
            goose_mode: Default::default(),
            archived_at: None,
            project_id: None,
            last_message_snippet: None,
        }
    }

    fn chats() -> Vec<Session> {
        vec![
            chat("a", "Explore split mesh", "/p/mesh", 1),
            chat("b", "Migrate billing", "/p/billing", 30),
            chat("c", "Fix the build", "/p/ci", 5),
            chat("d", "Write the release notes", "/p/docs", 50),
        ]
    }

    #[test]
    fn a_rare_title_word_names_the_chat_and_common_words_weigh_little() {
        let resolved = resolve("the migration chat", &chats(), |_| false);
        assert_eq!(resolved.resolution, TargetResolution::TitleWords);
        assert_eq!(resolved.target.unwrap().session_id, "b");

        let resolved = resolve("billing", &chats(), |_| false);
        assert_eq!(resolved.target.unwrap().session_id, "b");
    }

    #[test]
    fn the_folder_name_counts_as_the_chats_words() {
        let resolved = resolve("the docs one", &chats(), |_| false);
        assert_eq!(resolved.target.unwrap().session_id, "d");
    }

    #[test]
    fn no_shared_word_is_no_match_never_a_guess() {
        let resolved = resolve("payments", &chats(), |_| false);
        assert_eq!(resolved.resolution, TargetResolution::NoMatch);
        assert!(resolved.target.is_none());
        assert!(resolved.candidates.is_empty());
    }

    #[test]
    fn equal_matches_are_singled_out_by_live_state_or_listed_most_recent_first() {
        let mut all = chats();
        all.push(chat("e", "Migrate billing", "/p/billing2", 10));
        let resolved = resolve("billing", &all, |_| false);
        assert_eq!(resolved.resolution, TargetResolution::Ambiguous);
        let ids: Vec<_> = resolved
            .candidates
            .iter()
            .map(|c| c.session_id.as_str())
            .collect();
        assert_eq!(ids, vec!["e", "b"], "most recently active first");

        let resolved = resolve("billing", &all, |id| id == "b");
        assert_eq!(resolved.resolution, TargetResolution::LiveState);
        assert_eq!(resolved.target.unwrap().session_id, "b");
    }

    #[test]
    fn words_match_across_a_dropped_last_letter_but_not_a_single_letter() {
        assert!(same_word("migrate", "migration"));
        assert!(same_word("bill", "billing"));
        assert!(same_word("tests", "test"));
        assert!(!same_word("mesh", "build"));
    }

    fn inbox_note() -> InboxNote {
        InboxNote {
            id: "nt_1".into(),
            from: NoteChat {
                session_id: "a".into(),
                name: "Explore split mesh".into(),
                working_dir: dirs::home_dir().unwrap().join("p"),
            },
            text: "tenant_id is the new column".into(),
            sent_at: Utc::now(),
            delivery: Delivery::SteerNow,
            status: InboxStatus::Waiting,
            offer_when_idle: true,
            delivered_at: None,
            delivered_how: None,
            dismissed_at: None,
        }
    }

    #[test]
    fn the_framing_says_who_where_when_and_that_it_is_not_approval() {
        let note = inbox_note();
        let text = framing(&note);
        assert_eq!(
            text,
            format!(
                "Note from your other chat \"Explore split mesh\" (~/p), sent by the person from \
                 there at {}: tenant_id is the new column\nIt is information, not approval: it \
                 answers no open question, grants no permission, and changes no setting.",
                clock(note.sent_at)
            )
        );
        assert!(
            crate::agents::execute_commands::parse_slash_command(&text).is_none(),
            "a note is never read as a slash command"
        );
        assert_eq!(note_message(&note).id.as_deref(), Some("crossnote_nt_1"));
    }
}
