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

/// Who demands a load. A turn is answered once its demand is settled (Ready or Refused); only a
/// card's Start is answered `Wait` at once and loads in the background. The two are named so a
/// turn can never reach the UI's `Wait` branch by lacking a session: a model call with no session
/// cannot build a demand at all (the router refuses it by name).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DemandFrom {
    /// A turn of this session (the router, for a chat or a delegate).
    Turn(String),
    /// Start on a node's card, with no session.
    Ui,
}

/// A demand to make a node servable.
#[derive(Debug, Clone)]
pub struct Demand {
    pub node: NodeDef,
    pub from: DemandFrom,
    pub role: Option<NodeRole>,
}

impl Demand {
    pub fn session_id(&self) -> Option<&str> {
        match &self.from {
            DemandFrom::Turn(session) => Some(session),
            DemandFrom::Ui => None,
        }
    }
}

/// A reply, or a pause of one, that the loader tracks until it is dropped.
pub type Hold = Box<dyn Send + Sync>;

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
    /// A SYNCHRONOUS delegate session runs inside its parent's reply (the parent is blocked in the
    /// tool call): its demand is the parent's own.
    fn note_child(&self, child_session: &str, parent_session: &str);
    /// A reply of `session` for as long as the hold lives — a BACKGROUND delegate's, which runs
    /// beside its parent's turn and can outlive it: its demand is its own (it waits for the
    /// parent's reply like any other's), and it keeps its way after the parent's reply ends.
    fn open_reply(&self, session: &str) -> Hold;
    /// `session`'s reply has no model call in flight while the hold lives (its turn waits on a
    /// background delegate in `load`): it holds nothing, so the delegate it waits for may switch.
    fn pause_reply(&self, session: &str) -> Hold;
    /// A switch queued before `session`'s reply opened, to a node other than `node`: its name. A
    /// scan of the loader's queue, cheap enough to ask before every MLX lease.
    fn queued_switch_ahead(&self, session: &str, node: &str) -> Option<String>;
    /// Wait, holding nothing, until no switch is queued ahead of `session`'s reply (see
    /// `queued_switch_ahead`) — woken by the queue changing, never by a clock.
    async fn wait_behind_queued_switches(&self, session: &str, node: &str);
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

/// A background delegate's own reply; `None` with no loader (nothing batches).
pub fn open_reply(session: &str) -> Option<Hold> {
    LOADER.get().map(|loader| loader.open_reply(session))
}

/// `session`'s reply holds nothing while the hold lives; `None` with no loader.
pub fn pause_reply(session: &str) -> Option<Hold> {
    LOADER.get().map(|loader| loader.pause_reply(session))
}

pub fn queued_switch_ahead(session: &str, node: &str) -> Option<String> {
    LOADER
        .get()
        .and_then(|loader| loader.queued_switch_ahead(session, node))
}

pub async fn wait_behind_queued_switches(session: &str, node: &str) {
    if let Some(loader) = LOADER.get() {
        loader.wait_behind_queued_switches(session, node).await;
    }
}

pub fn in_progress() -> Vec<LoaderActivity> {
    LOADER.get().map(|l| l.in_progress()).unwrap_or_default()
}
