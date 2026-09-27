import {
  ROLES,
  effectiveEntry,
  type NodeRole,
  type NodeStrategy,
  type ResolvedNodeDef,
} from './model';

/**
 * WHAT A STRATEGY ASKS OF YOUR MACS (DESIGN-NODES-AND-STRATEGIES.md §8.4, review item 1). Pure.
 *
 * THE v1 RULE: ONE MLX way serves this Mac's goose at a time, across ALL your Macs — the router
 * serves chat from exactly one engine (a remote-single route, else the split, else this Mac's single
 * engine) and Run it stops every serving way before it starts another (Q-119). So ANY two distinct
 * MLX ways in one strategy conflict, whichever Macs they use: a split on both Macs and a single on
 * the Studio conflict exactly as two singles on this Mac do. Each conflicting pair is a swap, shown
 * with both MEASURED load times (or "not measured yet" — never an estimate).
 *
 * A way is what goosed's `nodes/write` compares (`pinned_way` in crates/goose/src/nodes/mod.rs): the
 * placement — kind, Macs in rank order, the split's link — plus the model. A node that FOLLOWS this
 * Mac's engine names no way (it serves whatever runs and never triggers a load); a cloud or
 * endpoint node is always available. Neither enters a swap.
 */

export interface FitUse {
  role: NodeRole;
  /** 1 = the role chain's 1st. */
  rank: number;
}

export interface FitWay {
  key: string;
  /** Every node of the strategy naming this way (usually one). */
  nodes: ResolvedNodeDef[];
  /** The set roles whose chains name it, in role order. */
  uses: FitUse[];
  /** The median of the first node's measured loads on this way; null = not measured yet. */
  load: { medianMs: number; count: number } | null;
}

export interface FitShareTwoWays {
  role: NodeRole;
  a: string;
  b: string;
}

export interface StrategyFit {
  /** The distinct MLX ways the strategy names, in the order its roles first name them. */
  ways: FitWay[];
  /** Every pair of distinct ways — each is a swap (one way at a time). */
  swaps: [FitWay, FitWay][];
  /** Cloud and endpoint nodes: always available. */
  cloud: ResolvedNodeDef[];
  /** Nodes that follow this Mac's engine: no way of their own, no load. */
  follows: ResolvedNodeDef[];
  /**
   * Chat's 1st and Build's 1st are different MLX ways and Build loads when not loaded: every
   * delegate call swaps twice (to Build's way and back to Chat's).
   */
  delegate: { chat: FitWay; build: FitWay } | null;
  /**
   * A `share` or `overflow` role whose chain holds two different MLX ways — `nodes/write` refuses
   * it, because it would stop one to load the other on every turn.
   */
  shareTwoWays: FitShareTwoWays[];
  /** Chain ids no definition carries (a write refuses them). */
  unknown: string[];
}

export type MeasuredLoad = (node: ResolvedNodeDef) => { medianMs: number; count: number } | null;

/** The way a node names, or null (follows, cloud, endpoint). */
export function wayKeyOf(node: ResolvedNodeDef): string | null {
  const placement = node.def.placement;
  if (node.def.kind !== 'mlx' || !placement || placement.kind === 'follows') return null;
  return JSON.stringify([
    placement.kind,
    placement.macs,
    placement.link ?? null,
    node.model ?? node.def.model ?? null,
  ]);
}

export function strategyFit(
  strategy: NodeStrategy,
  nodes: readonly ResolvedNodeDef[],
  measured: MeasuredLoad
): StrategyFit {
  const byId = new Map(nodes.map((n) => [n.def.id, n]));
  const ways = new Map<string, FitWay>();
  const cloud = new Map<string, ResolvedNodeDef>();
  const follows = new Map<string, ResolvedNodeDef>();
  const unknown = new Set<string>();
  const roles = strategy.roles ?? {};

  for (const role of ROLES) {
    const entry = roles[role];
    if (!entry) continue;
    entry.chain.forEach((link, index) => {
      const node = byId.get(link.node);
      if (!node) {
        unknown.add(link.node);
        return;
      }
      const key = wayKeyOf(node);
      if (key === null) {
        (node.def.kind === 'mlx' ? follows : cloud).set(node.def.id, node);
        return;
      }
      const way = ways.get(key) ?? { key, nodes: [], uses: [], load: measured(node) };
      if (!way.nodes.includes(node)) way.nodes.push(node);
      if (!way.uses.some((u) => u.role === role)) way.uses.push({ role, rank: index + 1 });
      ways.set(key, way);
    });
  }

  const list = [...ways.values()];
  const swaps: [FitWay, FitWay][] = [];
  for (let i = 0; i < list.length; i++) {
    for (let j = i + 1; j < list.length; j++) swaps.push([list[i], list[j]]);
  }

  const firstWay = (role: NodeRole): FitWay | null => {
    const first = effectiveEntry(strategy, role)?.chain[0];
    const node = first ? byId.get(first.node) : undefined;
    const key = node ? wayKeyOf(node) : null;
    return key ? (ways.get(key) ?? null) : null;
  };
  const chatWay = firstWay('chat');
  const buildWay = firstWay('build');
  const buildLoads = (effectiveEntry(strategy, 'build')?.ifNotLoaded ?? 'load') === 'load';
  const delegate =
    chatWay && buildWay && chatWay !== buildWay && buildLoads
      ? { chat: chatWay, build: buildWay }
      : null;

  const shareTwoWays: FitShareTwoWays[] = [];
  for (const role of ROLES) {
    const entry = roles[role];
    const when = entry?.when ?? 'failover';
    if (!entry || when === 'failover') continue;
    const pinned = entry.chain
      .map((link) => byId.get(link.node))
      .filter((n): n is ResolvedNodeDef => n != null && wayKeyOf(n) !== null);
    const other = pinned.find((n) => wayKeyOf(n) !== wayKeyOf(pinned[0]));
    if (other) shareTwoWays.push({ role, a: pinned[0].def.name, b: other.def.name });
  }

  return {
    ways: list,
    swaps,
    cloud: [...cloud.values()],
    follows: [...follows.values()],
    delegate,
    shareTwoWays,
    unknown: [...unknown],
  };
}
