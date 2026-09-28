//! The node loader (design DESIGN-NODES-AND-STRATEGIES.md §6.4, slice S5): makes a node's way
//! serve this Mac's goose when a demand asks for it, under the product's real rule — ONE MLX way
//! serves this Mac's goose at a time, across all Macs. Installed through S0's seam
//! (`crate::nodes::seam`), so the router, summon and `nodes/ensureServing` reach it without
//! depending on it.
//!
//! `ensure_serving(node, demand)`, in the design's order:
//!  1. served already → Ready (unless the demanding reply opened after a queued switch to another
//!     way: it waits behind that switch, so new replies never starve a queued one). A lease on the
//!     way that serves never reaches `ensure_serving`, so the router asks the same question before
//!     EVERY MLX lease (`queued_switch_ahead`, then `wait_behind_queued_switches`);
//!  2. cloud / endpoint → Ready;
//!  3. a swarm build holds the engine → Refused (the loader never stops an engine under a build);
//!  4. the stop set is EVERY serving way (Run it's `servingWays`, pinned by `switch.fixture.json`);
//!     one kept loaded → Refused;
//!  5. a step the loader never takes (provisioning, copying a model, permissions) → Refused in the
//!     planner's or the split plan's words; a split set up for another model is NOT a step
//!     (`split_config`, pinned by `split_config.fixture.json`);
//!  6. the one fit rule (the placement plan, crediting what the stop set frees) → Refused on Short;
//!  7. a reply open on a way in the stop set — in any goose process on this Mac, other than the
//!     demand's own root and replies waiting in a loader — or requests on this Mac's engine no
//!     reply explains → Wait, in ONE FIFO; woken by a reply ending (in-process: the reply guard's
//!     drop; another process: the kernel releasing that reply's flock), never by a clock. A loop
//!     tick's demand (session loops §5.5) says whose reply it waits for when it is a person's,
//!     and holds no place in the FIFO ahead of a person's demand until its stops have begun;
//!  8. batching is per REPLY (the guard `on_prompt` takes): a reply keeps its way for every model
//!     call; a SYNCHRONOUS delegate's demand is its parent reply's own (the parent is blocked in
//!     the tool call); a BACKGROUND delegate runs beside its parent's turn, so it opens a reply of
//!     its own (seam `open_reply`) and its parent, while it waits on it in `load`, holds nothing
//!     (seam `pause_reply`);
//!  9. the swap claim (one loader per Mac at a time), then the stops in Run it's order, each
//!     followed to its end, then the start — through the same ACP handlers Run it calls;
//! 10. the ready paths append the load row (`rows`);
//! 11. a failed load is not restored — the stop set stays stopped, the turn gets the load's words;
//! 12. a cancel before the claim leaves nothing; after the stops began the swap runs to its end
//!     and is recorded `cancelledAfterStop`;
//! 13. the serving intent is written by the handlers, as for any owner start.
//!
//! The only periodic look is [`LOOK_AGAIN`], for what no event announces: an engine finishing a
//! load or stop it reports only on its status, and requests from a client goose cannot see. It is
//! an observation cadence — each look reads a status; what it reads decides (gate 5).

mod holds;
mod live;
pub(super) mod rows;
mod split_config;
mod switch;

use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex, OnceLock, Weak};
use std::time::Duration;

use async_trait::async_trait;
use goose_sdk_types::custom_requests::{
    MlxDistributedConfigDto, MlxPlacementKeyDto, MlxPlacementKindDto, NodeBehindSwitchDto,
    NodeDisplacedDto, NodeEnsureServing, NodeIfServingOther, NodeLoadRefusalCode,
    NodeRefusalFactsDto, NodeRepliesWaitDto, NodeServingOtherDto,
};
use goose_sidecar::placement::store::{PlacementKey, PlacementKind};
use tokio::sync::{oneshot, Notify};

use crate::nodes::seam::{self, Demand, DemandFrom, LoaderActivity, NodeLoader};
use crate::nodes::{NodeDefKind, ResolvedNodeDef};

pub use holds::ReplyGuard;
use holds::{Blocker, Holds};
use switch::{Serving, Stop, SwitchPlan, WayRef};

/// How often a wait no event can end looks again (an engine's status, a foreign client's
/// requests). The distributed supervisor's own poll interval (`supervisor.rs` `POLL_INTERVAL`):
/// the states read here change at that cadence. An observation, never a verdict (gate 5).
pub(super) const LOOK_AGAIN: Duration = Duration::from_secs(2);

/// Why a demand cannot be served, in words the composer shows verbatim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Refusal {
    pub code: NodeLoadRefusalCode,
    pub reason: String,
    /// What the refusal names, for the refusals the composer words (design §8.7).
    pub facts: Option<NodeRefusalFactsDto>,
}

impl Refusal {
    fn new(code: NodeLoadRefusalCode, reason: impl Into<String>) -> Self {
        Refusal {
            code,
            reason: reason.into(),
            facts: None,
        }
    }

    fn with_facts(mut self, facts: NodeRefusalFactsDto) -> Self {
        self.facts = Some(facts);
        self
    }
}

/// An MLX node by its id and the name the Nodes page shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NamedNode {
    pub id: String,
    pub name: String,
}

/// What a start does.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Start {
    MountHere,
    RemoteSingle {
        peer: String,
    },
    /// The split with the saved setup, which already names this model.
    SplitSaved,
    /// The split with a config discovered for this model (Run it's `startSplitFor`).
    SplitFor(Box<MlxDistributedConfigDto>),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Prepared {
    pub model: String,
    pub key: PlacementKey,
    pub start: Start,
}

/// Whether a node's way serves this Mac's goose now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Residency {
    Serving,
    /// Its way is loading already (Run it, a restore, another loader): wait for it.
    Loading(Option<String>),
    NotServing,
}

/// Everything the loader reads and does outside its own bookkeeping — the ACP handlers Run it
/// calls, in production; a scripted fake in the tests.
#[async_trait]
pub(crate) trait Ways: Send + Sync {
    /// The node with its model read through (a pool node's device model).
    async fn resolve(&self, node: &crate::nodes::NodeDef) -> Result<ResolvedNodeDef, Refusal>;
    async fn residency(&self, node: &ResolvedNodeDef) -> Result<Residency, Refusal>;
    /// The three engines' reports (Run it's inputs), or a conflict this goose may not switch:
    /// a way another goose window owns.
    async fn serving(&self) -> Result<Serving, Refusal>;
    /// The node kept loaded whose way `stop` is.
    async fn kept_loaded(&self, stop: &Stop) -> Result<Option<NamedNode>, Refusal>;
    /// The MLX nodes that name `stop`'s way and model (what a stop of it takes away).
    async fn named_by(&self, stop: &Stop) -> Result<Vec<NamedNode>, Refusal>;
    /// Steps 5–6: a step or the fit refuses; otherwise what starting means.
    async fn prepare(
        &self,
        node: &ResolvedNodeDef,
        target: &WayRef,
        plan: &SwitchPlan,
    ) -> Result<Prepared, Refusal>;
    /// Stop one way and return once it has let go (Run it's `stopForSwitch`).
    async fn stop(&self, stop: &Stop, plan: &SwitchPlan) -> Result<(), String>;
    /// Start the way and follow it until it answers; `Err` carries the engine's words.
    async fn start(&self, node: &ResolvedNodeDef, prepared: &Prepared) -> Result<(), String>;
    /// Requests on this Mac's engine that no reply accounts for (another client on the port).
    /// `Err` when a running engine did not report them: unknown, never read as none.
    async fn unexplained_requests(&self) -> Result<Option<u32>, String>;
    /// The chat a session is, by the name the person sees.
    async fn chat_name(&self, session_id: &str) -> Result<String, String>;
    /// The chat is open in a goose window of this process (loaded by a connection that is still
    /// served): an open chat whose last turn was served on a way still counts as that way's
    /// (Q-428). No window connected = no chat open.
    async fn chat_open(&self, session_id: &str) -> bool;
    /// The Mac(s) `node` shares with the way serving now, by name ("Work's Mac Studio"); every Mac
    /// of the serving way when they share none the records name.
    async fn shared_mac(&self, node: &ResolvedNodeDef) -> Result<String, Refusal>;
}

/// Why a waiting demand looks again.
enum Wake {
    /// This process's bookkeeping changed (a reply ended, a demand left the queue, a lease moved).
    Changed,
    /// A reply of another goose process ends when its flock is released.
    ReplyEnd(std::path::PathBuf),
    /// A status only a look can read.
    LookAgain,
}

enum Look {
    Ready,
    Refused(Refusal),
    Wait {
        reason: String,
        wake: Wake,
        replies: Option<NodeRepliesWaitDto>,
        /// The role says `wait` and the Mac serves another node for chats between replies.
        serving_other: Option<NodeServingOtherDto>,
        /// This demand waits for the other node's chats to be done (the `wait` setting): it is
        /// no switch those chats' next replies wait behind, nor one other demands queue behind.
        yields: bool,
        /// Its node serves, and it waits behind this switch queued before its reply began: the
        /// node's residency reads serving, so the wait is said per chat (Q-442).
        behind: Option<Ahead>,
    },
    Go(Box<(Prepared, SwitchPlan)>),
}

struct Queued {
    seq: u64,
    node: String,
    node_name: String,
    /// The session that demanded it (`None` for a card's Start): the load it waits on is FOR it.
    session: Option<String>,
    /// A loop tick's demand (session loops §5.5).
    tick: bool,
    /// Its stops are about to begin: it runs to its end, whoever asked after it (§6.4 step 12).
    swapping: bool,
    /// It waits under `wait` for another node's chats (Q-428): it keeps its place in the queue
    /// but holds no one behind it (see `Look::Wait::yields`).
    yields: bool,
    /// What it does when the Mac serves another node: the role's setting, until the person takes
    /// the Mac over for this turn (Q-443, `take_over_now`).
    setting: NodeIfServingOther,
}

impl Queued {
    /// Whether this demand switches before the demand numbered `seq`. First come, first served
    /// — except that a loop tick's demand never keeps a person's waiting (session loops §5.5): a
    /// person's demand goes before every tick demand whose swap has not begun, older or not.
    fn goes_before(&self, seq: u64, asker_is_tick: bool) -> bool {
        if self.seq == seq {
            return false;
        }
        if self.swapping {
            return true;
        }
        if self.yields {
            return false;
        }
        match (self.tick, asker_is_tick) {
            (false, true) => true,
            (true, false) => false,
            _ => self.seq < seq,
        }
    }
}

pub(crate) struct Core {
    ways: Arc<dyn Ways>,
    holds: Arc<Holds>,
    queue: StdMutex<VecDeque<Queued>>,
    activity: StdMutex<BTreeMap<String, LoaderActivity>>,
    /// Nodes a swap stopped, by node id, until each serves again (design §8.7's displaced notice).
    displaced: StdMutex<BTreeMap<String, NodeDisplacedDto>>,
    /// Replies waiting behind a switch queued before they began, by the waiter's number (Q-442).
    behind: StdMutex<BTreeMap<u64, NodeBehindSwitchDto>>,
    changed: Arc<Notify>,
    /// One swap at a time in this process; the swap claim serialises processes.
    swapping: Arc<tokio::sync::Mutex<()>>,
}

/// A demand in the queue; dropping it (the turn went, or the demand ended) leaves the queue.
struct Ticket {
    core: Arc<Core>,
    seq: u64,
}

impl Drop for Ticket {
    fn drop(&mut self) {
        self.core
            .queue
            .lock()
            .unwrap()
            .retain(|q| q.seq != self.seq);
        self.core.changed.notify_waiters();
    }
}

/// A waiter's behind-a-switch record (Q-442) goes with the waiter, however it ends.
struct Behind {
    core: Arc<Core>,
    seq: u64,
}

impl Drop for Behind {
    fn drop(&mut self) {
        self.core.forget_behind(self.seq);
    }
}

/// The demanding reply waits in the loader: it holds nothing meanwhile (see `holds`).
struct Paused {
    core: Arc<Core>,
    root: String,
    session: String,
}

impl Drop for Paused {
    fn drop(&mut self) {
        self.core
            .holds
            .set_waiting(&self.root, &self.session, false);
    }
}

/// Flips when the demanding turn's future is dropped mid-swap.
struct CancelOnDrop(Arc<AtomicBool>, bool);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        if !self.1 {
            self.0.store(true, Ordering::SeqCst);
        }
    }
}

/// A demand's answer, with what a refusal names.
type Answer = (NodeEnsureServing, Option<NodeRefusalFactsDto>);

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

fn short(model: &str) -> &str {
    model.rsplit('/').next().unwrap_or(model)
}

/// A way that is starting now, in words; `None` when every way has settled.
fn loading_way(serving: &Serving) -> Option<String> {
    let model = |m: &Option<String>| short(m.as_deref().unwrap_or("a model")).to_string();
    if let Some(single) = serving.single.as_ref().filter(|s| s.state == "mounting") {
        return Some(format!(
            "this Mac's engine is loading {}",
            model(&single.model_id)
        ));
    }
    if let Some(remote) = serving.remote.as_ref().filter(|r| r.state == "mounting") {
        return Some(format!(
            "the engine on {} is loading {}",
            remote.peer.as_deref().unwrap_or("a linked Mac"),
            model(&remote.model_id)
        ));
    }
    if let Some(split) = serving
        .distributed
        .as_ref()
        .filter(|d| matches!(d.state.as_str(), "preflight" | "starting"))
    {
        return Some(format!("the split is starting {}", model(&split.model_id)));
    }
    None
}

/// The way a node names, as the loader starts it.
fn target_of(node: &ResolvedNodeDef) -> Result<(WayRef, PlacementKey), Refusal> {
    let Some(key) = crate::nodes::acp::node_key(node) else {
        return Err(Refusal::new(
            NodeLoadRefusalCode::NeedsStep,
            format!(
                "{} serves whatever this Mac's engine runs; start a model in Run it",
                node.def.name
            ),
        ));
    };
    let key = PlacementKey {
        kind: match key.kind {
            MlxPlacementKindDto::Single => PlacementKind::Single,
            MlxPlacementKindDto::Tensor => PlacementKind::Tensor,
            MlxPlacementKindDto::Pipeline => PlacementKind::Pipeline,
        },
        nodes: key.nodes,
        link: key.link,
    };
    let way = WayRef::of_key(&key).ok_or_else(|| {
        Refusal::new(
            NodeLoadRefusalCode::NeedsStep,
            format!(
                "{}'s way names no Mac goose can start it on; pick its way again in New node",
                node.def.name
            ),
        )
    })?;
    Ok((way, key))
}

pub(crate) fn key_of(dto: &MlxPlacementKeyDto) -> PlacementKey {
    PlacementKey {
        kind: match dto.kind {
            MlxPlacementKindDto::Single => PlacementKind::Single,
            MlxPlacementKindDto::Tensor => PlacementKind::Tensor,
            MlxPlacementKindDto::Pipeline => PlacementKind::Pipeline,
        },
        nodes: dto.nodes.clone(),
        link: dto.link.clone(),
    }
}

impl Core {
    pub(crate) fn new(ways: Arc<dyn Ways>, holders_dir: Option<std::path::PathBuf>) -> Arc<Self> {
        let changed = Arc::new(Notify::new());
        Arc::new(Core {
            ways,
            holds: Holds::new(holders_dir, Arc::clone(&changed)),
            queue: StdMutex::new(VecDeque::new()),
            activity: StdMutex::new(BTreeMap::new()),
            displaced: StdMutex::new(BTreeMap::new()),
            behind: StdMutex::new(BTreeMap::new()),
            changed,
            swapping: Arc::new(tokio::sync::Mutex::new(())),
        })
    }

    pub(crate) fn holds(&self) -> &Arc<Holds> {
        &self.holds
    }

    fn set_activity(&self, activity: LoaderActivity) {
        // A demand waiting while its own node's swap runs is part of that load (Q-382): the node
        // stays Loading — naming the waiting demand among the sessions it is for — until the swap
        // ends, instead of reading "waiting" over a load that is happening.
        if matches!(activity, LoaderActivity::Waiting { .. }) && self.swapping_to(activity.node()) {
            return;
        }
        self.activity
            .lock()
            .unwrap()
            .insert(activity.node().to_string(), activity);
    }

    /// A swap to `node` runs now (its demand's stops have begun and it is still queued).
    fn swapping_to(&self, node: &str) -> bool {
        self.queue
            .lock()
            .unwrap()
            .iter()
            .any(|q| q.node == node && q.swapping)
    }

    fn clear_activity(&self, node: &str) {
        self.activity.lock().unwrap().remove(node);
    }

    /// What the loader does per node. A load names the sessions it is FOR: every demand for that
    /// node still in the queue — the one whose swap runs and the ones waiting on the same load —
    /// read at the moment it is asked, so a demand that went (its turn ended) is not named.
    pub(crate) fn in_progress(&self) -> Vec<LoaderActivity> {
        let activity: Vec<LoaderActivity> =
            self.activity.lock().unwrap().values().cloned().collect();
        let queue = self.queue.lock().unwrap();
        activity
            .into_iter()
            .map(|a| match a {
                LoaderActivity::Loading { node, phase, .. } => {
                    let mut demanded_by: Vec<String> = Vec::new();
                    for session in queue
                        .iter()
                        .filter(|q| q.node == node)
                        .filter_map(|q| q.session.clone())
                    {
                        if !demanded_by.contains(&session) {
                            demanded_by.push(session);
                        }
                    }
                    LoaderActivity::Loading {
                        node,
                        phase,
                        demanded_by,
                    }
                }
                other => other,
            })
            .collect()
    }

    pub(crate) fn displaced(&self) -> Vec<NodeDisplacedDto> {
        self.displaced.lock().unwrap().values().cloned().collect()
    }

    /// `node` serves again (by any hand — this loader, Run it, a restore): its notice is over.
    pub(crate) fn forget_displaced(&self, node: &str) {
        self.displaced.lock().unwrap().remove(node);
    }

    /// Every reply waiting behind a switch queued before it began, one per chat and node.
    pub(crate) fn behind_switches(&self) -> Vec<NodeBehindSwitchDto> {
        let mut out: Vec<NodeBehindSwitchDto> = Vec::new();
        for entry in self.behind.lock().unwrap().values() {
            if !out.contains(entry) {
                out.push(entry.clone());
            }
        }
        out
    }

    /// Record that the reply of `session` (waiter number `seq`) waits on `node` behind `ahead`.
    /// The chat names are read only when the switch it waits behind changes.
    async fn note_behind(&self, seq: u64, session: &str, node: &str, ahead: &Ahead) {
        let current = self
            .behind
            .lock()
            .unwrap()
            .get(&seq)
            .is_some_and(|b| b.switch_to == ahead.node);
        if current {
            return;
        }
        let chats = match &ahead.session {
            Some(s) => vec![self.chat_words(&self.holds.root_of(s)).await],
            None => Vec::new(),
        };
        self.behind.lock().unwrap().insert(
            seq,
            NodeBehindSwitchDto {
                session: self.holds.root_of(session),
                node: node.to_string(),
                switch_to: ahead.node.clone(),
                switch_to_name: ahead.node_name.clone(),
                chats,
            },
        );
    }

    fn forget_behind(&self, seq: u64) {
        self.behind.lock().unwrap().remove(&seq);
    }

    /// Q-443: the person takes the Mac over for THIS turn — every demand of `session`'s chat for
    /// `node` that waits under `wait` looks again as `takeOver` (a running reply is still never
    /// cut). The role's setting is untouched: the next turn reads it again. False when no such
    /// demand is queued (it loaded, moved on or ended).
    pub(crate) fn take_over_now(&self, session: &str, node: &str) -> bool {
        let waiting: Vec<(u64, String)> = self
            .queue
            .lock()
            .unwrap()
            .iter()
            .filter(|q| q.node == node && q.setting == NodeIfServingOther::Wait)
            .filter_map(|q| q.session.clone().map(|s| (q.seq, s)))
            .collect();
        let root = self.holds.root_of(session);
        let chosen: Vec<u64> = waiting
            .into_iter()
            .filter(|(_, s)| self.holds.root_of(s) == root)
            .map(|(seq, _)| seq)
            .collect();
        let mut taken = false;
        for queued in self.queue.lock().unwrap().iter_mut() {
            if chosen.contains(&queued.seq) && queued.setting == NodeIfServingOther::Wait {
                queued.setting = NodeIfServingOther::TakeOver;
                taken = true;
            }
        }
        if taken {
            self.changed.notify_waiters();
        }
        taken
    }

    /// A demand's setting as it stands now (`take_over_now` may have changed it).
    fn setting_of(&self, seq: u64, asked: NodeIfServingOther) -> NodeIfServingOther {
        self.queue
            .lock()
            .unwrap()
            .iter()
            .find(|q| q.seq == seq)
            .map_or(asked, |q| q.setting)
    }

    pub(crate) async fn ensure_serving(self: &Arc<Self>, demand: Demand) -> NodeEnsureServing {
        if demand.node.kind != NodeDefKind::Mlx {
            return NodeEnsureServing::Ready;
        }
        if let DemandFrom::Turn(_) = demand.from {
            return Arc::clone(self).run(demand, None).await;
        }
        // A demand from the UI (Start on a card): the first answer comes back now; a wait or a
        // load continues in the background, and the card follows it through `nodes/residency`.
        let (first, answer) = oneshot::channel();
        let core = Arc::clone(self);
        tokio::spawn(async move {
            core.run(demand, Some(first)).await;
        });
        answer.await.unwrap_or_else(|_| NodeEnsureServing::Refused {
            code: NodeLoadRefusalCode::Unknown,
            reason: "the loader's task ended without an answer".to_string(),
            facts: None,
        })
    }

    async fn run(
        self: Arc<Self>,
        demand: Demand,
        mut first: Option<oneshot::Sender<NodeEnsureServing>>,
    ) -> NodeEnsureServing {
        let node_id = demand.node.id.clone();
        let (answer, facts) = self.demand(demand, &mut first).await;
        match &answer {
            NodeEnsureServing::Refused { reason, .. } => {
                self.set_activity(LoaderActivity::RefusedLastTime {
                    node: node_id,
                    reason: reason.clone(),
                    facts,
                })
            }
            _ => self.clear_activity(&node_id),
        }
        if let Some(first) = first {
            let _ = first.send(answer.clone());
        }
        answer
    }

    async fn demand(
        self: &Arc<Self>,
        demand: Demand,
        first: &mut Option<oneshot::Sender<NodeEnsureServing>>,
    ) -> Answer {
        let refused = |r: Refusal| {
            (
                NodeEnsureServing::Refused {
                    code: r.code,
                    reason: r.reason,
                    facts: r.facts.clone(),
                },
                r.facts,
            )
        };
        let asked = demand.if_serving_other;
        let node = match self.ways.resolve(&demand.node).await {
            Ok(node) => node,
            Err(r) => return refused(r),
        };
        let (target, _) = match target_of(&node) {
            Ok(target) => target,
            Err(r) => return refused(r),
        };
        let session = demand.session_id().map(str::to_string);
        let root = session.as_deref().map(|s| self.holds.root_of(s));
        let reply_opened = root.as_deref().and_then(|r| self.holds.reply_opened(r));
        let tick = root.as_deref().is_some_and(|r| self.holds.is_tick(r));
        let seq = self.holds.next_seq();
        self.queue.lock().unwrap().push_back(Queued {
            seq,
            node: node.def.id.clone(),
            node_name: node.def.name.clone(),
            session: session.clone(),
            tick,
            swapping: false,
            yields: false,
            setting: asked,
        });
        let ticket = Ticket {
            core: Arc::clone(self),
            seq,
        };
        let _behind = Behind {
            core: Arc::clone(self),
            seq,
        };
        let mut paused: Option<Paused> = None;
        let tell_first = |first: &mut Option<oneshot::Sender<NodeEnsureServing>>, reason: &str| {
            if let Some(tx) = first.take() {
                let _ = tx.send(NodeEnsureServing::Wait {
                    reason: reason.to_string(),
                });
            }
        };
        let mut looks = 0u64;
        loop {
            let notified = self.changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            looks += 1;
            let look = self
                .look(
                    &node,
                    &target,
                    seq,
                    own_of(&root, &session),
                    reply_opened,
                    tick,
                    looks == 1,
                    self.setting_of(seq, asked),
                )
                .await;
            let (first_prepared, _) = match look {
                Look::Ready => return (NodeEnsureServing::Ready, None),
                Look::Refused(r) => return refused(r),
                Look::Wait {
                    reason,
                    wake,
                    replies,
                    serving_other,
                    yields,
                    behind,
                } => {
                    if let (None, Some(root), Some(session)) = (&paused, &root, &session) {
                        paused = Some(self.pause(root, session));
                    }
                    match (&behind, &session) {
                        (Some(ahead), Some(session)) => {
                            self.note_behind(seq, session, &node.def.id, ahead).await
                        }
                        _ => self.forget_behind(seq),
                    }
                    self.set_yields(seq, yields);
                    self.set_activity(LoaderActivity::Waiting {
                        node: node.def.id.clone(),
                        reason: reason.clone(),
                        replies,
                        serving_other,
                    });
                    tell_first(first, &reason);
                    wait(notified, wake).await;
                    continue;
                }
                Look::Go(go) => *go,
            };
            self.forget_behind(seq);
            // It switches now: from here no reply that opens after it may lease the way it will
            // stop (a yielding entry would still let one through until the claim — Q-428).
            self.set_yields(seq, false);
            // From here the demanding reply has no model call in flight: it holds nothing.
            if let (None, Some(root), Some(session)) = (&paused, &root, &session) {
                paused = Some(self.pause(root, session));
            }
            // The swap: this process's one swap, then the Mac's swap claim — both waited for, and
            // both cancellable (a cancel here leaves nothing: no stop has begun).
            let swapping = Arc::clone(&self.swapping).lock_owned().await;
            let claim = match self.claim(&node, &first_prepared, first).await {
                Ok(claim) => claim,
                Err(r) => return refused(r),
            };
            // Under the claim, look again — and switch on what THIS look sees: another window's
            // loader may have switched while this one waited for the claim, and the stop set read
            // before it would miss the way that serves now.
            let (prepared, plan) = match self
                .look(
                    &node,
                    &target,
                    seq,
                    own_of(&root, &session),
                    reply_opened,
                    tick,
                    true,
                    self.setting_of(seq, asked),
                )
                .await
            {
                Look::Ready => return (NodeEnsureServing::Ready, None),
                Look::Refused(r) => return refused(r),
                Look::Wait { .. } => {
                    drop((claim, swapping));
                    continue;
                }
                Look::Go(go) => *go,
            };
            if let Some(queued) = self.queue.lock().unwrap().iter_mut().find(|q| q.seq == seq) {
                queued.swapping = true;
                queued.yields = false;
            }
            let loading = format!("loading {} for this chat", node.def.name);
            tell_first(first, &loading);
            let cancelled = Arc::new(AtomicBool::new(false));
            let mut on_drop = CancelOnDrop(Arc::clone(&cancelled), false);
            let (done, result) = oneshot::channel();
            let core = Arc::clone(self);
            let owned = (ticket, paused, claim, swapping);
            let holder = root.clone();
            let leaser = session.clone();
            tokio::spawn(async move {
                let way = prepared.key.clone();
                let answer = core
                    .execute(&node, prepared, plan, cancelled, holder.as_deref())
                    .await;
                // The demanding session leased it: its turn holds it, and — for a delegate — so
                // does the delegate, against its own turn's other delegates.
                if let ((NodeEnsureServing::Ready, _), Some(session)) = (&answer, &leaser) {
                    core.holds.note_lease(session, way);
                }
                drop(owned);
                let _ = done.send(answer);
            });
            let answer = result.await.unwrap_or_else(|_| {
                (
                    NodeEnsureServing::Refused {
                        code: NodeLoadRefusalCode::Unknown,
                        reason: "the loader's swap task ended without an answer".to_string(),
                        facts: None,
                    },
                    None,
                )
            });
            on_drop.1 = true;
            return answer;
        }
    }

    /// Take the Mac's swap claim, waiting (cancellably) while another goose window's loader holds
    /// it. With no holder directory there is no other process to serialise with.
    async fn claim(
        &self,
        node: &ResolvedNodeDef,
        prepared: &Prepared,
        first: &mut Option<oneshot::Sender<NodeEnsureServing>>,
    ) -> Result<Option<goose_sidecar::machine::LoadLock>, Refusal> {
        let Some(dir) = self.holds.dir().map(|d| d.to_path_buf()) else {
            return Ok(None);
        };
        let what = format!(
            "goose (pid {}) is switching this Mac's goose to {}",
            std::process::id(),
            node.def.name
        );
        let model = prepared.model.clone();
        let failed = |e: String| {
            Refusal::new(
                NodeLoadRefusalCode::Unknown,
                format!("the Mac's swap claim could not be taken: {e}"),
            )
        };
        let (d, w, m) = (dir.clone(), what.clone(), model.clone());
        let attempt =
            tokio::task::spawn_blocking(move || goose_sidecar::holders::try_claim_swap(&d, &w, &m))
                .await
                .map_err(|e| failed(e.to_string()))?
                .map_err(|e| failed(format!("{e:#}")))?;
        let held = match attempt {
            goose_sidecar::machine::LoadLockAttempt::Acquired(lock) => return Ok(Some(lock)),
            goose_sidecar::machine::LoadLockAttempt::Held(held) => held,
        };
        let by = held
            .holder
            .as_ref()
            .map_or_else(|| held.message(), |h| h.what.clone());
        let reason = format!(
            "another goose window is switching this Mac's goose ({by}); loading {} when it finishes",
            node.def.name
        );
        self.set_activity(LoaderActivity::Waiting {
            node: node.def.id.clone(),
            reason: reason.clone(),
            replies: None,
            serving_other: None,
        });
        if let Some(tx) = first.take() {
            let _ = tx.send(NodeEnsureServing::Wait { reason });
        }
        tokio::task::spawn_blocking(move || {
            goose_sidecar::holders::claim_swap_waiting(&dir, &what, &model)
        })
        .await
        .map_err(|e| failed(e.to_string()))?
        .map(Some)
        .map_err(|e| failed(format!("{e:#}")))
    }

    #[allow(clippy::too_many_arguments)]
    async fn look(
        &self,
        node: &ResolvedNodeDef,
        target: &WayRef,
        seq: u64,
        own: holds::Own<'_>,
        reply_opened: Option<u64>,
        tick: bool,
        prepare_now: bool,
        setting: NodeIfServingOther,
    ) -> Look {
        let name = &node.def.name;
        // 1. Served already?
        match self.ways.residency(node).await {
            Err(r) => return Look::Refused(r),
            Ok(Residency::Serving) => {
                self.forget_displaced(&node.def.id);
                let ahead = self.switch_ahead(seq, &node.def.id, reply_opened, tick);
                return match ahead {
                    Some(other) => Look::Wait {
                        reason: format!(
                            "{name} is serving, and a switch to {} was asked for before this reply began; this reply waits behind it",
                            other.node_name
                        ),
                        wake: Wake::Changed,
                        replies: None,
                        serving_other: None,
                        yields: false,
                        behind: Some(other),
                    },
                    None => Look::Ready,
                };
            }
            Ok(Residency::Loading(phase)) => {
                return Look::Wait {
                    reason: match phase {
                        Some(phase) => format!("{name} is loading ({phase})"),
                        None => format!("{name} is loading"),
                    },
                    wake: Wake::LookAgain,
                    replies: None,
                    serving_other: None,
                    yields: false,
                    behind: None,
                }
            }
            Ok(Residency::NotServing) => {}
        }
        // 3. Held by a build?
        match self.holds.build_holder() {
            Err(reason) => {
                return Look::Refused(Refusal::new(NodeLoadRefusalCode::Unknown, reason))
            }
            Ok(Some(build)) => {
                let way = build.way.clone().unwrap_or_else(|| build.what.clone());
                return Look::Refused(
                    Refusal::new(
                        NodeLoadRefusalCode::HeldByBuild,
                        format!(
                            "a swarm build is using {way} ({}, pid {}); it frees when the build ends",
                            build.what, build.pid
                        ),
                    )
                    .with_facts(NodeRefusalFactsDto::HeldByBuild { way }),
                );
            }
            Ok(None) => {}
        }
        // One way at a time is one queue: only the demand at its head switches.
        if let Some(ahead) = self.queue_ahead(seq, tick) {
            // Q-428: a switch to another node is next on this Mac — under `useNext` that is the
            // Mac serving another node, and the next node of the chain takes the work.
            if setting == NodeIfServingOther::UseNext && ahead.node != node.def.id {
                return Look::Refused(self.switching_to_other(node, &ahead).await);
            }
            return Look::Wait {
                reason: format!(
                    "waiting for the switch to {} first; then {name}",
                    ahead.node_name
                ),
                wake: Wake::Changed,
                replies: None,
                serving_other: None,
                yields: false,
                behind: None,
            };
        }
        // 4. The stop set.
        let serving = match self.ways.serving().await {
            Ok(serving) => serving,
            Err(r) => return Look::Refused(r),
        };
        let plan = switch::switch_plan(&serving, target);
        // A way another actor is starting (Run it, a restore) is the person's intent: waited for
        // until it settles, never stopped mid-load by an automatic demand.
        if let Some(loading) = loading_way(&serving) {
            return Look::Wait {
                reason: format!("{loading}; then {name}"),
                wake: Wake::LookAgain,
                replies: None,
                serving_other: None,
                yields: false,
                behind: None,
            };
        }
        for stop in &plan.stops {
            match self.ways.kept_loaded(stop).await {
                Err(r) => return Look::Refused(r),
                Ok(Some(kept)) => {
                    let mac = stop.way.mac_words();
                    return Look::Refused(
                        Refusal::new(
                            NodeLoadRefusalCode::KeptLoaded,
                            format!("Can't load {name}: {} is kept loaded on {mac}", kept.name),
                        )
                        .with_facts(NodeRefusalFactsDto::KeptLoaded {
                            kept_node: kept.id,
                            kept: kept.name,
                            mac,
                        }),
                    );
                }
                Ok(None) => {}
            }
        }
        // Q-428: the Mac serves another node for other chats — the role's setting decides. A way
        // no one uses is not "serving another node": it is switched under every setting.
        let mut yields = false;
        let mut resting: Option<NodeServingOtherDto> = None;
        if setting != NodeIfServingOther::TakeOver {
            match self.serving_other(node, &plan, own, seq).await {
                Err(r) => return Look::Refused(r),
                Ok(None) => {}
                Ok(Some(other)) => match setting {
                    NodeIfServingOther::UseNext => {
                        return Look::Refused(serving_other_refusal(name, &other));
                    }
                    // Between their replies: wait for the chats to be done with it. What ends it
                    // is a chat closing or moving to another node — read at each look, woken by
                    // this process's changes or the next look (no event announces a chat closing).
                    _ if other.replies == 0 => {
                        return Look::Wait {
                            reason: format!(
                                "{} is serving {} for {}; {name} loads when {} closed or moved to another node",
                                other.mac,
                                other.serving,
                                chats_words(&other.chats),
                                if other.chats.len() == 1 { "that chat is" } else { "those chats are" }
                            ),
                            wake: Wake::LookAgain,
                            replies: None,
                            serving_other: Some(other),
                            yields: true,
                            behind: None,
                        };
                    }
                    // Replies run on it: step 7 waits for them, this demand yielding meanwhile —
                    // and its line says what `wait` waits for: after those replies, the chats
                    // closing or moving, never "loading when they finish" (Q-443).
                    _ => {
                        yields = true;
                        resting = Some(other);
                    }
                },
            }
        }
        // 5–6. A step, or the fit: judged at the first look (a refusal is known before any wait)
        // and again at the look that switches — never on every wake of a wait.
        let prepared = if prepare_now {
            match self.ways.prepare(node, target, &plan).await {
                Ok(prepared) => Some(prepared),
                Err(r) => return Look::Refused(r),
            }
        } else {
            None
        };
        // 7. Replies in flight on a way that would stop.
        let blockers = match self.holds.blockers(&plan.stops, own) {
            Ok(blockers) => blockers,
            Err(reason) => {
                return Look::Refused(Refusal::new(NodeLoadRefusalCode::Unknown, reason))
            }
        };
        // A loop's tick never stops a way under a person's reply (session loops §5.5): it waits,
        // and says whose reply it waits for.
        let person = blockers
            .iter()
            .find(|b| b.kind() == goose_sidecar::holders::ReplyKind::User);
        if let (true, Some(person)) = (tick, person) {
            let chat = match self.ways.chat_name(person.session()).await {
                Ok(chat) => chat,
                Err(error) => {
                    tracing::warn!(session = person.session(), %error, "nodes loader: the chat a tick waits for could not be read; it is named by its id");
                    person.session().to_string()
                }
            };
            return Look::Wait {
                reason: format!(
                    "{} is answering you in {chat}; the loop's tick loads {name} when it finishes",
                    person.way()
                ),
                wake: match person {
                    Blocker::Here { .. } => Wake::Changed,
                    Blocker::Elsewhere { lock, .. } => Wake::ReplyEnd(lock.clone()),
                },
                replies: None,
                serving_other: None,
                yields,
                behind: None,
            };
        }
        if let Some(blocker) = blockers.first() {
            let count = blockers.iter().filter(|b| b.way() == blocker.way()).count();
            let replies = if count == 1 { "reply" } else { "replies" };
            let finish = if count == 1 {
                "it finishes"
            } else {
                "they finish"
            };
            // The way waited on, as the Nodes page names it (design §8.7 `nodes.turnWaiting`).
            let way_nodes = match plan.stops.iter().find(|s| s.way.words() == blocker.way()) {
                Some(stop) => match self.ways.named_by(stop).await {
                    Ok(named) => named.into_iter().map(|n| n.id).collect(),
                    Err(r) => return Look::Refused(r),
                },
                None => Vec::new(),
            };
            let mut chats: Vec<String> = Vec::new();
            for b in blockers.iter().filter(|b| b.way() == blocker.way()) {
                let chat = self.chat_words(&self.holds.root_of(b.session())).await;
                if !chats.contains(&chat) {
                    chats.push(chat);
                }
            }
            return Look::Wait {
                reason: format!(
                    "{} is answering {count} {replies}; loading {name} when {finish}",
                    blocker.way()
                ),
                wake: match blocker {
                    Blocker::Here { .. } => Wake::Changed,
                    Blocker::Elsewhere { lock, .. } => Wake::ReplyEnd(lock.clone()),
                },
                replies: Some(NodeRepliesWaitDto {
                    way: blocker.way().to_string(),
                    way_nodes,
                    count: count as u32,
                    chats,
                }),
                serving_other: resting,
                yields,
                behind: None,
            };
        }
        if plan.stops.iter().any(|s| s.way == WayRef::local()) {
            let requests = match self.ways.unexplained_requests().await {
                Ok(requests) => requests,
                Err(why) => {
                    return Look::Refused(Refusal::new(
                        NodeLoadRefusalCode::Unknown,
                        format!("Can't load {name}: whether this Mac's engine is answering anyone is unknown ({why})"),
                    ))
                }
            };
            if let Some(requests) = requests {
                return Look::Wait {
                    reason: format!(
                        "this Mac's engine is answering {requests} request(s) from a client no goose reply accounts for; loading {name} when they finish"
                    ),
                    wake: Wake::LookAgain,
                    replies: None,
                    serving_other: None,
                    yields: false,
                    behind: None,
                };
            }
        }
        let prepared = match prepared {
            Some(prepared) => prepared,
            None => match self.ways.prepare(node, target, &plan).await {
                Ok(prepared) => prepared,
                Err(r) => return Look::Refused(r),
            },
        };
        Look::Go(Box::new((prepared, plan)))
    }

    /// A queued switch older than the demanding reply, to another node: the reply waits behind it
    /// — unless it is a person's reply and the switch a loop tick's whose stops have not begun
    /// (session loops §5.5: a person never waits behind a tick's swap).
    fn switch_ahead(
        &self,
        seq: u64,
        node: &str,
        reply_opened: Option<u64>,
        asker_is_tick: bool,
    ) -> Option<Ahead> {
        let opened = reply_opened?;
        self.queue
            .lock()
            .unwrap()
            .iter()
            .find(|q| {
                q.seq < seq
                    && q.seq < opened
                    && q.node != node
                    && (!q.yields || q.swapping)
                    && (asker_is_tick || !q.tick || q.swapping)
            })
            .map(Ahead::of)
    }

    /// Step 1 for a LEASE (the router asks before every MLX lease, served or not): a switch queued
    /// before `session`'s reply opened, to another node, is honoured — the lease waits behind it.
    /// A reply opened before the switch keeps its way for every call (batching per reply), and a
    /// call outside any reply holds nothing across calls, so neither waits.
    pub(crate) fn queued_switch_ahead(&self, session: &str, node: &str) -> Option<String> {
        self.lease_behind(session, node).map(|a| a.node_name)
    }

    fn lease_behind(&self, session: &str, node: &str) -> Option<Ahead> {
        let root = self.holds.root_of(session);
        let opened = self.holds.reply_opened(&root)?;
        self.switch_ahead(u64::MAX, node, Some(opened), self.holds.is_tick(&root))
    }

    /// Wait until no switch is queued ahead of `session`'s reply. The reply holds nothing
    /// meanwhile (a switch it waits behind must never wait on it); woken by the queue changing —
    /// a demand leaving it, served or refused or cancelled — never by a clock. The chat's line
    /// names the switch it waits behind (Q-442, `behind_switches`) for as long as it waits.
    pub(crate) async fn wait_behind_queued_switches(self: &Arc<Self>, session: &str, node: &str) {
        let root = self.holds.root_of(session);
        let mut paused: Option<Paused> = None;
        let behind = Behind {
            core: Arc::clone(self),
            seq: self.holds.next_seq(),
        };
        loop {
            let notified = self.changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            let Some(ahead) = self.lease_behind(session, node) else {
                return;
            };
            if paused.is_none() {
                tracing::info!(%session, %node, switch_to = %ahead.node_name, "nodes loader: this reply opened after a queued switch; its lease waits behind it");
                paused = Some(self.pause(&root, session));
            }
            self.note_behind(behind.seq, session, node, &ahead).await;
            notified.await;
        }
    }

    /// Session loops §5.3 (the Mac-wide half): the way `session`'s reply holds, and the person's
    /// replies on it — the loader's own reader of the replies ([`Holds::persons_on`]).
    pub(crate) fn persons_on_way_of(&self, session: &str) -> Result<seam::WayShare, String> {
        let root = self.holds.root_of(session);
        let Some(way) = self.holds.way_of(&root) else {
            return Ok(seam::WayShare::default());
        };
        let persons = self.holds.persons_on(Some(&way), Some(&root))?;
        Ok(seam::WayShare {
            way: switch::WayRef::of_key(&way).map(|w| w.words()),
            persons: persons.into_iter().map(person_hold).collect(),
        })
    }

    pub(crate) fn persons_on_any_way(&self) -> Result<Vec<seam::PersonHold>, String> {
        Ok(self
            .holds
            .persons_on(None, None)?
            .into_iter()
            .map(person_hold)
            .collect())
    }

    /// The reply at `root` holds nothing for `session`'s wait until the pause is dropped (a
    /// count: pauses nest).
    fn pause(self: &Arc<Self>, root: &str, session: &str) -> Paused {
        self.holds.set_waiting(root, session, true);
        Paused {
            core: Arc::clone(self),
            root: root.to_string(),
            session: session.to_string(),
        }
    }

    fn queue_ahead(&self, seq: u64, asker_is_tick: bool) -> Option<Ahead> {
        self.queue
            .lock()
            .unwrap()
            .iter()
            .find(|q| q.goes_before(seq, asker_is_tick))
            .map(Ahead::of)
    }

    fn set_yields(&self, seq: u64, yields: bool) {
        let changed = match self.queue.lock().unwrap().iter_mut().find(|q| q.seq == seq) {
            Some(queued) if queued.yields != yields => {
                queued.yields = yields;
                true
            }
            _ => false,
        };
        // Who waits behind this demand looks again; an unchanged flag wakes no one.
        if changed {
            self.changed.notify_waiters();
        }
    }

    /// A chat by its name, or by its id with the read's failure logged.
    async fn chat_words(&self, session: &str) -> String {
        match self.ways.chat_name(session).await {
            Ok(chat) => chat,
            Err(error) => {
                tracing::warn!(session, %error, "nodes loader: a chat's name could not be read; it is named by its id");
                session.to_string()
            }
        }
    }

    /// Q-428's fact: whether the ways this switch would stop serve other chats — a reply running
    /// on one (any goose process on this Mac), or an open chat of this process whose last turn
    /// was served on one — and, if so, what a turn line names. `None` = nobody uses what would
    /// stop. Other processes publish only their running replies, so an idle chat of another goose
    /// process is not seen (its next turn is).
    async fn serving_other(
        &self,
        node: &ResolvedNodeDef,
        plan: &SwitchPlan,
        own: holds::Own<'_>,
        seq: u64,
    ) -> Result<Option<NodeServingOtherDto>, Refusal> {
        if plan.stops.is_empty() {
            return Ok(None);
        }
        // A chat whose own turn waits in this loader for a node is MOVING off the way it last
        // used, not resting on it: counting it would leave two `wait` demands each waiting for
        // the other (the refuter's case — two chats last on the split, both moved to the Studio).
        let moving: Vec<String> = {
            let queue = self.queue.lock().unwrap();
            queue
                .iter()
                .filter(|q| q.seq != seq)
                .filter_map(|q| q.session.clone())
                .collect()
        };
        let moving: Vec<String> = moving.iter().map(|s| self.holds.root_of(s)).collect();
        let running = self
            .holds
            .blockers(&plan.stops, own)
            .map_err(|reason| Refusal::new(NodeLoadRefusalCode::Unknown, reason))?;
        let own_root = match own {
            holds::Own::Demand { root, .. } | holds::Own::Root(root) => Some(root),
            holds::Own::Nobody => None,
        };
        let mut roots: Vec<String> = Vec::new();
        for b in &running {
            let root = self.holds.root_of(b.session());
            if !roots.contains(&root) {
                roots.push(root);
            }
        }
        let mut idle_way: Option<String> = None;
        for (root, way) in self.holds.last_on(&plan.stops, own_root) {
            if roots.contains(&root) || moving.contains(&root) || !self.ways.chat_open(&root).await
            {
                continue;
            }
            idle_way.get_or_insert(way);
            roots.push(root);
        }
        if roots.is_empty() {
            return Ok(None);
        }
        let way = running
            .first()
            .map(|b| b.way().to_string())
            .or(idle_way)
            .expect("a chat was found on a way that would stop");
        let serving = match plan.stops.iter().find(|s| s.way.words() == way) {
            Some(stop) => self
                .ways
                .named_by(stop)
                .await?
                .into_iter()
                .next()
                .map_or_else(|| way.clone(), |n| n.name),
            None => way,
        };
        let mut chats = Vec::new();
        for root in &roots {
            chats.push(self.chat_words(root).await);
        }
        Ok(Some(NodeServingOtherDto {
            mac: self.ways.shared_mac(node).await?,
            serving,
            chats,
            replies: running.len() as u32,
        }))
    }

    /// `useNext` while a switch to another node is next on this Mac.
    async fn switching_to_other(&self, node: &ResolvedNodeDef, ahead: &Ahead) -> Refusal {
        let chats = match &ahead.session {
            Some(session) => vec![self.chat_words(&self.holds.root_of(session)).await],
            None => Vec::new(),
        };
        let mac = match self.ways.shared_mac(node).await {
            Ok(mac) => mac,
            Err(r) => return r,
        };
        serving_other_refusal(
            &node.def.name,
            &NodeServingOtherDto {
                mac,
                serving: ahead.node_name.clone(),
                chats,
                replies: 0,
            },
        )
    }

    async fn execute(
        &self,
        node: &ResolvedNodeDef,
        prepared: Prepared,
        plan: SwitchPlan,
        cancelled: Arc<AtomicBool>,
        by: Option<&str>,
    ) -> Answer {
        self.set_activity(LoaderActivity::Loading {
            node: node.def.id.clone(),
            phase: None,
            demanded_by: Vec::new(),
        });
        let failed = |reason: String, words: String| {
            let facts = NodeRefusalFactsDto::LoadFailed { words };
            (
                NodeEnsureServing::Refused {
                    code: NodeLoadRefusalCode::LoadFailed,
                    reason,
                    facts: Some(facts.clone()),
                },
                Some(facts),
            )
        };
        let expected = rows::expect(&prepared.model, prepared.key.clone(), cancelled);
        let by_chat = match by {
            Some(root) => match self.ways.chat_name(root).await {
                Ok(chat) => Some(chat),
                Err(error) => {
                    tracing::warn!(session = root, %error, "nodes loader: the chat a swap is for could not be read; its displaced notice names no chat");
                    None
                }
            },
            None => None,
        };
        let at_ms = now_ms();
        for stop in &plan.stops {
            if let Err(words) = self.ways.stop(stop, &plan).await {
                rows::forget(&expected);
                let reason = format!(
                    "stopping {} ({}) to load {} failed: {words}",
                    stop.way.words(),
                    short(&stop.model_id),
                    node.def.name
                );
                self.displaced_failed(&node.def.id, at_ms, &reason);
                return failed(reason, words);
            }
            self.note_displaced(stop, node, by, by_chat.as_deref(), at_ms)
                .await;
        }
        let answer = match self.ways.start(node, &prepared).await {
            Ok(()) => {
                self.forget_displaced(&node.def.id);
                (NodeEnsureServing::Ready, None)
            }
            // Step 11: nothing is restored — a second load nobody asked for can fail too. The
            // chats whose way was stopped are told, with the load's words.
            Err(words) => {
                self.displaced_failed(&node.def.id, at_ms, &words);
                failed(format!("{} failed to load: {words}", node.def.name), words)
            }
        };
        self.changed.notify_waiters();
        answer
    }

    fn displaced_failed(&self, for_node: &str, at_ms: u64, words: &str) {
        for entry in self.displaced.lock().unwrap().values_mut() {
            if entry.for_node == for_node && entry.at_ms == at_ms {
                entry.failed = Some(words.to_string());
            }
        }
    }

    /// Record every node `stop` took away (design §6.4: "the sessions whose way was stopped get
    /// the displaced notice"). A read of the nodes that fails is logged: that notice is then not
    /// given, and the log says why.
    async fn note_displaced(
        &self,
        stop: &Stop,
        node: &ResolvedNodeDef,
        by: Option<&str>,
        by_chat: Option<&str>,
        at_ms: u64,
    ) {
        let named = match self.ways.named_by(stop).await {
            Ok(named) => named,
            Err(r) => {
                tracing::warn!(reason = %r.reason, "nodes loader: the nodes a stop took away could not be read; no displaced notice");
                return;
            }
        };
        let mut displaced = self.displaced.lock().unwrap();
        for stopped in named.into_iter().filter(|n| n.id != node.def.id) {
            displaced.insert(
                stopped.id.clone(),
                NodeDisplacedDto {
                    node: stopped.id,
                    for_node: node.def.id.clone(),
                    by_session: by.map(str::to_string),
                    by_chat: by_chat.map(str::to_string),
                    failed: None,
                    at_ms,
                },
            );
        }
    }
}

/// A queued switch ahead of a demand: the node it loads and the session it loads it for.
struct Ahead {
    node: String,
    node_name: String,
    session: Option<String>,
}

impl Ahead {
    fn of(q: &Queued) -> Self {
        Ahead {
            node: q.node.clone(),
            node_name: q.node_name.clone(),
            session: q.session.clone(),
        }
    }
}

/// "chat "Kickoff notes"", "chats "A" and "B"" — the loader's words for the chats a way serves.
fn chats_words(chats: &[String]) -> String {
    let quoted: Vec<String> = chats.iter().map(|c| format!("\"{c}\"")).collect();
    match quoted.as_slice() {
        [] => "another chat".to_string(),
        [one] => format!("chat {one}"),
        [rest @ .., last] => format!("chats {} and {last}", rest.join(", ")),
    }
}

/// The `useNext` answer: the next node of the chain takes the work, and the turn line says why.
fn serving_other_refusal(name: &str, other: &NodeServingOtherDto) -> Refusal {
    Refusal::new(
        NodeLoadRefusalCode::ServingOther,
        format!(
            "{} is serving {} for {}; {name} is left to it",
            other.mac,
            other.serving,
            chats_words(&other.chats)
        ),
    )
    .with_facts(other.as_facts())
}

fn person_hold(reply: Blocker) -> seam::PersonHold {
    match reply {
        Blocker::Here { session, way, .. } => seam::PersonHold {
            session,
            way,
            ends: seam::HoldEnds::Here,
        },
        Blocker::Elsewhere {
            session, way, lock, ..
        } => seam::PersonHold {
            session,
            way,
            ends: seam::HoldEnds::Elsewhere(lock),
        },
    }
}

async fn wait(notified: std::pin::Pin<&mut tokio::sync::futures::Notified<'_>>, wake: Wake) {
    match wake {
        Wake::Changed => notified.await,
        Wake::LookAgain => {
            tokio::select! {
                _ = notified => {}
                _ = tokio::time::sleep(LOOK_AGAIN) => {}
            }
        }
        // Only the reply's end (or its process's death) wakes this wait: the kernel releases the
        // flock. This process's own changes do not — the demand is the queue's head, and every
        // look it would make meanwhile would find the same reply open (and park one more thread on
        // its lock). A reply that starts waiting in its own loader releases the lock too.
        Wake::ReplyEnd(lock) => {
            let ended = tokio::task::spawn_blocking(move || {
                goose_sidecar::holders::wait_for_reply_end(&lock)
            })
            .await;
            if let Ok(Err(e)) = ended {
                tracing::warn!(error = %format!("{e:#}"), "nodes loader: waiting on another window's reply failed; looking again");
            }
        }
    }
}

/// Whose replies a demand's look leaves out: its own and the sessions blocked on it (see
/// [`holds::Own::Demand`]); a demand from no session leaves out nobody's.
fn own_of<'a>(root: &'a Option<String>, session: &'a Option<String>) -> holds::Own<'a> {
    match (root, session) {
        (Some(root), Some(session)) => holds::Own::Demand { root, session },
        _ => holds::Own::Nobody,
    }
}

struct Seam(Arc<Core>);

#[async_trait]
impl NodeLoader for Seam {
    async fn ensure_serving(&self, demand: Demand) -> NodeEnsureServing {
        self.0.ensure_serving(demand).await
    }

    fn note_lease(&self, session: &str, way: &MlxPlacementKeyDto) {
        self.0.holds.note_lease(session, key_of(way));
    }

    fn note_lease_off_mlx(&self, session: &str) {
        self.0.holds.note_lease_off_mlx(session);
    }

    fn note_child(&self, child_session: &str, parent_session: &str) {
        self.0.holds.note_child(child_session, parent_session);
    }

    fn open_reply(&self, session: &str, parent: &str) -> seam::Hold {
        Box::new(self.0.holds.open_reply_beside(session, parent))
    }

    fn pause_reply(&self, session: &str) -> seam::Hold {
        let root = self.0.holds.root_of(session);
        Box::new(self.0.pause(&root, session))
    }

    fn child_ended(&self, child_session: &str) {
        self.0.holds.child_ended(child_session);
    }

    fn queued_switch_ahead(&self, session: &str, node: &str) -> Option<String> {
        self.0.queued_switch_ahead(session, node)
    }

    async fn wait_behind_queued_switches(&self, session: &str, node: &str) {
        self.0.wait_behind_queued_switches(session, node).await
    }

    fn in_progress(&self) -> Vec<LoaderActivity> {
        self.0.in_progress()
    }

    fn displaced(&self) -> Vec<NodeDisplacedDto> {
        self.0.displaced()
    }

    fn forget_displaced(&self, node: &str) {
        self.0.forget_displaced(node)
    }

    fn behind_switches(&self) -> Vec<NodeBehindSwitchDto> {
        self.0.behind_switches()
    }

    fn take_over_now(&self, session: &str, node: &str) -> bool {
        self.0.take_over_now(session, node)
    }

    fn persons_on_way_of(&self, session: &str) -> Result<seam::WayShare, String> {
        self.0.persons_on_way_of(session)
    }

    fn persons_on_any_way(&self) -> Result<Vec<seam::PersonHold>, String> {
        self.0.persons_on_any_way()
    }

    fn holds_version(&self) -> u64 {
        self.0.holds.version()
    }

    async fn holds_changed(&self, since: u64, elsewhere: bool) {
        if elsewhere {
            tokio::select! {
                _ = self.0.holds.changed_since(since) => {}
                _ = tokio::time::sleep(LOOK_AGAIN) => {}
            }
        } else {
            self.0.holds.changed_since(since).await;
        }
    }

    async fn person_ended(&self, person: &seam::PersonHold, since: u64) {
        match &person.ends {
            seam::HoldEnds::Here => self.0.holds.changed_since(since).await,
            seam::HoldEnds::Elsewhere(lock) => {
                let lock = lock.clone();
                let ended = tokio::task::spawn_blocking(move || {
                    goose_sidecar::holders::wait_for_reply_end(&lock)
                })
                .await;
                if let Ok(Err(e)) = ended {
                    tracing::warn!(error = %format!("{e:#}"), "nodes loader: waiting on another window's reply failed; looking again at the next look");
                    self.holds_changed(since, true).await;
                }
            }
        }
    }
}

static LOADER: OnceLock<Arc<Core>> = OnceLock::new();
static AGENTS: LiveAgents<super::GooseAcpAgent> = LiveAgents::new();

/// Every ACP connection's agent this process serves, held weakly. One goosed serves every desktop
/// window (Q-257), one agent per connection: the loader must reach ANY window still connected — the
/// newest live one — not only the latest to connect, which is gone once that window closes.
pub(super) struct LiveAgents<T> {
    agents: StdMutex<Vec<Weak<T>>>,
}

impl<T> LiveAgents<T> {
    pub(super) const fn new() -> Self {
        Self {
            agents: StdMutex::new(Vec::new()),
        }
    }

    pub(super) fn attach(&self, agent: &Arc<T>) {
        let mut agents = self.agents.lock().unwrap();
        agents.retain(|weak| weak.strong_count() > 0);
        agents.push(Arc::downgrade(agent));
    }

    /// Every agent whose connection is still served, oldest first.
    pub(super) fn all(&self) -> Vec<Arc<T>> {
        let mut agents = self.agents.lock().unwrap();
        agents.retain(|weak| weak.strong_count() > 0);
        agents.iter().filter_map(Weak::upgrade).collect()
    }

    /// The newest agent whose connection is still served, if any.
    pub(super) fn live(&self) -> Option<Arc<T>> {
        let mut agents = self.agents.lock().unwrap();
        agents.retain(|weak| weak.strong_count() > 0);
        agents.iter().rev().find_map(Weak::upgrade)
    }
}

/// Install this process's loader (once; the ACP server calls it as it starts), and have this Mac's
/// engine report its loads to the load store.
pub(super) fn install() {
    // One loader per PROCESS, over the Mac-wide holder records: a unit test's agent must not
    // install it (the loader's own tests build a `Core` of their own).
    if cfg!(test) {
        return;
    }
    let mut installed_now = false;
    let core = LOADER.get_or_init(|| {
        installed_now = true;
        let dir = match goose_sidecar::holders::holders_dir() {
            Ok(dir) => Some(dir),
            Err(e) => {
                tracing::error!(error = %format!("{e:#}"), "nodes loader: this Mac's holder directory is unknown; this goose cannot see other windows' replies and will not switch ways");
                None
            }
        };
        Core::new(Arc::new(live::AgentWays), dir)
    });
    if installed_now {
        seam::install_loader(Arc::new(Seam(Arc::clone(core))));
        goose_sidecar::engine::global_manager().set_load_observer(rows::single_observer());
    }
}

/// An agent whose ACP handlers the loader may call: every connection this process serves.
pub(super) fn attach_agent(agent: &Arc<super::GooseAcpAgent>) {
    AGENTS.attach(agent);
}

fn agent() -> Result<Arc<super::GooseAcpAgent>, Refusal> {
    AGENTS.live().ok_or_else(|| {
        Refusal::new(
            NodeLoadRefusalCode::Unknown,
            "no goose window is connected to this goose process, so it cannot start a way",
        )
    })
}

/// The reply guard `on_prompt` holds for the whole turn, as a person's reply or a loop's tick
/// (session loops §5.5: a tick's reply never reads as a user's). `None` before the loader is
/// installed.
pub(super) fn open_reply(
    session_id: &str,
    kind: goose_sidecar::holders::ReplyKind,
) -> Option<ReplyGuard> {
    LOADER
        .get()
        .map(|core| core.holds().open_reply_as(session_id, kind))
}

#[cfg(test)]
mod tests;
