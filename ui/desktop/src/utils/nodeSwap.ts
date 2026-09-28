import type {
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
  const loading = new Map<string, string | null>();
  for (const row of residency.nodes) {
    if (row.residency.kind === 'loading') loading.set(row.node, row.residency.phase ?? null);
  }
  if (loading.size === 0) return null;
  const rank = (node: ResolvedNodeDef): number =>
    prefer.includes(node.def.id) ? 0 : swapWayOf(node) != null ? 1 : 2;
  const found = read.nodes
    .filter((n) => n.def.kind === 'mlx' && loading.has(n.def.id))
    .map((node, i) => ({ node, i }))
    .sort((a, b) => rank(a.node) - rank(b.node) || a.i - b.i)[0];
  if (!found) return null;
  return { target: targetOf(found.node), phase: loading.get(found.node.def.id) ?? null };
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

const WAYS: ReadonlySet<string> = new Set<SwapWay>(['single', 'remoteSingle', 'split']);

/** IPC is a trust boundary: a window's swap report is checked field by field. */
export function isNodeSwap(value: unknown): value is NodeSwap {
  if (value == null || typeof value !== 'object') return false;
  const v = value as Record<string, unknown>;
  const t = v.target as Record<string, unknown> | null | undefined;
  return (
    (v.phase === null || typeof v.phase === 'string') &&
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
