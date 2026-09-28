import type { NodeResidency } from '@aaif/goose-sdk';
import { effectiveEntry, nodeNamesById, parseRouteModel } from '../nodes/model';
import type { NodeRoleEntry } from '../nodes/model';
import { resolve, type EntryFact, type Facts } from '../nodes/resolve';
import type { ChatNodesFacts } from './chatServedBy';

/**
 * WHERE A MESSAGE SENT NOW GOES while the engine is busy in another chat (Q-431). The busy bar's
 * "shares the engine with that answer" is true only when this chat's next turn goes to the way
 * that is busy. A `node:`/`strategy:` chat's turn follows its own chain instead — the demo's chat on
 * "Studio chat, split for heavy work" (Chat: the Studio single, then the cloud, "Use the next
 * meanwhile") was told it would share the split while its turn went to the cloud.
 *
 * The chain is walked by the router's own rule (`nodes/resolve.ts`, the mirror of resolve.rs) over
 * goosed's residency: a serving node takes the turn (the engine batches it beside the other
 * answer), a cloud/endpoint node is always ready, any other MLX node is not loaded. Only the two
 * outcomes that differ from sharing are named; anything else (a queue, a chain with nothing to
 * take it, a residency goosed could not read) leaves the bar's own words.
 */
export type BusyNext =
  /** The turn goes to another node now — the busy answer is not waited for. */
  | { kind: 'goesTo'; node: string }
  /** The turn waits for the busy answer, then the loader loads its node. */
  | { kind: 'loadsAfter'; node: string };

function factOf(residency: NodeResidency | undefined): EntryFact | undefined {
  switch (residency?.kind) {
    case 'serving':
    case 'alwaysReady':
      return { kind: 'servable' };
    case 'notRunning':
    case 'loading':
    case 'waiting':
    case 'refusedLastTime':
      return { kind: 'notLoaded' };
    default:
      return undefined;
  }
}

function entryOf(facts: ChatNodesFacts, model: string): NodeRoleEntry | null {
  const route = parseRouteModel(model);
  if (route?.kind === 'node') {
    return { chain: [{ node: route.id, weight: 1 }], when: 'failover', ifNotLoaded: 'load' };
  }
  if (route?.kind !== 'strategy') return null;
  const strategy = (facts.read.config.strategies ?? []).find((s) => s.id === route.id);
  return strategy ? effectiveEntry(strategy, route.role ?? 'chat') : null;
}

export function busyNextOf(
  facts: ChatNodesFacts,
  model: string | null | undefined
): BusyNext | null {
  if (!model) return null;
  const entry = entryOf(facts, model);
  if (!entry || entry.chain.length === 0) return null;
  const residencyOf = new Map(facts.residency.nodes.map((r) => [r.node, r.residency]));
  const chainFacts: Facts = {};
  for (const link of entry.chain) chainFacts[link.node] = factOf(residencyOf.get(link.node));
  const { decision } = resolve(entry, chainFacts, facts.servedNode ?? null, {});
  const names = nodeNamesById(facts.read.nodes);
  const name = (id: string) => names[id] ?? id;
  if (decision.kind === 'load') return { kind: 'loadsAfter', node: name(decision.node) };
  if (decision.kind === 'serve' && residencyOf.get(decision.node)?.kind !== 'serving') {
    return { kind: 'goesTo', node: name(decision.node) };
  }
  return null;
}
