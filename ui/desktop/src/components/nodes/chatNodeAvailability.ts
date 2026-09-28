import type { GlanceLine, GlanceWhere, NodeGlance, NodeState } from './nodeGlance';
import {
  chatNodeSetOf,
  effectiveEntry,
  parseRouteModel,
  type NodeStrategy,
  type NodesConfig,
  type ResolvedNodeDef,
} from './model';
import { samePlacement } from './nodeDraft';

/**
 * ONE CHAT ON ONE NODE OR SEVERAL (Q-359, DESIGN-Q359-CHAT-NODES.md): what the chat runs on now, and
 * whether a node can join it. Pure and intl-free, like `nodeGlance`, whose state and line it reads
 * and never re-derives: availability detection stays the Nodes page's one derivation, and this only
 * adds the one rule a chat's SET adds — before C3 your Macs serve goose one MLX way at a time, so a
 * second Mac model cannot run beside the one the set already has.
 */

/** What a chat runs on, as its chip's top section shows it. */
export type ChatNodesNow =
  /** Its own node set (goosed's chat strategy). */
  | { kind: 'set'; strategyId: string; nodes: string[]; answerOnNext: boolean }
  /** One node (`node:<id>`). */
  | { kind: 'node'; node: string }
  /** A named strategy: its Chat chain answers. Adding a node makes this chat its own copy. */
  | { kind: 'strategy'; strategy: NodeStrategy; nodes: string[]; answerOnNext: boolean }
  /** Any node (Auto), another provider, or a route whose target is gone: no node answers by name. */
  | { kind: 'none' };

export function chatNodesNow(
  config: NodesConfig | null | undefined,
  session: string | null | undefined,
  model: string | null | undefined
): ChatNodesNow {
  const route = model ? parseRouteModel(model) : null;
  if (!route) return { kind: 'none' };
  if (route.kind === 'node') return { kind: 'node', node: route.id };
  if (route.kind !== 'strategy') return { kind: 'none' };
  const own = chatNodeSetOf(config, session);
  if (own && own.strategyId === route.id) {
    return {
      kind: 'set',
      strategyId: own.strategyId,
      nodes: own.nodes,
      answerOnNext: own.answerOnNext,
    };
  }
  const strategy = (config?.strategies ?? []).find((s) => s.id === route.id);
  if (!strategy || strategy.chat != null) return { kind: 'none' };
  const chat = effectiveEntry(strategy, 'chat');
  if (!chat || chat.chain.length === 0) return { kind: 'none' };
  return {
    kind: 'strategy',
    strategy,
    nodes: chat.chain.map((link) => link.node),
    answerOnNext: chat.when === 'failover' && chat.chain.length > 1,
  };
}

/** The chat's nodes in order (lead first), whatever it runs on; empty for `none`. */
export function chatNodeIds(now: ChatNodesNow): string[] {
  switch (now.kind) {
    case 'set':
    case 'strategy':
      return now.nodes;
    case 'node':
      return [now.node];
    case 'none':
      return [];
  }
}

/** A node with its glance, as the Nodes page derives it (`useNodeFacts().glanceOf`). */
export interface GlancedNode {
  node: ResolvedNodeDef;
  glance: NodeGlance;
}

/** A Mac as the words name it: this Mac, or another by its name. */
export type MacRef = { kind: 'thisMac' } | { kind: 'mac'; name: string };

/** What serves this Mac's goose now, for the one line that says whose answer an add would stop. */
export interface ServingNow {
  /** The node that serves, by name (null = nothing serves, or it is no node). */
  node: string | null;
  /** Where it runs (null = not known). */
  mac: MacRef | null;
  /** The chat the engine answers now, by name — only when it is ANOTHER chat than this one. */
  otherChat: string | null;
}

export type AddLine =
  | { kind: 'serving' }
  | { kind: 'notLoaded'; start: { medianMs: number; count: number } | null }
  | { kind: 'loading'; phase: string | null }
  | { kind: 'waiting'; words: string }
  /** The engine answers another chat on another way: adding this stops it after its answer. */
  | { kind: 'stopsOtherChat'; mac: MacRef; chat: string; node: string }
  /** Every other line the glance says (a cloud node's "Always available", "Measuring…"), verbatim. */
  | { kind: 'glance'; line: GlanceLine };

export type NotAddable =
  /** It runs on the Mac the set's Mac model runs on. */
  | { kind: 'sameMac'; mac: MacRef; lead: string }
  /** The set's Mac model is a split: it holds every Mac of the split. */
  | { kind: 'leadSplit'; lead: string }
  /** Before C3: goose serves one Mac model at a time across your Macs. */
  | { kind: 'beforeC3'; lead: string }
  /** The node cannot run now; the glance's own state and line say why. */
  | { kind: 'glance'; state: NodeState; line: GlanceLine };

export type ChatNodeAvailability =
  | { addable: true; line: AddLine }
  | { addable: false; reason: NotAddable };

/** The states the glance calls a blocker: the node cannot take work until someone acts. */
const BLOCKED: ReadonlySet<NodeState> = new Set<NodeState>([
  'cantRun',
  'needsStep',
  'heldByBuild',
  'keyMissing',
  'failing',
  'unknown',
]);

function isMlx(node: ResolvedNodeDef): boolean {
  return node.def.kind === 'mlx';
}

/**
 * Two MLX nodes on the same way AND model are one engine (a second name for the same thing); every
 * other pair of MLX nodes is two things the engine would swap between. A node that follows this
 * Mac's engine runs whatever it serves, so it is the same way only as another follower of its model.
 */
function sameWay(a: ResolvedNodeDef, b: ResolvedNodeDef): boolean {
  const pa = a.def.placement;
  const pb = b.def.placement;
  if (!pa || !pb || a.model == null || a.model !== b.model) return false;
  if (pa.kind === 'follows' || pb.kind === 'follows') return pa.kind === pb.kind;
  return samePlacement(pa, pb);
}

/** The placement Macs a node runs on (`local` for a follower: it runs where this Mac's engine does). */
function macsOf(node: ResolvedNodeDef): string[] {
  const placement = node.def.placement;
  if (!placement || placement.kind === 'follows') return ['local'];
  return placement.macs;
}

function isSplit(node: ResolvedNodeDef): boolean {
  const kind = node.def.placement?.kind;
  return kind === 'tensor' || kind === 'pipeline';
}

function macOf(where: GlanceWhere | null): MacRef | null {
  if (!where) return null;
  switch (where.kind) {
    case 'mac':
      return { kind: 'mac', name: where.name };
    case 'thisMac':
      return { kind: 'thisMac' };
    case 'split':
    case 'provider':
      return null;
  }
}

/**
 * Whether `candidate` can join the chat whose nodes are `set` (lead first). A node that can't is
 * named with its reason and shown at full strength — never greyed, never hidden.
 */
export function chatNodeAvailability(
  candidate: GlancedNode,
  set: readonly GlancedNode[],
  serving: ServingNow
): ChatNodeAvailability {
  const { node, glance } = candidate;
  if (isMlx(node)) {
    const beside = set.find((m) => isMlx(m.node) && !sameWay(m.node, node));
    if (beside) {
      const lead = beside.node.def.name;
      if (isSplit(beside.node)) return { addable: false, reason: { kind: 'leadSplit', lead } };
      const shared = macsOf(node).some((mac) => macsOf(beside.node).includes(mac));
      const mac = macOf(beside.glance.where) ?? macOf(glance.where);
      if (shared && mac) return { addable: false, reason: { kind: 'sameMac', mac, lead } };
      return { addable: false, reason: { kind: 'beforeC3', lead } };
    }
  }
  if (BLOCKED.has(glance.state)) {
    return { addable: false, reason: { kind: 'glance', state: glance.state, line: glance.line } };
  }
  switch (glance.state) {
    case 'serving':
      return { addable: true, line: { kind: 'serving' } };
    case 'loading':
      return {
        addable: true,
        line: { kind: 'loading', phase: glance.line.kind === 'loadPhase' ? glance.line.phase : null },
      };
    case 'waiting':
      return {
        addable: true,
        line: {
          kind: 'waiting',
          words: glance.line.kind === 'words' ? glance.line.text : '',
        },
      };
    case 'displaced':
    case 'ready': {
      if (glance.state === 'displaced' && serving.otherChat && serving.node) {
        return {
          addable: true,
          line: {
            kind: 'stopsOtherChat',
            mac: serving.mac ?? macOf(glance.where) ?? { kind: 'thisMac' },
            chat: serving.otherChat,
            node: serving.node,
          },
        };
      }
      if (glance.line.kind === 'startsIn') {
        return {
          addable: true,
          line: {
            kind: 'notLoaded',
            start: { medianMs: glance.line.medianMs, count: glance.line.count },
          },
        };
      }
      if (glance.line.kind === 'firstStart') {
        return { addable: true, line: { kind: 'notLoaded', start: null } };
      }
      return { addable: true, line: { kind: 'glance', line: glance.line } };
    }
    default:
      return { addable: true, line: { kind: 'glance', line: glance.line } };
  }
}
