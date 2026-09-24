use agent_client_protocol::schema::v1::{AvailableCommand, ContentBlock, McpServer, SessionInfo};
use agent_client_protocol::{JsonRpcRequest, JsonRpcResponse};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

mod recipe;
pub use recipe::*;
mod schedule;
pub use schedule::*;
mod proposals;
pub use proposals::*;

/// Schema descriptor for a single custom method, produced by the
/// `#[custom_methods]` macro's generated `custom_method_schemas()` function.
///
/// `params_schema` / `response_schema` hold `$ref` pointers or inline schemas
/// produced by `SchemaGenerator::subschema_for`. All referenced types are
/// collected in the generator's `$defs` map.
///
/// `params_type_name` / `response_type_name` carry the Rust struct name so the
/// binary can key `$defs` entries and annotate them with `x-method` / `x-side`.
#[derive(Debug, Serialize)]
pub struct CustomMethodSchema {
    pub method: String,
    pub params_schema: Option<schemars::Schema>,
    pub params_type_name: Option<String>,
    pub response_schema: Option<schemars::Schema>,
    pub response_type_name: Option<String>,
}

/// Add an extension to an active session.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/session/extensions/add", response = EmptyResponse)]
#[serde(rename_all = "camelCase")]
pub struct AddSessionExtensionRequest {
    pub session_id: String,
    pub extension: GooseExtension,
}

/// Remove an extension from an active session.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/session/extensions/remove", response = EmptyResponse)]
#[serde(rename_all = "camelCase")]
pub struct RemoveSessionExtensionRequest {
    pub session_id: String,
    pub name: String,
}

/// List all tools available in a session.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/tools/list", response = GetToolsResponse)]
#[serde(rename_all = "camelCase")]
pub struct GetToolsRequest {
    pub session_id: String,
    /// Filter tools to those belonging to this extension.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extension_name: Option<String>,
}

/// A single tool item returned by the tools list endpoint.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ToolListItem {
    pub name: String,
    pub description: String,
    pub parameters: Vec<String>,
    pub permission: Option<ToolPermissionLevel>,
    pub input_schema: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<serde_json::Value>,
}

/// Tools response.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
pub struct GetToolsResponse {
    pub tools: Vec<ToolListItem>,
}

/// Read a resource from an extension.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/resources/read", response = ReadResourceResponse)]
#[serde(rename_all = "camelCase")]
pub struct ReadResourceRequest {
    pub session_id: String,
    pub uri: String,
    pub extension_name: String,
}

/// Resource read response.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
pub struct ReadResourceResponse {
    /// The resource result from the extension (MCP ReadResourceResult).
    #[serde(default)]
    pub result: serde_json::Value,
}

/// Call a tool from an extension.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/tools/call", response = GooseToolCallResponse)]
#[serde(rename_all = "camelCase")]
pub struct GooseToolCallRequest {
    pub session_id: String,
    pub name: String,
    #[serde(default)]
    pub arguments: serde_json::Value,
}

/// Tool call response.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct GooseToolCallResponse {
    #[serde(default)]
    pub content: Vec<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub structured_content: Option<serde_json::Value>,
    pub is_error: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "_meta")]
    pub meta: Option<serde_json::Value>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/apps/list", response = AppsListResponse)]
#[serde(rename_all = "camelCase")]
pub struct AppsListRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
pub struct AppsListResponse {
    #[serde(default)]
    pub apps: Vec<serde_json::Value>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/apps/export", response = AppsExportResponse)]
#[serde(rename_all = "camelCase")]
pub struct AppsExportRequest {
    pub name: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
pub struct AppsExportResponse {
    pub html: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/apps/import", response = AppsImportResponse)]
#[serde(rename_all = "camelCase")]
pub struct AppsImportRequest {
    pub html: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
pub struct AppsImportResponse {
    pub name: String,
    pub message: String,
}

/// Update the working directory for a session.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/session/working-dir/update", response = EmptyResponse)]
#[serde(rename_all = "camelCase")]
pub struct UpdateWorkingDirRequest {
    pub session_id: String,
    pub working_dir: String,
}

/// How a session system prompt update should be applied.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SessionSystemPromptMode {
    /// Replace Goose's base system prompt with the provided text.
    Set,
    /// Append the provided text under Goose's "Additional Instructions" section.
    #[default]
    Append,
}

/// Set, append, or clear system prompt text for a session.
///
/// `mode: "set"` replaces Goose's base system prompt. `mode: "append"` adds an
/// instruction under "Additional Instructions". Reusing a key replaces the
/// previous value for that mode/key; sending empty text clears it.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/session/system-prompt/set",
    response = EmptyResponse
)]
#[serde(rename_all = "camelCase")]
pub struct SetSessionSystemPromptRequest {
    pub session_id: String,
    #[serde(default)]
    pub mode: SessionSystemPromptMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    pub text: String,
}

/// Add user input to the currently active prompt without starting a new prompt.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/session/steer",
    response = SteerSessionResponse
)]
#[serde(rename_all = "camelCase")]
pub struct SteerSessionRequest {
    pub session_id: String,
    #[serde(default)]
    pub prompt: Vec<ContentBlock>,
    pub expected_run_id: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct SteerSessionResponse {
    pub run_id: String,
    /// Stable id of the queued steer message. The same id later appears as
    /// `messageId` on the streamed `UserMessageChunk` (with `_meta.goose.steer`),
    /// letting clients correlate a queued steer with its pickup.
    pub message_id: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/diagnostics/get",
    response = DiagnosticsGetResponse
)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsGetRequest {
    pub session_id: String,
    #[serde(default)]
    pub level: DiagnosticsReportLevel,
}

#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticsReportLevel {
    #[default]
    Summary,
    Full,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
pub struct DiagnosticsGetResponse {
    pub report: serde_json::Value,
}

/// Information about a prompt template, including its default content and customization status.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PromptTemplateEntry {
    pub name: String,
    pub description: String,
    pub default_content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_content: Option<String>,
    pub is_customized: bool,
}

/// List all available Goose prompt templates.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/config/prompts/list", response = ListPromptsResponse)]
#[serde(rename_all = "camelCase")]
pub struct ListPromptsRequest {}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct ListPromptsResponse {
    pub prompts: Vec<PromptTemplateEntry>,
}

/// Read a Goose prompt template.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/config/prompts/get", response = GetPromptResponse)]
#[serde(rename_all = "camelCase")]
pub struct GetPromptRequest {
    pub name: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct GetPromptResponse {
    pub name: String,
    pub content: String,
    pub default_content: String,
    pub is_customized: bool,
}

/// Save a custom Goose prompt template.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/config/prompts/save", response = PromptOperationResponse)]
#[serde(rename_all = "camelCase")]
pub struct SavePromptRequest {
    pub name: String,
    pub content: String,
}

/// Reset a Goose prompt template to its default content.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/config/prompts/reset", response = PromptOperationResponse)]
#[serde(rename_all = "camelCase")]
pub struct ResetPromptRequest {
    pub name: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct PromptOperationResponse {
    pub message: String,
}

/// Delete a session.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "session/delete", response = EmptyResponse)]
#[serde(rename_all = "camelCase")]
pub struct DeleteSessionRequest {
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GooseExtension {
    Builtin {
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        display_name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeout: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bundled: Option<bool>,
        /// Tool allowlist for this extension. Omit this field to allow all tools.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        available_tools: Option<Vec<String>>,
    },
    Platform {
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        display_name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bundled: Option<bool>,
        /// Tool allowlist for this extension. Omit this field to allow all tools.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        available_tools: Option<Vec<String>>,
    },
    Mcp {
        server: McpServer,
        #[serde(default, rename = "envKeys", skip_serializing_if = "Vec::is_empty")]
        env_keys: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeout: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        socket: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bundled: Option<bool>,
        /// Tool allowlist for this extension. Omit this field to allow all tools.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        available_tools: Option<Vec<String>>,
    },
}

impl Default for GooseExtension {
    fn default() -> Self {
        Self::Builtin {
            name: String::new(),
            description: None,
            display_name: None,
            timeout: None,
            bundled: None,
            available_tools: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GooseExtensionEntry {
    pub extension: GooseExtension,
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_key: Option<String>,
}

/// List Goose-owned extension definitions available to configure or enable.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/extensions/available",
    response = GetAvailableExtensionsResponse
)]
pub struct GetAvailableExtensionsRequest {}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct GetAvailableExtensionsResponse {
    pub extensions: Vec<GooseExtension>,
}

/// List configured extensions and any warnings.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/config/extensions/list",
    response = GetConfigExtensionsResponse
)]
pub struct GetConfigExtensionsRequest {}

/// List configured extensions and any warnings.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
pub struct GetConfigExtensionsResponse {
    pub extensions: Vec<GooseExtensionEntry>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

pub type GetExtensionsRequest = GetConfigExtensionsRequest;
pub type GetExtensionsResponse = GetConfigExtensionsResponse;

/// Persist a new extension to the user's global goose config.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/config/extensions/add", response = EmptyResponse)]
#[serde(rename_all = "camelCase")]
pub struct AddConfigExtensionRequest {
    pub extension: GooseExtension,
    #[serde(default)]
    pub enabled: bool,
}

/// Remove a persisted extension from the user's global goose config.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/config/extensions/remove", response = EmptyResponse)]
#[serde(rename_all = "camelCase")]
pub struct RemoveConfigExtensionRequest {
    pub config_key: String,
}

/// Set the `enabled` flag for a persisted extension in the user's global goose config.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/config/extensions/set-enabled",
    response = EmptyResponse
)]
#[serde(rename_all = "camelCase")]
pub struct SetConfigExtensionEnabledRequest {
    pub config_key: String,
    pub enabled: bool,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/session/extensions/list", response = GetSessionExtensionsResponse)]
#[serde(rename_all = "camelCase")]
pub struct GetSessionExtensionsRequest {
    pub session_id: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
pub struct GetSessionExtensionsResponse {
    pub extensions: Vec<GooseExtension>,
}

/// Read allowlisted user preferences. Empty `keys` means all supported preferences.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/preferences/read", response = PreferencesReadResponse)]
#[serde(rename_all = "camelCase")]
pub struct PreferencesReadRequest {
    #[serde(default)]
    pub keys: Vec<PreferenceKey>,
}

/// Save allowlisted user preferences.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/preferences/save", response = EmptyResponse)]
#[serde(rename_all = "camelCase")]
pub struct PreferencesSaveRequest {
    #[serde(default)]
    pub values: Vec<PreferenceValue>,
}

/// Remove allowlisted user preferences.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/preferences/remove", response = EmptyResponse)]
#[serde(rename_all = "camelCase")]
pub struct PreferencesRemoveRequest {
    #[serde(default)]
    pub keys: Vec<PreferenceKey>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/config/read", response = ConfigReadResponse)]
#[serde(rename_all = "camelCase")]
pub struct ConfigReadRequest {
    pub key: String,
    #[serde(default)]
    pub is_secret: bool,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct ConfigReadResponse {
    #[serde(default)]
    pub value: serde_json::Value,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/config/upsert", response = EmptyResponse)]
#[serde(rename_all = "camelCase")]
pub struct ConfigUpsertRequest {
    pub key: String,
    pub value: serde_json::Value,
    #[serde(default)]
    pub is_secret: bool,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/config/remove", response = EmptyResponse)]
#[serde(rename_all = "camelCase")]
pub struct ConfigRemoveRequest {
    pub key: String,
    #[serde(default)]
    pub is_secret: bool,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/config/read-all", response = ConfigReadAllResponse)]
#[serde(rename_all = "camelCase")]
pub struct ConfigReadAllRequest {}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct ConfigReadAllResponse {
    pub config: std::collections::HashMap<String, serde_json::Value>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum PreferenceKey {
    #[default]
    AutoCompactThreshold,
    GooseThinkingEffort,
    VoiceAutoSubmitPhrases,
    VoiceDictationProvider,
    VoiceDictationPreferredMic,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PreferenceValue {
    pub key: PreferenceKey,
    #[serde(default)]
    pub value: serde_json::Value,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct PreferencesReadResponse {
    pub values: Vec<PreferenceValue>,
}

/// Read Goose default provider and model configuration.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/defaults/read", response = DefaultsReadResponse)]
#[serde(rename_all = "camelCase")]
pub struct DefaultsReadRequest {}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct DefaultsReadResponse {
    pub provider_id: Option<String>,
    pub model_id: Option<String>,
}

/// Save Goose default provider and model configuration.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/defaults/save", response = DefaultsReadResponse)]
#[serde(rename_all = "camelCase")]
pub struct DefaultsSaveRequest {
    pub provider_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
}

/// Clear Goose default provider and model configuration.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/defaults/clear", response = DefaultsReadResponse)]
#[serde(rename_all = "camelCase")]
pub struct DefaultsClearRequest {}

/// Sources that onboarding knows how to discover and import.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OnboardingImportSourceKind {
    #[default]
    GooseConfig,
    ClaudeDesktop,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OnboardingImportCounts {
    pub providers: u32,
    pub extensions: u32,
    pub sessions: u32,
    pub skills: u32,
    pub projects: u32,
    pub preferences: u32,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OnboardingImportCandidate {
    pub id: String,
    pub source_kind: OnboardingImportSourceKind,
    pub display_name: String,
    pub path: String,
    pub counts: OnboardingImportCounts,
    #[serde(default)]
    pub warnings: Vec<String>,
}

/// Scan for existing Goose and compatible app data that onboarding can import.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/onboarding/import/scan",
    response = OnboardingImportScanResponse
)]
#[serde(rename_all = "camelCase")]
pub struct OnboardingImportScanRequest {
    /// Empty means all supported import sources.
    #[serde(default)]
    pub sources: Vec<OnboardingImportSourceKind>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct OnboardingImportScanResponse {
    pub candidates: Vec<OnboardingImportCandidate>,
}

/// Import selected onboarding candidates.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/onboarding/import/apply",
    response = OnboardingImportApplyResponse
)]
#[serde(rename_all = "camelCase")]
pub struct OnboardingImportApplyRequest {
    #[serde(default)]
    pub candidate_ids: Vec<String>,
    #[serde(default)]
    pub enable_imported_extensions: bool,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct OnboardingImportApplyResponse {
    pub imported: OnboardingImportCounts,
    pub skipped: OnboardingImportCounts,
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_defaults: Option<DefaultsReadResponse>,
}

/// Set a dictation provider secret value.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/dictation/secret/save", response = EmptyResponse)]
#[serde(rename_all = "camelCase")]
pub struct DictationSecretSaveRequest {
    pub provider: String,
    pub value: String,
}

/// Remove a dictation provider secret value.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/dictation/secret/delete", response = EmptyResponse)]
#[serde(rename_all = "camelCase")]
pub struct DictationSecretDeleteRequest {
    pub provider: String,
}

/// Return list-style metadata for a single session without loading the conversation.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/session/info",
    response = GetSessionInfoResponse
)]
#[serde(rename_all = "camelCase")]
pub struct GetSessionInfoRequest {
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct GetSessionInfoResponse {
    pub session: SessionInfo,
}

/// Truncate a session conversation from the given message timestamp onward.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/session/conversation/truncate",
    response = EmptyResponse
)]
#[serde(rename_all = "camelCase")]
pub struct TruncateSessionConversationRequest {
    pub session_id: String,
    pub truncate_from: i64,
}

/// Update the project association for a session.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/session/project/update", response = EmptyResponse)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSessionProjectRequest {
    pub session_id: String,
    pub project_id: Option<String>,
}

/// Rename a session.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/session/rename", response = EmptyResponse)]
#[serde(rename_all = "camelCase")]
pub struct RenameSessionRequest {
    pub session_id: String,
    pub title: String,
}

/// Archive a session (soft delete).
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/session/archive", response = EmptyResponse)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveSessionRequest {
    pub session_id: String,
}

/// Unarchive a previously archived session.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/session/unarchive", response = EmptyResponse)]
#[serde(rename_all = "camelCase")]
pub struct UnarchiveSessionRequest {
    pub session_id: String,
}

/// Export a session as a JSON string.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/session/export", response = ExportSessionResponse)]
#[serde(rename_all = "camelCase")]
pub struct ExportSessionRequest {
    pub session_id: String,
}

/// Export session response — raw JSON of the goose session with `conversation`.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
pub struct ExportSessionResponse {
    pub data: String,
}

/// Import a session from a JSON string or share link.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/session/import", response = ImportSessionResponse)]
#[serde(rename_all = "camelCase")]
pub struct ImportSessionRequest {
    pub input: String,
    pub source: SessionImportSource,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SessionImportSource {
    #[default]
    Auto,
    Json,
    Nostr,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/session/share/nostr",
    response = ShareSessionNostrResponse
)]
#[serde(rename_all = "camelCase")]
pub struct ShareSessionNostrRequest {
    pub session_id: String,
    pub relays: Vec<String>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct ShareSessionNostrResponse {
    pub deeplink: String,
    pub nevent: String,
    pub event_id: String,
    pub relays: Vec<String>,
}

/// Import session response — metadata about the newly created session.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct ImportSessionResponse {
    pub session_id: String,
    pub title: Option<String>,
    pub updated_at: Option<String>,
    pub message_count: u64,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConfigKey {
    pub name: String,
    pub required: bool,
    pub secret: bool,
    #[serde(default)]
    pub default: Option<String>,
    #[serde(default)]
    pub oauth_flow: bool,
    #[serde(default)]
    pub device_code_flow: bool,
    #[serde(default)]
    pub primary: bool,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConfigFieldValueDto {
    pub key: String,
    #[serde(default)]
    pub value: Option<String>,
    pub is_set: bool,
    pub is_secret: bool,
    pub required: bool,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConfigStatusDto {
    pub provider_id: String,
    pub is_configured: bool,
    pub connection_checked: bool,
    pub connection_error: Option<String>,
    pub test_model: Option<String>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConfigFieldUpdate {
    pub key: String,
    pub value: String,
}

/// Read saved configuration field values for one provider.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/providers/config/read",
    response = ProviderConfigReadResponse
)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConfigReadRequest {
    pub provider_id: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConfigReadResponse {
    pub fields: Vec<ProviderConfigFieldValueDto>,
}

/// Return provider configured statuses. Empty provider_ids means all providers.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/providers/config/status",
    response = ProviderConfigStatusResponse
)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConfigStatusRequest {
    #[serde(default)]
    pub provider_ids: Vec<String>,
    #[serde(default)]
    pub check_connections: bool,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConfigStatusResponse {
    pub statuses: Vec<ProviderConfigStatusDto>,
}

/// Save provider configuration fields and start an inventory refresh when supported.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/providers/config/save",
    response = ProviderConfigChangeResponse
)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConfigSaveRequest {
    pub provider_id: String,
    pub test_model: Option<String>,
    pub fields: Vec<ProviderConfigFieldUpdate>,
}

/// Delete provider configuration fields and start an inventory refresh when supported.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/providers/config/delete",
    response = ProviderConfigChangeResponse
)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConfigDeleteRequest {
    pub provider_id: String,
}

/// Run a provider-owned native authentication flow and start an inventory refresh when supported.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/providers/config/authenticate",
    response = ProviderConfigChangeResponse
)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConfigAuthenticateRequest {
    pub provider_id: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConfigChangeResponse {
    pub status: ProviderConfigStatusDto,
    pub refresh: RefreshProviderInventoryResponse,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProviderSecretStorageDto {
    #[default]
    SecretStore,
    ProviderCache,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProviderSecretStatusDto {
    Valid,
    Expired,
    #[default]
    Unknown,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSecretDto {
    pub id: String,
    pub provider: String,
    pub provider_display_name: String,
    pub name: String,
    pub storage: ProviderSecretStorageDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    pub status: ProviderSecretStatusDto,
    pub configured: bool,
    pub has_secret: bool,
    pub can_delete: bool,
    pub can_configure: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configure_provider: Option<String>,
}

/// List provider credentials stored locally by Goose.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/providers/secrets/list",
    response = ProviderSecretsListResponse
)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSecretsListRequest {}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSecretsListResponse {
    pub secrets: Vec<ProviderSecretDto>,
}

/// Delete a locally stored provider credential by id.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/providers/secrets/delete",
    response = EmptyResponse
)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSecretDeleteRequest {
    pub id: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CanonicalModelInfoDto {
    pub provider: String,
    pub model: String,
    pub context_limit: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<usize>,
    pub reasoning: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_token_cost: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_token_cost: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_token_cost: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write_token_cost: Option<f64>,
    pub currency: String,
}

/// Look up canonical (bundled-registry) model info for a provider/model pair.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/providers/canonical-model-info",
    response = CanonicalModelInfoResponse
)]
#[serde(rename_all = "camelCase")]
pub struct CanonicalModelInfoRequest {
    pub provider: String,
    pub model: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct CanonicalModelInfoResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_info: Option<CanonicalModelInfoDto>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProviderTemplateCatalogEntryDto {
    pub provider_id: String,
    pub name: String,
    pub format: String,
    pub api_url: String,
    pub model_count: usize,
    pub doc_url: String,
    pub env_var: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProviderSetupCategoryDto {
    Agent,
    #[default]
    Model,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProviderSetupMethodDto {
    None,
    SingleApiKey,
    ConfigFields,
    HostWithOauthFallback,
    OauthBrowser,
    OauthDeviceCode,
    CloudCredentials,
    Local,
    CliAuth,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProviderSetupGroupDto {
    Default,
    Additional,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSetupFieldDto {
    pub key: String,
    pub label: String,
    pub secret: bool,
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_value: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSetupCatalogEntryDto {
    pub provider_id: String,
    pub name: String,
    pub category: ProviderSetupCategoryDto,
    pub description: String,
    pub setup_method: ProviderSetupMethodDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_connect_query: Option<String>,
    #[serde(default)]
    pub fields: Vec<ProviderSetupFieldDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binary_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doc_url: Option<String>,
    pub group: ProviderSetupGroupDto,
    pub show_only_when_installed: bool,
    #[serde(default)]
    pub aliases: Vec<String>,
    pub supports_install: bool,
    pub supports_auth: bool,
    pub supports_auth_status: bool,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProviderTemplateCapabilitiesDto {
    pub tool_call: bool,
    pub reasoning: bool,
    pub attachment: bool,
    pub temperature: bool,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProviderTemplateModelDto {
    pub id: String,
    pub name: String,
    pub context_limit: usize,
    pub capabilities: ProviderTemplateCapabilitiesDto,
    pub deprecated: bool,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProviderTemplateDto {
    pub provider_id: String,
    pub name: String,
    pub format: String,
    pub api_url: String,
    pub models: Vec<ProviderTemplateModelDto>,
    pub supports_streaming: bool,
    pub env_var: String,
    pub doc_url: String,
}

/// List custom-provider catalog entries. Omit `format` to list all formats.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/providers/catalog/list",
    response = ProviderCatalogListResponse
)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCatalogListRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCatalogListResponse {
    pub providers: Vec<ProviderTemplateCatalogEntryDto>,
}

/// List provider setup catalog entries
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/providers/setup/catalog/list",
    response = ProviderSetupCatalogListResponse
)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSetupCatalogListRequest {}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSetupCatalogListResponse {
    pub providers: Vec<ProviderSetupCatalogEntryDto>,
}

/// Return the editable template for one catalog provider.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/providers/catalog/template",
    response = ProviderCatalogTemplateResponse
)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCatalogTemplateRequest {
    pub provider_id: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCatalogTemplateResponse {
    pub template: ProviderTemplateDto,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CustomProviderConfigDto {
    pub provider_id: String,
    pub engine: String,
    pub display_name: String,
    pub api_url: String,
    #[serde(default)]
    pub models: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_streaming: Option<bool>,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    pub requires_auth: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub catalog_provider_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key_env: Option<String>,
    pub api_key_set: bool,
    pub preserves_thinking: bool,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CustomProviderUpsertDto {
    pub engine: String,
    pub display_name: String,
    pub api_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(default)]
    pub models: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_streaming: Option<bool>,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    pub requires_auth: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub catalog_provider_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preserves_thinking: Option<bool>,
}

/// Create a custom provider backed by Goose's declarative provider store.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/providers/custom/create",
    response = CustomProviderCreateResponse
)]
#[serde(rename_all = "camelCase")]
pub struct CustomProviderCreateRequest {
    #[serde(flatten)]
    pub provider: CustomProviderUpsertDto,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct CustomProviderCreateResponse {
    pub provider_id: String,
    pub status: ProviderConfigStatusDto,
    pub refresh: RefreshProviderInventoryResponse,
}

/// Read a declarative provider config. Custom configs are editable; bundled configs are read-only.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/providers/custom/read",
    response = CustomProviderReadResponse
)]
#[serde(rename_all = "camelCase")]
pub struct CustomProviderReadRequest {
    pub provider_id: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct CustomProviderReadResponse {
    pub provider: CustomProviderConfigDto,
    pub editable: bool,
    pub status: ProviderConfigStatusDto,
}

/// Update a custom provider backed by Goose's declarative provider store.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/providers/custom/update",
    response = CustomProviderUpdateResponse
)]
#[serde(rename_all = "camelCase")]
pub struct CustomProviderUpdateRequest {
    pub provider_id: String,
    #[serde(flatten)]
    pub provider: CustomProviderUpsertDto,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct CustomProviderUpdateResponse {
    pub provider_id: String,
    pub status: ProviderConfigStatusDto,
    pub refresh: RefreshProviderInventoryResponse,
}

/// Delete a custom provider from Goose's declarative provider store.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/providers/custom/delete",
    response = CustomProviderDeleteResponse
)]
#[serde(rename_all = "camelCase")]
pub struct CustomProviderDeleteRequest {
    pub provider_id: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct CustomProviderDeleteResponse {
    pub provider_id: String,
    pub refresh: RefreshProviderInventoryResponse,
}

/// The type of source entity.
#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "camelCase")]
pub enum SourceType {
    #[default]
    Skill,
    BuiltinSkill,
    Recipe,
    Subrecipe,
    Agent,
    Project,
}

impl std::fmt::Display for SourceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SourceType::Skill => write!(f, "skill"),
            SourceType::BuiltinSkill => write!(f, "builtin skill"),
            SourceType::Recipe => write!(f, "recipe"),
            SourceType::Subrecipe => write!(f, "subrecipe"),
            SourceType::Agent => write!(f, "agent"),
            SourceType::Project => write!(f, "project"),
        }
    }
}

/// A source discovered by Goose. Filesystem sources use an on-disk path;
/// built-in sources use a stable synthetic path. Sources may be either
/// `global` (shared across all projects) or project-specific.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SourceEntry {
    #[serde(rename = "type")]
    pub source_type: SourceType,
    pub name: String,
    pub description: String,
    pub content: String,
    /// Stable on-disk path identifying this source. Pass it back to
    /// update/delete/export to operate on this entry. Skills use the directory
    /// containing `SKILL.md`; projects use the project file path; built-in
    /// skills use `builtin://skills/<name>` synthetic paths.
    pub path: String,
    /// True when the source lives in the user's global sources directory; false
    /// when it lives inside a specific project.
    pub global: bool,
    /// True when this source can be modified through source CRUD methods.
    /// Client-provided bundled sources are returned as read-only.
    #[serde(default)]
    pub writable: bool,
    /// Paths (absolute) of additional files that live alongside the source.
    /// Only skills currently populate this; empty for other source types.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub supporting_files: Vec<String>,
    /// Arbitrary key/value pairs for type-specific metadata (e.g. icon, color,
    /// preferredProvider for projects). Stored in the frontmatter.
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub properties: std::collections::HashMap<String, serde_json::Value>,
}

impl SourceEntry {
    /// Render this source as a markdown block suitable for injecting into an
    /// LLM context. Used by the skills and summon runtimes when loading a
    /// source into the current conversation.
    pub fn to_load_text(&self) -> String {
        format!(
            "## {} ({})\n\n{}\n\n### Content\n\n{}",
            self.name, self.source_type, self.description, self.content
        )
    }
}

/// Target scope for creating or importing sources.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "scope", rename_all = "camelCase")]
pub enum SourceScope {
    #[default]
    Global,
    ProjectDir {
        #[serde(rename = "projectDir")]
        project_dir: String,
    },
    ProjectId {
        #[serde(rename = "projectId")]
        project_id: String,
    },
}

/// Create a new source in an explicit target scope (global or project-scoped).
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/sources/create", response = CreateSourceResponse)]
#[serde(rename_all = "camelCase")]
pub struct CreateSourceRequest {
    #[serde(rename = "type")]
    pub source_type: SourceType,
    pub name: String,
    pub description: String,
    pub content: String,
    pub target: SourceScope,
    /// Arbitrary key/value metadata.
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub properties: std::collections::HashMap<String, serde_json::Value>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct CreateSourceResponse {
    pub source: SourceEntry,
}

/// List discovered sources.
///
/// If `type` is omitted or `skill`, this lists filesystem/plugin skills only.
/// Both global and project-scoped skills are included when `project_dir` is
/// set. If `type` is `builtinSkill`, this lists shipped read-only built-in
/// skills.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/sources/list", response = ListSourcesResponse)]
#[serde(rename_all = "camelCase")]
pub struct ListSourcesRequest {
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub source_type: Option<SourceType>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_dir: Option<String>,
    /// When true, also scan the working directories of all known projects for
    /// project-scoped sources (e.g. skills stored under `{workingDir}/.agents/skills/`).
    #[serde(default)]
    pub include_project_sources: bool,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct ListSourcesResponse {
    pub sources: Vec<SourceEntry>,
}

/// A user-facing `@` mention target backed by an agent, recipe, or subrecipe source.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AgentMention {
    pub name: String,
    pub description: String,
    pub source_type: SourceType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,
    pub mention: String,
}

/// List user-facing agent mention targets for `@` autocomplete.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/agent-mentions/list",
    response = ListAgentMentionsResponse
)]
#[serde(rename_all = "camelCase")]
pub struct ListAgentMentionsRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct ListAgentMentionsResponse {
    pub agents: Vec<AgentMention>,
}

/// List slash commands available for `/` autocomplete.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/slash-commands/list",
    response = ListSlashCommandsResponse
)]
#[serde(rename_all = "camelCase")]
pub struct ListSlashCommandsRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct ListSlashCommandsResponse {
    pub available_commands: Vec<AvailableCommand>,
}

/// Update an existing source's name, description, and content by absolute path.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/sources/update", response = UpdateSourceResponse)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSourceRequest {
    #[serde(rename = "type")]
    pub source_type: SourceType,
    pub path: String,
    pub name: String,
    pub description: String,
    pub content: String,
    /// When `Some`, replaces all stored properties on the source. When
    /// `None` (or omitted), the source's existing properties are
    /// preserved. Callers that don't model the full property bag (e.g.
    /// the skills editor, which only edits name/description/content)
    /// should omit this so per-skill metadata isn't silently erased.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub properties: Option<std::collections::HashMap<String, serde_json::Value>>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSourceResponse {
    pub source: SourceEntry,
}

/// Delete a source and its on-disk directory by absolute path.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/sources/delete", response = EmptyResponse)]
#[serde(rename_all = "camelCase")]
pub struct DeleteSourceRequest {
    #[serde(rename = "type")]
    pub source_type: SourceType,
    pub path: String,
}

/// Export a source at an absolute path as a portable JSON payload.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/sources/export", response = ExportSourceResponse)]
#[serde(rename_all = "camelCase")]
pub struct ExportSourceRequest {
    #[serde(rename = "type")]
    pub source_type: SourceType,
    pub path: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct ExportSourceResponse {
    pub json: String,
    pub filename: String,
}

/// Import a source from a JSON export payload produced by `_goose/unstable/sources/export`.
/// The imported source is written into the explicit target scope; on name
/// collisions a `-imported` suffix is appended.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/sources/import", response = ImportSourcesResponse)]
#[serde(rename_all = "camelCase")]
pub struct ImportSourcesRequest {
    pub data: String,
    pub target: SourceScope,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct ImportSourcesResponse {
    pub sources: Vec<SourceEntry>,
}

/// Transcribe audio via a dictation provider.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/dictation/transcribe", response = DictationTranscribeResponse)]
#[serde(rename_all = "camelCase")]
pub struct DictationTranscribeRequest {
    /// Base64-encoded audio data
    pub audio: String,
    /// MIME type (e.g. "audio/wav", "audio/webm")
    pub mime_type: String,
    /// Provider to use: "openai", "groq", "elevenlabs", or "local"
    pub provider: String,
}

/// Transcription result.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
pub struct DictationTranscribeResponse {
    pub text: String,
}

/// Get the configuration status of all dictation providers.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/dictation/config", response = DictationConfigResponse)]
pub struct DictationConfigRequest {}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DictationModelOption {
    pub id: String,
    pub label: String,
    pub description: String,
}

/// Per-provider configuration status.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DictationProviderStatusEntry {
    pub configured: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    pub description: String,
    pub uses_provider_config: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settings_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_config_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_model: Option<String>,
    #[serde(default)]
    pub available_models: Vec<DictationModelOption>,
}

/// Dictation config response — map of provider name to status.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
pub struct DictationConfigResponse {
    pub providers: HashMap<String, DictationProviderStatusEntry>,
}

/// List providers with setup metadata and the current model inventory snapshot.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/providers/list", response = ListProvidersResponse)]
#[serde(rename_all = "camelCase")]
pub struct ListProvidersRequest {
    /// Only return entries for these providers. Empty means all.
    #[serde(default)]
    pub provider_ids: Vec<String>,
}

/// Provider list response.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
pub struct ListProvidersResponse {
    pub entries: Vec<ProviderInventoryEntryDto>,
}

/// List the raw model identifiers returned by a provider's live supported-models API.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/providers/supported-models/list",
    response = ProviderSupportedModelsListResponse
)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSupportedModelsListRequest {
    pub provider_id: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSupportedModelsListResponse {
    pub provider_id: String,
    pub models: Vec<String>,
}

/// Trigger a background refresh of provider inventories.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/providers/inventory/refresh",
    response = RefreshProviderInventoryResponse
)]
#[serde(rename_all = "camelCase")]
pub struct RefreshProviderInventoryRequest {
    /// Which providers to refresh. Empty means all known providers.
    #[serde(default)]
    pub provider_ids: Vec<String>,
}

/// Refresh acknowledgement.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct RefreshProviderInventoryResponse {
    /// Which providers will be refreshed.
    pub started: Vec<String>,
    /// Which providers were skipped and why.
    #[serde(default)]
    pub skipped: Vec<RefreshProviderInventorySkipDto>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RefreshProviderInventorySkipDto {
    pub provider_id: String,
    pub reason: RefreshProviderInventorySkipReasonDto,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RefreshProviderInventorySkipReasonDto {
    #[default]
    UnknownProvider,
    NotConfigured,
    DoesNotSupportRefresh,
    AlreadyRefreshing,
}

/// A single model in provider inventory.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInventoryModelDto {
    /// Model identifier as the provider knows it.
    pub id: String,
    /// Human-readable display name.
    pub name: String,
    /// Model family for grouping in UI.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,
    /// Context window size in tokens.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_limit: Option<usize>,
    /// Whether the model supports reasoning/extended thinking.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<bool>,
    /// Whether this model should appear in the compact recommended picker.
    #[serde(default)]
    pub recommended: bool,
}

/// Provider inventory entry.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInventoryEntryDto {
    /// Provider identifier.
    pub provider_id: String,
    /// Human-readable provider name.
    pub provider_name: String,
    /// Description of the provider's capabilities.
    pub description: String,
    /// The default/recommended model for this provider.
    pub default_model: String,
    /// Whether Goose has enough configuration to use this provider.
    pub configured: bool,
    /// Provider classification such as `Preferred`, `Builtin`, `Declarative`, or `Custom`.
    pub provider_type: String,
    /// Whether this inventory entry represents an agent provider or a model provider.
    pub category: ProviderSetupCategoryDto,
    /// Required configuration keys and setup metadata.
    pub config_keys: Vec<ProviderConfigKey>,
    /// Step-by-step setup instructions, when present.
    pub setup_steps: Vec<String>,
    /// Whether this provider supports background inventory refresh.
    pub supports_refresh: bool,
    /// Whether a refresh is currently in flight.
    pub refreshing: bool,
    /// The list of available models.
    pub models: Vec<ProviderInventoryModelDto>,
    /// When this entry was last successfully refreshed (ISO 8601).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_updated_at: Option<String>,
    /// When a refresh was most recently attempted (ISO 8601).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_refresh_attempt_at: Option<String>,
    /// The last refresh failure message, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_refresh_error: Option<String>,
    /// Whether we believe this data may be outdated.
    pub stale: bool,
    /// Guidance message shown when this provider manages its own model selection externally.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_selection_hint: Option<String>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LocalInferenceToolCallingMode {
    #[default]
    Auto,
    ForceNative,
    ForceEmulated,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LocalInferenceChatTemplate {
    #[default]
    Embedded,
    Builtin {
        name: String,
    },
    CustomInline {
        template: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all_fields = "camelCase")]
pub enum LocalInferenceSamplingConfig {
    Greedy,
    Temperature {
        temperature: f32,
        top_k: i32,
        top_p: f32,
        min_p: f32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        seed: Option<u32>,
    },
    MirostatV2 {
        tau: f32,
        eta: f32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        seed: Option<u32>,
    },
}

impl Default for LocalInferenceSamplingConfig {
    fn default() -> Self {
        Self::Temperature {
            temperature: 0.8,
            top_k: 40,
            top_p: 0.95,
            min_p: 0.05,
            seed: None,
        }
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LocalInferenceModelSettingsDto {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_size: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draft_model: Option<String>,
    #[serde(default)]
    pub sampling: LocalInferenceSamplingConfig,
    pub repeat_penalty: f32,
    pub repeat_last_n: i32,
    pub frequency_penalty: f32,
    pub presence_penalty: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub n_batch: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub n_gpu_layers: Option<u32>,
    pub use_mlock: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flash_attention: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub n_threads: Option<i32>,
    #[serde(default)]
    pub tool_calling: LocalInferenceToolCallingMode,
    #[serde(default)]
    pub chat_template: LocalInferenceChatTemplate,
    pub enable_thinking: bool,
    pub vision_capable: bool,
    pub image_token_estimate: usize,
    pub mmproj_size_bytes: u64,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum LocalInferenceDownloadState {
    #[default]
    NotDownloaded,
    Downloading,
    Downloaded,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LocalInferenceModelDownloadStatusDto {
    pub state: LocalInferenceDownloadState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress_percent: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytes_downloaded: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed_bps: Option<u64>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LocalInferenceDownloadProgressDto {
    pub model_id: String,
    pub status: String,
    pub bytes_downloaded: u64,
    pub total_bytes: u64,
    pub progress_percent: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed_bps: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eta_seconds: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub task_exited: bool,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LocalInferenceModelDto {
    pub id: String,
    pub repo_id: String,
    pub filename: String,
    pub quantization: String,
    pub size_bytes: u64,
    pub status: LocalInferenceModelDownloadStatusDto,
    pub recommended: bool,
    pub settings: LocalInferenceModelSettingsDto,
    pub vision_capable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mmproj_status: Option<LocalInferenceModelDownloadStatusDto>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LocalInferenceHfModelVariantDto {
    pub variant_id: String,
    pub label: String,
    pub backend_id: String,
    pub format: String,
    pub model_id: String,
    pub download_id: String,
    pub size_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub download_url: Option<String>,
    pub description: String,
    pub quality_rank: u8,
    pub sharded: bool,
    pub supported: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unsupported_reason: Option<String>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LocalInferenceHfGgufFileDto {
    pub filename: String,
    pub size_bytes: u64,
    pub quantization: String,
    pub download_url: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LocalInferenceHfModelInfoDto {
    pub repo_id: String,
    pub author: String,
    pub model_name: String,
    pub downloads: u64,
    #[serde(default)]
    pub gguf_files: Vec<LocalInferenceHfGgufFileDto>,
    #[serde(default)]
    pub variants: Vec<LocalInferenceHfModelVariantDto>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/local-inference/models/list",
    response = LocalInferenceModelsListResponse
)]
#[serde(rename_all = "camelCase")]
pub struct LocalInferenceModelsListRequest {}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct LocalInferenceModelsListResponse {
    pub models: Vec<LocalInferenceModelDto>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/local-inference/models/download",
    response = LocalInferenceModelDownloadResponse
)]
#[serde(rename_all = "camelCase")]
pub struct LocalInferenceModelDownloadRequest {
    pub spec: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant_id: Option<String>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct LocalInferenceModelDownloadResponse {
    pub model_id: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/local-inference/models/download/progress",
    response = LocalInferenceModelDownloadProgressResponse
)]
#[serde(rename_all = "camelCase")]
pub struct LocalInferenceModelDownloadProgressRequest {
    pub model_id: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct LocalInferenceModelDownloadProgressResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<LocalInferenceDownloadProgressDto>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/local-inference/models/download/cancel",
    response = EmptyResponse
)]
#[serde(rename_all = "camelCase")]
pub struct LocalInferenceModelDownloadCancelRequest {
    pub model_id: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/local-inference/models/delete",
    response = EmptyResponse
)]
#[serde(rename_all = "camelCase")]
pub struct LocalInferenceModelDeleteRequest {
    pub model_id: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/local-inference/models/settings/read",
    response = LocalInferenceModelSettingsReadResponse
)]
#[serde(rename_all = "camelCase")]
pub struct LocalInferenceModelSettingsReadRequest {
    pub model_id: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct LocalInferenceModelSettingsReadResponse {
    pub settings: LocalInferenceModelSettingsDto,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/local-inference/models/settings/update",
    response = LocalInferenceModelSettingsUpdateResponse
)]
#[serde(rename_all = "camelCase")]
pub struct LocalInferenceModelSettingsUpdateRequest {
    pub model_id: String,
    pub settings: LocalInferenceModelSettingsDto,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct LocalInferenceModelSettingsUpdateResponse {
    pub settings: LocalInferenceModelSettingsDto,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/local-inference/huggingface/search",
    response = LocalInferenceHuggingFaceSearchResponse
)]
#[serde(rename_all = "camelCase")]
pub struct LocalInferenceHuggingFaceSearchRequest {
    pub query: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct LocalInferenceHuggingFaceSearchResponse {
    pub models: Vec<LocalInferenceHfModelInfoDto>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/local-inference/huggingface/repo/variants",
    response = LocalInferenceHuggingFaceRepoVariantsResponse
)]
#[serde(rename_all = "camelCase")]
pub struct LocalInferenceHuggingFaceRepoVariantsRequest {
    pub repo_id: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct LocalInferenceHuggingFaceRepoVariantsResponse {
    pub variants: Vec<LocalInferenceHfModelVariantDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommended_index: Option<usize>,
    pub available_memory_bytes: u64,
    pub downloaded_quants: Vec<String>,
    pub downloaded_variants: Vec<String>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/local-inference/chat-templates/builtin/list",
    response = LocalInferenceBuiltinChatTemplatesListResponse
)]
#[serde(rename_all = "camelCase")]
pub struct LocalInferenceBuiltinChatTemplatesListRequest {}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct LocalInferenceBuiltinChatTemplatesListResponse {
    pub templates: Vec<String>,
}

/// Empty success response for operations that return no data.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
pub struct EmptyResponse {}

/// List available local Whisper models with their download status.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/dictation/models/list",
    response = DictationModelsListResponse
)]
#[serde(rename_all = "camelCase")]
pub struct DictationModelsListRequest {}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct DictationModelsListResponse {
    pub models: Vec<DictationLocalModelStatus>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DictationLocalModelStatus {
    pub id: String,
    pub label: String,
    pub description: String,
    pub size_mb: u32,
    pub downloaded: bool,
    pub download_in_progress: bool,
    pub recommended: bool,
}

/// Kick off a background download of a local Whisper model.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/dictation/models/download", response = EmptyResponse)]
#[serde(rename_all = "camelCase")]
pub struct DictationModelDownloadRequest {
    pub model_id: String,
}

/// Poll the progress of an in-flight download.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/dictation/models/download/progress",
    response = DictationModelDownloadProgressResponse
)]
#[serde(rename_all = "camelCase")]
pub struct DictationModelDownloadProgressRequest {
    pub model_id: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct DictationModelDownloadProgressResponse {
    /// None when no download is active for this model id.
    pub progress: Option<DictationDownloadProgress>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DictationDownloadProgress {
    pub bytes_downloaded: u64,
    pub total_bytes: u64,
    pub progress_percent: f32,
    /// serde lowercase of DownloadStatus: "downloading" | "completed" | "failed" | "cancelled"
    pub status: String,
    pub error: Option<String>,
}

/// Cancel an in-flight download.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/dictation/models/cancel", response = EmptyResponse)]
#[serde(rename_all = "camelCase")]
pub struct DictationModelCancelRequest {
    pub model_id: String,
}

/// Delete a downloaded local Whisper model from disk.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/dictation/models/delete", response = EmptyResponse)]
#[serde(rename_all = "camelCase")]
pub struct DictationModelDeleteRequest {
    pub model_id: String,
}

/// Persist the user's model selection for a given provider.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/dictation/models/select", response = EmptyResponse)]
#[serde(rename_all = "camelCase")]
pub struct DictationModelSelectRequest {
    pub provider: String,
    pub model_id: String,
}

/// Permission level for a tool.
#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ToolPermissionLevel {
    AlwaysAllow,
    #[default]
    AskBefore,
    NeverAllow,
}

/// A single tool permission entry.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ToolPermissionEntry {
    pub tool_name: String,
    pub permission: ToolPermissionLevel,
}

/// Set permission levels for one or more tools.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/tools/permissions/set", response = SetToolPermissionsResponse)]
#[serde(rename_all = "camelCase")]
pub struct SetToolPermissionsRequest {
    pub tool_permissions: Vec<ToolPermissionEntry>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
pub struct SetToolPermissionsResponse {}

/// Per-model sampling/context profile. Sampling is per MODEL: the engine spawns each
/// mounted model with the flags from ITS profile in `MlxEngineSettingsDto::model_profiles`.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MlxModelProfileDto {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_p: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repetition_penalty: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presence_penalty: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frequency_penalty: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_limit: Option<u32>,
    /// Speculative decoding: `"mtp"` (demand the MTP head, skipped with a warning when the
    /// model dir has no `mtp.safetensors`), `"off"`, or absent = auto (on when the head exists).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speculative: Option<String>,
    /// Directory of an mlx-lm LoRA/DoRA adapter fused at load (`--adapter-path`); `~` expands.
    /// The mount fails, naming the missing file, when it is not one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adapter_path: Option<String>,
    /// `false` lets a vision-bearing checkpoint take the engine's MLLM lane; absent/`true` pins
    /// the text lane (`--text-only`). No effect on a checkpoint that declares no vision.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_only: Option<bool>,
    /// Sent as `chat_template_kwargs.enable_thinking` on every turn a session routes to this
    /// model: `"on"` | `"off"`. Absent = auto — nothing is sent and the engine decides (Rapid-MLX
    /// turns thinking off whenever the request carries tools). Captured once per session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking: Option<MlxThinkingModeDto>,
    /// One of the model's `MlxThinkingCapabilitiesDto::effort_levels`, sent as
    /// `chat_template_kwargs.reasoning_effort`. Absent = the template's own default. Captured once
    /// per session (it rewrites the system prompt, so a mid-session change would void the cache).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    /// Compressed live KV cache, passed as `--kv-cache-dtype`: `"int8"` | `"int4"`. Absent = off
    /// (the engine's bf16 cache, no flag). A change restarts the engine. The mount is refused when
    /// the model's KV cannot take it (goose: no attention layers / no quantization group for its
    /// head_dim; the engine: a cache layout it cannot quantize — both name the reason).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kv_cache: Option<MlxKvCacheModeDto>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum MlxThinkingModeDto {
    On,
    Off,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum MlxKvCacheModeDto {
    Int8,
    Int4,
}

/// What one token of context costs in KV for this model at each cache setting, from its
/// config.json and the engine's packed layout (bits/8 bytes per element + one scale and one bias
/// per group). Only full-attention layers grow with context; linear-attention state is fixed-size.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MlxKvCacheFactsDto {
    pub attention_layers: u64,
    pub state_layers: u64,
    pub sliding_layers: u64,
    pub kv_heads: u64,
    pub head_dim: u64,
    /// The engine's quantization group for this head_dim; absent = none fits, so the model's KV
    /// cannot be compressed (both byte figures below are absent too).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_size: Option<u64>,
    pub bf16_bytes_per_token: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub int8_bytes_per_token: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub int4_bytes_per_token: Option<u64>,
}

/// One setting's measured quality against the bf16 cache on the same prompts, greedy.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MlxKvModeMeasurementDto {
    /// Tokens emitted before the first divergence from bf16, over bf16's tokens (0..1).
    pub agreement: f64,
    pub identical_answers: u32,
    /// The fact buried ~31k tokens deep was answered correctly.
    pub retrieval_found: bool,
    /// Decode tok/s at the longest measured context, over bf16's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decode_tps_ratio: Option<f64>,
}

/// The model directory's `goose-kv-cache.json`, written by evals/mlx-engine-bench/kv_quant_compare.py.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MlxKvCacheMeasurementDto {
    pub measured_at: String,
    pub engine: String,
    pub prompts: u32,
    /// bf16 measured against itself — the floor every setting is read against.
    pub noise_floor: MlxKvModeMeasurementDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub int8: Option<MlxKvModeMeasurementDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub int4: Option<MlxKvModeMeasurementDto>,
    pub source: String,
}

/// What the model's own chat template lets a request steer about reasoning, proven on the
/// template's Jinja AST with the engine's detection rules.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MlxThinkingCapabilitiesDto {
    /// `"enable_thinking"` or `"reasoning"`; absent = the template has no on/off switch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking_switch: Option<String>,
    /// The template's own effort vocabulary in template order; empty = none declared.
    pub effort_levels: Vec<String>,
    /// The level the template renders when a request names none; absent when not provable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_effort: Option<String>,
    /// The template reads `preserve_thinking` (history rows keep their think block).
    pub preserve_thinking: bool,
    /// The template renders a `<think>`…`</think>` span, the shape a reasoning token budget can
    /// force-close (necessary, not sufficient — the engine skips it on tool requests).
    pub budget_forcible: bool,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MlxEngineSettingsDto {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    pub models_dir: String,
    pub port: u16,
    /// LEGACY flat sampling/context fields, accepted inbound for old UI states and
    /// migrated server-side into `model_profiles[model_id]`. Responses carry them
    /// only until that one-time migration has run; profiles are the source of truth.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_limit: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_p: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repetition_penalty: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presence_penalty: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frequency_penalty: Option<f64>,
    /// Swarm-facing model id advertised by the engine (`--served-model-name`); the HF
    /// directory id is served when unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub served_model_name: Option<String>,
    pub spawn_command: Vec<String>,
    /// Per-model sampling/context profiles keyed by HF model id — the source of truth
    /// for the flags each model mounts with.
    #[serde(default)]
    pub model_profiles: BTreeMap<String, MlxModelProfileDto>,
}

/// Live MLX engine state. `state` is one of "stopped" | "mounting" | "running" | "failed".
/// `context_window` / `tool_call_parser` come from a live `/v1/models` probe and are never
/// fabricated: a failed probe leaves them unset and reports `probe_error` instead.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MlxEngineStatusDto {
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_parser: Option<String>,
    /// The id the live engine serves on its API (/v1/models) — the id chat requests must use;
    /// differs from `model_id` (the HF directory) when `served_model_name` aliases it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub served_model_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub probe_error: Option<String>,
    /// Requests the engine has accepted and not finished (`/v1/status` `num_running +
    /// num_waiting`), read on the same probe as `served_model_id`. Absent when the engine did
    /// not report it — never a fabricated 0; `active_requests_error` says why. `> 0` is the
    /// node's BUSY fact for the fleet corroboration; an explicit `0` is idle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_requests: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_requests_error: Option<String>,
    /// The admission cap this goose starts its engine with (`--max-concurrent-requests`) — what
    /// a Link peer routing chat here sizes its node to. Absent from goose builds before it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_concurrent_requests: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate_message: Option<String>,
    /// The last memory-gate verdict for `gate_message`: "allow" | "warn" | "block".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate_verdict: Option<String>,
    /// Something already listens on the configured port while the manager supervises
    /// nothing — an engine orphaned by a previous goosed. Unmount reclaims it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stray_listener_port: Option<u16>,
    /// Memory a mount can take: free pages plus the file cache the OS reclaims on demand
    /// (on macOS, Activity Monitor's physical-minus-used). 0 exactly when `memory_error`
    /// is set.
    pub available_memory_gb: f64,
    pub total_memory_gb: f64,
    /// The part of `available_memory_gb` that is reclaimable file cache. Absent where the
    /// platform does not split it out (Linux folds it into its available figure).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reclaimable_cache_gb: Option<f64>,
    /// The OS memory probe failed; the memory figures are 0 and must not be read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_error: Option<String>,
    /// True when the persisted settings would spawn the running engine differently
    /// (model, port, sampling): the engine keeps running with its old arguments until
    /// the user remounts.
    pub restart_required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MlxLocalModelDto {
    pub id: String,
    pub size_bytes: u64,
    pub complete: bool,
    /// Files provably missing or unfinished — shards the model's safetensors index
    /// names that are absent/empty, plus `.part` leftovers. 0 when complete.
    pub missing_files: u32,
    /// The reasoning controls the model's chat template declares; absent exactly when
    /// `thinking_error` says why (no template in the directory, unreadable, unparseable).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking: Option<MlxThinkingCapabilitiesDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking_error: Option<String>,
    /// KV bytes per token at each cache setting; absent exactly when `kv_cache_error` says why.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kv_cache: Option<MlxKvCacheFactsDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kv_cache_error: Option<String>,
    /// The measured quality of each setting on THIS model; absent with no error = never measured.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kv_cache_measurement: Option<MlxKvCacheMeasurementDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kv_cache_measurement_error: Option<String>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MlxHfModelHitDto {
    pub id: String,
    pub downloads: u64,
    pub likes: u64,
    pub updated_at: String,
}

/// Snapshot download progress. `state` is one of
/// "queued" | "downloading" | "paused" | "done" | "failed" | "cancelled".
/// A cancelled download has no on-disk claim (its partial repo dir is deleted), so a
/// "cancelled" row may simply be dropped from the UI.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MlxDownloadProgressDto {
    pub state: String,
    pub total_bytes: u64,
    pub downloaded_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_file: Option<String>,
    /// Files this attempt restarted from zero because their on-disk `.part` or the
    /// server's range answer disagreed with the repo tree's size.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub restarted_files: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Read the MLX engine's live status, including a `/v1/models` probe when running.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/mlxEngine/status",
    response = MlxEngineStatusResponse
)]
#[serde(rename_all = "camelCase")]
pub struct MlxEngineStatusRequest {
    /// LeanZero Link mesh target. Absent or equal to the local node → runs on THIS node's
    /// MLX engine exactly as before. A peer's `nodeId` forwards the operation over the mesh
    /// to that node's control service, which runs it against ITS local engine and returns
    /// the result. A named peer must be Connected on the mesh, else a loud
    /// `not connected to the mesh` error — never a local fallback.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct MlxEngineStatusResponse {
    pub status: MlxEngineStatusDto,
}

/// Mount a local model into the MLX engine. Returns once mounting has started;
/// poll status for running/failed.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/mlxEngine/mount", response = EmptyResponse)]
#[serde(rename_all = "camelCase")]
pub struct MlxEngineMountRequest {
    pub model_id: String,
    /// Mesh target; see [`MlxEngineStatusRequest::node_id`]. Absent/self → local; a peer
    /// forwards over the mesh.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
}

/// Stop the MLX engine and unmount its model.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/mlxEngine/unmount", response = EmptyResponse)]
#[serde(rename_all = "camelCase")]
pub struct MlxEngineUnmountRequest {
    /// Mesh target; see [`MlxEngineStatusRequest::node_id`]. Absent/self → local; a peer
    /// forwards over the mesh.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
}

/// Read the persisted MLX engine settings.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/mlxEngine/settingsRead",
    response = MlxEngineSettingsResponse
)]
#[serde(rename_all = "camelCase")]
pub struct MlxEngineSettingsReadRequest {
    /// Mesh target; see [`MlxEngineStatusRequest::node_id`]. Absent/self → local; a peer
    /// forwards over the mesh.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct MlxEngineSettingsResponse {
    pub settings: MlxEngineSettingsDto,
}

/// Persist MLX engine settings. A running engine keeps its old arguments; status reports
/// `restartRequired` until the model is remounted.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/mlxEngine/settingsUpdate",
    response = MlxEngineSettingsResponse
)]
#[serde(rename_all = "camelCase")]
pub struct MlxEngineSettingsUpdateRequest {
    pub settings: MlxEngineSettingsDto,
    /// Mesh target; see [`MlxEngineStatusRequest::node_id`]. Absent/self → local; a peer
    /// forwards over the mesh (the settings persist on THAT node).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
}

/// List models present in the configured models dir.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/mlxEngine/modelsList",
    response = MlxEngineModelsListResponse
)]
#[serde(rename_all = "camelCase")]
pub struct MlxEngineModelsListRequest {
    /// Mesh target; see [`MlxEngineStatusRequest::node_id`]. Absent/self → local; a peer
    /// forwards over the mesh (the models + disk space are THAT node's).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct MlxEngineModelsListResponse {
    pub models: Vec<MlxLocalModelDto>,
    /// Free bytes an unprivileged writer can use on the models dir's volume.
    pub disk_available_bytes: u64,
    pub disk_total_bytes: u64,
}

/// Delete a downloaded model from the models dir.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/mlxEngine/modelDelete", response = EmptyResponse)]
#[serde(rename_all = "camelCase")]
pub struct MlxEngineModelDeleteRequest {
    pub model_id: String,
    /// Mesh target; see [`MlxEngineStatusRequest::node_id`]. Absent/self → local; a peer
    /// forwards over the mesh. DESTRUCTIVE remotely too: the model is deleted from THAT
    /// node's disk and the op is logged loudly there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
}

/// Search HuggingFace for MLX models, sorted by downloads.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/mlxEngine/hfSearch",
    response = MlxEngineHfSearchResponse
)]
#[serde(rename_all = "camelCase")]
pub struct MlxEngineHfSearchRequest {
    pub query: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    /// Mesh target; see [`MlxEngineStatusRequest::node_id`]. Absent/self → local; a peer
    /// forwards over the mesh (runs the HF search from THAT node's network/token).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct MlxEngineHfSearchResponse {
    pub hits: Vec<MlxHfModelHitDto>,
}

/// Start a background snapshot download of a HuggingFace repo into the models dir.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/mlxEngine/download", response = EmptyResponse)]
#[serde(rename_all = "camelCase")]
pub struct MlxEngineDownloadRequest {
    pub repo_id: String,
    /// Mesh target; see [`MlxEngineStatusRequest::node_id`]. Absent/self → local; a peer
    /// forwards over the mesh (the download runs on THAT node; poll its progress with the
    /// same `nodeId`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
}

/// Poll download progress for a repo. `progress` is unset when no download was tracked.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/mlxEngine/downloadProgress",
    response = MlxEngineDownloadProgressResponse
)]
#[serde(rename_all = "camelCase")]
pub struct MlxEngineDownloadProgressRequest {
    pub repo_id: String,
    /// Mesh target; see [`MlxEngineStatusRequest::node_id`]. Absent/self → local; a peer
    /// forwards over the mesh (reads progress from THAT node's tracker).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct MlxEngineDownloadProgressResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<MlxDownloadProgressDto>,
}

/// Cancel a download AND delete its on-disk claim: every `.part` and the whole partial
/// repo directory are removed. Works on active and paused/failed downloads; the state
/// becomes "cancelled" once the deletion has run.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/mlxEngine/downloadCancel",
    response = EmptyResponse
)]
#[serde(rename_all = "camelCase")]
pub struct MlxEngineDownloadCancelRequest {
    pub repo_id: String,
    /// Mesh target; see [`MlxEngineStatusRequest::node_id`]. Absent/self → local; a peer
    /// forwards over the mesh. DESTRUCTIVE remotely too: the partial repo is deleted on
    /// THAT node and the op is logged loudly there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
}

/// One MLX browse hit. `quant`/`arch` are DERIVED display fields (the repo's tags
/// first, name patterns as fallback) — they describe the hit, they are not proof the
/// server filtered on them unless the request set the corresponding filter.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MlxBrowseHitDto {
    pub id: String,
    /// The `publisher` prefix of `id`.
    pub author: String,
    pub downloads: u64,
    pub likes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_modified: Option<String>,
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quant: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arch: Option<String>,
    /// Exact repository download bytes; field name retained for wire compatibility.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_bytes_estimate: Option<u64>,
}

/// Paginated MLX-only HuggingFace browse. All filters apply server-side: `query`
/// searches names, `author` restricts the publisher, `quant` ("4-bit", "8-bit", …)
/// and `arch` ("qwen3_5", "llama", …) AND-combine as HF tag filters — so a quant
/// filter cannot see repos whose bit width appears only in the repo NAME. `sort` is
/// "downloads" or "newest" (createdAt descending). Pass `nextCursor` from a response
/// back as `cursor` for the next page; other parameters are baked into it.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/mlxEngine/browse",
    response = MlxEngineBrowseResponse
)]
#[serde(rename_all = "camelCase")]
pub struct MlxEngineBrowseRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quant: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arch: Option<String>,
    /// "downloads" | "newest"
    pub sort: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    /// Page size, default 20, capped at 50.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    /// Mesh target; see [`MlxEngineStatusRequest::node_id`]. Absent/self → local; a peer
    /// forwards over the mesh (browses from THAT node's network/token).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct MlxEngineBrowseResponse {
    pub hits: Vec<MlxBrowseHitDto>,
    /// Opaque continuation for the next page; absent on the last page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

/// Filter vocabularies for the browse UI, aggregated live from HuggingFace (a bounded
/// crawl of MLX listings, cached in-process with a ~1h TTL — the first call pays the
/// crawl, later calls are instant). Values are raw filterable tag/author strings; free
/// text beyond them still passes to the browse filters.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/mlxEngine/browseFilters",
    response = MlxEngineBrowseFiltersResponse
)]
#[serde(rename_all = "camelCase")]
pub struct MlxEngineBrowseFiltersRequest {
    /// Mesh target; see [`MlxEngineStatusRequest::node_id`]. Absent/self → local; a peer
    /// forwards over the mesh.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct MlxEngineBrowseFiltersResponse {
    /// Quant tags observed live ("4-bit", "8-bit", "bf16", "mxfp4", …), ordered by
    /// observed frequency.
    pub quants: Vec<String>,
    /// Architectures (`config.model_type` values) observed live, each also usable as a
    /// server-side tag filter; ordered by observed frequency.
    pub archs: Vec<String>,
    /// Publishers observed live, ordered by observed frequency.
    pub authors: Vec<String>,
    /// Distinct repos the vocabulary was aggregated from — a top-N sample of the MLX
    /// corpus (by downloads plus by newest), not an exhaustive census.
    pub sampled_repos: u32,
    /// Unix epoch seconds when the crawl ran.
    pub computed_at: u64,
    /// Present when this vocabulary is served stale because a TTL refresh failed;
    /// carries the refresh failure.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_error: Option<String>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MlxRepoFileDto {
    pub path: String,
    pub size_bytes: u64,
}

/// Everything the fullscreen model-card modal needs for one repo: README markdown,
/// the file listing with sizes, and repo metadata. A repo without a README yields no
/// `readmeMarkdown` — that is an absent field, not an error.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/mlxEngine/modelCard",
    response = MlxEngineModelCardResponse
)]
#[serde(rename_all = "camelCase")]
pub struct MlxEngineModelCardRequest {
    pub repo_id: String,
    /// Mesh target; see [`MlxEngineStatusRequest::node_id`]. Absent/self → local; a peer
    /// forwards over the mesh.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct MlxEngineModelCardResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub readme_markdown: Option<String>,
    /// True when the README exceeded the 1 MiB cap and was cut.
    pub readme_truncated: bool,
    pub files: Vec<MlxRepoFileDto>,
    /// Exact sum of every file size the repo tree lists.
    pub total_bytes: u64,
    pub tags: Vec<String>,
    pub downloads: u64,
    pub likes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_modified: Option<String>,
}

/// Pause an active download: the task stops cleanly between chunks and every `.part`
/// stays on disk for a later resume. Loud on an unknown or inactive repo.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/mlxEngine/downloadPause",
    response = EmptyResponse
)]
#[serde(rename_all = "camelCase")]
pub struct MlxEngineDownloadPauseRequest {
    pub repo_id: String,
    /// Mesh target; see [`MlxEngineStatusRequest::node_id`]. Absent/self → local; a peer
    /// forwards over the mesh (pauses the download on THAT node).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
}

/// Resume a paused/failed/cancelled download — or partial files left on disk by an
/// earlier session. Complete files are skipped, `.part` files continue via HTTP Range;
/// a file whose partial no longer matches the repo tree restarts from zero and is
/// reported in `restartedFiles`. Loud on an unknown repo with nothing on disk.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/mlxEngine/downloadResume",
    response = EmptyResponse
)]
#[serde(rename_all = "camelCase")]
pub struct MlxEngineDownloadResumeRequest {
    pub repo_id: String,
    /// Mesh target; see [`MlxEngineStatusRequest::node_id`]. Absent/self → local; a peer
    /// forwards over the mesh (resumes the download on THAT node).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
}

// ============================================================================
// Model replication between LeanZero Link nodes.
//
// A model already on one node is copied to a peer over the best DIRECT path the two
// share: a Thunderbolt cable (both ends' TB ports in one subnet) first, else a shared
// LAN subnet. The sender offers the model on a listener bound to that one interface; the
// receiver pulls it file by file (HTTP Range resume, per-file SHA-256 against the
// sender's bytes) into its own models dir. The control messages ride the mesh's
// node-token-authenticated `/v1/swarm/mlx/*` proxy; the bytes never do.
// ============================================================================

/// One IPv4 address on one of a node's interfaces.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MlxNetInterfaceDto {
    pub device: String,
    /// macOS hardware-port name ("Thunderbolt 3", "Wi-Fi"); absent on platforms without one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hardware_port: Option<String>,
    /// "thunderbolt" | "ethernet" | "wifi" | "other"
    pub kind: String,
    pub ipv4: String,
    pub prefix_len: u8,
    /// Negotiated Thunderbolt link speed as the OS reports it ("80 Gb/s"); absent when it
    /// could not be attributed to this port.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link_speed: Option<String>,
}

/// Read a node's interface facts.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/mlxEngine/linkFacts",
    response = MlxEngineLinkFactsResponse
)]
#[serde(rename_all = "camelCase")]
pub struct MlxEngineLinkFactsRequest {
    /// Mesh target; see [`MlxEngineStatusRequest::node_id`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct MlxEngineLinkFactsResponse {
    pub interfaces: Vec<MlxNetInterfaceDto>,
    /// A part of the report that could not be read (the TB link speed), stated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

/// The direct path between two nodes.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MlxReplicaLinkDto {
    /// "thunderbolt" when BOTH ends are Thunderbolt ports in one subnet; otherwise
    /// "network" (a shared Ethernet/Wi-Fi subnet).
    pub kind: String,
    pub local: MlxNetInterfaceDto,
    pub peer: MlxNetInterfaceDto,
}

/// A peer this node could copy a model to.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MlxReplicaTargetDto {
    pub node_id: String,
    pub hostname: String,
    /// The chosen direct path; absent exactly when `unavailable` says why.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<MlxReplicaLinkDto>,
    /// Why no copy can go to this peer now (offline, observe-only, no shared subnet, the
    /// peer's address did not answer).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unavailable: Option<String>,
}

/// The peers a model could be copied to from this node (or from `nodeId`).
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/mlxEngine/replicaTargets",
    response = MlxEngineReplicaTargetsResponse
)]
#[serde(rename_all = "camelCase")]
pub struct MlxEngineReplicaTargetsRequest {
    /// Mesh target; see [`MlxEngineStatusRequest::node_id`]. The targets are computed from
    /// THAT node's viewpoint (its interfaces, its peers).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct MlxEngineReplicaTargetsResponse {
    /// False when this node is not on the mesh: there are no peers, `targets` is empty,
    /// and the Models tab shows no copy action (a single machine looks exactly as before).
    pub mesh_connected: bool,
    pub targets: Vec<MlxReplicaTargetDto>,
    /// A part of THIS node's interface report that could not be read, stated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

/// Copy a complete local model to `targetNodeId`. Returns once the peer has ACCEPTED the
/// pull; poll `replicaProgress` with `nodeId = targetNodeId` for the copy itself.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/mlxEngine/replicate",
    response = MlxEngineReplicateResponse
)]
#[serde(rename_all = "camelCase")]
pub struct MlxEngineReplicateRequest {
    pub model_id: String,
    pub target_node_id: String,
    /// The SENDING node; see [`MlxEngineStatusRequest::node_id`]. Absent/self → this node
    /// sends; a peer's id makes THAT node send its copy to `targetNodeId`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct MlxEngineReplicateResponse {
    pub link: MlxReplicaLinkDto,
    /// The sender's replica listener the peer pulls from (`http://<local address>:<port>`).
    pub source_url: String,
}

/// Node-to-node: pull `modelId` from a sender's replica listener into this node's models
/// dir. Sent by the SENDING node over the mesh proxy after it offered the model; the
/// `offerToken` is the capability for that one offer. Refused while the model is already
/// complete here or a download/copy of it is running.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/mlxEngine/replicaPull",
    response = EmptyResponse
)]
#[serde(rename_all = "camelCase")]
pub struct MlxEngineReplicaPullRequest {
    pub model_id: String,
    pub source_url: String,
    pub offer_token: String,
    pub link: MlxReplicaLinkDto,
    /// Mesh target; see [`MlxEngineStatusRequest::node_id`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
}

/// A copy in progress (or finished) on the RECEIVING node. `state` is one of
/// "queued" | "copying" | "done" | "failed" | "cancelled".
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MlxReplicaProgressDto {
    pub state: String,
    pub source_url: String,
    /// "thunderbolt" | "network"
    pub link: String,
    pub link_detail: String,
    pub total_bytes: u64,
    pub copied_bytes: u64,
    pub files_total: u32,
    pub files_done: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_file: Option<String>,
    /// "transferring" | "verifying" (the file's SHA-256 against the sender's)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    /// Continued from an on-disk `.part` via HTTP Range.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resumed_files: Vec<String>,
    /// Restarted from zero (a `.part` at/past the size, or a refused range).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub restarted_files: Vec<String>,
    /// Already present at full size and verified in place.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skipped_files: Vec<String>,
    /// Bytes that crossed the link in this attempt and the milliseconds spent receiving
    /// them — `wireBytes / wireMillis` is the link's measured rate.
    pub wire_bytes: u64,
    pub wire_millis: u64,
    pub elapsed_millis: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// The copy failed because macOS refused the RECEIVING node's app the local network
    /// (EHOSTUNREACH on the direct path): allowed in System Settings › Privacy & Security ›
    /// Local Network on that Mac.
    #[serde(default)]
    pub local_network_blocked: bool,
    /// The sender's offer could not be released (it lapses when the sender exits).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_error: Option<String>,
}

/// Poll a copy on the receiving node (`nodeId` = that node). `progress` is unset when no
/// copy of the model was tracked there.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/mlxEngine/replicaProgress",
    response = MlxEngineReplicaProgressResponse
)]
#[serde(rename_all = "camelCase")]
pub struct MlxEngineReplicaProgressRequest {
    pub model_id: String,
    /// Mesh target; see [`MlxEngineStatusRequest::node_id`] — the RECEIVING node.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct MlxEngineReplicaProgressResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<MlxReplicaProgressDto>,
}

/// Cancel a running copy on the receiving node and delete its partial model dir.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/mlxEngine/replicaCancel",
    response = EmptyResponse
)]
#[serde(rename_all = "camelCase")]
pub struct MlxEngineReplicaCancelRequest {
    pub model_id: String,
    /// Mesh target; see [`MlxEngineStatusRequest::node_id`] — the RECEIVING node.
    /// DESTRUCTIVE there: the partial copy is deleted and the op is logged loudly.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
}

// ============================================================================
// Distributed MLX engine — one model split across several Macs (this Mac = rank 0).
//
// Methods `_goose/unstable/mlxEngine/distributed*`, local to THIS goosed (no nodeId: the
// distributed engine is supervised by the Mac that is rank 0). Only one engine owns a Mac at a
// time: `distributedStart` is refused with refusal code `singleEngineMounted` while the single
// engine is mounted (the UI offers "unmount and continue"), and `mount` is refused while the
// distributed engine owns the Mac. Enum-like fields are strings; their values are listed on each.
// ============================================================================

/// One node of the distributed engine. `ssh` absent = this Mac, which must be `nodes[0]` (rank 0).
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MlxDistributedNodeConfigDto {
    pub name: String,
    /// The peer's ssh alias (e.g. `workhorse`); absent exactly for this Mac.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssh: Option<String>,
    /// This node's IPv4 on the Thunderbolt /30 (e.g. 192.168.0.1).
    pub tb_ip: String,
    /// The /30's mask (255.255.255.252), re-applied by the link repair.
    pub tb_netmask: String,
    /// The TB interface (e.g. en3).
    pub tb_interface: String,
    /// The macOS network service on that interface (e.g. "EXO Thunderbolt 3"); the ONLY service
    /// the link repair may toggle, and only after proving it sits on a Thunderbolt hardware port.
    pub tb_service: String,
    /// The RDMA device JACCL uses (e.g. rdma_en3); empty under ring, which needs none.
    #[serde(default)]
    pub rdma_device: String,
    /// Absolute path of the interpreter carrying mlx + mlx_lm on this node.
    pub python: String,
    /// Absolute path of the fork's interpreter (`rapid_mlx`); only the qwen4_exp runner uses it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pipeline_python: Option<String>,
    /// Absolute path of the model directory ON THIS NODE (paths may differ per node).
    pub model_dir: String,
    /// "Free memory automatically": when a preflight finds this node short, goose compacts it
    /// (asks macOS to reclaim memory) and preflights again. Absent reads as ON — the default, so a
    /// config saved before the switch existed keeps it on; goose always sends it explicitly.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub free_memory_automatically: Option<bool>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MlxDistributedConfigDto {
    /// The id the engine serves on `/v1/models` — the id chat requests use.
    pub model_id: String,
    /// "jaccl" (RDMA over TB5) | "ring" (TCP over the TB /30).
    pub backend: String,
    /// The OpenAI API port on this Mac (loopback). Refused when equal to the single engine's port.
    pub port: u16,
    /// JACCL coordinator port on rank 0's TB IP; ring uses it + rank on each node. Must sit below
    /// each node's ephemeral range (preflight check `portRange`).
    pub coordinator_port: u16,
    /// The context to allow; absent = derived (the largest every rank fits, capped at the model's).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<u64>,
    /// Pipeline runner (qwen4_exp) only: the full-context sequences the split is planned and
    /// KV-budgeted for — the most requests one batch runs. Absent = 2. The tensor runner has none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slots: Option<u32>,
    /// Restart automatically after a rank death or a hang (a breaker still applies; a memory
    /// watchdog CRITICAL stop never restarts).
    #[serde(default)]
    pub restart_on_failure: bool,
    /// Diagnostic: skip the ps-stat-T fast path so a stopped rank is caught only by the
    /// progress-ratio hang rule. Default false.
    #[serde(default)]
    pub hang_ratio_only: bool,
    /// The memory watchdog's WARN reserve as a fraction of each node's RAM (available below it →
    /// admission closes, 503). Absent = 0.05.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub watchdog_warn_ratio: Option<f64>,
    /// The CRITICAL reserve (available below it → verified stop, never restarted). Absent = 0.02.
    /// Must satisfy 0 < critical < warn < 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub watchdog_critical_ratio: Option<f64>,
    pub nodes: Vec<MlxDistributedNodeConfigDto>,
}

/// One preflight check. `verdict`: "pass" | "warn" | "fail". `id`: "reachable" |
/// "foreignEngines" | "memory" | "model" | "modelManifest" | "python" | "tbIpv4" | "ping" |
/// "rdmaGid" | "linkRepair" | "portRange" | "ports" | "runner" | "plan". `message` carries the numbers.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MlxDistributedCheckDto {
    pub id: String,
    pub verdict: String,
    pub message: String,
}

/// What one rank will hold, in bytes. Tensor split: every layer's shard (`shardIndex` of
/// `shardCount`, layers [layerStart, layerEnd) = all). Pipeline split: layers [layerStart, layerEnd).
/// Tensor split: `withOverheadBytes` = `plannedBytes` × the measured runtime-overhead ratio (1.10),
/// `fits` = `withOverheadBytes` ≤ `budgetBytes` = min(available × 0.90, RAM × 0.75). Pipeline split:
/// the fork planner's figures verbatim — `withOverheadBytes` = `plannedBytes` (no multiplier),
/// `budgetBytes` = min(available − RAM × 0.21, RAM × 0.75), `fits` is the planner's verdict; the
/// state and workspace bytes are for the preflight's `slots` full-context sequences.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MlxDistributedRankPlanDto {
    pub layer_start: u32,
    pub layer_end: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shard_index: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shard_count: Option<u32>,
    pub weights_bytes: u64,
    pub state_bytes: u64,
    pub workspace_bytes: u64,
    pub prompt_cache_bytes: u64,
    pub planned_bytes: u64,
    pub with_overhead_bytes: u64,
    pub budget_bytes: u64,
    pub fits: bool,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MlxDistributedNodePreflightDto {
    pub name: String,
    pub rank: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    pub checks: Vec<MlxDistributedCheckDto>,
    /// Measured available memory ((free − speculative) + file-backed + purgeable pages); absent
    /// exactly when the `memory` check says the probe failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub available_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_bytes: Option<u64>,
    /// Kernel memory pressure: "normal" | "warn" | "critical".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pressure: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<MlxDistributedRankPlanDto>,
    /// The TB receptacle's live speed (e.g. "80 Gb/s"); absent when not reported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link_speed: Option<String>,
    /// e.g. "mlx 0.32.2 · mlx_lm 0.31.3".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mlx_version: Option<String>,
    /// The node's GPU ceiling: Metal's `max_recommended_working_set_size`, read on the node. The
    /// budget is min(available − RAM × availableMargin, this). Absent exactly when a `gpuCeiling`
    /// FAIL says why it could not be read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ceiling_bytes: Option<u64>,
    /// `sysctl iogpu.wired_limit_mb` on the node (0 = macOS's default wired ceiling).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wired_limit_mb: Option<u64>,
    /// How far this rank's plan exceeds its budget; absent when it fits or has no plan.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub short_bytes: Option<u64>,
    /// The node's biggest apps by resident memory (helpers folded into their app; goose left out),
    /// largest first — what the owner could close when compaction is off or not enough.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub top_apps: Vec<MlxDistributedAppMemoryDto>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MlxDistributedAppMemoryDto {
    pub name: String,
    pub rss_bytes: u64,
}

/// One node's latest memory compaction ("Make room"): goose raised memory pressure to the
/// kernel's WARN with Apple's `memory_pressure`, released it at once, and sampled until available
/// memory stopped rising. Nothing is quit; macOS compresses idle apps and apps drop caches.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MlxDistributedCompactionDto {
    pub node: String,
    pub at_ms: u64,
    /// "automatic" (a preflight found the node short) | "manual" (Make room).
    pub trigger: String,
    /// "compacted" | "refused" | "failed".
    pub outcome: String,
    /// Refusals only: "engineLoaded" (an MLX engine runs on the node — never pressured) |
    /// "notNormal" (kernel pressure already WARN/CRITICAL).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    /// The one-line account: freed/lost GiB, before → settled, the kernel's WARN point; or the
    /// refusal / failure reason.
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before_available_bytes: Option<u64>,
    /// Available memory when the kernel raised WARN (the ballast was released right then).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peak_available_bytes: Option<u64>,
    /// Available memory once it stopped rising after the release.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settled_available_bytes: Option<u64>,
    /// settled − before; negative when the node ended with less.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gained_bytes: Option<i64>,
    /// How the pressure phase ended: "warn" | "critical" (released at once) | "toolExited".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settle_samples: Option<u32>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MlxDistributedPreflightDto {
    /// Every check passed (warns allowed) and every rank has a plan.
    pub ok: bool,
    pub ran_at_ms: u64,
    /// "jaccl" | "ring".
    pub backend: String,
    /// "mlxLmTensor" (qwen3_5) | "pipelineQwen4" (qwen4_exp); absent when the model type has none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runner: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_type: Option<String>,
    /// The context this launch allows (goose-side bookkeeping — the chat context limit).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_limit: Option<u64>,
    /// "requested" | "derived".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_source: Option<String>,
    /// The largest context every rank fits on the measured memory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_context_fits: Option<u64>,
    /// Pipeline only: the full-context sequences every rank's plan was made for (the same on
    /// every rank). Absent for the tensor runner, which has no slots.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slots: Option<u32>,
    /// Cluster-wide checks (runner, cross-node versions, the plan).
    pub checks: Vec<MlxDistributedCheckDto>,
    pub nodes: Vec<MlxDistributedNodePreflightDto>,
    /// Thunderbolt link repairs performed (each names the service and the before/after).
    pub repairs: Vec<String>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MlxDistributedLinkDto {
    /// "jaccl" | "ring".
    pub backend: String,
    pub tb_ip: String,
    pub interface: String,
    /// e.g. "80 Gb/s"; absent when the node did not report it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed: Option<String>,
}

/// One node of the running (or last) distributed engine. Memory figures are GiB.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MlxDistributedNodeStatusDto {
    pub name: String,
    pub rank: u32,
    /// "coordinator" (rank 0, serves HTTP) | "worker".
    pub role: String,
    /// The ssh alias; absent for this Mac.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    /// "preflight" | "loading" | "ready" | "serving" | "failed" | "stopped".
    pub state: String,
    /// The rank's own pid on its node.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer_start: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer_end: Option<u32>,
    /// Tensor split only: this rank's shard of `shardCount`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shard_index: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shard_count: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub available_memory_gb: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_memory_gb: Option<f64>,
    /// "normal" | "warn" | "critical".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pressure: Option<String>,
    /// The watchdog could not sample this node on the last poll (the figures above are stale).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_error: Option<String>,
    /// MLX's own counters on the rank (`mx.get_active_memory` / the highest `get_peak_memory` seen).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_memory_gb: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peak_memory_gb: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub planned_memory_gb: Option<f64>,
    /// The in-process caps the rank reported applying: `mx.set_memory_limit` (0.75 × RAM),
    /// `mx.set_wired_limit` (min(0.60 × RAM, the GPU's recommended working set)) and
    /// `mx.set_cache_limit`. Absent until the rank reported them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_limit_gb: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wired_limit_gb: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_limit_gb: Option<f64>,
    /// Pipeline only, from rank 0's `/v1/status`: the KV/state + workspace the requests in flight
    /// hold on this rank (0 when idle) and this rank's budget for them (its planned state +
    /// workspace for `slots` sequences). Absent for the tensor runner or when the poll failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kv_reserved_gb: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kv_budget_gb: Option<f64>,
    pub link: MlxDistributedLinkDto,
}

/// A supervisor event. `kind`: "preflight" | "linkRepaired" | "launched" | "ready" |
/// "startFailed" | "rankDied" | "rankFrozen" | "hang" | "streamWithoutDone" | "restart" |
/// "breakerOpen" | "watchdogWarn" | "watchdogCritical" | "watchdogBlind" | "admissionClosed" |
/// "admissionOpened" | "stopRequested" | "stopped" | "orphanReclaimed".
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MlxDistributedEventDto {
    pub at_ms: u64,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node: Option<String>,
    pub message: String,
}

/// The supervisor's liveness measure: progress intervals sampled so far, their running median,
/// the hang bound (10 × median; absent until 3 samples exist) and how long progress has been silent.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MlxDistributedLivenessDto {
    pub samples: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub median_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bound_ms: Option<u64>,
    pub silent_ms: u64,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MlxDistributedStatusDto {
    /// Which engine owns this Mac: "single" | "distributed".
    pub mode: String,
    /// "stopped" | "preflight" | "starting" | "ready" | "serving" | "failed" | "stopping".
    pub state: String,
    /// "jaccl" | "ring".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend: Option<String>,
    /// "mlxLmTensor" | "pipelineQwen4".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runner: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    /// The id the ranks serve on `/v1/models` — derived like the single engine's
    /// `--served-model-name` (`mlx_engine.served_model_name`, else `model_id`); the id a swarm
    /// node's `model_id` must equal. Absent while nothing was started in this goosed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub served_model_id: Option<String>,
    /// The distributed engine's OpenAI base URL (goose's `omlx` provider targets it while
    /// `mode` = "distributed").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_limit: Option<u64>,
    /// False while the memory watchdog holds new requests out (WARN on some node).
    pub admission_open: bool,
    /// Requests the engine has accepted and not finished; absent when not measured.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inflight: Option<u32>,
    /// Requests queued behind the running batch (rank 0's `/v1/status` `num_waiting`; the pipeline
    /// holds a request whose KV would overrun some rank's budget — FIFO, never dropped). Absent
    /// before the first poll or when it failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waiting: Option<u32>,
    /// Pipeline only: the full-context sequences the split was planned for ("slots in use /
    /// slots"). Absent for the tensor runner, before the first poll, or when it failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slots: Option<u32>,
    /// Pipeline only: the worst rank's KV reservation in slot units (rounded up).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slots_in_use: Option<u32>,
    /// Pipeline only: sequences in the running batch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sequences_in_flight: Option<u32>,
    /// Why the last `/v1/status` poll failed or broke its contract; absent when it answered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_status_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub liveness: Option<MlxDistributedLivenessDto>,
    pub nodes: Vec<MlxDistributedNodeStatusDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_preflight: Option<MlxDistributedPreflightDto>,
    /// Supervisor events, oldest first (bounded).
    pub events: Vec<MlxDistributedEventDto>,
    pub restarts: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    /// The running config, else the persisted one (`mlx_distributed` config key).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config: Option<MlxDistributedConfigDto>,
    /// The last (or running) provisioning of the nodes' goose-managed Python.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provision: Option<MlxDistributedProvisionDto>,
    /// Set when ANOTHER goosed on this Mac (another desktop window) published a distributed run
    /// and this one supervises none: that run is read-only here — start and stop are refused with
    /// `ownedByAnotherWindow`, and `mode`/`state` above stay this goosed's own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<MlxDistributedOwnerDto>,
    /// Set while THIS Mac serves a rank of another Mac's distributed engine over LeanZero Link
    /// ("Rank 1 of <requester>'s distributed engine · <model> · JACCL"); its single engine and its
    /// own distributed engine are refused meanwhile.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hosting: Option<MlxDistributedHostedRankDto>,
    /// This Mac's switch "Allow this Mac to serve as a distributed node" (config key
    /// `LEANZERO_LINK_ALLOW_DISTRIBUTED_NODE`, off by default), as the control route reads it.
    #[serde(default)]
    pub allow_distributed_node: bool,
    /// The latest memory compaction per node (automatic or Make room), newest last.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub compactions: Vec<MlxDistributedCompactionDto>,
}

/// Another goosed's distributed run, from the record it published under the goose state dir.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MlxDistributedOwnerDto {
    /// "answering" (its /v1/models lists `servedModelId`) | "notAnswering" (its goosed is alive,
    /// the engine does not answer — loading, or the run failed) | "stale" (the goosed that wrote
    /// the record is gone; the record is ignored) | "unreadable" (`detail` says why).
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub served_model_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend: Option<String>,
    #[serde(default)]
    pub node_names: Vec<String>,
    /// Why the engine is not answering, or why the record could not be read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// Read the distributed engine's status (this Mac's supervisor).
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/mlxEngine/distributedStatus",
    response = MlxEngineDistributedStatusResponse
)]
#[serde(rename_all = "camelCase")]
pub struct MlxEngineDistributedStatusRequest {}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct MlxEngineDistributedStatusResponse {
    pub status: MlxDistributedStatusDto,
}

/// Dry run: every preflight check on every node, no launch. `config` absent = the persisted one.
/// `repairLink` = perform the documented TB repair (toggle the configured TB service + re-apply
/// its manual IP) when a JACCL GID/IPv4 check fails; default false (a dry run changes nothing).
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/mlxEngine/distributedPreflight",
    response = MlxEngineDistributedPreflightResponse
)]
#[serde(rename_all = "camelCase")]
pub struct MlxEngineDistributedPreflightRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config: Option<MlxDistributedConfigDto>,
    #[serde(default)]
    pub repair_link: bool,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct MlxEngineDistributedPreflightResponse {
    pub preflight: MlxDistributedPreflightDto,
}

/// Why a start was refused. `code`: "singleEngineMounted" (unmount the single engine, then start
/// again — the UI's "unmount and continue") | "alreadyRunning" | "preflightFailed".
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MlxDistributedRefusalDto {
    pub code: String,
    pub message: String,
}

/// Preflight (repairing the TB link when a JACCL check fails), then launch under supervision.
/// Returns once launching has begun (`started: true`) — poll `distributedStatus` for
/// ready/serving/failed — or with `started: false` and a `refusal`. `config` absent = the
/// persisted one; a given config is persisted.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/mlxEngine/distributedStart",
    response = MlxEngineDistributedStartResponse
)]
#[serde(rename_all = "camelCase")]
pub struct MlxEngineDistributedStartRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config: Option<MlxDistributedConfigDto>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct MlxEngineDistributedStartResponse {
    pub started: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refusal: Option<MlxDistributedRefusalDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preflight: Option<MlxDistributedPreflightDto>,
}

/// The verified stop: each step (what was signalled, per pid, and what was observed).
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MlxDistributedStopReportDto {
    pub steps: Vec<String>,
    /// Every rank's own pid was observed gone (the peers' over ssh).
    pub verified: bool,
}

/// Stop the distributed engine: SIGTERM rank 0, then each peer rank's pid verified gone over ssh
/// (SIGTERM, then SIGKILL, per pid). With nothing supervised, the configured nodes are swept for
/// goose ranks a previous goosed left behind. Returns after the stop completed.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/mlxEngine/distributedStop",
    response = MlxEngineDistributedStopResponse
)]
#[serde(rename_all = "camelCase")]
pub struct MlxEngineDistributedStopRequest {}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct MlxEngineDistributedStopResponse {
    pub stop: MlxDistributedStopReportDto,
    pub status: MlxDistributedStatusDto,
}

/// "Make room" on one configured node: compact its memory now (refused while the distributed
/// engine runs, or beside any MLX engine on that node). Returns after the settle.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/mlxEngine/distributedMakeRoom",
    response = MlxEngineDistributedMakeRoomResponse
)]
#[serde(rename_all = "camelCase")]
pub struct MlxEngineDistributedMakeRoomRequest {
    /// The node's configured `name`.
    pub node: String,
    /// Absent = the saved config.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config: Option<MlxDistributedConfigDto>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct MlxEngineDistributedMakeRoomResponse {
    pub compaction: MlxDistributedCompactionDto,
    pub status: MlxDistributedStatusDto,
}

/// Persist the distributed config without starting (validated first).
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/mlxEngine/distributedConfigUpdate",
    response = MlxEngineDistributedConfigResponse
)]
#[serde(rename_all = "camelCase")]
pub struct MlxEngineDistributedConfigUpdateRequest {
    pub config: MlxDistributedConfigDto,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct MlxEngineDistributedConfigResponse {
    pub config: MlxDistributedConfigDto,
}

/// One ssh alias from `~/.ssh/config` (a `Host` line without wildcards) and whether it answered a
/// non-interactive ssh (`BatchMode=yes`) just now.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MlxDistributedPeerCandidateDto {
    pub alias: String,
    pub answered: bool,
    /// The peer's `hostname -s` when it answered, else ssh's own error.
    pub detail: String,
}

/// The peers this Mac could reach: every non-wildcard `Host` alias in `~/.ssh/config`, probed.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/mlxEngine/distributedPeerCandidates",
    response = MlxEngineDistributedPeerCandidatesResponse
)]
#[serde(rename_all = "camelCase")]
pub struct MlxEngineDistributedPeerCandidatesRequest {}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct MlxEngineDistributedPeerCandidatesResponse {
    pub candidates: Vec<MlxDistributedPeerCandidateDto>,
    /// The file read (absent `~/.ssh/config` = no candidates, and this says so).
    pub source: String,
    /// The same-account Macs on LeanZero Link, offered FIRST: each answered goose's discovery
    /// probe over the mesh (its own goosed ran it), or says why not.
    #[serde(default)]
    pub link: MlxDistributedLinkDiscoveryDto,
}

/// LeanZero Link's side of the peer candidates.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MlxDistributedLinkDiscoveryDto {
    /// "connected" (the peers below are the mesh's) | "notConnected" (this Mac is not signed in
    /// / not connected — `detail` says which; no Link peer can be offered) | "unavailable" (this
    /// goosed runs no Link).
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default)]
    pub peers: Vec<MlxDistributedLinkPeerDto>,
}

/// One Thunderbolt port a Link peer reported (LeanZero Link's path detector over its own
/// `ifconfig` / hardware ports / `system_profiler`).
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MlxDistributedLinkPortDto {
    pub device: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hardware_port: Option<String>,
    pub ipv4: String,
    pub prefix_len: u8,
    /// The negotiated speed ("80 Gb/s") when the OS attributes one to this port.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed: Option<String>,
}

/// One RDMA device a Link peer reported (`ibv_devinfo -v`).
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MlxDistributedLinkRdmaDto {
    pub device: String,
    /// Its first port is `PORT_ACTIVE`.
    pub active: bool,
    /// The GID index holding an IPv4-mapped address (JACCL needs one; the soak's rule is 1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ipv4_gid_index: Option<u32>,
}

/// One model directory on a Link peer (a `config.json` with a `model_type`).
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MlxDistributedLinkPeerModelDto {
    pub dir: String,
    pub model_type: String,
    /// The loaded files' bytes (safetensors).
    pub weights_bytes: u64,
}

/// A same-account Mac on LeanZero Link, as its own goosed described itself.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MlxDistributedLinkPeerDto {
    pub node_id: String,
    pub hostname: String,
    /// The host `distributedDiscover` takes for this Mac (`link:<nodeId>`).
    pub host: String,
    /// "ready" (it answered the probe) | "servingDisabled" (its owner's switch "Allow this Mac
    /// to serve as a distributed node" is off) | "notServed" (its goose predates or lacks the
    /// node side) | "offline" (the mesh reports it offline) | "unreachable" | "unreadable" (it
    /// answered, the probe did not parse). `detail` carries the words.
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// Its ComputerName.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub available_bytes: Option<u64>,
    /// "normal" | "warn" | "critical".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pressure: Option<String>,
    #[serde(default)]
    pub thunderbolt: Vec<MlxDistributedLinkPortDto>,
    #[serde(default)]
    pub rdma: Vec<MlxDistributedLinkRdmaDto>,
    #[serde(default)]
    pub models: Vec<MlxDistributedLinkPeerModelDto>,
}

/// The rank THIS Mac serves for another Mac's distributed engine over LeanZero Link.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MlxDistributedHostedRankDto {
    pub rank: u32,
    pub size: u32,
    /// The requesting Mac's ComputerName, node id and hostname.
    pub requester_name: String,
    pub requester_node_id: String,
    pub requester_hostname: String,
    pub model_id: String,
    pub served_model_id: String,
    /// "jaccl" | "ring".
    pub backend: String,
    /// "mlxLmTensor" | "pipelineQwen4".
    pub runner: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    /// "loading" | "serving".
    pub state: String,
    pub started_ms: u64,
    pub last_poll_ms: u64,
}

/// Where one discovered value came from. `node` absent = a config-level field (`modelId`,
/// `backend`, `port`, `coordinatorPort`); else the rank, and `field` is the node field's wire name
/// (`name`, `tbIp`, `tbNetmask`, `tbInterface`, `tbService`, `rdmaDevice`, `python`,
/// `pipelinePython`, `modelDir`).
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MlxDistributedEvidenceDto {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node: Option<u32>,
    pub field: String,
    pub value: String,
    pub evidence: String,
}

/// A field discovery could NOT fill, and why. The config carries it empty — never a guess.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MlxDistributedGapDto {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node: Option<u32>,
    pub field: String,
    pub reason: String,
}

/// A node's goose-managed Python. `state`: "ready" (imports the pinned mlx + mlx_lm) | "absent"
/// (not provisioned yet — Save provisions it) | "broken" (present, wrong or failing import) |
/// "noUv" (absent and the node has no uv to build it).
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MlxDistributedDiscoveredEnvDto {
    pub python: String,
    pub state: String,
    pub detail: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MlxDistributedDiscoveredNodeDto {
    pub rank: u32,
    pub name: String,
    /// The ssh alias; absent for this Mac.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    pub reachable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub home: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub available_bytes: Option<u64>,
    /// "normal" | "warn" | "critical".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pressure: Option<String>,
    /// `path · version` of the uv that provisions this node.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uv: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<MlxDistributedDiscoveredEnvDto>,
    /// The Thunderbolt receptacle's negotiated speed ("80 Gb/s").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link_speed: Option<String>,
}

/// One model's presence on one node. `state`: "match" (same loaded files, same sizes as rank 0's)
/// | "differs" (a directory of that name whose files differ) | "absent".
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MlxDistributedDiscoveredModelNodeDto {
    pub rank: u32,
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dir: Option<String>,
    pub detail: String,
}

/// A splittable model on this Mac (a `model_type` a runner exists for) and where it is on every
/// other node. `manifest`: "agree" (SHA256SUMS identical on every node) | "differ" | "absent" (no
/// manifest on rank 0; files compared by name and size).
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MlxDistributedDiscoveredModelDto {
    pub id: String,
    pub model_type: String,
    /// "mlxLmTensor" | "pipelineQwen4".
    pub runner: String,
    /// What a rank loads on this Mac (config, tokenizer, index, model*.safetensors), in bytes.
    pub weights_bytes: u64,
    pub on_every_node: bool,
    pub manifest: String,
    pub nodes: Vec<MlxDistributedDiscoveredModelNodeDto>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MlxDistributedDiscoveryDto {
    /// The config, filled from evidence. A field listed in `gaps` is empty here.
    pub config: MlxDistributedConfigDto,
    /// Why this backend: JACCL needs RDMA on both ends of a Thunderbolt link, else ring.
    pub backend_reason: String,
    pub evidence: Vec<MlxDistributedEvidenceDto>,
    pub gaps: Vec<MlxDistributedGapDto>,
    pub nodes: Vec<MlxDistributedDiscoveredNodeDto>,
    pub models: Vec<MlxDistributedDiscoveredModelDto>,
    /// Wall time of the probe (every node in parallel), ms.
    pub probe_ms: u64,
}

/// Probe this Mac locally and each peer over ssh (one script each) and return a filled config:
/// names, the Thunderbolt link (interface, IPv4, netmask, network service — the LeanZero Link path
/// detector), RDMA devices and their IPv4-mapped GID, the backend with its reason, the models on
/// every node, free ports, memory and each node's goose-managed Python. Read-only.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/mlxEngine/distributedDiscover",
    response = MlxEngineDistributedDiscoverResponse
)]
#[serde(rename_all = "camelCase")]
pub struct MlxEngineDistributedDiscoverRequest {
    /// ssh aliases of the peers, in rank order (rank 0 is this Mac).
    pub peers: Vec<String>,
    /// Prefer this model when it is on every node.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct MlxEngineDistributedDiscoverResponse {
    pub discovery: MlxDistributedDiscoveryDto,
}

/// One node's provisioning. `state`: "running" | "done" | "failed" | "skipped" (its Python is the
/// operator's own, set under Advanced — goose never touches it). `step`: the last `GOOSE_PROV`
/// step (check | uv | venv | install | done | fail).
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MlxDistributedProvisionNodeDto {
    pub rank: u32,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    pub python: String,
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<String>,
    pub detail: String,
    /// Every line the node's script printed, in order.
    pub lines: Vec<String>,
    pub started_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_ms: Option<u64>,
}

/// `state`: "running" | "done" (every node done or skipped) | "failed" (a node failed).
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MlxDistributedProvisionDto {
    pub state: String,
    pub started_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_ms: Option<u64>,
    pub nodes: Vec<MlxDistributedProvisionNodeDto>,
}

/// Build each node's goose-managed Python (uv venv, pinned mlx + mlx_lm; the fork too when a node
/// carries a managed `pipelinePython`), idempotently, in the background. Progress rides
/// `distributedStatus.provision`. `config` absent = the persisted one. Refused while a
/// provisioning run is in flight.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/mlxEngine/distributedProvision",
    response = MlxEngineDistributedProvisionResponse
)]
#[serde(rename_all = "camelCase")]
pub struct MlxEngineDistributedProvisionRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config: Option<MlxDistributedConfigDto>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct MlxEngineDistributedProvisionResponse {
    pub provision: MlxDistributedProvisionDto,
}

// ============================================================================
// Remote single: the single MLX engine on ANOTHER Mac, this Mac's chat routed to it through
// LeanZero Link's inference proxy (the placement planner's "<Mac> alone" when <Mac> is a peer).
// ============================================================================

/// Why a remote-single start did not happen — one named code the caller acts on, and the
/// message to show verbatim. Codes: `linkNotConnected` · `unknownPeer` · `chatServingDisabled`
/// (the peer's "Allow this Mac to serve chat to linked devices" is off) ·
/// `remoteManagementDisabled` (the peer does not let linked devices mount models) ·
/// `peerTooOld` (its goose has no chat proxy / reports no admission cap) · `peerMountFailed`
/// (the peer's own mount refusal, e.g. its memory gate) · `distributedOwnsThisMac` ·
/// `remoteSingleActive` (a route to a different peer or model is up — stop it first).
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MlxRemoteSingleRefusalDto {
    pub code: String,
    pub message: String,
}

/// The remote-single route of THIS goosed. `state`: `off` (no route) · `mounting` (the peer is
/// loading the model) · `ready` (the peer's engine serves `servedModelId` through the proxy —
/// chat goes there) · `failed` (the peer's engine failed or went away; `lastError` says why).
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MlxRemoteSingleStatusDto {
    pub state: String,
    /// The peer as the caller named it (a Link `nodeId`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peer: Option<String>,
    /// The peer's hostname as the mesh reports it — what "Serving from <peer>" shows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peer_hostname: Option<String>,
    /// The HF directory id mounted on the peer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    /// `served_model_id` over the PEER's saved settings — the id chat requests carry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub served_model_id: Option<String>,
    /// The peer's admission cap (its `maxConcurrentRequests`) — the router node's capacity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capacity: Option<u32>,
    /// The peer engine's live in-flight count (`/v1/status` through the proxy). Absent when it
    /// did not answer — never a fabricated 0; `activeRequestsError` says why.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_requests: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_requests_error: Option<String>,
    /// The peer engine's own decode rate over its last generation (`generation_tps`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation_tps: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

/// Mount `modelId` on Link peer `peer` (the existing `mlxEngine/mount` over the mesh — the
/// peer's memory gate decides) and route this Mac's MLX chat to it through the peer's chat
/// proxy. Returns once the mount is accepted (`state: mounting`) or the engine already serves
/// it (`state: ready`); poll `remoteSingleStatus` for `ready`. Idempotent for the same
/// peer + model. While a route is up, the swarm router's MLX node is the peer's engine
/// (`remote-<peerHostname>`) and this Mac's own sidecar node is not a candidate.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/mlxEngine/remoteSingleStart",
    response = MlxEngineRemoteSingleStartResponse
)]
#[serde(rename_all = "camelCase")]
pub struct MlxEngineRemoteSingleStartRequest {
    pub peer: String,
    pub model_id: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct MlxEngineRemoteSingleStartResponse {
    pub started: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refusal: Option<MlxRemoteSingleRefusalDto>,
    pub status: MlxRemoteSingleStatusDto,
}

/// Drop the route (chat returns to this Mac's own engine) and, unless `keepMounted`, unmount
/// the model on the peer. `unmounted` reports the peer's answer; `unmountError` its failure
/// verbatim (the route is dropped either way).
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/mlxEngine/remoteSingleStop",
    response = MlxEngineRemoteSingleStopResponse
)]
#[serde(rename_all = "camelCase")]
pub struct MlxEngineRemoteSingleStopRequest {
    #[serde(default)]
    pub keep_mounted: bool,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct MlxEngineRemoteSingleStopResponse {
    pub unmounted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unmount_error: Option<String>,
    pub status: MlxRemoteSingleStatusDto,
}

/// The route's state, re-probed through the proxy on every call.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/mlxEngine/remoteSingleStatus",
    response = MlxEngineRemoteSingleStatusResponse
)]
#[serde(rename_all = "camelCase")]
pub struct MlxEngineRemoteSingleStatusRequest {}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct MlxEngineRemoteSingleStatusResponse {
    pub status: MlxRemoteSingleStatusDto,
}

// ============================================================================
// LeanZero Link — passwordless account identity + a goose-owned Tailscale mesh.
//
// Method namespace: `_goose/unstable/leanzeroLink/*`. These camelCase DTOs are the
// desktop's contract for the Link tab. The cross-node wire types carried by
// `leanzeroLink/nodes` (`NodeState`) are a SEPARATE, snake_case `/v1/swarm/*`
// contract shared with peer nodes and the iOS companion app, and are passed
// through verbatim as JSON (see `LeanzeroLinkNodesResponse`).
// ============================================================================

/// Which auth-worker capabilities the deployment has configured (from env presence).
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LeanzeroLinkCapabilitiesDto {
    pub mail: bool,
    pub audience: bool,
    pub mesh: bool,
}

/// `GET /v1/health` on the auth worker — what the deployment supports.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/leanzeroLink/health",
    response = LeanzeroLinkHealthResponse
)]
#[serde(rename_all = "camelCase")]
pub struct LeanzeroLinkHealthRequest {}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct LeanzeroLinkHealthResponse {
    pub ok: bool,
    pub version: String,
    pub capabilities: LeanzeroLinkCapabilitiesDto,
}

/// Request an email OTP → `codeSent`.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/leanzeroLink/requestCode",
    response = LeanzeroLinkRequestCodeResponse
)]
#[serde(rename_all = "camelCase")]
pub struct LeanzeroLinkRequestCodeRequest {
    pub email: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct LeanzeroLinkRequestCodeResponse {
    /// The worker-normalized email the code was sent to.
    pub email: String,
    pub expires_in_seconds: u64,
}

/// Verify the OTP → persist identity, `loggedIn`.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/leanzeroLink/verify",
    response = LeanzeroLinkVerifyResponse
)]
#[serde(rename_all = "camelCase")]
pub struct LeanzeroLinkVerifyRequest {
    pub email: String,
    pub code: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct LeanzeroLinkVerifyResponse {
    /// The auth state after a successful verify (`"loggedIn"`).
    pub state: String,
    /// The worker-normalized account email.
    pub email: String,
    /// Worker contact-sync verdict: `"synced" | "skipped" | "failed"`.
    pub audience_sync: String,
}

/// Bring up the mesh + control service (requires a verified identity).
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/leanzeroLink/connect",
    response = LeanzeroLinkStateResponse
)]
#[serde(rename_all = "camelCase")]
pub struct LeanzeroLinkConnectRequest {}

/// The composed auth + mesh state.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/leanzeroLink/status",
    response = LeanzeroLinkStateResponse
)]
#[serde(rename_all = "camelCase")]
pub struct LeanzeroLinkStatusRequest {}

/// Tear down the connection, clear the stored identity, drop to `loggedOut`.
/// `wipe` also removes the mesh state dir (slower re-login).
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/leanzeroLink/logout",
    response = LeanzeroLinkStateResponse
)]
#[serde(rename_all = "camelCase")]
pub struct LeanzeroLinkLogoutRequest {
    #[serde(default)]
    pub wipe: bool,
}

/// The auth lifecycle state. Internally tagged on `state`:
/// `loggedOut | codeSent | loggedIn | connecting | connected`.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum LeanzeroLinkAuthStateDto {
    #[default]
    LoggedOut,
    CodeSent {
        email: String,
        #[serde(rename = "expiresAt")]
        expires_at: String,
    },
    LoggedIn {
        email: String,
    },
    Connecting {
        email: String,
    },
    Connected {
        email: String,
        #[serde(rename = "meshIp")]
        mesh_ip: String,
    },
}

/// One raw Tailscale peer seen by the local mesh daemon (status-panel diagnostics —
/// distinct from a swarm `NodeState`).
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LeanzeroLinkMeshPeerDto {
    pub hostname: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ip: Option<String>,
    pub online: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_seen: Option<String>,
}

/// Live mesh status from the goose-owned tailscaled.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LeanzeroLinkMeshStatusDto {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub self_ip: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub self_hostname: Option<String>,
    pub backend_state: String,
    pub online: bool,
    #[serde(default)]
    pub peers: Vec<LeanzeroLinkMeshPeerDto>,
}

/// One mesh binary as discovered when the Link manager was built: `found` at `path`, or
/// `missing` with the full text of where discovery looked (the env override, every PATH
/// directory, the known install locations) — so the UI can show a broken bundle BEFORE
/// the connect click, and `connect` is refused with the same text.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum LeanzeroLinkBinaryDto {
    Found { path: String },
    Missing { error: String },
}

/// The two binaries the goose-owned mesh needs.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LeanzeroLinkMeshBinariesDto {
    pub tailscaled: LeanzeroLinkBinaryDto,
    pub tailscale: LeanzeroLinkBinaryDto,
}

/// What goosed surfaces for the Link tab: auth + live mesh + total node count
/// (self + reachable peers) + the last error (never swallowed).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct LeanzeroLinkStateResponse {
    pub auth: LeanzeroLinkAuthStateDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mesh: Option<LeanzeroLinkMeshStatusDto>,
    pub node_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    /// The user's remote-execution switch (goose config key
    /// `LEANZERO_LINK_ALLOW_REMOTE_EXECUTION`) as configured NOW — what a toggle shows.
    /// Default `false`: the node is observe-only (peers get `403` on `/execute` and `/mlx/*`).
    #[serde(default)]
    pub remote_execution_allowed: bool,
    /// The value the RUNNING control service enforces, present only while connected. When
    /// it differs from `remote_execution_allowed`, the change applies at the next connect.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_execution_allowed_live: Option<bool>,
    /// Discovery's verdict on the mesh binaries at manager build — shown before any click.
    pub mesh_binaries: LeanzeroLinkMeshBinariesDto,
    /// Whether this goosed injected a remote executor at boot. `false` means every
    /// `/v1/swarm/execute` (own or a peer's) answers `501` — "not wired", not "busy".
    #[serde(default)]
    pub remote_execution_wired: bool,
    /// Whether this goosed injected the local MLX control at boot. `false` means every
    /// `/v1/swarm/mlx/*` op answers `501`.
    #[serde(default)]
    pub mlx_control_wired: bool,
}

/// The swarm node view (`self` + peers). Proxies the local control service's
/// `GET /v1/swarm/nodes`. The `self` and `peers` objects are the snake_case wire
/// `NodeState` shared with peer nodes + the iOS companion — passed through verbatim:
/// `{ node_id, hostname, mesh_ip, status: { type, session_id? }, sessions_active,
/// updated_at }`.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/leanzeroLink/nodes",
    response = LeanzeroLinkNodesResponse
)]
#[serde(rename_all = "camelCase")]
pub struct LeanzeroLinkNodesRequest {}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
pub struct LeanzeroLinkNodesResponse {
    #[serde(rename = "self")]
    pub self_node: serde_json::Value,
    #[serde(default)]
    pub peers: Vec<serde_json::Value>,
}

/// Send a fresh prompt to an idle linked node (self or a peer) and start a NEW session
/// there. Wraps `LinkManager::remote_execute` with `session_id: None`. The caller picks
/// `targetNodeId` from `leanzeroLink/nodes` (filter to `status == "Idle"`); the receive
/// side's idle guard on the peer is the backstop, so a busy/disabled/unwired target
/// surfaces its status text verbatim as an `invalid_params` error. Requires the manager
/// be `connected` — otherwise `invalid_params` "not connected to the mesh". The returned
/// `sessionId` is the session on the TARGET node, mirrored over `/v1/swarm/stream`.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/leanzeroLink/remoteExecute",
    response = LeanzeroLinkRemoteExecuteResponse
)]
#[serde(rename_all = "camelCase")]
pub struct LeanzeroLinkRemoteExecuteRequest {
    pub target_node_id: String,
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_dir: Option<String>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct LeanzeroLinkRemoteExecuteResponse {
    /// The session id created on the target node — mirror it over the swarm stream.
    pub session_id: String,
}

#[cfg(test)]
mod mlx_engine_status_dto_tests {
    use super::*;

    // Q1: `activeRequests` is the engine's own in-flight count. Absent must stay absent on
    // the wire (no `null`), an explicit 0 is a FACT (idle) distinct from absent, and a body
    // from an older agent that never sends the field deserializes to None — never 0.
    #[test]
    fn active_requests_absent_stays_absent_and_zero_is_a_fact() {
        let silent = MlxEngineStatusDto {
            state: "running".to_string(),
            active_requests_error: Some(
                "GET http://127.0.0.1:8090/v1/status returned HTTP 401".to_string(),
            ),
            ..Default::default()
        };
        let value = serde_json::to_value(&silent).unwrap();
        assert!(
            value.get("activeRequests").is_none(),
            "absent must not serialize as null"
        );
        assert_eq!(
            value["activeRequestsError"],
            "GET http://127.0.0.1:8090/v1/status returned HTTP 401"
        );

        let idle = MlxEngineStatusDto {
            state: "running".to_string(),
            active_requests: Some(0),
            ..Default::default()
        };
        let value = serde_json::to_value(&idle).unwrap();
        assert_eq!(value["activeRequests"], 0);
        let back: MlxEngineStatusDto = serde_json::from_value(value).unwrap();
        assert_eq!(
            back.active_requests,
            Some(0),
            "an explicit idle zero survives the trip"
        );
        assert!(back.active_requests_error.is_none());

        let busy = MlxEngineStatusDto {
            state: "running".to_string(),
            active_requests: Some(3),
            ..Default::default()
        };
        let value = serde_json::to_value(&busy).unwrap();
        assert_eq!(value["activeRequests"], 3);
        let back: MlxEngineStatusDto = serde_json::from_value(value).unwrap();
        assert_eq!(back.active_requests, Some(3));

        let legacy: MlxEngineStatusDto = serde_json::from_value(serde_json::json!({
            "state": "running",
            "availableMemoryGb": 1.0,
            "totalMemoryGb": 2.0,
            "restartRequired": false
        }))
        .unwrap();
        assert_eq!(
            legacy.active_requests, None,
            "an older agent's body is an absence, not 0"
        );
        assert_eq!(legacy.active_requests_error, None);
        assert_eq!(legacy.reclaimable_cache_gb, None);
        assert_eq!(legacy.memory_error, None);

        let cached = MlxEngineStatusDto {
            available_memory_gb: 59.3,
            total_memory_gb: 128.0,
            reclaimable_cache_gb: Some(24.6),
            ..Default::default()
        };
        let value = serde_json::to_value(&cached).unwrap();
        assert_eq!(value["reclaimableCacheGb"], 24.6);
        assert!(value.get("memoryError").is_none());
    }
}

#[cfg(test)]
mod mlx_node_id_tests {
    use super::*;

    // The mesh `nodeId` on every mlxEngine request is optional (camelCase `nodeId`),
    // defaults to absent, and an absent value must NOT serialize as `null` — the wire
    // contract the panel-surgeon and the control proxy both read.

    #[test]
    fn status_node_id_absent_stays_absent_and_present_round_trips() {
        let minimal = MlxEngineStatusRequest::default();
        let value = serde_json::to_value(&minimal).unwrap();
        assert!(
            value.get("nodeId").is_none(),
            "an absent nodeId must not serialize as null"
        );

        let targeted = MlxEngineStatusRequest {
            node_id: Some("studio-ab12cd".to_string()),
        };
        let value = serde_json::to_value(&targeted).unwrap();
        assert_eq!(value["nodeId"], "studio-ab12cd");
        let back: MlxEngineStatusRequest = serde_json::from_value(value).unwrap();
        assert_eq!(back.node_id.as_deref(), Some("studio-ab12cd"));

        // A body from an older UI that omits nodeId still deserializes (absent).
        let legacy: MlxEngineStatusRequest = serde_json::from_value(serde_json::json!({})).unwrap();
        assert!(legacy.node_id.is_none());
    }

    #[test]
    fn field_carrying_requests_keep_their_fields_and_gain_node_id() {
        let dl = MlxEngineDownloadRequest {
            repo_id: "org/model".to_string(),
            node_id: Some("peer-1".to_string()),
        };
        let value = serde_json::to_value(&dl).unwrap();
        assert_eq!(value["repoId"], "org/model");
        assert_eq!(value["nodeId"], "peer-1");
        let back: MlxEngineDownloadRequest = serde_json::from_value(value).unwrap();
        assert_eq!(back.repo_id, "org/model");
        assert_eq!(back.node_id.as_deref(), Some("peer-1"));

        // A legacy download body without nodeId still parses, nodeId absent.
        let legacy: MlxEngineDownloadRequest =
            serde_json::from_value(serde_json::json!({ "repoId": "org/model" })).unwrap();
        assert!(legacy.node_id.is_none());
        let legacy_value = serde_json::to_value(&legacy).unwrap();
        assert!(legacy_value.get("nodeId").is_none());
    }

    #[test]
    fn settings_update_carries_node_id_alongside_settings() {
        let req = MlxEngineSettingsUpdateRequest {
            settings: MlxEngineSettingsDto {
                models_dir: "~/models".to_string(),
                port: 8090,
                spawn_command: vec!["mlx_lm.server".to_string()],
                ..Default::default()
            },
            node_id: Some("peer-2".to_string()),
        };
        let value = serde_json::to_value(&req).unwrap();
        assert_eq!(value["nodeId"], "peer-2");
        assert_eq!(value["settings"]["modelsDir"], "~/models");
        let back: MlxEngineSettingsUpdateRequest = serde_json::from_value(value).unwrap();
        assert_eq!(back.node_id.as_deref(), Some("peer-2"));
        assert_eq!(back.settings.port, 8090);
    }
}

/// Connect a saved MCP using its stored credentials and discover its actual tools.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/config/extensions/inspect", response = InspectConfigExtensionResponse)]
#[serde(rename_all = "camelCase")]
pub struct InspectConfigExtensionRequest {
    pub name: String,
    #[serde(default)]
    pub settings_only: bool,
    #[serde(default)]
    pub source_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct InspectConfigExtensionResponse {
    pub settings: BTreeMap<String, String>,
    pub tools: Vec<serde_json::Value>,
    pub saved_file: Option<String>,
}

// ---------------------------------------------------------------------------------------------
// MLX placement planner (local-edition/mlx/DESIGN-PLACEMENT.md): which way of running a model is
// best on the Macs goose can reach, and "Measure speed". Shapes mirror goose_sidecar::placement.
// ---------------------------------------------------------------------------------------------

/// What the recommendation optimises: one conversation's writing speed (decode), reading long
/// prompts (prefill), or total speed across many requests.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum MlxPlacementGoalDto {
    #[default]
    Chat,
    LongDocuments,
    ManyRequests,
}

/// A Mac's chip: `hw.model`, the brand string, IOKit's GPU core count.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MlxChipDto {
    pub hw_model: String,
    pub brand: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu_cores: Option<u32>,
}

/// A figure and its range (tok/s).
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MlxEstimateDto {
    pub value: f64,
    pub low: f64,
    pub high: f64,
}

/// `measured` = goose timed this placement (median of `runs`, range = their extremes); else the
/// calibrated formula's estimate.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MlxSpeedFigureDto {
    pub estimate: MlxEstimateDto,
    pub measured: bool,
    pub runs: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_measured_ms: Option<u64>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MlxPlacementSpeedDto {
    /// Writing speed of one conversation (tok/s) at a ~2k-token prompt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decode: Option<MlxSpeedFigureDto>,
    /// Prompt reading speed (tok/s) at the goal's prompt size.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefill: Option<MlxSpeedFigureDto>,
    /// Total tok/s across `concurrency` requests (derived from decode × a measured gain).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub throughput: Option<MlxSpeedFigureDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub concurrency: Option<u32>,
    /// What the figures rest on, one line each (English diagnostics).
    #[serde(default)]
    pub basis: Vec<String>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum MlxPlacementKindDto {
    #[default]
    Single,
    Tensor,
    Pipeline,
}

/// A placement's identity: kind, node ids in rank order (`local` = this Mac, a peer by its
/// configured host), the link of a split.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MlxPlacementKeyDto {
    pub kind: MlxPlacementKindDto,
    pub nodes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum MlxFitStatusDto {
    Fits,
    /// Fits only at a smaller context than wanted (`context` says which).
    SmallerContext,
    Short,
    #[default]
    Unknown,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MlxNodeFitDto {
    pub name: String,
    pub need_bytes: u64,
    pub budget_bytes: u64,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MlxPlacementFitDto {
    pub status: MlxFitStatusDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub short_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub short_node: Option<String>,
    #[serde(default)]
    pub nodes: Vec<MlxNodeFitDto>,
    /// The arithmetic in words (English), a refusal's message verbatim.
    pub detail: String,
}

/// What [Use this] does. Internally tagged on `kind`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum MlxPlacementActionDto {
    /// Mount on this Mac's single engine (`mlxEngine/mount`).
    MountHere,
    /// Start the distributed engine (`mlxEngine/distributedStart`); `setupMatches` = the saved
    /// setup already names this model (else Set up must pick it first).
    StartSplit { setup_matches: bool },
    /// Start the single engine on the peer and chat through Link.
    RemoteSingle,
    /// Nothing goose can start for it today — why (English).
    Unavailable { reason: String },
}

/// Why a candidate won or lost. Internally tagged on `code`; `mine`/`best` are the goal's tok/s.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "code", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum MlxPlacementOutcomeDto {
    Best,
    BestAvailableNow,
    NotSupported { reason: String },
    DoesNotFit,
    FitUnknown { reason: String },
    NoFigure { reason: String },
    Slower { mine: f64, best: f64 },
    TiedNeedsMoreMacs { mine: f64, best: f64 },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MlxPlacementCandidateDto {
    /// `single:local`, `tensor:jaccl:local+link:<id>` — what `measureSpeed` takes.
    pub id: String,
    pub key: MlxPlacementKeyDto,
    pub node_names: Vec<String>,
    /// Per node; `null` = the node did not report its chip.
    pub chips: Vec<Option<MlxChipDto>>,
    /// "rapid-mlx" | "mlx_lm" | "pipeline_qwen4".
    pub backend: String,
    /// `false` = goose cannot run this placement for this architecture yet (listed, never offered).
    pub supported: bool,
    pub fit: MlxPlacementFitDto,
    pub speed: MlxPlacementSpeedDto,
    pub action: MlxPlacementActionDto,
    pub outcome: MlxPlacementOutcomeDto,
}

/// The model-picker badge. Internally tagged on `kind`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum MlxPlacementBadgeDto {
    FitsThisMac,
    FitsPeer { name: String },
    NeedsBothMacs,
    TooBig { short_bytes: u64 },
    Unknown { reason: String },
}

/// One model's plan, or why it could not be planned (`error`).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MlxPlacementPlanDto {
    pub model_id: String,
    pub goal: MlxPlacementGoalDto,
    /// Best first, then the rest in the order they lost.
    #[serde(default)]
    pub candidates: Vec<MlxPlacementCandidateDto>,
    /// The fastest candidate's id, whether or not it can be started today.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub best: Option<String>,
    /// The fastest candidate goose can start today.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub best_available: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub badge: Option<MlxPlacementBadgeDto>,
    #[serde(default)]
    pub notes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// One Mac as the planner measured it. Every figure it could not read is absent with its `*Error`.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MlxPlacementNodeDto {
    /// `local`, or the configured host (ssh alias / `link:<id>`).
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chip: Option<MlxChipDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chip_error: Option<String>,
    /// Apple's published memory bandwidth for the chip.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bandwidth_gbs: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bandwidth_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bandwidth_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub available_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_error: Option<String>,
    /// Metal's working-set ceiling.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ceiling_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ceiling_error: Option<String>,
}

/// Plan one model (`modelId`) or every model in the models folder (absent — the picker's badges).
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/mlxEngine/placementPlan",
    response = MlxEnginePlacementPlanResponse
)]
#[serde(rename_all = "camelCase")]
pub struct MlxEnginePlacementPlanRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    #[serde(default)]
    pub goal: MlxPlacementGoalDto,
    /// Context wanted (tokens); absent = the largest that fits, capped at the model's maximum.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct MlxEnginePlacementPlanResponse {
    pub plans: Vec<MlxPlacementPlanDto>,
    pub nodes: Vec<MlxPlacementNodeDto>,
    /// Lines of the measurement store that did not parse, by number.
    #[serde(default)]
    pub store_errors: Vec<String>,
    pub probe_ms: u64,
}

/// One measured run, as stored.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MlxSpeedRecordDto {
    pub model_id: String,
    pub placement: MlxPlacementKeyDto,
    pub node_names: Vec<String>,
    pub chips: Vec<Option<MlxChipDto>>,
    pub backend: String,
    pub context_bucket: u64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefill_tps: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decode_tps: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ttft_ms: Option<f64>,
    pub recorded_at_ms: u64,
    /// "benchmark" | "chat".
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workload: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kv_cache: Option<String>,
}

/// "Measure speed": the fixed workload on the RUNNING engine of `placementId` (the single engine
/// on this Mac, or the distributed engine) — ~1.9k prompt tokens + 256 greedy tokens, and with
/// `longDocument` a ~30k-token document too. Recorded into the store.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/mlxEngine/measureSpeed",
    response = MlxEngineMeasureSpeedResponse
)]
#[serde(rename_all = "camelCase")]
pub struct MlxEngineMeasureSpeedRequest {
    pub model_id: String,
    pub placement_id: String,
    #[serde(default)]
    pub long_document: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct MlxEngineMeasureSpeedResponse {
    pub records: Vec<MlxSpeedRecordDto>,
}

/// Every stored measurement (optionally one model's), newest last.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/mlxEngine/speedHistory",
    response = MlxEngineSpeedHistoryResponse
)]
#[serde(rename_all = "camelCase")]
pub struct MlxEngineSpeedHistoryRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct MlxEngineSpeedHistoryResponse {
    pub records: Vec<MlxSpeedRecordDto>,
    #[serde(default)]
    pub store_errors: Vec<String>,
    /// The store file.
    pub path: String,
}
