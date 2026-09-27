//! Pure chain resolution (design §6.3): given a role's entry and what each chain node can do
//! right now, which node takes the work — or that it waits on a load, queues, or is refused with
//! every entry named. No I/O, no clock. The desktop's `components/nodes/resolve.ts` is the mirror;
//! both suites run `nodes.fixture.json`, so the two cannot drift.
//!
//! The rules:
//! - `failover`: walk the chain in order. The first servable entry serves; a busy one queues the
//!   work on itself; a not-loaded one loads (`load`) or is passed over (`useNext`); an entry that
//!   can't run or failed to load is passed over with its reason.
//! - `overflow`: the same walk, but a busy entry is passed over; when nothing serves or loads,
//!   the work queues on every busy entry.
//! - `share`: smooth weighted round-robin (deterministic, never random) over the entries that can
//!   take work now — servable ones, plus not-loaded ones under `load`. A picked not-loaded entry
//!   loads. With none, the work queues on the busy entries. A conversation's sticky node keeps its
//!   work while it can take it (moving a conversation throws away its prompt cache).
//! - A chain whose entries are all passed over is a refusal naming each entry and its reason; it
//!   never falls to "any node" (gate 1).

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

use super::{effective_role, NodeIfNotLoaded, NodeRole, NodeRoleEntry, NodeStrategy, NodeWhen};

/// What one chain node can do right now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum EntryFact {
    /// Serving, with a free slot.
    Servable,
    /// Serving, every slot taken.
    Busy,
    /// Not loaded (an MLX node whose way does not serve now); loadable.
    NotLoaded,
    CantRun {
        reason: String,
    },
    /// A load of it for this work already failed; `words` are the load's own.
    LoadFailed {
        words: String,
    },
}

/// Why an entry did not take the work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PassedOver {
    Busy,
    NotLoaded,
    CantRun {
        reason: String,
    },
    LoadFailed {
        words: String,
    },
    /// No fact was given for it: nothing is known, so it is not guessed servable.
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tried {
    pub node: String,
    pub why: PassedOver,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Decision {
    /// `node` takes the work now; `rank` is its place in the chain (1 = the 1st).
    Serve {
        node: String,
        rank: u32,
        tried: Vec<Tried>,
    },
    /// `node` must be loaded first; the work waits on the load.
    Load {
        node: String,
        rank: u32,
        tried: Vec<Tried>,
    },
    /// Every candidate is busy: the work queues on these.
    Queue {
        nodes: Vec<String>,
        tried: Vec<Tried>,
    },
    /// No entry can take the work; each is named with its reason.
    Exhausted { tried: Vec<Tried> },
}

/// The smooth weighted round-robin's current weights, per node (absent = 0). Owned by the caller
/// (per role, per conversation start) and returned updated.
pub type ShareState = BTreeMap<String, i64>;

fn fact_of<'a>(facts: &'a HashMap<String, EntryFact>, node: &str) -> Option<&'a EntryFact> {
    facts.get(node)
}

fn passed(fact: Option<&EntryFact>) -> PassedOver {
    match fact {
        Some(EntryFact::Busy) => PassedOver::Busy,
        Some(EntryFact::NotLoaded) => PassedOver::NotLoaded,
        Some(EntryFact::CantRun { reason }) => PassedOver::CantRun {
            reason: reason.clone(),
        },
        Some(EntryFact::LoadFailed { words }) => PassedOver::LoadFailed {
            words: words.clone(),
        },
        Some(EntryFact::Servable) | None => PassedOver::Unknown,
    }
}

/// Which node of `entry` takes the work. `sticky` is the conversation's node (read by `share`
/// only); `share` is the round-robin state, updated in place when `share` picks anew.
pub fn resolve(
    entry: &NodeRoleEntry,
    facts: &HashMap<String, EntryFact>,
    sticky: Option<&str>,
    share: &mut ShareState,
) -> Decision {
    match entry.when {
        NodeWhen::Failover | NodeWhen::Overflow => walk(entry, facts),
        NodeWhen::Share => shared(entry, facts, sticky, share),
    }
}

fn walk(entry: &NodeRoleEntry, facts: &HashMap<String, EntryFact>) -> Decision {
    let mut tried = Vec::new();
    let mut busy = Vec::new();
    for (index, link) in entry.chain.iter().enumerate() {
        let rank = index as u32 + 1;
        let node = link.node.clone();
        match fact_of(facts, &link.node) {
            Some(EntryFact::Servable) => return Decision::Serve { node, rank, tried },
            Some(EntryFact::Busy) if entry.when == NodeWhen::Failover => {
                return Decision::Queue {
                    nodes: vec![node],
                    tried,
                }
            }
            Some(EntryFact::Busy) => busy.push(node.clone()),
            Some(EntryFact::NotLoaded) if entry.if_not_loaded == NodeIfNotLoaded::Load => {
                return Decision::Load { node, rank, tried }
            }
            _ => {}
        }
        tried.push(Tried {
            node,
            why: passed(fact_of(facts, &link.node)),
        });
    }
    if busy.is_empty() {
        Decision::Exhausted { tried }
    } else {
        tried.retain(|t| t.why != PassedOver::Busy);
        Decision::Queue { nodes: busy, tried }
    }
}

fn shared(
    entry: &NodeRoleEntry,
    facts: &HashMap<String, EntryFact>,
    sticky: Option<&str>,
    share: &mut ShareState,
) -> Decision {
    let rank_of = |node: &str| {
        entry
            .chain
            .iter()
            .position(|l| l.node == node)
            .map_or(0, |i| i as u32 + 1)
    };
    let can_take = |fact: Option<&EntryFact>| match fact {
        Some(EntryFact::Servable) => true,
        Some(EntryFact::NotLoaded) => entry.if_not_loaded == NodeIfNotLoaded::Load,
        _ => false,
    };
    let mut tried: Vec<Tried> = Vec::new();

    if let Some(sticky) = sticky.filter(|s| entry.chain.iter().any(|l| l.node == *s)) {
        match fact_of(facts, sticky) {
            Some(EntryFact::Servable) => {
                return Decision::Serve {
                    node: sticky.to_string(),
                    rank: rank_of(sticky),
                    tried,
                }
            }
            Some(EntryFact::Busy) => {
                return Decision::Queue {
                    nodes: vec![sticky.to_string()],
                    tried,
                }
            }
            _ => {}
        }
    }

    let candidates: Vec<(&str, i64)> = entry
        .chain
        .iter()
        .filter(|l| can_take(fact_of(facts, &l.node)))
        .map(|l| (l.node.as_str(), i64::from(l.weight)))
        .collect();
    for link in &entry.chain {
        if !candidates.iter().any(|(n, _)| *n == link.node) {
            tried.push(Tried {
                node: link.node.clone(),
                why: passed(fact_of(facts, &link.node)),
            });
        }
    }
    if candidates.is_empty() {
        let busy: Vec<String> = tried
            .iter()
            .filter(|t| t.why == PassedOver::Busy)
            .map(|t| t.node.clone())
            .collect();
        if busy.is_empty() {
            return Decision::Exhausted { tried };
        }
        tried.retain(|t| t.why != PassedOver::Busy);
        return Decision::Queue { nodes: busy, tried };
    }

    let total: i64 = candidates.iter().map(|(_, w)| w).sum();
    let mut pick: Option<(&str, i64)> = None;
    for (node, weight) in &candidates {
        let current = share.entry(node.to_string()).or_insert(0);
        *current += weight;
        if pick.is_none_or(|(_, best)| *current > best) {
            pick = Some((node, *current));
        }
    }
    let (node, _) = pick.expect("candidates is not empty");
    *share.entry(node.to_string()).or_insert(0) -= total;
    let rank = rank_of(node);
    let node = node.to_string();
    match fact_of(facts, &node) {
        Some(EntryFact::NotLoaded) => Decision::Load { node, rank, tried },
        _ => Decision::Serve { node, rank, tried },
    }
}

/// What a sentence about a role says (the desktop's `strategySentence.ts` composes the words from
/// these facts through ICU templates).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SentenceFacts {
    pub role: NodeRole,
    /// The role whose entry this one uses when it is unset ("Same as Build"); absent when set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub same_as: Option<NodeRole>,
    pub when: NodeWhen,
    pub if_not_loaded: NodeIfNotLoaded,
    pub entries: Vec<SentenceEntry>,
    /// The sum of the weights, when the role shares (the "parts" of each entry are its weight).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub share_total: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SentenceEntry {
    pub node: String,
    /// The node's display name; the id when no def carries it.
    pub name: String,
    pub rank: u32,
    pub weight: u32,
}

/// The facts of `role`'s sentence in `strategy`; `None` when neither the role nor anything it
/// inherits from is set.
pub fn sentence_facts(
    strategy: &NodeStrategy,
    role: NodeRole,
    names: &HashMap<String, String>,
) -> Option<SentenceFacts> {
    let source = effective_role(&strategy.roles, role)?;
    let entry = strategy.roles.get(source)?;
    let entries = entry
        .chain
        .iter()
        .enumerate()
        .map(|(i, l)| SentenceEntry {
            node: l.node.clone(),
            name: names
                .get(&l.node)
                .cloned()
                .unwrap_or_else(|| l.node.clone()),
            rank: i as u32 + 1,
            weight: l.weight,
        })
        .collect();
    Some(SentenceFacts {
        role,
        same_as: (source != role).then_some(source),
        when: entry.when,
        if_not_loaded: entry.if_not_loaded,
        entries,
        share_total: (entry.when == NodeWhen::Share)
            .then(|| entry.chain.iter().map(|l| l.weight).sum()),
    })
}
