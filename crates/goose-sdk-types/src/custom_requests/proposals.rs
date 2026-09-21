//! Memory proposals (frame 1.14): what the agent ASKED to remember at the end of a turn, or a
//! knowledge piece its research filed. Listed and answered here, never through elicitation — an
//! elicitation times out and is cancelled without client support; a proposal sits patiently.

use agent_client_protocol::{JsonRpcRequest, JsonRpcResponse};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum MemoryProposalKind {
    Memory,
    Knowledge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum MemoryProposalPolarity {
    Positive,
    Negative,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum MemoryProposalState {
    Open,
    Saved,
    Declined,
    Expired,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MemoryProposalDto {
    pub id: String,
    /// The store key the proposal lives under: the session id, or the working-dir key for a
    /// piece the memory extension filed. Echoed back on answer.
    pub key: String,
    pub kind: MemoryProposalKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub polarity: Option<MemoryProposalPolarity>,
    pub text: String,
    pub why: String,
    pub category: String,
    pub tags: Vec<String>,
    pub is_global: bool,
    pub sources: Vec<String>,
    pub created_at: u64,
    pub state: MemoryProposalState,
}

/// Every proposal for a session: those the end-of-turn assessment filed under the session id
/// and those `propose_knowledge` filed under the session's working directory.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/memory_proposals/list",
    response = ListMemoryProposalsResponse
)]
#[serde(rename_all = "camelCase")]
pub struct ListMemoryProposalsRequest {
    pub session_id: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct ListMemoryProposalsResponse {
    pub proposals: Vec<MemoryProposalDto>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum MemoryProposalDecision {
    Save,
    Decline,
}

/// The human's answer. Save writes the (possibly edited) text through the memory store;
/// Decline writes nothing. Either way the proposal leaves the open state.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/memory_proposals/answer",
    response = AnswerMemoryProposalResponse
)]
#[serde(rename_all = "camelCase")]
pub struct AnswerMemoryProposalRequest {
    pub session_id: String,
    pub key: String,
    pub proposal_id: String,
    pub decision: MemoryProposalDecision,
    /// The text as the user edited it; omitted = as proposed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

impl Default for AnswerMemoryProposalRequest {
    fn default() -> Self {
        Self {
            session_id: String::new(),
            key: String::new(),
            proposal_id: String::new(),
            decision: MemoryProposalDecision::Decline,
            text: None,
        }
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct AnswerMemoryProposalResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proposal: Option<MemoryProposalDto>,
    /// What the memory store did on Save: "added" | "updated" | "unchanged"; None on Decline.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<String>,
}
