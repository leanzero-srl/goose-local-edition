import type {
  NodeRefusalFactsDto,
  NodeServingOtherDto,
  NodesReadResponse_unstable,
  NodesResidencyResponse_unstable,
  ResolvedNodeDef,
} from '@aaif/goose-sdk';

/**
 * THE SWAP — the node loader's own state, as every surface reads it (Q-254). One MLX way serves this
 * Mac's goose at a time, so a strategy that hands Chat and Build to different ways stops one and
 * loads the other (design DESIGN-NODES-AND-STRATEGIES.md §6.4). While it does, the way it stopped
 * reads "failed · exit 143" (the SIGTERM of the stop) or "not mounted" to anything that looks only at
 * that engine — live J3 on 3.0.65 showed both on the composer and the glance for a swap that worked.
 *
 * The truth is goosed's `nodes/residency`: the loader marks the node it is loading `loading` (with
 * the engine's load phase once the way reports one) from the moment its stops begin until the way
 * serves or the load is refused — then the mark goes (a way Run it is starting reads `loading` the
 * same way). So while a node reads `loading`, an engine that is stopped, stopping or failed is the
 * switch's stop — unless it is the target's own way and model failing, which is a real failure and
 * is said as Failed with its words.
 *
 * Pure and React-free: the renderer derives it from its nodes read (glanceStore) and hands main the
 * result with its sessions report, so the composer, the chip, the Engine tile, the glance, the nav
 * chip and the tray all read this one derivation.
 */

export type SwapWay = 'single' | 'remoteSingle' | 'split';

export interface SwapTarget {
  id: string;
  /** The node's own name — the one name the Nodes page shows (`def.name`). */
  name: string;
  /** The model the node loads (read through for a pool node); null = not known. */
  modelId: string | null;
  /** The way it loads on; null = it follows whatever this Mac's engine runs. */
  way: SwapWay | null;
}

export interface NodeSwap {
  target: SwapTarget;
  /** The engine's load phase ("makingRoom" | "starting" | "loading" | "warming" | …); null = none yet. */
  phase: string | null;
  /** The target's measured loads (its way's Ready median); null = not measured yet. */
  load: MeasuredLoad | null;
  /**
   * The sessions whose demands the loader loads it for (`loading.demandedBy`, Q-382); empty = no
   * loader demand is behind it (Run it, a restore, a card's Start).
   */
  demandedBy: string[];
}

/**
 * Whether a load is not `sessionId`'s own: the loader names the sessions it loads for, and this one
 * is not among them — a delegate's load, or another chat's (Q-434: the parent's composer said
 * "Loading … for this chat" while its delegate's demand was being loaded). A load no demand is
 * behind names no one, so it is not claimed to be someone else's.
 */
export function loadIsOthers(swap: NodeSwap, sessionId: string | null): boolean {
  return swap.demandedBy.length > 0 && (sessionId == null || !swap.demandedBy.includes(sessionId));
}

/** A node's measured loads: the median of its way's Ready loads and how many it is over. */
export interface MeasuredLoad {
  medianMs: number;
  count: number;
}

/** The Mac key a placement uses for this Mac (components/nodes/model.ts `THIS_MAC`). */
const THIS_MAC = 'local';

export function swapWayOf(node: ResolvedNodeDef): SwapWay | null {
  const placement = node.def.placement;
  if (placement == null || placement.kind === 'follows') return null;
  if (placement.kind === 'single') {
    return placement.macs.length === 1 && placement.macs[0] === THIS_MAC
      ? 'single'
      : 'remoteSingle';
  }
  return 'split';
}

/** A node pinned to a way names it (0) before one that follows this Mac's engine (1). */
function followsRank(node: ResolvedNodeDef): number {
  return swapWayOf(node) != null ? 0 : 1;
}

/**
 * THE ONE NAME of a way goosed reports by the nodes that name it (`wayNodes`, `servingNodes`): the
 * Nodes page's name of a node pinned to it before one that follows this Mac's engine, so the split
 * the person made reads "… · both Macs", never "This Mac's engine" it is also served through
 * (Q-459). Ids the read no longer lists are skipped; none left = goosed's own words.
 */
export function wayNodeName(
  read: NodesReadResponse_unstable,
  ids: readonly string[] | null | undefined,
  words: string
): string {
  const named = (ids ?? [])
    .map((id) => read.nodes.find((n) => n.def.id === id))
    .filter((n): n is ResolvedNodeDef => n != null)
    .map((node, i) => ({ node, i }))
    .sort((a, b) => followsRank(a.node) - followsRank(b.node) || a.i - b.i)[0];
  return named ? named.node.def.name : words;
}

/** goosed's serving-other facts with the serving node named by `wayNodeName` (Q-459). */
export function servingOtherNamed(
  read: NodesReadResponse_unstable,
  other: NodeServingOtherDto
): NodeServingOtherDto {
  return { ...other, serving: wayNodeName(read, other.servingNodes, other.serving) };
}

function targetOf(node: ResolvedNodeDef): SwapTarget {
  return {
    id: node.def.id,
    name: node.def.name,
    modelId: node.model ?? node.def.model ?? null,
    way: swapWayOf(node),
  };
}

/**
 * The node the loader is loading, from one `nodes/read` + `nodes/residency`; null = no swap. A way
 * two nodes name (a pinned node and one that follows this Mac's engine) reads `loading` on both: the
 * one in `prefer` leads (the chat's own node), then a pinned node — it names the way — then one that
 * follows.
 */
export function nodeSwapOf(
  read: NodesReadResponse_unstable,
  residency: NodesResidencyResponse_unstable,
  prefer: readonly string[] = []
): NodeSwap | null {
  const loading = new Map<string, { phase: string | null; demandedBy: string[] }>();
  for (const row of residency.nodes) {
    if (row.residency.kind === 'loading') {
      loading.set(row.node, {
        phase: row.residency.phase ?? null,
        demandedBy: row.residency.demandedBy ?? [],
      });
    }
  }
  if (loading.size === 0) return null;
  const rank = (node: ResolvedNodeDef): number =>
    prefer.includes(node.def.id) ? 0 : 1 + followsRank(node);
  const found = read.nodes
    .filter((n) => n.def.kind === 'mlx' && loading.has(n.def.id))
    .map((node, i) => ({ node, i }))
    .sort((a, b) => rank(a.node) - rank(b.node) || a.i - b.i)[0];
  if (!found) return null;
  const mark = loading.get(found.node.def.id);
  return {
    target: targetOf(found.node),
    phase: mark?.phase ?? null,
    load: measuredLoadOf(residency, found.node.def.id),
    demandedBy: mark?.demandedBy ?? [],
  };
}

/**
 * A switch to `nodeId` that no residency mark shows yet (Q-462): this chat's turn is in flight,
 * its chain loads `nodeId` next, and the engine it faced is down or coming up — the loader is between
 * its marks (a take-over leaves `waiting` before its `loading` is read). No phase is claimed.
 */
export function pendingSwapOf(
  read: NodesReadResponse_unstable,
  residency: NodesResidencyResponse_unstable,
  nodeId: string
): NodeSwap {
  return {
    target: nodeTarget(read, nodeId),
    phase: null,
    load: measuredLoadOf(residency, nodeId),
    demandedBy: [],
  };
}

/** goosed's measured load of a node (`nodes/residency` `load`); null = not measured yet. */
export function measuredLoadOf(
  residency: NodesResidencyResponse_unstable,
  nodeId: string
): MeasuredLoad | null {
  const load = residency.nodes.find((r) => r.node === nodeId)?.load;
  return load ? { medianMs: load.medianMs, count: load.count } : null;
}

function nodeTarget(read: NodesReadResponse_unstable, id: string): SwapTarget {
  const node = read.nodes.find((n) => n.def.id === id);
  // A node the read no longer lists is named by its id — the one name there is.
  return node ? targetOf(node) : { id, name: id, modelId: null, way: null };
}

/**
 * A demand queued in the loader for one of `nodeIds` (design §8.7 `nodes.turnWaiting`): the
 * loader's own words, and — when what it waits for is replies on a way the switch would stop — that
 * way, named as the Nodes page names it (its first node), and how many replies are ahead.
 */
export interface NodeWait {
  target: SwapTarget;
  reason: string;
  /** `chats`: the chats those replies answer, by name (Q-430); empty from an older goosed. */
  replies: { way: string; count: number; chats: string[] } | null;
  /** The role says wait while the Mac serves another node for chats between replies (Q-428). */
  servingOther: NodeServingOtherDto | null;
  /**
   * Q-442: the chat's own node serves, and its reply waits behind a switch to `switchTo` asked for
   * before the reply began (for `chats`; none = a Start on a node's card). `load` is then the
   * switch's measured load, the part of the wait a measurement can say.
   */
  behind: { switchTo: SwapTarget; chats: string[] } | null;
  load: MeasuredLoad | null;
}

/** Q-442: `sessionId`'s reply waits behind a queued switch (goosed's `behindSwitches`). */
export function nodeBehindSwitchOf(
  read: NodesReadResponse_unstable,
  residency: NodesResidencyResponse_unstable,
  sessionId: string | null
): NodeWait | null {
  if (!sessionId) return null;
  const entry = (residency.behindSwitches ?? []).find((b) => b.session === sessionId);
  if (!entry) return null;
  const switchTo = read.nodes.some((n) => n.def.id === entry.switchTo)
    ? nodeTarget(read, entry.switchTo)
    : { id: entry.switchTo, name: entry.switchToName, modelId: null, way: null };
  return {
    target: nodeTarget(read, entry.node),
    reason: '',
    replies: null,
    servingOther: null,
    behind: { switchTo, chats: entry.chats ?? [] },
    load: measuredLoadOf(residency, entry.switchTo),
  };
}

export function nodeWaitOf(
  read: NodesReadResponse_unstable,
  residency: NodesResidencyResponse_unstable,
  nodeIds: readonly string[]
): NodeWait | null {
  const row = residency.nodes.find(
    (r) => r.residency.kind === 'waiting' && nodeIds.includes(r.node)
  );
  if (!row || row.residency.kind !== 'waiting') return null;
  const { reason, replies, servingOther } = row.residency;
  return {
    target: nodeTarget(read, row.node),
    reason,
    replies: replies
      ? {
          way: wayNodeName(read, replies.wayNodes, replies.way),
          count: replies.count,
          chats: replies.chats ?? [],
        }
      : null,
    servingOther: servingOther ? servingOtherNamed(read, servingOther) : null,
    behind: null,
    load: measuredLoadOf(residency, row.node),
  };
}

/** The loader refused its last demand for a node (§8.7 `nodes.refused*`, `nodes.loadFailed`). */
export interface NodeRefusal {
  target: SwapTarget;
  reason: string;
  facts: NodeRefusalFactsDto | null;
}

export function nodeRefusalOf(
  read: NodesReadResponse_unstable,
  residency: NodesResidencyResponse_unstable,
  nodeId: string
): NodeRefusal | null {
  const row = residency.nodes.find((r) => r.node === nodeId);
  if (row?.residency.kind !== 'refusedLastTime') return null;
  const facts = row.residency.facts ?? null;
  return {
    target: nodeTarget(read, nodeId),
    reason: row.residency.reason,
    facts:
      facts?.kind === 'servingOther'
        ? { ...facts, serving: wayNodeName(read, facts.servingNodes, facts.serving) }
        : facts,
  };
}

/**
 * A node the loader stopped to load another, as the chat it was on sees it (§8.7
 * `nodes.displacedNotice` / `nodes.displacedFailed`): the chat that asked for the other node never
 * gets it — it is where the swap went.
 */
export interface NodeDisplaced {
  node: SwapTarget;
  other: SwapTarget;
  /** The chat whose turn asked for `other`, by its name; null = a Start on its card, or unread. */
  chat: string | null;
  /** `other` failed to load: the engine's words. */
  failed: string | null;
  /** `node`'s measured load — how long the next message's load back takes. */
  load: MeasuredLoad | null;
}

/**
 * `servedNode` is the node the router says served this chat's last turn (`nodes/servedLast`):
 * the notice says "was stopped … your next message loads it back", which is true only of a chat
 * that node was serving — never of a chat that never ran on it (Q-435: a brand-new chat set to
 * the stopped node was told it). Unread (undefined) or another node proves nothing: no notice.
 */
export function displacedOf(
  read: NodesReadResponse_unstable,
  residency: NodesResidencyResponse_unstable,
  nodeId: string,
  sessionId: string | null,
  servedNode: string | null | undefined
): NodeDisplaced | null {
  if (servedNode !== nodeId) return null;
  const entry = (residency.displaced ?? []).find((d) => d.node === nodeId);
  if (!entry || (sessionId != null && entry.bySession === sessionId)) return null;
  // goosed drops a notice the moment its node serves again; a read that raced it is not said.
  const row = residency.nodes.find((r) => r.node === nodeId);
  if (row?.residency.kind === 'serving' || row?.residency.kind === 'loading') return null;
  return {
    node: nodeTarget(read, nodeId),
    other: nodeTarget(read, entry.forNode),
    chat: entry.byChat ?? null,
    failed: entry.failed ?? null,
    load: measuredLoadOf(residency, nodeId),
  };
}

/** The same model, however the two reads spell its case. */
function sameModel(a: string, b: string): boolean {
  return a.toLowerCase() === b.toLowerCase();
}

/**
 * Whether the state an engine shows is the swap's doing: while a node loads, an engine that is
 * stopped or stopping is the swap's stop, and one that FAILED is too — unless it is the target's
 * own way and model, which is the load itself failing (a real failure, said as Failed with its
 * words). A model either side does not know counts as the target's own: the failure then stays.
 */
export function swapStopsEngine(
  swap: NodeSwap | null | undefined,
  engine: { way: SwapWay; modelId: string | null; failed: boolean }
): swap is NodeSwap {
  if (!swap) return false;
  if (!engine.failed) return true;
  const { target } = swap;
  const ownWay = target.way == null ? engine.way !== 'remoteSingle' : target.way === engine.way;
  const ownModel =
    target.modelId == null || engine.modelId == null || sameModel(target.modelId, engine.modelId);
  return !(ownWay && ownModel);
}

/** The node ids a chat's model can run on: `node:<id>` one, `strategy:<id>` every chain entry. */
export function routeNodeIds(
  read: NodesReadResponse_unstable,
  model: string | null | undefined
): string[] | null {
  if (!model) return null;
  if (model.startsWith('node:')) return [model.slice('node:'.length)];
  if (!model.startsWith('strategy:')) return null;
  const id = model.slice('strategy:'.length).split('@')[0];
  const strategy = (read.config.strategies ?? []).find((s) => s.id === id);
  if (!strategy) return [];
  const ids = new Set<string>();
  for (const entry of Object.values(strategy.roles ?? {})) {
    for (const link of entry?.chain ?? []) ids.add(link.node);
  }
  return [...ids];
}

/**
 * A chat's turn on a `node:` / `strategy:` route with an MLX node may wait in the loader at any
 * point of the turn, and nothing the glance keys on announces it: behind another chat's reply
 * while none of its nodes serves (Q-430 — the way serving the other chat kept its model and
 * stage), and behind a switch queued before its reply began while its OWN node serves (Q-442 —
 * the other chat's switch waits on a third chat's reply, so no engine moves either). While such a
 * turn is in flight the composer keeps reading the loader's marks (glanceStore
 * `watchGlanceNodes`). A route of cloud nodes only, or Auto, never waits on the loader.
 */
export function turnCanWaitInLoader(
  read: NodesReadResponse_unstable,
  model: string | null | undefined
): boolean {
  const ids = routeNodeIds(read, model);
  if (!ids || ids.length === 0) return false;
  return ids.some((id) => read.nodes.some((n) => n.def.id === id && n.def.kind === 'mlx'));
}

const WAYS: ReadonlySet<string> = new Set<SwapWay>(['single', 'remoteSingle', 'split']);

function isMeasuredLoad(value: unknown): value is MeasuredLoad {
  if (value == null || typeof value !== 'object') return false;
  const v = value as Record<string, unknown>;
  return typeof v.medianMs === 'number' && typeof v.count === 'number';
}

/** IPC is a trust boundary: a window's swap report is checked field by field. */
export function isNodeSwap(value: unknown): value is NodeSwap {
  if (value == null || typeof value !== 'object') return false;
  const v = value as Record<string, unknown>;
  const t = v.target as Record<string, unknown> | null | undefined;
  return (
    (v.phase === null || typeof v.phase === 'string') &&
    (v.load == null || isMeasuredLoad(v.load)) &&
    t != null &&
    typeof t === 'object' &&
    typeof t.id === 'string' &&
    typeof t.name === 'string' &&
    (t.modelId === null || typeof t.modelId === 'string') &&
    (t.way === null || (typeof t.way === 'string' && WAYS.has(t.way)))
  );
}

/**
 * The swap any window reports. The loader lives in each window's own goosed, so only the window
 * whose goosed is swapping reads a node `loading` from its loader: the first report that has one.
 */
export function swapOfReports(reports: Iterable<{ swap?: NodeSwap | null }>): NodeSwap | null {
  for (const r of reports) if (r.swap) return r.swap;
  return null;
}
