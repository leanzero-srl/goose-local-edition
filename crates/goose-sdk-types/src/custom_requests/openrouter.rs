//! OpenRouter hosts: which upstream providers serve a model, a one-call speed and tool-call probe
//! per host, and the routing pin (`OPENROUTER_PARAMETERS` in config.yaml) that keeps every
//! OpenRouter request on one host. The pin is the SAME key an external benchmark driver writes, so
//! the app and that driver always agree on where a model runs.

use agent_client_protocol::{JsonRpcRequest, JsonRpcResponse};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The routing pin as config.yaml holds it right now.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OpenRouterPinDto {
    /// The key's value as written (compact JSON); absent when no pin is set and OpenRouter routes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<String>,
    /// The one host the pin keeps every request on: `provider.order` names exactly this tag and
    /// `allow_fallbacks` is false. Absent for no pin and for any other routing (several hosts,
    /// fallbacks allowed, an `ignore` list) — `raw` then says what it is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/openrouter/pin/read",
    response = OpenRouterPinResponse
)]
#[serde(rename_all = "camelCase")]
pub struct OpenRouterPinReadRequest {}

/// Pin every OpenRouter request to one host (`tag`), or remove the pin (`tag` absent) so
/// OpenRouter routes. Writes `OPENROUTER_PARAMETERS:
/// '{"provider":{"order":["<tag>"],"allow_fallbacks":false}}'`.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/openrouter/pin/set",
    response = OpenRouterPinResponse
)]
#[serde(rename_all = "camelCase")]
pub struct OpenRouterPinSetRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct OpenRouterPinResponse {
    pub pin: OpenRouterPinDto,
}

/// One upstream host OpenRouter runs the model on, as its endpoints listing declares it.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OpenRouterHostDto {
    /// What `provider.order` takes: `wafer`, `deepinfra/bf16`.
    pub tag: String,
    pub provider_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quantization: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_length: Option<u64>,
    /// `supported_parameters` lists `tools`.
    pub supports_tools: bool,
    /// Percent of requests that succeeded in the last 30 minutes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uptime_last_30m: Option<f64>,
    /// OpenRouter's endpoint status (0 = normal; negative = degraded or down).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<i64>,
}

/// The hosts OpenRouter's endpoints listing names for `model` (`author/slug`).
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/openrouter/hosts/list",
    response = OpenRouterHostsListResponse
)]
#[serde(rename_all = "camelCase")]
pub struct OpenRouterHostsListRequest {
    pub model: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct OpenRouterHostsListResponse {
    pub model: String,
    pub hosts: Vec<OpenRouterHostDto>,
    /// Provider names of listed endpoints that carry no tag, so no pin can name them.
    #[serde(default)]
    pub untagged: Vec<String>,
}

/// One chat completion sent to `model` on exactly the host `tag` (no fallbacks), with one `ping`
/// tool the prompt asks the model to call.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/openrouter/hosts/probe",
    response = OpenRouterHostProbeResponse
)]
#[serde(rename_all = "camelCase")]
pub struct OpenRouterHostProbeRequest {
    pub model: String,
    pub tag: String,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct OpenRouterHostProbeResponse {
    pub tag: String,
    /// Wall seconds from sending the request to the whole answer read.
    pub seconds: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion_tokens: Option<u64>,
    /// `completion_tokens / seconds`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens_per_second: Option<f64>,
    /// The answer carried a `ping` tool call.
    pub tool_call: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http_status: Option<u16>,
    /// The host's or OpenRouter's own error text, verbatim; absent when the call answered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}
