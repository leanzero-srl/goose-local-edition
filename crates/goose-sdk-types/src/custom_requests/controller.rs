use super::*;

/// Canonical domain bodies are validated against leanzero-link/controller_wire.rs.
/// This envelope deliberately carries no account, host or device credentials.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(transparent)]
pub struct ControllerResponse(pub serde_json::Value);

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/controller/status", response = ControllerResponse)]
#[serde(deny_unknown_fields)]
pub struct ControllerStatusRequest {}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/controller/requestCode", response = ControllerResponse)]
#[serde(deny_unknown_fields)]
pub struct ControllerRequestCodeRequest {}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/controller/verify", response = ControllerResponse)]
#[serde(deny_unknown_fields)]
pub struct ControllerVerifyRequest {
    pub code: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/controller/logout", response = ControllerResponse)]
#[serde(deny_unknown_fields)]
pub struct ControllerLogoutRequest {}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/controller/hosts", response = ControllerResponse)]
#[serde(deny_unknown_fields)]
pub struct ControllerHostsRequest {}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/controller/catalog", response = ControllerResponse)]
#[serde(deny_unknown_fields)]
pub struct ControllerCatalogRequest {}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/controller/sync", response = ControllerResponse)]
#[serde(deny_unknown_fields)]
pub struct ControllerSyncRequest {}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/controller/openWork", response = ControllerResponse)]
#[serde(deny_unknown_fields)]
pub struct ControllerOpenWorkRequest {
    pub work: serde_json::Value,
    pub store_generation: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/controller/history", response = ControllerResponse)]
#[serde(deny_unknown_fields)]
pub struct ControllerHistoryRequest {
    pub work: serde_json::Value,
    pub store_generation: String,
    pub page_token: Option<String>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/controller/historyLatest", response = ControllerResponse)]
#[serde(deny_unknown_fields)]
pub struct ControllerHistoryLatestRequest {
    pub work: serde_json::Value,
    pub store_generation: String,
    pub cursor: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/controller/workChanges", response = ControllerResponse)]
#[serde(deny_unknown_fields)]
pub struct ControllerWorkChangesRequest {
    pub work: serde_json::Value,
    pub store_generation: String,
    pub cursor: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/controller/command", response = ControllerResponse)]
#[serde(deny_unknown_fields)]
pub struct ControllerCommandRequest {
    pub command: serde_json::Value,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/controller/receipt", response = ControllerResponse)]
#[serde(deny_unknown_fields)]
pub struct ControllerReceiptRequest {
    pub owner_node_id: String,
    pub command_id: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/controller/pendingCommands", response = ControllerResponse)]
#[serde(deny_unknown_fields)]
pub struct ControllerPendingCommandsRequest {
    pub work: serde_json::Value,
}
