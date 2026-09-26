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

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct SessionActivityResponse {
    pub running: Vec<RunningSessionDto>,
    pub needs_you: Vec<NeedsYouItemDto>,
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
