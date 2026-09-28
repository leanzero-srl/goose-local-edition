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

/// What a not-loaded MLX entry under `load` does when its Mac is serving ANOTHER node for other
/// work (Q-428, the owner: "the strategy should have the option hopefully to avoid interrupting a
/// node doing its thing"). "Serving another node" is the loader's own fact: a way this switch
/// would stop has a reply running on it, or an open chat whose last turn was served on it — never
/// a clock. A way nobody uses is not "serving another node": it is switched under every setting.
/// A running reply is never cut under any of them (design §6.4 step 7).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum NodeIfServingOther {
    /// The next chain entry takes the work; nothing is stopped.
    UseNext,
    /// The work waits until the other node's chats are done with it (their replies end, and each
    /// chat closes or moves to another node), then loads.
    Wait,
    /// Load it: the other node is stopped once its running replies end.
    ///
    /// MIGRATION (Q-428): a role entry saved before the setting existed carries none and reads
    /// this — exactly the behaviour it had. New entries are written with [`Self::for_new_chain`].
    #[default]
    TakeOver,
}

impl NodeIfServingOther {
    pub fn is_take_over(&self) -> bool {
        *self == NodeIfServingOther::TakeOver
    }

    /// A new role entry's setting: the next node when the chain has one, otherwise wait.
    pub fn for_new_chain(entries: usize) -> Self {
        if entries > 1 {
            NodeIfServingOther::UseNext
        } else {
            NodeIfServingOther::Wait
        }
    }
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
    /// Read only under `ifNotLoaded: load`. Absent = saved before Q-428 = `takeOver`, and
    /// `takeOver` is written as absent, so a config saved before Q-428 round-trips unchanged.
    #[serde(default, skip_serializing_if = "NodeIfServingOther::is_take_over")]
    pub if_serving_other: NodeIfServingOther,
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
    /// Set when the strategy is ONE chat's node set (Q-359): the session it belongs to. Made only by
    /// `nodes/setChatNodes`; never what new chats start on or what swarm builds use; removed with
    /// its chat. "Save as a strategy" names it and clears this.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat: Option<String>,
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
    /// A chat's node set is that chat's alone: it is never what new chats start on or what swarm
    /// builds use.
    ChatNodeSetNotShared,
    /// Two node sets name the same chat, or one names no chat.
    BadChatNodeSet,
    /// A node is in chats' node sets; `alsoFromChatNodeSets` takes it out of them.
    NodeInChatNodeSets,
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
    /// "Also take it out of N chats' node sets": a set left empty goes, and its chat is told on its
    /// next message.
    #[serde(default)]
    pub also_from_chat_node_sets: bool,
}

/// One chat's own nodes (Q-359): `nodes[0]` answers the chat, delegates share every node of the
/// set (Build, `share`, weight 1 each); with `answerOnNext` the chat fails over down the set when
/// its 1st can't run. goosed builds the chat's strategy and sets the chat's model to
/// `strategy:<id>` in the same call. An empty `nodes` removes the chat's set and leaves its model
/// alone (the caller has already moved the chat to what it runs on next).
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(method = "_goose/unstable/nodes/setChatNodes", response = NodesSetChatNodesResponse)]
#[serde(rename_all = "camelCase")]
pub struct NodesSetChatNodesRequest {
    /// The chat's session id.
    pub session: String,
    pub nodes: Vec<String>,
    #[serde(default)]
    pub answer_on_next: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct NodesSetChatNodesResponse {
    pub write: NodesWriteResponse,
    /// The model the chat now runs on (`strategy:<id>`), set in this call; absent when nothing was
    /// written or the set was removed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
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
    /// The strategy as the editor holds it now, unsaved: checked in place of the stored one (or as
    /// a new one), so the editor answers while the person edits instead of after Save (Q-311).
    /// Nothing is written.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draft: Option<NodeStrategy>,
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
    /// The distributed engine across Macs. Its owner record names its Macs in rank order (its
    /// way), so a split node is matched on its Macs, link and model.
    Split,
}

/// The way serving this Mac's goose now (one MLX way at a time, across all Macs).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NodesServingWayDto {
    pub kind: NodesServingKind,
    /// Placement keys of the Macs when the record knows them: `["local"]` for this Mac's single,
    /// `["link:<peer>"]` for a remote single, the split's in rank order (none from an older owner).
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
        /// The sessions whose demands the installed loader loads it for (Q-382: a delegate's
        /// card says "Loading {node} for this delegate"). Empty when no loader demand is behind
        /// the load (Run it, a restore, a card's Start).
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        demanded_by: Vec<String>,
    },
    /// The installed loader queued a demand for it; `reason` is the loader's words. `replies` is
    /// set when what it waits on is replies on a way the switch would stop (design §8.7
    /// `nodes.turnWaiting`).
    Waiting {
        reason: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        replies: Option<NodeRepliesWaitDto>,
        /// Set when the demand waits because its role says `wait` while the Mac serves another
        /// node for chats that are between replies (Q-428).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        serving_other: Option<NodeServingOtherDto>,
    },
    /// Not running. `otherWay` names the way that serves instead, when one does.
    NotRunning {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        other_way: Option<String>,
    },
    /// The installed loader refused its last demand; `reason` is its words, `facts` what the
    /// refusal names (design §8.7 `nodes.refused*` / `nodes.loadFailed`) when it is one of those.
    RefusedLastTime {
        reason: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        facts: Option<NodeRefusalFactsDto>,
    },
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
    /// The measured loads of the node's own way and model (the load store's Ready median);
    /// absent = not measured yet, or a node with no one way — never an estimate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub load: Option<NodeLoadMedianDto>,
}

/// Replies on a way the switch would stop, which the loader waits for before it stops it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NodeRepliesWaitDto {
    /// The way in the loader's words ("this Mac's engine").
    pub way: String,
    /// The nodes that name that way (ids), so a surface names it as the Nodes page does.
    #[serde(default)]
    pub way_nodes: Vec<String>,
    /// The replies open on it that the switch waits for.
    pub count: u32,
    /// The chats those replies answer, by the names the person sees (Q-430).
    #[serde(default)]
    pub chats: Vec<String>,
}

/// The Mac a node runs on is serving another node for other chats (Q-428): what a turn line, a
/// refusal and a wait name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NodeServingOtherDto {
    /// The Mac(s) the wanted node shares with the way serving now, by name ("Work's Mac Studio").
    pub mac: String,
    /// The node serving now, as the Nodes page names it (the way's words when no node names it).
    pub serving: String,
    /// The chats it serves, by name: running a reply on it, or last served on it and still open.
    pub chats: Vec<String>,
    /// How many of their replies run on it now (0 = every one of those chats is between replies).
    pub replies: u32,
}

/// What a refusal names, for the refusals the composer words (design §8.7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum NodeRefusalFactsDto {
    /// A way the switch would stop belongs to a node kept loaded.
    KeptLoaded {
        /// The kept node's id, and its name.
        kept_node: String,
        kept: String,
        /// Where it is loaded, in the loader's words ("this Mac").
        mac: String,
    },
    /// A swarm build holds the engine; `way` is the way it holds, in the loader's words.
    HeldByBuild { way: String },
    /// The fit rule refused it on `mac`; `verdict` is the fit's own message.
    Fit { mac: String, verdict: String },
    /// The switch ran and the node's way failed to start; `words` are the engine's.
    LoadFailed { words: String },
    /// Its Mac serves another node for other chats and the role says `useNext` (Q-428).
    ServingOther {
        mac: String,
        serving: String,
        chats: Vec<String>,
        replies: u32,
    },
}

impl NodeServingOtherDto {
    pub fn as_facts(&self) -> NodeRefusalFactsDto {
        NodeRefusalFactsDto::ServingOther {
            mac: self.mac.clone(),
            serving: self.serving.clone(),
            chats: self.chats.clone(),
            replies: self.replies,
        }
    }

    pub fn of_facts(facts: &NodeRefusalFactsDto) -> Option<Self> {
        match facts {
            NodeRefusalFactsDto::ServingOther {
                mac,
                serving,
                chats,
                replies,
            } => Some(NodeServingOtherDto {
                mac: mac.clone(),
                serving: serving.clone(),
                chats: chats.clone(),
                replies: *replies,
            }),
            _ => None,
        }
    }
}

/// The median of a node's measured Ready loads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NodeLoadMedianDto {
    pub median_ms: u64,
    /// How many Ready loads it is the median of.
    pub count: u32,
}

/// A node whose way the loader stopped to load another (design §8.7 `nodes.displacedNotice`),
/// kept until that node serves again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NodeDisplacedDto {
    /// The node that was stopped.
    pub node: String,
    /// The node it was stopped for.
    pub for_node: String,
    /// The chat whose turn asked for `forNode` (its root session); absent = a Start on its card.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by_session: Option<String>,
    /// That chat's name as the person sees it; absent without `bySession`, or when unreadable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by_chat: Option<String>,
    /// Set once `forNode` failed to load: the engine's words.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failed: Option<String>,
    pub at_ms: u64,
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
    /// Nodes this process's loader stopped for another node, until each serves again.
    #[serde(default)]
    pub displaced: Vec<NodeDisplacedDto>,
    /// Why no node carries a measured load: the load store could not be read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loads_error: Option<String>,
    /// Replies of this process waiting behind a switch queued before they began (Q-442). Their
    /// own node still serves, so no node's residency can say it: each names its chat.
    #[serde(default)]
    pub behind_switches: Vec<NodeBehindSwitchDto>,
}

/// A chat's reply waits behind a switch to another node that was asked for before the reply
/// began (design §6.4 step 1: new replies never starve a queued switch). The reply holds nothing
/// meanwhile; when the switch leaves the queue the turn routes again on what serves then.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NodeBehindSwitchDto {
    /// The chat whose reply waits (a delegate's reply is its chat's).
    pub session: String,
    /// The node the reply was going to use.
    pub node: String,
    /// The node the queued switch loads, by id and by the name the Nodes page shows.
    pub switch_to: String,
    pub switch_to_name: String,
    /// The chat the switch is for, by name; empty for a Start on a node's card.
    #[serde(default)]
    pub chats: Vec<String>,
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
    /// The person asked this one turn to answer past the chain's 1st ("Answer on {next} for now",
    /// Q-381): the 1st was passed over by that ask, and the next turn goes back to it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub asked_for_this_turn: bool,
    /// The 1st was passed over because its Mac serves another node for other chats and the role
    /// says `useNext` (Q-428): what the turn line names.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub serving_other: Option<NodeServingOtherDto>,
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
    /// Its Mac serves another node for other chats, and the role says use the next node (Q-428).
    ServingOther,
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
        /// What the refusal names, when it is one the surfaces word (the router's turn line reads
        /// `servingOther`).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        facts: Option<NodeRefusalFactsDto>,
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

/// Q-443: take the Mac over for ONE turn. The demand `sessionId`'s chat has queued for `node`
/// under the role's `wait` stops waiting for the other node's chats and loads (a reply running
/// there still finishes first). The strategy's setting is not changed.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/nodes/takeOverNow",
    response = NodesTakeOverNowResponse
)]
#[serde(rename_all = "camelCase")]
pub struct NodesTakeOverNowRequest {
    pub node: String,
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct NodesTakeOverNowResponse {
    /// False when no turn of that chat waits for the node under `wait` any more (it loaded,
    /// moved on or ended).
    pub taken: bool,
}
