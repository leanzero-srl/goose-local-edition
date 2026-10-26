//! Version 1 controller protocol. Device credentials never enter the legacy node API.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const CONTROLLER_PROTOCOL_VERSION: u32 = 1;
pub const DEFAULT_CONTROLLER_PORT: u16 = 41227;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ControllerScope {
    #[serde(rename = "catalog.read")]
    CatalogRead,
    #[serde(rename = "history.read")]
    HistoryRead,
    #[serde(rename = "work.send")]
    WorkSend,
    #[serde(rename = "work.steer")]
    WorkSteer,
    #[serde(rename = "work.stop")]
    WorkStop,
    #[serde(rename = "input.answer")]
    InputAnswer,
    #[serde(rename = "resources.read")]
    ResourcesRead,
    #[serde(rename = "models.read")]
    ModelsRead,
    #[serde(rename = "models.download")]
    ModelsDownload,
    #[serde(rename = "models.manage")]
    ModelsManage,
    #[serde(rename = "providers.read")]
    ProvidersRead,
    #[serde(rename = "providers.manage")]
    ProvidersManage,
    #[serde(rename = "devices.read")]
    DevicesRead,
    #[serde(rename = "devices.manage")]
    DevicesManage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceRole {
    Controller,
    Host,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControllerProtocol {
    pub version: u32,
    pub owner_node_id: Uuid,
    pub device_role: DeviceRole,
    pub capabilities: Vec<ControllerScope>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceRecord {
    pub device_id: Uuid,
    pub name: String,
    pub platform: String,
    pub role: DeviceRole,
    pub revision: u64,
    pub scopes: Vec<ControllerScope>,
    pub created_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

// Deliberately no Debug: both fields contain bearer credentials.
#[derive(Clone, Serialize, Deserialize)]
pub struct DeviceCredentials {
    pub access_token: String,
    pub token_type: String,
    pub expires_at: DateTime<Utc>,
    pub refresh_token: String,
    pub device: DeviceRecord,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MeshEndpoint {
    pub url: String,
    /// SHA256 of SubjectPublicKeyInfo DER, 64 lowercase hexadecimal characters.
    pub tls_spki_sha256: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostAvailability {
    Online,
    Offline,
    UpdateRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostDescriptor {
    pub owner_node_id: Uuid,
    pub name: String,
    pub device_role: DeviceRole,
    pub protocol_version: u32,
    pub availability: HostAvailability,
    pub last_seen_at: Option<DateTime<Utc>>,
    pub endpoints: Vec<MeshEndpoint>,
    pub capabilities: Vec<ControllerScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostsResponse {
    pub revision: u64,
    pub hosts: Vec<HostDescriptor>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisteredPeer {
    pub mesh_node_key: String,
    pub role: DeviceRole,
    pub owner_node_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisteredPeers {
    pub protocol_version: u32,
    pub lease_seconds: u32,
    pub revision: u64,
    pub peers: Vec<RegisteredPeer>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostRegistration {
    pub runtime_id: Uuid,
    pub owner_node_id: Uuid,
    pub registration_id: Uuid,
    pub host_name: String,
    pub protocol_version: u32,
    pub tls_spki_sha256: String,
    pub controller_port: u16,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct HostCredentials {
    pub host_credential: String,
    pub host_revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostLeaseRequest {
    pub owner_node_id: Uuid,
    pub runtime_id: Uuid,
    pub store_generation: Uuid,
    pub catalog_revision: u64,
    pub mesh_node_key: String,
    pub capabilities: Vec<ControllerScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostBootstrap {
    pub account_id: Uuid,
    pub owner_node_id: Uuid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostEnrollmentStatus {
    Pending,
    Issued,
    Confirmed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostMeshEnrollment {
    pub enrollment_id: Uuid,
    pub status: HostEnrollmentStatus,
    pub mesh_node_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostMeshContext {
    pub login_server: String,
    pub enrollment: Option<HostMeshEnrollment>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct HostMeshJoin {
    pub enrollment_id: Uuid,
    pub auth_key: String,
    pub login_server: String,
    pub expiry_seconds: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostMeshConfirmation {
    pub owner_node_id: Uuid,
    pub mesh_node_key: String,
    pub host_revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostLease {
    pub lease_id: Uuid,
    pub lease_expires_at: DateTime<Utc>,
    pub host_revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthorizationLease {
    pub active: bool,
    pub account_id: Uuid,
    pub device_id: Uuid,
    pub device_revision: u64,
    pub role: DeviceRole,
    pub owner_node_id: Uuid,
    pub scopes: Vec<ControllerScope>,
    pub token_expires_at: DateTime<Utc>,
    pub lease_seconds: u32,
    pub host_revision: u64,
    pub store_generation: Uuid,
    pub host_lease_id: Uuid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkKind {
    Chat,
    AgentWork,
    Benchmark,
    ShellJob,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WorkRef {
    pub owner_node_id: Uuid,
    pub kind: WorkKind,
    pub work_id: Uuid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogCursor {
    pub owner_node_id: Uuid,
    pub store_generation: Uuid,
    pub revision: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkState {
    Idle,
    Running,
    WaitingInput,
    Paused,
    Finished,
    Failed,
    Interrupted,
    Deleted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkAction {
    Send,
    Steer,
    Stop,
    AnswerInput,
    Note,
    Pause,
    Resume,
    Tick,
}

impl WorkAction {
    pub fn required_scope(self) -> ControllerScope {
        match self {
            Self::Send => ControllerScope::WorkSend,
            Self::Steer | Self::Note | Self::Pause | Self::Resume | Self::Tick => {
                ControllerScope::WorkSteer
            }
            Self::Stop => ControllerScope::WorkStop,
            Self::AnswerInput => ControllerScope::InputAnswer,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventPreview {
    pub kind: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CatalogEntry {
    pub work: WorkRef,
    pub store_generation: Uuid,
    pub revision: u64,
    pub title: String,
    pub working_dir: Option<String>,
    pub state: WorkState,
    pub current_run_id: Option<Uuid>,
    pub capabilities: Vec<WorkAction>,
    pub parent: Option<WorkRef>,
    pub related: Vec<WorkRef>,
    pub updated_at: DateTime<Utc>,
    pub last_event: Option<EventPreview>,
    pub archived: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogPage {
    pub resume_cursor: String,
    pub entries: Vec<CatalogEntry>,
    pub watermark: CatalogCursor,
    pub next_page_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogChange {
    pub resume_cursor: String,
    pub cursor: CatalogCursor,
    #[serde(rename = "type")]
    pub kind: CatalogChangeKind,
    pub work: WorkRef,
    pub entry: Option<CatalogEntry>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogChangeKind {
    Upsert,
    Tombstone,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogChanges {
    pub resume_cursor: String,
    pub changes: Vec<CatalogChange>,
    pub cursor: CatalogCursor,
    pub has_more: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryItem {
    pub message_id: String,
    pub revision: u64,
    /// The adapter supplies only the existing user-visible message contract.
    pub message: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryPage {
    pub work_cursor: String,
    pub work: WorkRef,
    pub store_generation: Uuid,
    pub revision: u64,
    pub items: Vec<HistoryItem>,
    pub next_page_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingInput {
    pub input_id: Uuid,
    pub run_id: Uuid,
    pub prompt: String,
    pub options: Vec<InputOption>,
    pub payload: PendingInputPayload,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PendingInputPayload {
    Permission {
        request_id: String,
        tool_name: String,
        arguments: serde_json::Map<String, serde_json::Value>,
    },
    Form {
        schema: serde_json::Value,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputOption {
    pub option_id: String,
    pub label: String,
    pub accepts_value: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkSnapshot {
    pub live_messages: Vec<HistoryItem>,
    pub work_cursor: String,
    pub entry: CatalogEntry,
    pub pending_inputs: Vec<PendingInput>,
    pub history_watermark: CatalogCursor,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ControllerContent {
    Text { text: String },
    Resource { resource_id: Uuid },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "action",
    content = "payload",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ActionPayload {
    Send {
        content: Vec<ControllerContent>,
    },
    Steer {
        content: Vec<ControllerContent>,
    },
    Stop {},
    AnswerInput {
        input_id: Uuid,
        option_id: String,
        value: Option<String>,
    },
    Note {
        text: String,
    },
    Pause {},
    Resume {},
    Tick {},
}

impl ActionPayload {
    pub fn action(&self) -> WorkAction {
        match self {
            Self::Send { .. } => WorkAction::Send,
            Self::Steer { .. } => WorkAction::Steer,
            Self::Stop {} => WorkAction::Stop,
            Self::AnswerInput { .. } => WorkAction::AnswerInput,
            Self::Note { .. } => WorkAction::Note,
            Self::Pause {} => WorkAction::Pause,
            Self::Resume {} => WorkAction::Resume,
            Self::Tick {} => WorkAction::Tick,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkCommand {
    pub command_id: Uuid,
    pub work: WorkRef,
    pub expected_store_generation: Uuid,
    pub expected_revision: u64,
    pub expected_run_id: Option<Uuid>,
    #[serde(flatten)]
    pub action: ActionPayload,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandState {
    Accepted,
    Waiting,
    Dispatching,
    Delivered,
    Rejected,
    OutcomeUnknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandReceipt {
    pub command_id: Uuid,
    pub work: WorkRef,
    pub authenticated_device_id: Uuid,
    pub revision: u64,
    pub state: CommandState,
    pub run_id: Option<Uuid>,
    pub message_id: Option<String>,
    pub input_id: Option<Uuid>,
    pub checkpoint_id: Option<String>,
    pub reason: Option<String>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkFrame {
    pub resume_cursor: String,
    pub work: WorkRef,
    pub store_generation: Uuid,
    pub stream_epoch: Uuid,
    pub sequence: u64,
    pub work_revision: u64,
    pub run_id: Option<Uuid>,
    #[serde(rename = "type")]
    pub kind: String,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControllerCapabilities {
    pub protocol_version: u32,
    pub host: HostDescriptor,
    pub store_generation: Uuid,
    pub allowed_scopes: Vec<ControllerScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceSnapshot {
    pub load: crate::load::NodeLoad,
    pub sampled_at: DateTime<Utc>,
}
