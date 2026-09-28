//! Nodes and strategies: the `nodes` config key and the pure rules over it (design:
//! local-edition/mlx/quality/DESIGN-NODES-AND-STRATEGIES.md, slice S0).
//!
//! - The types are the ACP contract (`goose_sdk_types::custom_requests`, re-exported here), so the
//!   config key, the wire and the desktop's generated mirror are one shape.
//! - `nodes` is read and written ONLY through the `nodes/*` ACP methods, and it never writes the
//!   `swarm` block: the pool keeps its one UI writer, and a pool node reads its model and provider
//!   THROUGH from its device, so the two can never diverge.
//! - A read never writes config: the pool is adopted on every read, and the adopted defs are
//!   stored by the next successful write.
//! - `forNewChats = auto` and `forBuilds = pool` are the defaults and change nothing: behaviour
//!   stays byte-identical until the user chooses otherwise.

pub mod acp;
pub mod project;
pub mod residency;
pub mod resolve;
pub mod seam;
pub mod served;

use std::collections::{BTreeSet, HashMap, HashSet};

use anyhow::{anyhow, Result};
use serde::Deserialize;

use crate::config::{Config, ConfigError};

pub use goose_sdk_types::custom_requests::{
    BuildRefusal, BuildRefusalDto, NodeChainEntry, NodeDef, NodeDefKind, NodeIfNotLoaded,
    NodeModelFrom, NodeOrigin, NodePlacement, NodeRole, NodeRoleEntry, NodeStrategy,
    NodeStrategyRoles, NodeWhen, NodesConfig, NodesForBuilds, NodesForNewChats, NodesReadResponse,
    NodesRefusal, NodesRefusalCode, NodesWriteResponse, ResolvedNodeDef,
};

/// The config key.
pub const CONFIG_KEY: &str = "nodes";
/// The one version of the `nodes` key this build reads and writes.
pub const VERSION: u32 = goose_sdk_types::custom_requests::NODES_CONFIG_VERSION;
/// The `swarm` config key the pool lives in (read here, never written).
pub const SWARM_KEY: &str = "swarm";
/// The provider every node and strategy session runs on.
pub const SWARM_PROVIDER: &str = "swarm";
/// The swarm device `engine` value of the supervised MLX engine.
const MLX_SIDECAR_ENGINE: &str = "mlx-sidecar";
/// The Mac key a placement uses for this Mac.
pub const THIS_MAC: &str = "local";

pub const ROLES: [NodeRole; 6] = [
    NodeRole::Chat,
    NodeRole::Planning,
    NodeRole::Build,
    NodeRole::Testing,
    NodeRole::Frontend,
    NodeRole::Backend,
];

/// A config nobody has written: new chats on Auto, builds on the pool (today's behaviour).
pub fn empty_config() -> NodesConfig {
    NodesConfig::default()
}

// ---------------------------------------------------------------------------------------------
// The model-id grammar on the `swarm` provider (design §7.1). The desktop's `model.ts` carries the
// same grammar and both suites run `nodes.fixture.json`'s `modelIds` cases.
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RouteModel {
    /// `swarm`: "Any node (Auto)", today's pool routing.
    Auto,
    /// `swarm-build`: a build on today's pool.
    Build,
    /// `swarm-build:strategy:<id>`: a build driven by that strategy.
    BuildStrategy { id: String },
    /// `node:<id>`: exactly that node.
    Node { id: String },
    /// `strategy:<id>` (the Chat chain) or `strategy:<id>@<role>`.
    Strategy {
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        role: Option<NodeRole>,
    },
}

/// What a model name on the `swarm` provider means. `None` = not a nodes route (the router's
/// existing handling of any other name applies); a malformed `node:`/`strategy:` id is `None` too,
/// so it can never be read as another node.
pub fn parse_route_model(name: &str) -> Option<RouteModel> {
    match name {
        "swarm" => return Some(RouteModel::Auto),
        "swarm-build" => return Some(RouteModel::Build),
        _ => {}
    }
    if let Some(id) = name.strip_prefix("swarm-build:strategy:") {
        return valid_id(id).then(|| RouteModel::BuildStrategy { id: id.to_string() });
    }
    if let Some(id) = name.strip_prefix("node:") {
        return valid_id(id).then(|| RouteModel::Node { id: id.to_string() });
    }
    let rest = name.strip_prefix("strategy:")?;
    let (id, role) = match rest.split_once('@') {
        Some((id, role)) => (id, Some(role_from_str(role)?)),
        None => (rest, None),
    };
    valid_id(id).then(|| RouteModel::Strategy {
        id: id.to_string(),
        role,
    })
}

pub fn format_route_model(route: &RouteModel) -> String {
    match route {
        RouteModel::Auto => "swarm".to_string(),
        RouteModel::Build => "swarm-build".to_string(),
        RouteModel::BuildStrategy { id } => format!("swarm-build:strategy:{id}"),
        RouteModel::Node { id } => format!("node:{id}"),
        RouteModel::Strategy { id, role: None } => format!("strategy:{id}"),
        RouteModel::Strategy { id, role: Some(r) } => format!("strategy:{id}@{}", role_str(*r)),
    }
}

/// The model name "new chats start on" writes as the global default.
pub fn new_chats_model(for_new_chats: &NodesForNewChats) -> String {
    format_route_model(&match for_new_chats {
        NodesForNewChats::Auto => RouteModel::Auto,
        NodesForNewChats::Node { id } => RouteModel::Node { id: id.clone() },
        NodesForNewChats::Strategy { id } => RouteModel::Strategy {
            id: id.clone(),
            role: None,
        },
    })
}

/// An id is non-empty and never carries the grammar's delimiters.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && !id.contains([':', '@']) && id.trim() == id
}

pub fn role_str(role: NodeRole) -> &'static str {
    match role {
        NodeRole::Chat => "chat",
        NodeRole::Planning => "planning",
        NodeRole::Build => "build",
        NodeRole::Testing => "testing",
        NodeRole::Frontend => "frontend",
        NodeRole::Backend => "backend",
    }
}

fn role_from_str(s: &str) -> Option<NodeRole> {
    ROLES.into_iter().find(|r| role_str(*r) == s)
}

// ---------------------------------------------------------------------------------------------
// Role inheritance (design §6.1): an unset role inherits; the UI says "Same as <role>".
// ---------------------------------------------------------------------------------------------

pub fn inherits_from(role: NodeRole) -> NodeRole {
    match role {
        NodeRole::Chat => NodeRole::Build,
        NodeRole::Planning | NodeRole::Build => NodeRole::Chat,
        NodeRole::Testing | NodeRole::Frontend | NodeRole::Backend => NodeRole::Build,
    }
}

/// The role whose entry `role` uses: itself when set, else the first set role up its inheritance.
/// `None` only when Chat and Build are both unset (the cycle validation refuses).
pub fn effective_role(roles: &NodeStrategyRoles, role: NodeRole) -> Option<NodeRole> {
    let mut seen = BTreeSet::new();
    let mut current = role;
    loop {
        if roles.get(current).is_some() {
            return Some(current);
        }
        if !seen.insert(current) {
            return None;
        }
        current = inherits_from(current);
    }
}

pub fn effective_entry(strategy: &NodeStrategy, role: NodeRole) -> Option<&NodeRoleEntry> {
    effective_role(&strategy.roles, role).and_then(|r| strategy.roles.get(r))
}

// ---------------------------------------------------------------------------------------------
// The pool, as the nodes read it (a view of `swarm.devices`; the block itself is never written).
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize)]
pub struct PoolView {
    #[serde(default)]
    pub devices: Vec<PoolDeviceView>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PoolDeviceView {
    pub id: String,
    #[serde(default)]
    pub model_id: String,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub engine: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceClass {
    LmStudio,
    Mlx,
    Cloud,
}

impl PoolDeviceView {
    /// Decided the way the engine and the router decide it: a cloud `provider` wins, then
    /// `engine`, then LM Studio.
    pub fn class(&self) -> DeviceClass {
        match (
            self.provider
                .as_deref()
                .filter(|p| !p.eq_ignore_ascii_case("lmstudio")),
            self.engine.as_deref(),
        ) {
            (Some(_), _) => DeviceClass::Cloud,
            (None, Some(MLX_SIDECAR_ENGINE)) => DeviceClass::Mlx,
            (None, _) => DeviceClass::LmStudio,
        }
    }
}

impl PoolView {
    pub fn device(&self, id: &str) -> Option<&PoolDeviceView> {
        self.devices.iter().find(|d| d.id == id)
    }
}

/// The `swarm` block as the nodes read it: `Ok(None)` = no block (no pool — nothing to adopt),
/// `Err` = the block exists and could not be read (named, never an empty pool).
pub fn read_pool(config: &Config) -> Result<Option<PoolView>, String> {
    match config.get_param::<PoolView>(SWARM_KEY) {
        Ok(pool) => Ok(Some(pool)),
        Err(ConfigError::NotFound(_)) => Ok(None),
        Err(e) => Err(format!("the `swarm` config block could not be read ({e})")),
    }
}

// ---------------------------------------------------------------------------------------------
// Adoption of today's pool (design §4.5).
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct Adoption {
    pub config: NodesConfig,
    /// Ids of the defs this adoption added.
    pub adopted: Vec<String>,
    /// LM Studio devices in the pool: not offered in this edition, left untouched.
    pub lm_studio: u32,
}

/// The name an adopted MLX pool node gets. It follows whatever THIS Mac's engine serves — on this
/// Mac alone, on another Mac, or split across several — so it is named after what it follows,
/// never after one Mac (Q-303: "Mihai Macbook engine" wore a "This Mac" chip above "Split across 2
/// Macs"). The card shows where it runs now beside the name.
pub const FOLLOWS_NODE_NAME: &str = "This Mac's engine";

/// A pool node still carrying the name adoption used to give (`<Mac name> engine`, and its
/// ` · <model>` / ` · <device>` / ` (n)` forms) gets the name adoption gives now, its suffix kept.
/// Only that exact generated name moves; a name the person typed is theirs and is never touched.
fn rename_followed_defaults(config: &mut NodesConfig, this_mac: &str) {
    let old_base = format!("{this_mac} engine");
    for index in 0..config.defs.len() {
        let def = &config.defs[index];
        let Some(device) = def.pool_device.clone() else {
            continue;
        };
        if def.origin != NodeOrigin::Pool
            || def.kind != NodeDefKind::Mlx
            || def.placement != Some(NodePlacement::Follows)
            || def.name.starts_with(FOLLOWS_NODE_NAME)
        {
            continue;
        }
        let Some(suffix) = def.name.strip_prefix(&old_base) else {
            continue;
        };
        if !(suffix.is_empty() || suffix.starts_with(" · ") || suffix.starts_with(" (")) {
            continue;
        }
        let wanted = format!("{FOLLOWS_NODE_NAME}{suffix}");
        config.defs[index].name = free_name(&config.defs, vec![wanted], &device);
    }
}

/// Adopt every pool device that has no node and was not declined, and move a pool node still named
/// by the old rule to today's name (`rename_followed_defaults`). Pure and idempotent: a second
/// adoption of its own output adopts and renames nothing. It never touches `swarm`.
pub fn adopt(pool: Option<&PoolView>, config: NodesConfig, this_mac: &str) -> Adoption {
    let mut config = config;
    rename_followed_defaults(&mut config, this_mac);
    let mut adopted = Vec::new();
    let mut lm_studio = 0;
    let Some(pool) = pool else {
        return Adoption {
            config,
            adopted,
            lm_studio,
        };
    };
    for device in &pool.devices {
        let class = device.class();
        if class == DeviceClass::LmStudio {
            lm_studio += 1;
            continue;
        }
        if config.declined.iter().any(|d| d == &device.id)
            || config
                .defs
                .iter()
                .any(|def| def.pool_device.as_deref() == Some(&device.id))
        {
            continue;
        }
        let id = free_id(&config.defs, &device.id);
        let (kind, placement, candidates) = match class {
            DeviceClass::Mlx => {
                let base = FOLLOWS_NODE_NAME.to_string();
                (
                    NodeDefKind::Mlx,
                    Some(NodePlacement::Follows),
                    vec![
                        base.clone(),
                        format!(
                            "{base} · {}",
                            goose_sidecar::model_identity::model_tag(&device.model_id)
                        ),
                        format!("{base} · {}", device.id),
                    ],
                )
            }
            _ => {
                let base = format!(
                    "{} · {}",
                    model_short(&device.model_id),
                    device.provider.as_deref().unwrap_or_default()
                );
                (
                    NodeDefKind::Cloud,
                    None,
                    vec![base.clone(), format!("{base} · {}", device.id)],
                )
            }
        };
        let name = free_name(&config.defs, candidates, &device.id);
        config.defs.push(NodeDef {
            id: id.clone(),
            name,
            kind,
            model: None,
            placement,
            goal: None,
            provider: None,
            keep_loaded: false,
            pool_device: Some(device.id.clone()),
            origin: NodeOrigin::Pool,
        });
        adopted.push(id);
    }
    Adoption {
        config,
        adopted,
        lm_studio,
    }
}

/// The device id, unless a def already holds it (a user node named like a device): then the id
/// with a `-pool` suffix, numbered until free. Deterministic, never dropped.
fn free_id(defs: &[NodeDef], device_id: &str) -> String {
    let taken = |id: &str| defs.iter().any(|d| d.id == id);
    if !taken(device_id) {
        return device_id.to_string();
    }
    let base = format!("{device_id}-pool");
    let mut id = base.clone();
    let mut n = 1;
    while taken(&id) {
        n += 1;
        id = format!("{base}-{n}");
    }
    id
}

/// The first candidate no def carries (Q-154: never two "mihai"); past them, the last one
/// numbered until free.
fn free_name(defs: &[NodeDef], candidates: Vec<String>, device_id: &str) -> String {
    let taken = |name: &str| defs.iter().any(|d| d.name == name);
    for name in &candidates {
        if !taken(name) {
            return name.clone();
        }
    }
    let last = candidates
        .last()
        .cloned()
        .unwrap_or_else(|| device_id.to_string());
    let mut n = 2;
    loop {
        let name = format!("{last} ({n})");
        if !taken(&name) {
            return name;
        }
        n += 1;
    }
}

/// A cloud model's short name: the last path segment of its id (`anthropic/claude-sonnet-4` →
/// `claude-sonnet-4`).
fn model_short(model_id: &str) -> &str {
    model_id.rsplit('/').next().unwrap_or(model_id)
}

// ---------------------------------------------------------------------------------------------
// Resolution: a pool node reads its model and provider through from its device.
// ---------------------------------------------------------------------------------------------

pub fn resolve_def(
    def: &NodeDef,
    pool: &Result<Option<PoolView>, String>,
    pending: bool,
) -> ResolvedNodeDef {
    let (model, provider, model_from) = match &def.pool_device {
        None => (def.model.clone(), def.provider.clone(), NodeModelFrom::Own),
        Some(device_id) => match pool {
            Err(error) => (
                None,
                None,
                NodeModelFrom::PoolUnreadable {
                    error: error.clone(),
                },
            ),
            Ok(pool) => match pool.as_ref().and_then(|p| p.device(device_id)) {
                Some(device) => (
                    Some(device.model_id.clone()),
                    match device.class() {
                        DeviceClass::Cloud => device.provider.clone(),
                        _ => None,
                    },
                    NodeModelFrom::Pool,
                ),
                None => (None, None, NodeModelFrom::LeftPool),
            },
        },
    };
    ResolvedNodeDef {
        def: def.clone(),
        model,
        provider,
        model_from,
        pending_adoption: pending,
    }
}

// ---------------------------------------------------------------------------------------------
// Validation (design §4.2, §6.1, §6.3). Every refusal is returned; the UI shows each verbatim.
// ---------------------------------------------------------------------------------------------

fn refusal(code: NodesRefusalCode, subject: Option<&str>, message: String) -> NodesRefusal {
    NodesRefusal {
        code,
        message,
        subject: subject.map(str::to_string),
        live_sessions: None,
    }
}

/// The way an MLX node names: its placement and model. `follows` nodes have none (they serve
/// whatever this Mac's engine serves and never trigger a load).
fn pinned_way(def: &NodeDef) -> Option<(&NodePlacement, Option<&str>)> {
    match (&def.kind, &def.placement) {
        (NodeDefKind::Mlx, Some(NodePlacement::Follows)) | (NodeDefKind::Mlx, None) => None,
        (NodeDefKind::Mlx, Some(p)) => Some((p, def.model.as_deref())),
        _ => None,
    }
}

pub fn placement_macs(placement: &NodePlacement) -> Option<(&[String], Option<&str>)> {
    match placement {
        NodePlacement::Single { macs, link }
        | NodePlacement::Tensor { macs, link }
        | NodePlacement::Pipeline { macs, link } => Some((macs, link.as_deref())),
        NodePlacement::Follows => None,
    }
}

pub fn validate(config: &NodesConfig) -> Vec<NodesRefusal> {
    use NodesRefusalCode as C;
    let mut out = Vec::new();
    if config.version != VERSION {
        out.push(refusal(
            C::UnsupportedVersion,
            None,
            format!(
                "this goose reads nodes version {VERSION}; the config says version {}",
                config.version
            ),
        ));
    }

    let mut ids = HashSet::new();
    let mut names = HashSet::new();
    for def in &config.defs {
        if !valid_id(&def.id) {
            out.push(refusal(
                C::BadId,
                Some(&def.id),
                format!("the node id '{}' is empty or carries ':' or '@'", def.id),
            ));
        }
        if !ids.insert(def.id.as_str()) {
            out.push(refusal(
                C::DuplicateId,
                Some(&def.id),
                format!("two nodes have the id '{}'", def.id),
            ));
        }
        if def.name.trim().is_empty() {
            out.push(refusal(
                C::EmptyName,
                Some(&def.id),
                format!("the node '{}' has no name", def.id),
            ));
        } else if !names.insert(def.name.as_str()) {
            out.push(refusal(
                C::DuplicateName,
                Some(&def.id),
                format!("two nodes are named \"{}\"", def.name),
            ));
        }
        validate_def(def, &mut out);
    }

    let mut strategy_ids = HashSet::new();
    let mut strategy_names = HashSet::new();
    let defs: HashMap<&str, &NodeDef> = config.defs.iter().map(|d| (d.id.as_str(), d)).collect();
    for strategy in &config.strategies {
        if !valid_id(&strategy.id) {
            out.push(refusal(
                C::BadId,
                Some(&strategy.id),
                format!(
                    "the strategy id '{}' is empty or carries ':' or '@'",
                    strategy.id
                ),
            ));
        }
        if !strategy_ids.insert(strategy.id.as_str()) {
            out.push(refusal(
                C::DuplicateId,
                Some(&strategy.id),
                format!("two strategies have the id '{}'", strategy.id),
            ));
        }
        if strategy.name.trim().is_empty() {
            out.push(refusal(
                C::EmptyName,
                Some(&strategy.id),
                format!("the strategy '{}' has no name", strategy.id),
            ));
        } else if !strategy_names.insert(strategy.name.as_str()) {
            out.push(refusal(
                C::DuplicateName,
                Some(&strategy.id),
                format!("two strategies are named \"{}\"", strategy.name),
            ));
        }
        validate_strategy(strategy, &defs, &mut out);
    }

    match &config.for_new_chats {
        NodesForNewChats::Auto => {}
        NodesForNewChats::Node { id } if !defs.contains_key(id.as_str()) => out.push(refusal(
            C::UnknownNode,
            Some(id),
            format!("new chats are set to start on the node '{id}', which does not exist"),
        )),
        NodesForNewChats::Strategy { id } if !strategy_ids.contains(id.as_str()) => {
            out.push(refusal(
                C::UnknownStrategy,
                Some(id),
                format!("new chats are set to start on the strategy '{id}', which does not exist"),
            ))
        }
        _ => {}
    }
    if let NodesForBuilds::Strategy { id } = &config.for_builds {
        if !strategy_ids.contains(id.as_str()) {
            out.push(refusal(
                C::UnknownStrategy,
                Some(id),
                format!("swarm builds are set to use the strategy '{id}', which does not exist"),
            ));
        }
    }
    out
}

fn validate_def(def: &NodeDef, out: &mut Vec<NodesRefusal>) {
    use NodesRefusalCode as C;
    let label = format!("\"{}\"", def.name);
    if def.pool_device.is_some() {
        if def.model.is_some() || def.provider.is_some() {
            out.push(refusal(
                C::PoolNodeOwnsModel,
                Some(&def.id),
                format!(
                    "{label} reads its model and provider from your swarm pool; change them in your swarm pool"
                ),
            ));
        }
    } else {
        if def.model.as_deref().is_none_or(|m| m.trim().is_empty()) {
            out.push(refusal(
                C::MissingModel,
                Some(&def.id),
                format!("{label} names no model"),
            ));
        }
        if def.kind != NodeDefKind::Mlx
            && def.provider.as_deref().is_none_or(|p| p.trim().is_empty())
        {
            out.push(refusal(
                C::MissingProvider,
                Some(&def.id),
                format!("{label} names no provider"),
            ));
        }
    }
    match (&def.kind, &def.placement) {
        (NodeDefKind::Mlx, None) => out.push(refusal(
            C::PlacementMismatch,
            Some(&def.id),
            format!("{label} is an MLX node with no way to run"),
        )),
        (NodeDefKind::Mlx, Some(NodePlacement::Follows)) if def.pool_device.is_none() => {
            out.push(refusal(
                C::PlacementMismatch,
                Some(&def.id),
                format!(
                    "{label} follows this Mac's engine, which only a node from your swarm pool can do"
                ),
            ))
        }
        (NodeDefKind::Mlx, Some(p)) => validate_macs(def, p, &label, out),
        (_, Some(_)) => out.push(refusal(
            C::PlacementMismatch,
            Some(&def.id),
            format!("{label} is not an MLX node, so it has no way on your Macs"),
        )),
        (_, None) => {}
    }
}

/// `placement.macs` names Macs as Run it does: `local` is this Mac, anything else a peer. An
/// unknown peer is KEPT (shown "not connected" by the cards), never refused or dropped; only a
/// shape no way can have is refused.
fn validate_macs(
    def: &NodeDef,
    placement: &NodePlacement,
    label: &str,
    out: &mut Vec<NodesRefusal>,
) {
    let Some((macs, _)) = placement_macs(placement) else {
        return;
    };
    let mut refuse = |why: String| {
        out.push(refusal(
            NodesRefusalCode::BadMacs,
            Some(&def.id),
            format!("{label}: {why}"),
        ))
    };
    if macs.iter().any(|m| m.trim().is_empty()) {
        refuse("a Mac in its way has no name".to_string());
    }
    let distinct: HashSet<&str> = macs.iter().map(String::as_str).collect();
    if distinct.len() != macs.len() {
        refuse("its way names the same Mac twice".to_string());
    }
    match placement {
        NodePlacement::Single { .. } if macs.len() != 1 => {
            refuse(format!("a single way runs on one Mac, not {}", macs.len()))
        }
        NodePlacement::Tensor { .. } | NodePlacement::Pipeline { .. } if macs.len() < 2 => {
            refuse("a split runs across at least two Macs".to_string())
        }
        _ => {}
    }
}

fn validate_strategy(
    strategy: &NodeStrategy,
    defs: &HashMap<&str, &NodeDef>,
    out: &mut Vec<NodesRefusal>,
) {
    use NodesRefusalCode as C;
    let label = format!("\"{}\"", strategy.name);
    if ROLES.iter().all(|r| strategy.roles.get(*r).is_none()) {
        out.push(refusal(
            C::NoRoleSet,
            Some(&strategy.id),
            format!("{label} sets no role"),
        ));
    } else if strategy.roles.chat.is_none() && strategy.roles.build.is_none() {
        out.push(refusal(
            C::InheritanceCycle,
            Some(&strategy.id),
            format!(
                "{label} sets neither Chat nor Build; each inherits from the other, so one of them must be set"
            ),
        ));
    }
    for role in ROLES {
        let Some(entry) = strategy.roles.get(role) else {
            continue;
        };
        let role_name = role_str(role);
        if entry.chain.is_empty() {
            out.push(refusal(
                C::EmptyChain,
                Some(&strategy.id),
                format!("{label}: {role_name} names no node"),
            ));
        }
        let mut seen = HashSet::new();
        for link in &entry.chain {
            if !defs.contains_key(link.node.as_str()) {
                out.push(refusal(
                    C::UnknownNode,
                    Some(&strategy.id),
                    format!(
                        "{label}: {role_name} names the node '{}', which does not exist",
                        link.node
                    ),
                ));
            }
            if !seen.insert(link.node.as_str()) {
                out.push(refusal(
                    C::DuplicateEntry,
                    Some(&strategy.id),
                    format!("{label}: {role_name} names '{}' twice", link.node),
                ));
            }
            if link.weight == 0 {
                out.push(refusal(
                    C::ZeroWeight,
                    Some(&strategy.id),
                    format!(
                        "{label}: {role_name} gives '{}' a weight of 0; a weight is 1 or more",
                        link.node
                    ),
                ));
            }
        }
        if entry.when != NodeWhen::Failover {
            if let Some((a, b)) = two_mlx_ways(entry, defs) {
                out.push(refusal(
                    C::SharesTwoWays,
                    Some(&strategy.id),
                    format!(
                        "{label}: sharing {role_name} between {a} and {b} would stop one to load the other on every turn"
                    ),
                ));
            }
        }
    }
}

/// The first two MLX entries of a chain whose ways differ. One MLX way serves this Mac's goose at
/// a time, so `share` and `overflow` across two ways would swap on every alternation.
fn two_mlx_ways(entry: &NodeRoleEntry, defs: &HashMap<&str, &NodeDef>) -> Option<(String, String)> {
    let pinned: Vec<(&NodeDef, (&NodePlacement, Option<&str>))> = entry
        .chain
        .iter()
        .filter_map(|link| defs.get(link.node.as_str()))
        .filter_map(|def| pinned_way(def).map(|way| (*def, way)))
        .collect();
    let (first, first_way) = pinned.first()?;
    pinned
        .iter()
        .find(|(_, way)| way != first_way)
        .map(|(other, _)| (first.name.clone(), other.name.clone()))
}

// ---------------------------------------------------------------------------------------------
// The store: read (never writes) and the one write door.
// ---------------------------------------------------------------------------------------------

/// The stored `nodes` key: `Ok(None)` = never written; an unreadable key is an error, never an
/// empty config.
pub fn read_stored(config: &Config) -> Result<Option<NodesConfig>> {
    match config.get_param::<NodesConfig>(CONFIG_KEY) {
        Ok(stored) => Ok(Some(stored)),
        Err(ConfigError::NotFound(_)) => Ok(None),
        Err(e) => Err(anyhow!(
            "the `{CONFIG_KEY}` config key could not be read ({e}); nothing was changed"
        )),
    }
}

/// This Mac's name as adoption names its engine: the name, or — when it could not be read —
/// "This Mac", with the reason in `notes`.
pub fn mac_name_for_adoption(this_mac: Result<String, String>, notes: &mut Vec<String>) -> String {
    match this_mac {
        Ok(name) => name,
        Err(e) => {
            notes.push(format!(
                "this Mac's name could not be read ({e}); nodes adopted from your swarm pool say \"This Mac\""
            ));
            "This Mac".to_string()
        }
    }
}

/// The config, the pool adopted, every def resolved. Never writes.
pub fn read(config: &Config, this_mac: Result<String, String>) -> Result<NodesReadResponse> {
    let stored = read_stored(config)?;
    let is_stored = stored.is_some();
    let pool = read_pool(config);
    let mut notes = Vec::new();
    let mac = mac_name_for_adoption(this_mac, &mut notes);
    let base = stored.unwrap_or_else(empty_config);
    let adoption = match &pool {
        Ok(view) => adopt(view.as_ref(), base, &mac),
        Err(_) => Adoption {
            config: base,
            adopted: Vec::new(),
            lm_studio: 0,
        },
    };
    Ok(respond(adoption, &pool, is_stored, notes))
}

fn respond(
    adoption: Adoption,
    pool: &Result<Option<PoolView>, String>,
    stored: bool,
    notes: Vec<String>,
) -> NodesReadResponse {
    let nodes = adoption
        .config
        .defs
        .iter()
        .map(|def| resolve_def(def, pool, adoption.adopted.contains(&def.id)))
        .collect();
    NodesReadResponse {
        config: adoption.config,
        nodes,
        stored,
        lm_studio_hidden: adoption.lm_studio,
        swarm_error: pool.as_ref().err().cloned(),
        notes,
    }
}

/// What a write needs besides the config: the facts `forBuilds` eligibility reads.
pub struct WriteFacts<'a> {
    pub this_mac: Result<String, String>,
    pub engine: &'a Result<goose_sidecar::engine::EngineSettings, String>,
}

/// The one write door. Validates the whole config (every refusal returned, nothing written on
/// any), refuses dropping a node outside `removeNode`, stores `nodes`, and — only when
/// `forNewChats` CHANGED — writes the global defaults in the same call.
pub fn write(
    config: &Config,
    incoming: NodesConfig,
    facts: WriteFacts,
) -> Result<NodesWriteResponse> {
    let current = read(config, facts.this_mac.clone())?;
    let incoming = match read_pool(config) {
        Ok(view) => {
            let mut notes = Vec::new();
            let mac = mac_name_for_adoption(facts.this_mac.clone(), &mut notes);
            adopt(view.as_ref(), incoming, &mac).config
        }
        Err(_) => incoming,
    };
    let kept: HashSet<&str> = incoming.defs.iter().map(|d| d.id.as_str()).collect();
    let mut refusals: Vec<NodesRefusal> = current
        .config
        .defs
        .iter()
        .filter(|d| !kept.contains(d.id.as_str()))
        .map(|d| {
            refusal(
                NodesRefusalCode::RemovedOutsideRemoveNode,
                Some(&d.id),
                format!(
                    "\"{}\" is missing from this write; a node is removed with Remove, which checks what uses it",
                    d.name
                ),
            )
        })
        .collect();
    let previous = current.config.for_new_chats.clone();
    store(config, incoming, &previous, facts, &mut refusals, current)
}

fn store(
    config: &Config,
    incoming: NodesConfig,
    previous_for_new_chats: &NodesForNewChats,
    facts: WriteFacts,
    refusals: &mut Vec<NodesRefusal>,
    current: NodesReadResponse,
) -> Result<NodesWriteResponse> {
    let pool = read_pool(config);
    let mut notes = Vec::new();
    let mac = mac_name_for_adoption(facts.this_mac.clone(), &mut notes);
    let incoming = match &pool {
        Ok(view) => adopt(view.as_ref(), incoming, &mac).config,
        Err(_) => incoming,
    };
    refusals.extend(validate(&incoming));
    if let NodesForBuilds::Strategy { id } = &incoming.for_builds {
        let swarm = config.get_param::<serde_json::Value>(SWARM_KEY);
        let inputs = project::BuildInputs::from_reads(swarm, facts.engine, &incoming);
        if let Err(reasons) = project::build_eligibility(&inputs, id) {
            refusals.extend(reasons.into_iter().map(|r| {
                refusal(
                    NodesRefusalCode::BuildIneligible,
                    Some(id),
                    format!("swarm builds can't use this strategy: {}", r.message),
                )
            }));
        }
    }
    if !refusals.is_empty() {
        return Ok(NodesWriteResponse {
            written: false,
            refusals: std::mem::take(refusals),
            read: current,
        });
    }
    config
        .set_param(CONFIG_KEY, &incoming)
        .map_err(|e| anyhow!("the nodes could not be saved ({e})"))?;
    if &incoming.for_new_chats != previous_for_new_chats {
        let model = new_chats_model(&incoming.for_new_chats);
        crate::config::set_active_provider(config, SWARM_PROVIDER, &model).map_err(|e| {
            anyhow!(
                "the nodes were saved, but new chats could not be set to start on {model} ({e})"
            )
        })?;
    }
    Ok(NodesWriteResponse {
        written: true,
        refusals: Vec::new(),
        read: read(config, facts.this_mac)?,
    })
}

/// What a removal changes, before the write door validates and stores it.
pub struct RemoveNode<'a> {
    pub id: &'a str,
    pub also_from_strategies: bool,
    pub and_new_chats_auto: bool,
    pub acknowledged_sessions: Option<u32>,
    /// Sessions whose model is `node:<id>` (counted by the caller from the session store).
    pub live_sessions: u32,
}

pub fn remove_node(
    config: &Config,
    req: RemoveNode,
    facts: WriteFacts,
) -> Result<NodesWriteResponse> {
    use NodesRefusalCode as C;
    let current = read(config, facts.this_mac.clone())?;
    let mut next = current.config.clone();
    let mut refusals = Vec::new();
    let Some(index) = next.defs.iter().position(|d| d.id == req.id) else {
        refusals.push(refusal(
            C::UnknownNode,
            Some(req.id),
            format!("there is no node '{}'", req.id),
        ));
        return Ok(NodesWriteResponse {
            written: false,
            refusals,
            read: current,
        });
    };
    let def = next.defs.remove(index);
    let users: Vec<String> = next
        .strategies
        .iter()
        .filter(|s| {
            ROLES
                .iter()
                .filter_map(|r| s.roles.get(*r))
                .any(|e| e.chain.iter().any(|l| l.node == req.id))
        })
        .map(|s| s.name.clone())
        .collect();
    if !users.is_empty() {
        if req.also_from_strategies {
            for strategy in &mut next.strategies {
                for role in ROLES {
                    let slot = strategy.roles.get_mut(role);
                    if let Some(entry) = slot {
                        entry.chain.retain(|l| l.node != req.id);
                        if entry.chain.is_empty() {
                            *slot = None;
                        }
                    }
                }
            }
        } else {
            refusals.push(refusal(
                C::NodeInUse,
                Some(req.id),
                format!(
                    "\"{}\" is used by {}; remove it from those strategies too",
                    def.name,
                    users.join(", ")
                ),
            ));
        }
    }
    if next.for_new_chats
        == (NodesForNewChats::Node {
            id: req.id.to_string(),
        })
    {
        if req.and_new_chats_auto {
            next.for_new_chats = NodesForNewChats::Auto;
        } else {
            refusals.push(refusal(
                C::NodeIsForNewChats,
                Some(req.id),
                format!(
                    "new chats start on \"{}\"; remove it and start new chats on Any node (Auto)",
                    def.name
                ),
            ));
        }
    }
    if req.live_sessions > 0 && req.acknowledged_sessions != Some(req.live_sessions) {
        refusals.push(NodesRefusal {
            live_sessions: Some(req.live_sessions),
            ..refusal(
                C::LiveSessionsNotAcknowledged,
                Some(req.id),
                format!(
                    "{} {} set to \"{}\"; their next message will say it was removed",
                    req.live_sessions,
                    if req.live_sessions == 1 {
                        "chat is"
                    } else {
                        "chats are"
                    },
                    def.name
                ),
            )
        });
    }
    if let Some(device) = &def.pool_device {
        if !next.declined.contains(device) {
            next.declined.push(device.clone());
        }
    }
    let previous = current.config.for_new_chats.clone();
    store(config, next, &previous, facts, &mut refusals, current)
}

/// The config with `draft` in place of the strategy `id` (appended when none has it yet) — what the
/// editor asks eligibility about before Save (Q-311). Never stored.
pub fn with_draft_strategy(config: &mut NodesConfig, id: &str, draft: NodeStrategy) {
    let draft = NodeStrategy {
        id: id.to_string(),
        ..draft
    };
    match config.strategies.iter_mut().find(|s| s.id == id) {
        Some(stored) => *stored = draft,
        None => config.strategies.push(draft),
    }
}

pub struct RemoveStrategy<'a> {
    pub id: &'a str,
    pub and_new_chats_auto: bool,
    pub and_builds_pool: bool,
}

pub fn remove_strategy(
    config: &Config,
    req: RemoveStrategy,
    facts: WriteFacts,
) -> Result<NodesWriteResponse> {
    use NodesRefusalCode as C;
    let current = read(config, facts.this_mac.clone())?;
    let mut next = current.config.clone();
    let mut refusals = Vec::new();
    let Some(index) = next.strategies.iter().position(|s| s.id == req.id) else {
        refusals.push(refusal(
            C::UnknownStrategy,
            Some(req.id),
            format!("there is no strategy '{}'", req.id),
        ));
        return Ok(NodesWriteResponse {
            written: false,
            refusals,
            read: current,
        });
    };
    let strategy = next.strategies.remove(index);
    if next.for_new_chats
        == (NodesForNewChats::Strategy {
            id: req.id.to_string(),
        })
    {
        if req.and_new_chats_auto {
            next.for_new_chats = NodesForNewChats::Auto;
        } else {
            refusals.push(refusal(
                C::StrategyIsForNewChats,
                Some(req.id),
                format!(
                    "new chats start on \"{}\"; remove it and start new chats on Any node (Auto)",
                    strategy.name
                ),
            ));
        }
    }
    if next.for_builds
        == (NodesForBuilds::Strategy {
            id: req.id.to_string(),
        })
    {
        if req.and_builds_pool {
            next.for_builds = NodesForBuilds::Pool;
        } else {
            refusals.push(refusal(
                C::StrategyIsForBuilds,
                Some(req.id),
                format!(
                    "swarm builds use \"{}\"; remove it and let swarm builds use your swarm pool",
                    strategy.name
                ),
            ));
        }
    }
    let previous = current.config.for_new_chats.clone();
    store(config, next, &previous, facts, &mut refusals, current)
}

#[cfg(test)]
mod tests;
