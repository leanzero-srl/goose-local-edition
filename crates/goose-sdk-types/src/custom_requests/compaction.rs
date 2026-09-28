//! Q-357: a chat's compaction, seen and steered from the desktop — what the next compaction would
//! keep word for word, what the model would write, the person's note and pins, and the last one.

use crate::custom_notifications::{CompactionNoteVerdict, CompactionTriggerKind};
use agent_client_protocol::{JsonRpcRequest, JsonRpcResponse};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// What the person set for this chat's compactions (`compaction.v0` in the session).
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CompactionSteerDto {
    /// The note for the next compaction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// The note is used for every compaction of this chat, not only the next.
    #[serde(default)]
    pub standing: bool,
    /// Lines kept word for word in every compaction of this chat.
    #[serde(default)]
    pub pins: Vec<String>,
    /// The next manual compaction follows the note as written instead of asking about it (the
    /// person answered "Compact as written").
    #[serde(default)]
    pub follow_as_written: bool,
}

/// The chat's latest compaction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LastCompactionDto {
    /// RFC 3339.
    pub at: String,
    pub trigger: CompactionTriggerKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens_before: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens_after: Option<u64>,
    pub elapsed_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note_verdict: Option<CompactionNoteVerdict>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub said: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum KeptPillarId {
    /// P1 — every message the person wrote.
    Asked,
    /// P2 — the files the chat's write/edit calls changed.
    Files,
    /// P3 — the tool calls that failed.
    Failed,
    /// P4 — the note and the pins.
    Notes,
    /// P5 — the ledger.
    Ledger,
}

/// One part goose keeps word for word, as the next compaction would keep it now.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct KeptPillarDto {
    pub id: KeptPillarId,
    /// Each item as the block carries it (a message, a file line, a failed call, an entry).
    pub items: Vec<String>,
    /// Older items the block's share of the window left out.
    #[serde(default)]
    pub left_out: u64,
    /// Items cut to their start to fit.
    #[serde(default)]
    pub cut: u64,
    /// The part's size by goose's tokenizer; absent when it could not be built.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens: Option<u64>,
    /// The part could not be read (the ledger), said instead of an empty list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// A section the model writes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WrittenPartDto {
    pub heading: String,
    pub ask: String,
}

/// What rides every turn already, beside the conversation.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AlwaysHereDto {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scratchpad: Option<String>,
    /// The ledger's newest entries, newest first.
    #[serde(default)]
    pub ledger_tail: Vec<String>,
}

/// What the next compaction of this chat would keep, computed by code alone — no model call.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/session/compaction/preview",
    response = CompactionPreviewResponse
)]
#[serde(rename_all = "camelCase")]
pub struct CompactionPreviewRequest {
    pub session_id: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct CompactionPreviewResponse {
    pub kept: Vec<KeptPillarDto>,
    pub written_parts: Vec<WrittenPartDto>,
    pub always_here: AlwaysHereDto,
    pub steer: CompactionSteerDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last: Option<LastCompactionDto>,
    /// The latest compaction's stored summary: what the model wrote, then what goose kept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_kept: Option<String>,
    /// The kept block's share of the window, in tokens; absent when the window is unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kept_budget_tokens: Option<u64>,
    /// The note saved for this chat could not be read; the steer shown is empty because of it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub steer_error: Option<String>,
}

/// Replaces what the person set for this chat's compactions.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/session/compaction/steer",
    response = CompactionSteerResponse
)]
#[serde(rename_all = "camelCase")]
pub struct CompactionSteerRequest {
    pub session_id: String,
    pub steer: CompactionSteerDto,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct CompactionSteerResponse {
    pub steer: CompactionSteerDto,
}
