//! The loader seam (design §4.2): how the router, summon and the ACP handlers reach the node
//! loader without S0 depending on it. The same shape as the router's `RouteLoad` /
//! `install_route_load`: a trait, one install point, and — with nothing installed — a NAMED
//! answer, never a silent default. S5 implements the trait in goosed and installs it; the CLI and
//! any process that never installs one refuse loads by name, and the two notes do nothing (with no
//! loader, nothing batches).

use std::sync::{Arc, OnceLock};

use async_trait::async_trait;
use goose_sdk_types::custom_requests::{
    MlxPlacementKeyDto, NodeEnsureServing, NodeLoadRefusalCode,
};

use super::{NodeDef, NodeRole};

/// A demand to make a node servable.
#[derive(Debug, Clone)]
pub struct Demand {
    pub node: NodeDef,
    /// The session whose turn demands it; `None` for a demand made from the UI.
    pub session_id: Option<String>,
    pub role: Option<NodeRole>,
}

/// What the loader is doing for one node right now (read by `nodes/residency`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoaderActivity {
    Loading { node: String, phase: Option<String> },
    Waiting { node: String, reason: String },
    RefusedLastTime { node: String, reason: String },
}

impl LoaderActivity {
    pub fn node(&self) -> &str {
        match self {
            LoaderActivity::Loading { node, .. }
            | LoaderActivity::Waiting { node, .. }
            | LoaderActivity::RefusedLastTime { node, .. } => node,
        }
    }
}

#[async_trait]
pub trait NodeLoader: Send + Sync {
    /// Make the node's way serve this Mac's goose: `Ready`, `Wait(reason)` or
    /// `Refused(code, reason)`. Only MLX nodes that are not serving reach the loader.
    async fn ensure_serving(&self, demand: Demand) -> NodeEnsureServing;
    /// The way an MLX lease of `session` used (the reply holds it for its whole life).
    fn note_lease(&self, session: &str, way: &MlxPlacementKeyDto);
    /// A delegate session runs inside its parent's reply: its demand is the parent's own.
    fn note_child(&self, child_session: &str, parent_session: &str);
    fn in_progress(&self) -> Vec<LoaderActivity>;
}

static LOADER: OnceLock<Arc<dyn NodeLoader>> = OnceLock::new();

/// Install this process's loader. The first install wins (one loader per process).
pub fn install_loader(loader: Arc<dyn NodeLoader>) -> bool {
    LOADER.set(loader).is_ok()
}

pub fn loader_installed() -> bool {
    LOADER.get().is_some()
}

/// The words of the named absence.
pub fn loader_absent_reason(node_name: &str) -> String {
    format!("loading nodes is not available in this goose process; start {node_name} in Run it")
}

pub async fn ensure_serving(demand: Demand) -> NodeEnsureServing {
    match LOADER.get() {
        Some(loader) => loader.ensure_serving(demand).await,
        None => NodeEnsureServing::Refused {
            code: NodeLoadRefusalCode::LoaderAbsent,
            reason: loader_absent_reason(&demand.node.name),
        },
    }
}

pub fn note_lease(session: &str, way: &MlxPlacementKeyDto) {
    if let Some(loader) = LOADER.get() {
        loader.note_lease(session, way);
    }
}

pub fn note_child(child_session: &str, parent_session: &str) {
    if let Some(loader) = LOADER.get() {
        loader.note_child(child_session, parent_session);
    }
}

pub fn in_progress() -> Vec<LoaderActivity> {
    LOADER.get().map(|l| l.in_progress()).unwrap_or_default()
}
