/**
 * Pure chain resolution — the mirror of `crates/goose/src/nodes/resolve.rs` (design §6.3). Both
 * suites run `nodes.fixture.json`, so the desktop's explanation of a turn and the router's choice
 * cannot drift. No I/O, no clock.
 *
 * - failover: walk the chain; the first servable entry serves, a busy one queues the work on
 *   itself, a not-loaded one loads (`load`) or is passed over (`useNext`); an entry that can't run
 *   or failed to load is passed over with its reason.
 * - overflow: the same walk, but a busy entry is passed over; when nothing serves or loads, the
 *   work queues on every busy entry.
 * - share: smooth weighted round-robin over the entries that can take work now (servable, plus
 *   not-loaded under `load`); a conversation's sticky node keeps its work while it can take it.
 * - A chain whose entries are all passed over is a refusal naming each one; never "any node".
 */
import type { NodeIfNotLoaded, NodeRole, NodeRoleEntry, NodeStrategy, NodeWhen } from './model';
import { effectiveRole } from './model';

export type EntryFact =
  | { kind: 'servable' }
  | { kind: 'busy' }
  | { kind: 'notLoaded' }
  | { kind: 'cantRun'; reason: string }
  | { kind: 'loadFailed'; words: string };

export type PassedOver =
  | { kind: 'busy' }
  | { kind: 'notLoaded' }
  | { kind: 'cantRun'; reason: string }
  | { kind: 'loadFailed'; words: string }
  /** No fact was given for it: nothing is known, so it is not guessed servable. */
  | { kind: 'unknown' };

export interface Tried {
  node: string;
  why: PassedOver;
}

export type Decision =
  | { kind: 'serve'; node: string; rank: number; tried: Tried[] }
  | { kind: 'load'; node: string; rank: number; tried: Tried[] }
  | { kind: 'queue'; nodes: string[]; tried: Tried[] }
  | { kind: 'exhausted'; tried: Tried[] };

/** The round-robin's current weights per node (absent = 0); owned by the caller. */
export type ShareState = Record<string, number>;

export type Facts = Record<string, EntryFact | undefined>;

function passed(fact: EntryFact | undefined): PassedOver {
  switch (fact?.kind) {
    case 'busy':
      return { kind: 'busy' };
    case 'notLoaded':
      return { kind: 'notLoaded' };
    case 'cantRun':
      return { kind: 'cantRun', reason: fact.reason };
    case 'loadFailed':
      return { kind: 'loadFailed', words: fact.words };
    default:
      return { kind: 'unknown' };
  }
}

/**
 * Which node of `entry` takes the work. `sticky` is the conversation's node (read by `share`
 * only). Returns the decision and the round-robin state after it (a new object; `share` is not
 * mutated).
 */
export function resolve(
  entry: NodeRoleEntry,
  facts: Facts,
  sticky: string | null,
  share: ShareState
): { decision: Decision; share: ShareState } {
  const when: NodeWhen = entry.when ?? 'failover';
  const ifNotLoaded: NodeIfNotLoaded = entry.ifNotLoaded ?? 'load';
  if (when === 'share') return shared(entry, ifNotLoaded, facts, sticky, share);
  return { decision: walk(entry, when, ifNotLoaded, facts), share: { ...share } };
}

function walk(
  entry: NodeRoleEntry,
  when: NodeWhen,
  ifNotLoaded: NodeIfNotLoaded,
  facts: Facts
): Decision {
  const tried: Tried[] = [];
  const busy: string[] = [];
  for (const [index, link] of entry.chain.entries()) {
    const rank = index + 1;
    const node = link.node;
    const fact = facts[node];
    if (fact?.kind === 'servable') return { kind: 'serve', node, rank, tried };
    if (fact?.kind === 'busy') {
      if (when === 'failover') return { kind: 'queue', nodes: [node], tried };
      busy.push(node);
    }
    if (fact?.kind === 'notLoaded' && ifNotLoaded === 'load') {
      return { kind: 'load', node, rank, tried };
    }
    tried.push({ node, why: passed(fact) });
  }
  if (busy.length === 0) return { kind: 'exhausted', tried };
  return { kind: 'queue', nodes: busy, tried: tried.filter((t) => t.why.kind !== 'busy') };
}

function shared(
  entry: NodeRoleEntry,
  ifNotLoaded: NodeIfNotLoaded,
  facts: Facts,
  sticky: string | null,
  share: ShareState
): { decision: Decision; share: ShareState } {
  const next: ShareState = { ...share };
  const rankOf = (node: string) => entry.chain.findIndex((l) => l.node === node) + 1;
  const canTake = (fact: EntryFact | undefined) =>
    fact?.kind === 'servable' || (fact?.kind === 'notLoaded' && ifNotLoaded === 'load');

  if (sticky !== null && entry.chain.some((l) => l.node === sticky)) {
    const fact = facts[sticky];
    if (fact?.kind === 'servable') {
      return {
        decision: { kind: 'serve', node: sticky, rank: rankOf(sticky), tried: [] },
        share: next,
      };
    }
    if (fact?.kind === 'busy') {
      return { decision: { kind: 'queue', nodes: [sticky], tried: [] }, share: next };
    }
  }

  const candidates = entry.chain.filter((l) => canTake(facts[l.node]));
  const tried: Tried[] = entry.chain
    .filter((l) => !canTake(facts[l.node]))
    .map((l) => ({ node: l.node, why: passed(facts[l.node]) }));
  if (candidates.length === 0) {
    const busy = tried.filter((t) => t.why.kind === 'busy').map((t) => t.node);
    if (busy.length === 0) return { decision: { kind: 'exhausted', tried }, share: next };
    return {
      decision: { kind: 'queue', nodes: busy, tried: tried.filter((t) => t.why.kind !== 'busy') },
      share: next,
    };
  }

  const total = candidates.reduce((sum, l) => sum + l.weight, 0);
  let best: string | null = null;
  for (const link of candidates) {
    next[link.node] = (next[link.node] ?? 0) + link.weight;
    if (best === null || next[link.node] > next[best]) best = link.node;
  }
  const node = best as string;
  next[node] -= total;
  const kind = facts[node]?.kind === 'notLoaded' ? 'load' : 'serve';
  return { decision: { kind, node, rank: rankOf(node), tried }, share: next };
}

export interface SentenceEntry {
  node: string;
  /** The node's display name; the id when no def carries it. */
  name: string;
  rank: number;
  weight: number;
}

/** What a sentence about a role says; `strategySentence.ts` composes the words from these. */
export interface SentenceFacts {
  role: NodeRole;
  /** The role this one inherits its entry from ("Same as Build"); absent when set. */
  sameAs?: NodeRole;
  when: NodeWhen;
  ifNotLoaded: NodeIfNotLoaded;
  entries: SentenceEntry[];
  /** The sum of the weights, when the role shares. */
  shareTotal?: number;
}

export function sentenceFacts(
  strategy: NodeStrategy,
  role: NodeRole,
  names: Record<string, string>
): SentenceFacts | null {
  const roles = strategy.roles ?? {};
  const source = effectiveRole(roles, role);
  const entry = source ? roles[source] : null;
  if (!source || !entry) return null;
  const when: NodeWhen = entry.when ?? 'failover';
  const facts: SentenceFacts = {
    role,
    when,
    ifNotLoaded: entry.ifNotLoaded ?? 'load',
    entries: entry.chain.map((l, i) => ({
      node: l.node,
      name: names[l.node] ?? l.node,
      rank: i + 1,
      weight: l.weight,
    })),
  };
  if (source !== role) facts.sameAs = source;
  if (when === 'share') facts.shareTotal = entry.chain.reduce((sum, l) => sum + l.weight, 0);
  return facts;
}
