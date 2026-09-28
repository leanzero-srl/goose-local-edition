//! "Needs you": questions the model put to the person with `ask_user`, pinned until answered or
//! dismissed. Durable in the session; listed across every session so the app can count them and
//! jump to the one that is waiting.

use agent_client_protocol::{JsonRpcRequest, JsonRpcResponse};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum NeedsYouStatus {
    Open,
    Answered,
    Dismissed,
    // The person sent a chat message while the question was open instead of answering it (Q-298).
    Superseded,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NeedsYouItemDto {
    pub id: String,
    pub session_id: String,
    pub session_name: String,
    pub working_dir: String,
    pub question: String,
    pub why: String,
    pub recommended_answer: String,
    pub options: Vec<String>,
    /// RFC 3339.
    pub created_at: String,
    pub status: NeedsYouStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<String>,
    /// The message that superseded the question, verbatim; set exactly when `status` is
    /// `superseded`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub superseded_by: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RunningSessionDto {
    pub session_id: String,
    pub session_name: String,
    pub working_dir: String,
    /// RFC 3339: when the turn in flight began.
    pub started_at: String,
}

/// What the person's sessions are doing right now — the one source every session list, the pinned
/// card and the top bar read. `running`: the sessions holding an in-flight turn in the engine's busy
/// set (the token map LeanZero Link reads too), user-visible sessions only. `needsYou`: every open
/// item, oldest first.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/session_activity/get",
    response = SessionActivityResponse
)]
#[serde(rename_all = "camelCase")]
pub struct SessionActivityRequest {}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FailedSessionDto {
    pub session_id: String,
    pub session_name: String,
    pub working_dir: String,
    /// RFC 3339: when the failed turn ended.
    pub failed_at: String,
    /// The failure text the chat showed, when it had one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// A session whose LAST turn the person stopped (Q-169).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StoppedSessionDto {
    pub session_id: String,
    pub session_name: String,
    pub working_dir: String,
    /// RFC 3339: when the person stopped the turn.
    pub stopped_at: String,
    /// How long the turn had run.
    pub elapsed_ms: u64,
    /// The output tokens the model had written; absent when goose could not count them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct SessionActivityResponse {
    pub running: Vec<RunningSessionDto>,
    pub needs_you: Vec<NeedsYouItemDto>,
    /// User/scheduled sessions whose LAST turn failed (a later completed turn clears it).
    pub failed: Vec<FailedSessionDto>,
    /// User/scheduled sessions whose LAST turn the person stopped (a later completed turn clears it).
    #[serde(default)]
    pub stopped: Vec<StoppedSessionDto>,
    /// User/scheduled sessions goose is doing background work for, oldest call first (Q-185).
    #[serde(default)]
    pub background: Vec<BackgroundSessionDto>,
    /// Sessions with a loop that has not ended (session loops, Q-228), each with its status as
    /// read now; an unreadable loop record is listed with its error.
    #[serde(default)]
    pub looping: Vec<super::LoopSummaryDto>,
}

/// What goose asks the model FOR a session besides the answer being written (Q-185): the call's
/// kind, set once where the call is made (`goose::background_work`) and read by every surface that
/// names it — the session lists, the chat, the MLX engine card and the cut guard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum BackgroundWorkKind {
    /// The end-of-turn fact checker ("goose check:"), after the reply.
    FactCheck,
    /// The end-of-turn reviewer that proposes memories, after the reply.
    MemoryReview,
    /// The session's title.
    Title,
    /// The short labels on a turn's tool calls.
    ToolLabel,
    /// Compaction of the conversation.
    Compaction,
    /// A tool call and its result summarized to save tokens.
    ToolDigest,
    /// The permission judge deciding whether a tool call only reads.
    PermissionCheck,
    /// The adversary inspector reviewing a tool call.
    SafetyCheck,
    /// The orchestrator summarizing a session's conversation.
    SessionSummary,
    /// A recipe written from the session.
    Recipe,
}

/// A session goose is doing background work for right now (Q-185): a model call in flight on the
/// session's behalf that is not its turn. Listed whether or not a turn also runs; a surface shows it
/// as the session's state only while no turn does.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundSessionDto {
    pub session_id: String,
    pub session_name: String,
    pub working_dir: String,
    pub kind: BackgroundWorkKind,
    /// RFC 3339: when the call began.
    pub started_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum NeedsYouAction {
    Answer,
    Dismiss,
}

/// Close an open item. `Answer` records the person's text (required); `Dismiss` records nothing.
/// The answer itself reaches the model as the person's next chat message, sent by the client.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/needs_you/resolve", response = ResolveNeedsYouResponse)]
#[serde(rename_all = "camelCase")]
pub struct ResolveNeedsYouRequest {
    pub session_id: String,
    pub item_id: String,
    pub action: NeedsYouAction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<String>,
}

impl Default for ResolveNeedsYouRequest {
    fn default() -> Self {
        Self {
            session_id: String::new(),
            item_id: String::new(),
            action: NeedsYouAction::Dismiss,
            answer: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct ResolveNeedsYouResponse {
    pub item: NeedsYouItemDto,
}
