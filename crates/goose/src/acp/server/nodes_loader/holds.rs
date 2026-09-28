//! Reply holds (design §6.4 step 8, §13 item 3): the unit the loader batches on is an agent REPLY,
//! not a model call. `on_prompt` opens a reply for the whole turn; every lease of the reply (and of
//! its delegates, whose root is the reply's session) notes the way it used; the reply holds that
//! way until it ends. A reply in a tool loop therefore keeps its way for every completion and never
//! swaps mid-reply.
//!
//! This process's replies live here; other processes' are read from the Mac-wide holder records
//! (`goose_sidecar::holders`). A reply is published there only once it has a way (a reply that
//! never touched an MLX engine holds nothing), with a lock another process's loader can wait on.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};

use goose_sidecar::holders::{self, HolderEntry, HolderKind, Registration, ReplyHold, ReplyKind};
use goose_sidecar::placement::store::PlacementKey;
use tokio::sync::Notify;

use super::switch::Stop;

#[derive(Debug, Clone)]
pub(crate) struct Reply {
    pub session: String,
    pub root: String,
    pub way: Option<PlacementKey>,
    /// When the reply opened, on the loader's one sequence (demands are numbered on it too).
    pub opened: u64,
    /// How many of its demands wait in the loader. While any does, no model call of it is in
    /// flight (a delegate's demand counts for its parent's reply).
    pub waiting: u32,
    pub kind: ReplyKind,
}

#[derive(Default)]
struct State {
    replies: HashMap<u64, Reply>,
    /// The open reply of each session.
    open: HashMap<String, u64>,
    /// A delegate session's parent (summon's `note_child`).
    parents: HashMap<String, String>,
}

/// What must end before a switch may stop a way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Blocker {
    /// A reply of this process.
    Here {
        session: String,
        way: String,
        kind: ReplyKind,
    },
    /// A reply of another goose process on this Mac; its lock wakes a waiter when it ends. Its
    /// kind is the one that process published in its holder record.
    Elsewhere {
        pid: u32,
        session: String,
        way: String,
        lock: PathBuf,
        kind: ReplyKind,
    },
}

impl Blocker {
    pub fn way(&self) -> &str {
        match self {
            Blocker::Here { way, .. } | Blocker::Elsewhere { way, .. } => way,
        }
    }

    pub fn session(&self) -> &str {
        match self {
            Blocker::Here { session, .. } | Blocker::Elsewhere { session, .. } => session,
        }
    }

    pub fn kind(&self) -> ReplyKind {
        match self {
            Blocker::Here { kind, .. } | Blocker::Elsewhere { kind, .. } => *kind,
        }
    }
}

/// A holder the loader must not stop under: a swarm build (S8 registers it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BuildHolder {
    pub pid: u32,
    pub what: String,
}

pub(crate) struct Holds {
    state: StdMutex<State>,
    seq: AtomicU64,
    changed: Arc<Notify>,
    /// The Mac-wide holder directory; `None` = this loader publishes nothing and reads no other
    /// process (a unit test of this process alone).
    dir: Option<PathBuf>,
    registration: StdMutex<Option<Arc<Registration>>>,
}

impl Holds {
    pub fn new(dir: Option<PathBuf>, changed: Arc<Notify>) -> Arc<Self> {
        Arc::new(Holds {
            state: StdMutex::new(State::default()),
            seq: AtomicU64::new(1),
            changed,
            dir,
            registration: StdMutex::new(None),
        })
    }

    pub fn dir(&self) -> Option<&Path> {
        self.dir.as_deref()
    }

    /// The loader's one sequence: replies and demands are ordered on it.
    pub fn next_seq(&self) -> u64 {
        self.seq.fetch_add(1, Ordering::SeqCst)
    }

    /// The session at the root of `session`'s delegate chain.
    pub fn root_of(&self, session: &str) -> String {
        let state = self.state.lock().unwrap();
        root_in(&state, session)
    }

    /// When the reply `root` has open opened, if it has one.
    pub fn reply_opened(&self, root: &str) -> Option<u64> {
        let state = self.state.lock().unwrap();
        let id = state.open.get(root)?;
        state.replies.get(id).map(|r| r.opened)
    }

    /// Whether the reply `root` has open is a loop's tick. A root with no open reply (a card's
    /// Start, a scheduled job) is no tick: every tick opens its reply in `on_prompt` first.
    pub fn is_tick(&self, root: &str) -> bool {
        let state = self.state.lock().unwrap();
        state
            .open
            .get(root)
            .and_then(|id| state.replies.get(id))
            .is_some_and(|r| r.kind == ReplyKind::Tick)
    }

    pub fn open_reply(self: &Arc<Self>, session: &str) -> ReplyGuard {
        self.open_reply_as(session, ReplyKind::User)
    }

    /// A reply of `kind`: `on_prompt` opens a loop's tick as `Tick`, every other turn as `User`.
    pub fn open_reply_as(self: &Arc<Self>, session: &str, kind: ReplyKind) -> ReplyGuard {
        let id = self.next_seq();
        {
            let mut state = self.state.lock().unwrap();
            let root = root_in(&state, session);
            state.replies.insert(
                id,
                Reply {
                    session: session.to_string(),
                    root,
                    way: None,
                    opened: id,
                    waiting: 0,
                    kind,
                },
            );
            state.open.insert(session.to_string(), id);
        }
        ReplyGuard {
            holds: Arc::clone(self),
            id,
        }
    }

    fn close(&self, id: u64) {
        let published = {
            let mut state = self.state.lock().unwrap();
            let Some(reply) = state.replies.remove(&id) else {
                return;
            };
            if state.open.get(&reply.session) == Some(&id) {
                state.open.remove(&reply.session);
            }
            // A delegate runs inside its parent's reply: when the reply ends, so do they.
            state.parents.retain(|_, root| *root != reply.session);
            reply.way.is_some()
        };
        if published {
            self.publish(|reg| reg.close_reply(id));
        }
        self.changed.notify_waiters();
    }

    /// The way a lease of `session` used: the reply at its root holds it from now on.
    pub fn note_lease(&self, session: &str, way: PlacementKey) {
        let (id, root, newly, kind) = {
            let mut state = self.state.lock().unwrap();
            let root = root_in(&state, session);
            let Some(&id) = state.open.get(&root) else {
                // A lease with no reply open (a scheduled job, the Link mirror's executor): it
                // holds nothing across calls. Named, so an unexpected door shows in the log.
                tracing::debug!(%session, %root, "nodes loader: a lease of a session with no open reply holds nothing");
                return;
            };
            let reply = state
                .replies
                .get_mut(&id)
                .expect("an open reply is registered");
            if reply.way.as_ref() == Some(&way) {
                return;
            }
            let newly = reply.way.is_none();
            reply.way = Some(way.clone());
            (id, root, newly, reply.kind)
        };
        if newly {
            let hold = ReplyHold {
                reply: id,
                session: session.to_string(),
                root_session: root,
                way: Some(way),
                waiting: false,
                kind,
            };
            self.publish(|reg| reg.open_reply(hold));
        } else {
            self.publish(|reg| reg.set_reply_way(id, way));
        }
        self.changed.notify_waiters();
    }

    pub fn note_child(&self, child: &str, parent: &str) {
        let mut state = self.state.lock().unwrap();
        let root = root_in(&state, parent);
        if root != child {
            state.parents.insert(child.to_string(), root);
        }
    }

    /// The reply at `root` waits in the loader (or runs again): while it waits it holds nothing.
    pub fn set_waiting(&self, root: &str, waiting: bool) {
        let target = {
            let mut state = self.state.lock().unwrap();
            let Some(&id) = state.open.get(root) else {
                return;
            };
            let reply = state
                .replies
                .get_mut(&id)
                .expect("an open reply is registered");
            let before = reply.waiting;
            reply.waiting = if waiting {
                before + 1
            } else {
                before.saturating_sub(1)
            };
            // Only the first demand to wait and the last to stop change what others see.
            let crossed = (before == 0) != (reply.waiting == 0);
            (crossed && reply.way.is_some()).then_some(id)
        };
        if let Some(id) = target {
            if waiting {
                self.publish(|reg| reg.pause_reply(id));
            } else {
                self.publish(|reg| reg.resume_reply(id));
            }
        }
        self.changed.notify_waiters();
    }

    /// Open replies — in this process and every other goose process on this Mac — that hold a way
    /// in `stops`, except the demand's own root and replies waiting in a loader.
    pub fn blockers(&self, stops: &[Stop], own_root: Option<&str>) -> Result<Vec<Blocker>, String> {
        let words =
            |way: &PlacementKey| stops.iter().find(|s| s.held_by(way)).map(|s| s.way.words());
        let mut out = Vec::new();
        {
            let state = self.state.lock().unwrap();
            let mut here: Vec<&Reply> = state.replies.values().collect();
            here.sort_by_key(|r| r.opened);
            for reply in here {
                if reply.waiting > 0 || Some(reply.root.as_str()) == own_root {
                    continue;
                }
                if let Some(way) = reply.way.as_ref().and_then(words) {
                    out.push(Blocker::Here {
                        session: reply.session.clone(),
                        way,
                        kind: reply.kind,
                    });
                }
            }
        }
        for record in self.other_records()? {
            let HolderKind::Goosed { replies } = &record.kind else {
                continue;
            };
            for reply in replies {
                if reply.waiting {
                    continue;
                }
                if let Some(way) = reply.way.as_ref().and_then(words) {
                    out.push(Blocker::Elsewhere {
                        pid: record.pid,
                        session: reply.session.clone(),
                        way,
                        lock: holders::reply_lock_path(
                            self.dir
                                .as_deref()
                                .expect("other records come from a directory"),
                            record.pid,
                            record.started_at,
                            reply.reply,
                        ),
                        kind: reply.kind,
                    });
                }
            }
        }
        Ok(out)
    }

    /// A live swarm build holding this Mac's engine (a record of another process).
    pub fn build_holder(&self) -> Result<Option<BuildHolder>, String> {
        Ok(self
            .other_records()?
            .into_iter()
            .find_map(|record| match record.kind {
                HolderKind::SwarmRun { what, .. } => Some(BuildHolder {
                    pid: record.pid,
                    what,
                }),
                HolderKind::Goosed { .. } => None,
            }))
    }

    /// Every other process's live record. An unreadable one is named: which replies hold the
    /// engine is then unknown, and nothing is stopped on a guess.
    fn other_records(&self) -> Result<Vec<holders::HolderRecord>, String> {
        let Some(dir) = &self.dir else {
            return Ok(Vec::new());
        };
        let entries = holders::read_all(dir).map_err(|e| {
            format!(
                "the MLX holder records under {} could not be read: {e:#}",
                dir.display()
            )
        })?;
        let pid = std::process::id();
        let mut out = Vec::new();
        for entry in entries {
            match entry {
                HolderEntry::Live(record) if record.pid != pid => out.push(record),
                HolderEntry::Live(_) | HolderEntry::Stale { .. } => {}
                HolderEntry::Unreadable { path, error } => {
                    return Err(format!(
                        "the MLX holder record {} is unreadable ({error}); which goose window's replies use the engine is unknown",
                        path.display()
                    ))
                }
            }
        }
        Ok(out)
    }

    /// Mirror a change into this process's holder record, registering on first use. A failure is
    /// logged loudly: other windows then cannot see this reply, and could stop its way under it.
    fn publish(&self, change: impl FnOnce(&Registration) -> anyhow::Result<()>) {
        let Some(dir) = &self.dir else {
            return;
        };
        let registration = {
            let mut slot = self.registration.lock().unwrap();
            match &*slot {
                Some(reg) => Arc::clone(reg),
                None => match Registration::register(
                    dir,
                    HolderKind::Goosed {
                        replies: Vec::new(),
                    },
                ) {
                    Ok(reg) => {
                        let reg = Arc::new(reg);
                        *slot = Some(Arc::clone(&reg));
                        reg
                    }
                    Err(e) => {
                        tracing::error!(dir = %dir.display(), error = %format!("{e:#}"), "nodes loader: this goose could not register as an MLX engine holder; other windows cannot see its replies");
                        return;
                    }
                },
            }
        };
        if let Err(e) = change(&registration) {
            tracing::error!(error = %format!("{e:#}"), "nodes loader: this goose's MLX holder record could not be updated; other windows see a stale view of its replies");
        }
    }

    #[cfg(test)]
    pub fn reply(&self, session: &str) -> Option<Reply> {
        let state = self.state.lock().unwrap();
        state
            .open
            .get(session)
            .and_then(|id| state.replies.get(id))
            .cloned()
    }
}

fn root_in(state: &State, session: &str) -> String {
    let mut root = session;
    // A chain is a tree built by summon (`note_child` never links a session to itself); the
    // bound is the map's own size, which a cycle could not outrun.
    for _ in 0..=state.parents.len() {
        match state.parents.get(root) {
            Some(parent) => root = parent,
            None => break,
        }
    }
    root.to_string()
}

/// An open reply: taken in `on_prompt` for the whole turn, dropped when the turn ends — however it
/// ends (answered, cancelled, errored).
pub struct ReplyGuard {
    holds: Arc<Holds>,
    id: u64,
}

impl Drop for ReplyGuard {
    fn drop(&mut self) {
        self.holds.close(self.id);
    }
}
