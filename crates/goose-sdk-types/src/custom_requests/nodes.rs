//! Nodes and strategies (design: local-edition/mlx/quality/DESIGN-NODES-AND-STRATEGIES.md §4.2).
//!
//! These types ARE the shape of the `nodes` config key and of every `nodes/*` ACP method; the
//! rules over them live in `goose::nodes`, and the desktop's `components/nodes/model.ts`
//! re-exports the generated mirror. The whole contract is closed here (S0): later slices fill
//! module bodies behind it and never reopen this file.

use agent_client_protocol::{JsonRpcRequest, JsonRpcResponse};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{MlxPlacementGoalDto, MlxPlacementKeyDto};

/// What work a node is handed. Chat is a chat's own turns; the other five act on swarm builds
/// (Tier B) — a chat's delegates use Build.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "camelCase")]
pub enum NodeRole {
    Chat,
    Planning,
    Build,
    Testing,
    Frontend,
    Backend,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum NodeDefKind {
    /// The LeanZero MLX engine, on one Mac or split across Macs.
    Mlx,
    /// A cloud provider's model.
    Cloud,
    /// An OpenAI-compatible server the user added.
    Endpoint,
}

/// The way an MLX node runs: one of the placement planner's candidates (`PlacementKey`; the Mac
/// `local` is always this Mac), or `follows` — a node adopted from the swarm pool that serves
/// whatever this Mac's engine serves (the Q-128 behaviour).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum NodePlacement {
    Single {
        macs: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        link: Option<String>,
    },
    Tensor {
        macs: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        link: Option<String>,
    },
    Pipeline {
        macs: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        link: Option<String>,
    },
    Follows,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum NodeOrigin {
    /// Adopted from the `swarm` pool; reads its model and provider through from the device.
    Pool,
    /// Made in the New node dialog.
    User,
    /// Saved from Run it ("Save as node").
    RunIt,
}

/// A named definition: a model plus one way (MLX), or a provider plus a model (cloud, endpoint).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NodeDef {
    /// Stable and unique; equals the swarm device id when it came from the pool.
    pub id: String,
    /// Unique display name (Q-154).
    pub name: String,
    pub kind: NodeDefKind,
    /// MLX: the model id as the models folder names it; cloud/endpoint: the provider's model id.
    /// Absent when `poolDevice` is set (read through from the device).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// MLX only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placement: Option<NodePlacement>,
    /// The goal the way was chosen for (MLX).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal: Option<MlxPlacementGoalDto>,
    /// Cloud/endpoint: the goose provider. Absent when `poolDevice` is set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// MLX: the loader never stops this node's way for another demand.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub keep_loaded: bool,
    /// The swarm device this node reads through (nodes adopted from the pool).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pool_device: Option<String>,
    pub origin: NodeOrigin,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NodeChainEntry {
    pub node: String,
    /// A share; read only when the role's `when` is `share`. An integer of 1 or more.
    pub weight: u32,
}

/// When the next node in a chain takes work.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum NodeWhen {
    /// The next one only when the one before can't run.
    #[default]
    Failover,
    /// The next one when the one before is busy.
    Overflow,
    /// Share the work by weight (smooth weighted round-robin; per conversation for chat).
    Share,
}

/// What a not-loaded MLX entry does (cloud is always loaded).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum NodeIfNotLoaded {
    /// Load it and wait.
    #[default]
    Load,
    /// The next entry servable now takes the work; no load is started.
    UseNext,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NodeRoleEntry {
    /// 1st = primary; the 2nd and later are fallbacks.
    pub chain: Vec<NodeChainEntry>,
    #[serde(default)]
    pub when: NodeWhen,
    #[serde(default)]
    pub if_not_loaded: NodeIfNotLoaded,
}

/// The roles a strategy sets; an unset role inherits (Chat ← Build, Planning ← Chat, Build ←
/// Chat, Testing/Frontend/Backend ← Build).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NodeStrategyRoles {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat: Option<NodeRoleEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub planning: Option<NodeRoleEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build: Option<NodeRoleEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub testing: Option<NodeRoleEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frontend: Option<NodeRoleEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend: Option<NodeRoleEntry>,
}

impl NodeStrategyRoles {
    pub fn get(&self, role: NodeRole) -> Option<&NodeRoleEntry> {
        match role {
            NodeRole::Chat => self.chat.as_ref(),
            NodeRole::Planning => self.planning.as_ref(),
            NodeRole::Build => self.build.as_ref(),
            NodeRole::Testing => self.testing.as_ref(),
            NodeRole::Frontend => self.frontend.as_ref(),
            NodeRole::Backend => self.backend.as_ref(),
        }
    }

    pub fn get_mut(&mut self, role: NodeRole) -> &mut Option<NodeRoleEntry> {
        match role {
            NodeRole::Chat => &mut self.chat,
            NodeRole::Planning => &mut self.planning,
            NodeRole::Build => &mut self.build,
            NodeRole::Testing => &mut self.testing,
            NodeRole::Frontend => &mut self.frontend,
            NodeRole::Backend => &mut self.backend,
        }
    }
}

/// Roles mapped to ordered chains of nodes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NodeStrategy {
    pub id: String,
    pub name: String,
    /// The owner's own words for this strategy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default)]
    pub roles: NodeStrategyRoles,
}

/// What a new chat starts on. `auto` = "Any node (Auto)", today's pool routing.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum NodesForNewChats {
    #[default]
    Auto,
    Node {
        id: String,
    },
    Strategy {
        id: String,
    },
}

/// What a swarm build started from a chat uses. `pool` = today's `swarm` block, untouched.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum NodesForBuilds {
    #[default]
    Pool,
    Strategy {
        id: String,
    },
}

/// The config key `nodes`. Read and written only through the `nodes/*` methods.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NodesConfig {
    pub version: u32,
    #[serde(default)]
    pub defs: Vec<NodeDef>,
    #[serde(default)]
    pub strategies: Vec<NodeStrategy>,
    /// Pool device ids the user removed as nodes: never re-adopted.
    #[serde(default)]
    pub declined: Vec<String>,
    #[serde(default)]
    pub for_new_chats: NodesForNewChats,
    #[serde(default)]
    pub for_builds: NodesForBuilds,
}

/// The one version of the `nodes` key this contract describes.
pub const NODES_CONFIG_VERSION: u32 = 1;

/// A config nobody has written: no nodes, no strategies, new chats on Auto, builds on the pool —
/// today's behaviour, byte for byte.
impl Default for NodesConfig {
    fn default() -> Self {
        Self {
            version: NODES_CONFIG_VERSION,
            defs: Vec::new(),
            strategies: Vec::new(),
            declined: Vec::new(),
            for_new_chats: NodesForNewChats::Auto,
            for_builds: NodesForBuilds::Pool,
        }
    }
}

/// Where a resolved node's model and provider came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum NodeModelFrom {
    /// The node's own fields.
    Own,
    /// Read through from its swarm device.
    Pool,
    /// Its swarm device is no longer in the pool ("No longer in your swarm pool"); the node is
    /// kept, never dropped.
    LeftPool,
    /// The `swarm` block could not be read, so the device's model is unknown.
    PoolUnreadable { error: String },
}

/// A definition as `nodes/read` answers it: the def, plus its model and provider resolved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedNodeDef {
    pub def: NodeDef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    pub model_from: NodeModelFrom,
    /// Adopted from the pool on this read and not written yet: it is written by the next
    /// successful `nodes/write` (a read never writes config).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pending_adoption: bool,
}

/// Why a write, removal or build choice was refused. `code` is stable for the UI's i18n; the
/// message is the verbatim English the UI may show as is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NodesRefusal {
    pub code: NodesRefusalCode,
    pub message: String,
    /// The node, strategy or device the refusal is about, when it names one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    /// `liveSessionsNotAcknowledged` only: how many live chats are set to the node. The removal
    /// goes through when the caller passes this same number as `acknowledgedSessions`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub live_sessions: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum NodesRefusalCode {
    UnsupportedVersion,
    /// An id is empty or carries `:` or `@`, the model-id grammar's delimiters.
    BadId,
    DuplicateId,
    DuplicateName,
    EmptyName,
    MissingModel,
    MissingProvider,
    PoolNodeOwnsModel,
    PlacementMismatch,
    BadMacs,
    UnknownNode,
    UnknownStrategy,
    EmptyChain,
    /// A chain names the same node twice.
    DuplicateEntry,
    ZeroWeight,
    NoRoleSet,
    InheritanceCycle,
    SharesTwoWays,
    NodeInUse,
    NodeIsForNewChats,
    StrategyIsForNewChats,
    StrategyIsForBuilds,
    LiveSessionsNotAcknowledged,
    RemovedOutsideRemoveNode,
    BuildIneligible,
}

/// The named reasons a strategy cannot drive a swarm build (Tier A, design §7.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum BuildRefusal {
    UnknownStrategy {
        strategy: String,
    },
    /// Neither Planning nor Build resolves (every role unset).
    NoRole {
        role: NodeRole,
    },
    UnknownNode {
        node: String,
    },
    /// A split: builds reach LeanZero MLX only through this Mac's single engine.
    Split {
        node: String,
    },
    /// A single on another Mac.
    Remote {
        node: String,
        mac: String,
    },
    /// A single on this Mac of a model other than the engine's configured one (`model`; absent
    /// when the engine has none configured).
    OtherModel {
        node: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
    },
    /// Planning's 1st is a cloud node: the engine replaces a cloud planner with an LM Studio
    /// model whenever LM Studio has anything loaded.
    CloudPlanner {
        node: String,
    },
    /// An endpoint node: builds reach cloud models through their swarm family only.
    Endpoint {
        node: String,
    },
    /// A pool node whose device has left the pool.
    LeftPool {
        node: String,
    },
    /// The projected device id is already another device's in the pool.
    DeviceIdTaken {
        node: String,
    },
    /// The `swarm` or `mlx_engine` block could not be read.
    Unreadable {
        what: String,
        error: String,
    },
}

/// One `BuildRefusal` with its English words.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BuildRefusalDto {
    pub reason: BuildRefusal,
    pub message: String,
}

/// Read the `nodes` config, with the pool adopted and every def resolved. Never writes config.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/nodes/read", response = NodesReadResponse)]
#[serde(rename_all = "camelCase")]
pub struct NodesReadRequest {}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct NodesReadResponse {
    /// The config as a write would store it: the stored key plus this read's pending adoptions
    /// (a fresh config when the key was never written).
    pub config: NodesConfig,
    /// Every def, resolved, in `config.defs` order.
    pub nodes: Vec<ResolvedNodeDef>,
    /// Whether the `nodes` key exists in config.yaml.
    pub stored: bool,
    /// LM Studio devices in the `swarm` block (not offered in this edition, left untouched).
    pub lm_studio_hidden: u32,
    /// Why the `swarm` block could not be read: nothing was adopted, pool nodes read
    /// `poolUnreadable`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub swarm_error: Option<String>,
    /// Facts about this read the UI may show (this Mac's name could not be read, …).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

/// Validate and store the whole `nodes` config. A write that changes `forNewChats` also writes
/// the global defaults (provider `swarm`, model `swarm` | `node:<id>` | `strategy:<id>`).
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/nodes/write", response = NodesWriteResponse)]
#[serde(rename_all = "camelCase")]
pub struct NodesWriteRequest {
    pub config: NodesConfig,
}

/// The answer of every write-shaped method: stored with no refusals, or refused (nothing
/// written) with every refusal, each the UI shows verbatim.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct NodesWriteResponse {
    pub written: bool,
    #[serde(default)]
    pub refusals: Vec<NodesRefusal>,
    /// The state after the call (the stored config when written; the unchanged one when
    /// refused).
    pub read: NodesReadResponse,
}

/// Remove a node. Refused while a strategy uses it (unless `alsoFromStrategies`) or while new
/// chats start on it (unless `andNewChatsAuto`); when live sessions are set to it the caller must
/// pass their count in `acknowledgedSessions`. A pool node's device id joins `declined`.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/nodes/removeNode", response = NodesWriteResponse)]
#[serde(rename_all = "camelCase")]
pub struct NodesRemoveNodeRequest {
    pub id: String,
    #[serde(default)]
    pub also_from_strategies: bool,
    #[serde(default)]
    pub and_new_chats_auto: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acknowledged_sessions: Option<u32>,
}

/// Remove a strategy. Refused while new chats start on it (unless `andNewChatsAuto`) or while
/// swarm builds use it (unless `andBuildsPool`).
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/nodes/removeStrategy", response = NodesWriteResponse)]
#[serde(rename_all = "camelCase")]
pub struct NodesRemoveStrategyRequest {
    pub id: String,
    #[serde(default)]
    pub and_new_chats_auto: bool,
    #[serde(default)]
    pub and_builds_pool: bool,
}

/// Whether a strategy can drive a swarm build (Tier A), and every reason when it cannot.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/nodes/buildEligibility",
    response = NodesBuildEligibilityResponse
)]
#[serde(rename_all = "camelCase")]
pub struct NodesBuildEligibilityRequest {
    pub strategy: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct NodesBuildEligibilityResponse {
    pub eligible: bool,
    #[serde(default)]
    pub reasons: Vec<BuildRefusalDto>,
    /// What a Tier A build cannot express, stated rather than hidden (LM Studio residents join,
    /// Testing/Frontend/Backend use Build, Planning's 2nd is not used).
    #[serde(default)]
    pub notes: Vec<String>,
}

/// How the way serving this Mac's goose runs, as the engine records know it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum NodesServingKind {
    /// This Mac's single engine.
    Single,
    /// The single engine on a peer, reached through this Mac's relay.
    RemoteSingle,
    /// The distributed engine across Macs. Its owner record carries neither tensor/pipeline nor
    /// the Macs' ids (only their names), so a split node is matched on its model and link.
    Split,
}

/// The way serving this Mac's goose now (one MLX way at a time, across all Macs).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NodesServingWayDto {
    pub kind: NodesServingKind,
    /// Placement keys of the Macs when the record knows them: `["local"]` for this Mac's single,
    /// `["link:<peer>"]` for a remote single; empty for a split.
    #[serde(default)]
    pub macs: Vec<String>,
    /// The split's link backend.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
    /// The model as the models folder names it.
    pub model_id: String,
    /// The id the engine serves it under.
    pub served_model_id: String,
    /// Display names of the Macs, in the way's order.
    pub mac_names: Vec<String>,
    /// While this way is loading: the engine's phase ("waitingForLoad" | "makingRoom" |
    /// "starting" | "loading" | "warming").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub load_phase: Option<String>,
}

/// One node's residency.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum NodeResidency {
    /// Its way and model serve this Mac's goose now.
    Serving,
    /// Its way is loading (the engine's phase when this Mac's engine reports one).
    Loading {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        phase: Option<String>,
    },
    /// The installed loader queued a demand for it; `reason` is the loader's words.
    Waiting { reason: String },
    /// Not running. `otherWay` names the way that serves instead, when one does.
    NotRunning {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        other_way: Option<String>,
    },
    /// The installed loader refused its last demand; `reason` is its words.
    RefusedLastTime { reason: String },
    /// Cloud and endpoint nodes are always loaded.
    AlwaysReady,
    /// Which way serves is unknown (an unreadable record); never guessed.
    Unknown { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NodeResidencyDto {
    pub node: String,
    pub residency: NodeResidency,
}

/// Per node: serving / loading / waiting / not running / refused last time — from engine truth
/// (the route record, the split's owner record, this Mac's engine) plus the installed loader.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/nodes/residency", response = NodesResidencyResponse)]
#[serde(rename_all = "camelCase")]
pub struct NodesResidencyRequest {}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct NodesResidencyResponse {
    pub nodes: Vec<NodeResidencyDto>,
    /// Absent = nothing serves this Mac's goose (or which way serves is unknown: `servingError`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub serving: Option<NodesServingWayDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub serving_error: Option<String>,
    /// Whether this goose process has a node loader installed. Without one, a not-loaded node
    /// is started from Run it.
    pub loader_installed: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NodeLoadPhasesMsDto {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub starting: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loading: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warming: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum NodeLoadOutcomeDto {
    Ready,
    Failed { words: String },
    CancelledAfterStop,
}

/// One measured load, as stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NodeLoadRecordDto {
    pub model: String,
    pub placement: MlxPlacementKeyDto,
    pub macs: Vec<String>,
    pub weights_bytes: u64,
    pub phases_ms: NodeLoadPhasesMsDto,
    pub total_ms: u64,
    pub file_cache_warm: bool,
    pub outcome: NodeLoadOutcomeDto,
    pub recorded_at_ms: u64,
}

/// The measured loads of one way of the node's model. `medianTotalMs` is over Ready loads only;
/// absent = not measured yet (never an estimate).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NodeLoadGroupDto {
    pub placement: MlxPlacementKeyDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub median_total_ms: Option<u64>,
    /// How many Ready loads the median is over.
    pub count: u32,
    pub records: Vec<NodeLoadRecordDto>,
}

/// The measured loads of a node's model, way and Macs (a node that follows this Mac's engine has
/// no one way: its model's loads are listed per way).
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/nodes/loadHistory", response = NodesLoadHistoryResponse)]
#[serde(rename_all = "camelCase")]
pub struct NodesLoadHistoryRequest {
    pub node: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct NodesLoadHistoryResponse {
    pub groups: Vec<NodeLoadGroupDto>,
    /// Lines of the load store that did not parse, by number.
    #[serde(default)]
    pub store_errors: Vec<String>,
    /// The store file.
    pub path: String,
}

/// A chain entry that did not take the turn, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NodeTriedDto {
    pub node: String,
    pub reason: String,
}

/// The node that served one turn, as the router leased it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NodeServedTurnDto {
    pub node: String,
    /// Absent for `node:` and `swarm` sessions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<NodeRole>,
    /// 1 = the chain's 1st.
    pub rank: u32,
    /// Why the 1st did not serve, when it did not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default)]
    pub tried: Vec<NodeTriedDto>,
    /// Set when this turn loaded the node: the measured load time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loaded_ms: Option<u64>,
    pub at_ms: u64,
}

/// The last served-turn record of a session (this process's, else the one persisted in the
/// session).
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/nodes/servedLast", response = NodesServedLastResponse)]
#[serde(rename_all = "camelCase")]
pub struct NodesServedLastRequest {
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct NodesServedLastResponse {
    /// Absent = no turn of this session was served through a node yet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record: Option<NodeServedTurnDto>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum NodeLoadRefusalCode {
    /// No node loader in this goose process: start the node in Run it.
    LoaderAbsent,
    UnknownNode,
    HeldByBuild,
    KeptLoaded,
    NeedsStep,
    Fit,
    LoadFailed,
    /// Which way serves is unknown (an unreadable record).
    Unknown,
}

/// The answer to "make this node servable".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum NodeEnsureServing {
    Ready,
    Wait {
        reason: String,
    },
    Refused {
        code: NodeLoadRefusalCode,
        reason: String,
    },
}

/// Make a node servable: `ready`, `wait(reason)` or `refused(code, reason)`. With no loader
/// installed, a not-serving MLX node answers the named refusal `loaderAbsent`.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/nodes/ensureServing",
    response = NodesEnsureServingResponse
)]
#[serde(rename_all = "camelCase")]
pub struct NodesEnsureServingRequest {
    pub node: String,
    /// The session demanding it, when a turn does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct NodesEnsureServingResponse {
    pub answer: NodeEnsureServing,
}
