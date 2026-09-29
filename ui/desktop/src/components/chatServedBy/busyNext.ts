import type { NodeResidency } from '@aaif/goose-sdk';
import { effectiveEntry, ifServingOtherOf, nodeNamesById, parseRouteModel } from '../nodes/model';
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
 * answer), a cloud/endpoint node is always ready, any other MLX node is not loaded. Loading a node
 * stops the way this Mac's goose is served by — the busy one, serving another chat — so the role's
 * `ifServingOther` decides what the loader does with that load (Q-458, live 3.0.74: under "Use the
 * next node" the bar said the message waits for the answer, then loads the Studio, while the turn
 * went to the cloud at once): `takeOver` waits for the answer and loads, `wait` holds the turn until
 * that chat is closed or moved, `useNext` leaves the node to it and walks on. Only the outcomes
 * that differ from sharing are named; anything else (a queue, a chain with nothing to take it, a
 * residency goosed could not read) leaves the bar's own words.
 */
export type BusyNext =
  /** The turn goes to another node now — the busy answer is not waited for. */
  | { kind: 'goesTo'; node: string }
  /**
   * `useNext`: the turn goes to `node` now; `passed` is the node before it that is not loaded —
   * loading it would stop the busy way, so nothing is stopped.
   */
  | { kind: 'goesToNext'; node: string; passed: string }
  /** `takeOver`: the turn waits for the busy answer, then the loader loads its node. */
  | { kind: 'loadsAfter'; node: string }
  /** `wait`: the turn waits until the busy chat is closed or moved, then its node loads. */
  | { kind: 'waitsForChat'; node: string };

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

function chainFactsOf(facts: ChatNodesFacts, entry: NodeRoleEntry): Facts {
  const residencyOf = new Map(facts.residency.nodes.map((r) => [r.node, r.residency]));
  const chainFacts: Facts = {};
  for (const link of entry.chain) chainFacts[link.node] = factOf(residencyOf.get(link.node));
  return chainFacts;
}

/**
 * The node this chat's chain LOADS for its turn now, by the router's rule over goosed's residency;
 * null = the turn is served as things stand (or the chat names no chain). Q-462: while a switch is
 * between the loader's marks, this is the node the switch is for.
 */
export function chainLoadOf(facts: ChatNodesFacts, model: string | null | undefined): string | null {
  const entry = model ? entryOf(facts, model) : null;
  if (!entry || entry.chain.length === 0) return null;
  const { decision } = resolve(entry, chainFactsOf(facts, entry), facts.servedNode ?? null, {});
  return decision.kind === 'load' ? decision.node : null;
}

export function busyNextOf(
  facts: ChatNodesFacts,
  model: string | null | undefined
): BusyNext | null {
  if (!model) return null;
  const entry = entryOf(facts, model);
  if (!entry || entry.chain.length === 0) return null;
  const residencyOf = new Map(facts.residency.nodes.map((r) => [r.node, r.residency]));
  const chainFacts = chainFactsOf(facts, entry);
  const names = nodeNamesById(facts.read.nodes);
  const name = (id: string) => names[id] ?? id;
  const setting = ifServingOtherOf(entry);
  let passed: string | null = null;
  for (;;) {
    const { decision } = resolve(entry, chainFacts, facts.servedNode ?? null, {});
    if (decision.kind === 'load') {
      if (setting === 'takeOver') return { kind: 'loadsAfter', node: name(decision.node) };
      if (setting === 'wait') return { kind: 'waitsForChat', node: name(decision.node) };
      // `useNext`: the loader leaves this node to the busy way; the router walks on.
      passed ??= decision.node;
      chainFacts[decision.node] = { kind: 'cantRun', reason: 'servingOther' };
      continue;
    }
    if (decision.kind === 'serve' && residencyOf.get(decision.node)?.kind !== 'serving') {
      return passed != null
        ? { kind: 'goesToNext', node: name(decision.node), passed: name(passed) }
        : { kind: 'goesTo', node: name(decision.node) };
    }
    return null;
  }
}
