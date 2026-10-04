//! Per-model custom fields: the request knobs one model takes (reasoning effort, verbosity,
//! sampling), read from the provider's own model metadata where it publishes some, with the values
//! saved per provider+model.

use agent_client_protocol::{JsonRpcRequest, JsonRpcResponse};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ModelFieldKindDto {
    Select {
        options: Vec<String>,
    },
    #[serde(rename_all = "camelCase")]
    Number {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        min: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max: Option<f64>,
        integer: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ModelFieldDto {
    /// The saved value's key (`effort`, `verbosity`, `top_k`, …).
    pub id: String,
    pub label: String,
    /// What the field is sent as and where its choices come from.
    pub description: String,
    pub kind: ModelFieldKindDto,
    /// The model's own value when a request sends none, where the metadata declares it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_default: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ModelFieldSourceDto {
    /// The provider's own per-model metadata (OpenRouter's models listing).
    ProviderMetadata,
    /// The provider publishes per-model metadata, but its listing does not carry this model id.
    UnlistedModel,
    /// goose's own effort mapping for a model goose knows reasons.
    GooseEffort,
    /// No field applies to this model.
    None,
}

/// The custom fields `modelId` takes on `providerId`, and the values saved for it.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/providers/model-fields/list",
    response = ModelFieldsListResponse
)]
#[serde(rename_all = "camelCase")]
pub struct ModelFieldsListRequest {
    pub provider_id: String,
    pub model_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct ModelFieldsListResponse {
    pub fields: Vec<ModelFieldDto>,
    pub source: ModelFieldSourceDto,
    /// The saved values, keyed by field id; a field with no entry runs on the model's default.
    pub values: BTreeMap<String, serde_json::Value>,
}

/// Replaces the saved values for `modelId` on `providerId`. Each value is checked against the
/// field the model declares; a null value clears the field. An empty map clears the model.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/providers/model-fields/save",
    response = ModelFieldsSaveResponse
)]
#[serde(rename_all = "camelCase")]
pub struct ModelFieldsSaveRequest {
    pub provider_id: String,
    pub model_id: String,
    pub values: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct ModelFieldsSaveResponse {
    /// The values as saved.
    pub values: BTreeMap<String, serde_json::Value>,
}
