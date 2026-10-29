//! Device-authorized management of the receiving host, with durable command receipts.
use crate::controller::ControllerError;
use crate::controller_auth::{AuthenticatedDevice, DispatchAuthorization};
use crate::controller_wire::{CommandState, ControllerScope};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

mod routes;
pub(crate) use routes::router;

// No Debug: a command may contain provider credentials. Only its digest is persisted.
#[derive(Clone, Serialize, Deserialize)]
pub struct ManagementCommand {
    pub command_id: Uuid,
    pub owner_node_id: Uuid,
    pub expected_store_generation: Uuid,
    #[serde(flatten)]
    pub action: ManagementAction,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(
    tag = "action",
    content = "payload",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ManagementAction {
    Download {
        repo_id: String,
    },
    DownloadCancel {
        repo_id: String,
    },
    DownloadPause {
        repo_id: String,
    },
    DownloadResume {
        repo_id: String,
    },
    NodeCreate {
        id: String,
        name: String,
        kind: ManagedNodeKind,
        model: String,
        #[serde(default)]
        provider: Option<String>,
    },
    NodeRemove {
        id: String,
        expected_definition_revision: String,
    },
    ProviderConfigure {
        provider_id: String,
        fields: BTreeMap<String, String>,
        #[serde(default)]
        test_model: Option<String>,
    },
    ProviderCreate {
        engine: String,
        display_name: String,
        api_url: String,
        #[serde(default)]
        api_key: Option<String>,
        models: Vec<String>,
        requires_auth: bool,
    },
}
impl ManagementAction {
    pub fn name(&self) -> &'static str {
        match self {
            Self::NodeCreate { .. } => "node_create",
            Self::NodeRemove { .. } => "node_remove",
            Self::Download { .. } => "download",
            Self::DownloadCancel { .. } => "download_cancel",
            Self::DownloadPause { .. } => "download_pause",
            Self::DownloadResume { .. } => "download_resume",
            Self::ProviderConfigure { .. } => "provider_configure",
            Self::ProviderCreate { .. } => "provider_create",
        }
    }
    pub fn required_scope(&self) -> ControllerScope {
        match self {
            Self::NodeCreate { .. } | Self::NodeRemove { .. } => ControllerScope::ModelsManage,
            Self::Download { .. }
            | Self::DownloadCancel { .. }
            | Self::DownloadPause { .. }
            | Self::DownloadResume { .. } => ControllerScope::ModelsDownload,
            Self::ProviderConfigure { .. } | Self::ProviderCreate { .. } => {
                ControllerScope::ProvidersManage
            }
        }
    }
    pub fn target(&self) -> &str {
        match self {
            Self::NodeCreate { id, .. } | Self::NodeRemove { id, .. } => id,
            Self::Download { repo_id }
            | Self::DownloadCancel { repo_id }
            | Self::DownloadPause { repo_id }
            | Self::DownloadResume { repo_id } => repo_id,
            Self::ProviderConfigure { provider_id, .. } => provider_id,
            Self::ProviderCreate { display_name, .. } => display_name,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagementReceipt {
    pub command_id: Uuid,
    pub owner_node_id: Uuid,
    pub authenticated_device_id: Uuid,
    pub action: String,
    pub state: CommandState,
    pub target: String,
    pub reason: Option<String>,
    pub updated_at: DateTime<Utc>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderField {
    pub key: String,
    pub required: bool,
    pub secret: bool,
    pub is_set: bool,
    /// Always null for secrets, including prefixes and suffixes.
    pub value: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagedProvider {
    pub provider_id: String,
    pub display_name: String,
    pub configured: bool,
    pub fields: Vec<ProviderField>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagedProviders {
    pub providers: Vec<ManagedProvider>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagedNodeKind {
    Cloud,
    Endpoint,
    Mlx,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagedNode {
    pub id: String,
    pub name: String,
    pub kind: ManagedNodeKind,
    pub model: Option<String>,
    pub provider: Option<String>,
    pub macs: Vec<String>,
    pub editable: bool,
    pub definition_revision: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeProviderChoice {
    pub id: String,
    pub display_name: String,
    pub models: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagedNodes {
    pub nodes: Vec<ManagedNode>,
    pub providers: Vec<NodeProviderChoice>,
    pub local_models: Vec<String>,
    pub local_models_error: Option<String>,
    pub capabilities: Vec<String>,
    pub supported_kinds: Vec<ManagedNodeKind>,
}

#[async_trait]
pub trait ManagementBackend: Send + Sync {
    fn scopes(&self) -> Vec<ControllerScope>;
    async fn nodes(&self, _device: &AuthenticatedDevice) -> Result<ManagedNodes, ControllerError> {
        Err(ControllerError::new(
            axum::http::StatusCode::NOT_IMPLEMENTED,
            "nodes_unavailable",
            "Update the owner Mac to manage compute nodes",
            false,
        ))
    }
    async fn models(
        &self,
        device: &AuthenticatedDevice,
    ) -> Result<serde_json::Value, ControllerError>;
    async fn download(
        &self,
        device: &AuthenticatedDevice,
        repo_id: &str,
    ) -> Result<serde_json::Value, ControllerError>;
    async fn providers(
        &self,
        device: &AuthenticatedDevice,
    ) -> Result<ManagedProviders, ControllerError>;
    async fn accept(
        &self,
        device: &AuthenticatedDevice,
        command: ManagementCommand,
        authorization: DispatchAuthorization,
    ) -> Result<ManagementReceipt, ControllerError>;
    async fn receipt(
        &self,
        device: &AuthenticatedDevice,
        id: Uuid,
    ) -> Result<ManagementReceipt, ControllerError>;
}
