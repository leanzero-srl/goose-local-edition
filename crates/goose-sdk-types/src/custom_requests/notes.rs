//! Notes to another chat, on the person's direction (Q-358 part 2): the draft a model pins in the
//! sending chat with `send_note`, the person's click that sends it (`notes/send` — the only way a
//! note leaves a chat), and the target chat's inbox, where the person there decides what goose
//! does with it.

use agent_client_protocol::{JsonRpcRequest, JsonRpcResponse};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Where a chat stands right now, as the draft card says it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NoteLiveState {
    /// goose is working there (a turn runs).
    Working,
    /// Open in a window, no turn running.
    #[default]
    Idle,
    /// Not open in any window.
    NotOpen,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NoteChatDto {
    pub session_id: String,
    pub name: String,
    pub working_dir: String,
    /// The folder as the card shows it: `~/billing` under the home folder.
    pub folder: String,
    pub live: NoteLiveState,
    /// RFC 3339: the chat's last message.
    pub last_active_at: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NoteResolution {
    /// The title words named one chat.
    #[default]
    TitleWords,
    /// Several matched equally; only one was live.
    LiveState,
    /// Several matched equally: `candidates` lists them for the person to pick.
    Ambiguous,
    /// No chat's title or folder matched: the person picks one or cancels.
    NoMatch,
    PickedByPerson,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NoteDelivery {
    /// Into the chat's running turn, or its own turn as soon as it is idle in a window.
    #[default]
    SteerNow,
    /// Waits in the chat's inbox for the person there.
    LeaveThere,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NoteDraftStatus {
    #[default]
    Draft,
    Sent,
    Cancelled,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum InboxNoteStatus {
    #[default]
    Waiting,
    /// Queued into the running turn; goose reads it between tool calls.
    Steering,
    /// Goes with the person's next message there.
    WithNextMessage,
    Delivered,
    Dismissed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NoteDeliveredHow {
    Steered,
    OwnTurn,
    WithYourMessage,
}

/// What became of a sent note, read from the target chat.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NoteOutcomeState {
    #[default]
    Waiting,
    Steering,
    WithNextMessage,
    Delivered,
    Dismissed,
    /// The target chat, or the note in it, is gone: `reason` says why.
    Gone,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NoteOutcomeDto {
    pub state: NoteOutcomeState,
    /// RFC 3339: when it was delivered or dismissed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub how: Option<NoteDeliveredHow>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// A note in the chat that wrote it.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NoteDraftDto {
    pub id: String,
    /// The words the model used for the other chat.
    pub to_query: String,
    pub text: String,
    /// RFC 3339.
    pub created_at: String,
    pub resolution: NoteResolution,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<NoteChatDto>,
    /// The equally matching chats when `resolution` is `ambiguous`, most recent first.
    #[serde(default)]
    pub candidates: Vec<NoteChatDto>,
    pub status: NoteDraftStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivery: Option<NoteDelivery>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sent_at: Option<String>,
    /// Set for a sent note.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<NoteOutcomeDto>,
}

/// A note in the chat it was sent to.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct InboxNoteDto {
    pub id: String,
    pub from_session_id: String,
    pub from_name: String,
    pub from_working_dir: String,
    /// The sending chat's folder as the tray shows it: `~/p` under the home folder.
    pub from_folder: String,
    pub text: String,
    /// RFC 3339.
    pub sent_at: String,
    pub delivery: NoteDelivery,
    pub status: InboxNoteStatus,
    /// Offered as a turn of its own as soon as the chat is idle in a window.
    pub offer_when_idle: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivered_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivered_how: Option<NoteDeliveredHow>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dismissed_at: Option<String>,
    /// The id its message takes in this chat.
    pub message_id: String,
    /// Exactly what goose reads: submit it with `_meta.goose.crossNote = {noteId, messageId}`.
    pub prompt: String,
}

/// A chat's notes: the drafts written in it and the notes sent to it.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/notes/list", response = NotesListResponse)]
#[serde(rename_all = "camelCase")]
pub struct NotesListRequest {
    pub session_id: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct NotesListResponse {
    pub drafts: Vec<NoteDraftDto>,
    pub inbox: Vec<InboxNoteDto>,
}

/// The chats a note written in `sessionId` could go to, most recently active first.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/notes/targets", response = NotesTargetsResponse)]
#[serde(rename_all = "camelCase")]
pub struct NotesTargetsRequest {
    pub session_id: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct NotesTargetsResponse {
    pub chats: Vec<NoteChatDto>,
}

/// THE PERSON'S CLICK on a draft: the only request that sends a note. `text` is the draft as the
/// person left it.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/notes/send", response = NotesSendResponse)]
#[serde(rename_all = "camelCase")]
pub struct NotesSendRequest {
    pub session_id: String,
    pub note_id: String,
    pub text: String,
    pub delivery: NoteDelivery,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct NotesSendResponse {
    pub draft: NoteDraftDto,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NoteDraftAction {
    /// "Not this chat": the person picked another.
    #[serde(rename_all = "camelCase")]
    Retarget { to_session_id: String },
    #[default]
    Cancel,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/notes/draft", response = NotesDraftResponse)]
#[serde(rename_all = "camelCase")]
pub struct NotesDraftRequest {
    pub session_id: String,
    pub note_id: String,
    pub action: NoteDraftAction,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct NotesDraftResponse {
    pub draft: NoteDraftDto,
}

/// What the person in the target chat does with a note. "Give it to goose now" is not one: the
/// window submits the note's `prompt` with `_meta.goose.crossNote`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NoteInboxAction {
    /// goose is working: into this turn, between tool calls.
    SteerThisTurn,
    /// goose is working: its own turn once this one ends.
    AfterThisTurn,
    /// goose is idle: with the person's next message.
    AddToNextMessage,
    #[default]
    Dismiss,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/notes/inbox", response = NotesInboxResponse)]
#[serde(rename_all = "camelCase")]
pub struct NotesInboxRequest {
    pub session_id: String,
    pub note_id: String,
    pub action: NoteInboxAction,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct NotesInboxResponse {
    pub note: InboxNoteDto,
}

/// This window shows (or stopped showing) `sessionId`. goosed offers a chat's due note only to the
/// windows that show it, and a draft card says "not open in any window" from these.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/notes/showing", response = NotesShowingResponse)]
#[serde(rename_all = "camelCase")]
pub struct NotesShowingRequest {
    pub session_id: String,
    pub showing: bool,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct NotesShowingResponse {}

/// A chat with notes waiting for the person: the sidebar's Note chip and "1 note waiting".
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NotesWaitingDto {
    pub session_id: String,
    pub session_name: String,
    pub working_dir: String,
    pub count: u32,
    /// The chat the oldest waiting note came from.
    pub from_name: String,
}
