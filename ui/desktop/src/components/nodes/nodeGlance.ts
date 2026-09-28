import type {
  NodeLoadGroupDto,
  NodeResidency,
  NodeRole,
  NodesServingWayDto,
} from '@aaif/goose-sdk';
import type { PlacementCandidate, PlacementGoal } from '../../acp/mlx-placement';
import type { MlxLocalModel } from '../../acp/mlx-engine';
import type { ProviderDetails } from '../../types/providers';
import type { EngineGlance } from '../../utils/engineGlance';
import type { EngineFigure } from '../leanzero-swarm/engineFigures';
import {
  macForPlacementNode,
  peerRefuses,
  SELF_KEY,
  type Mac,
  type Permission,
} from '../leanzero-swarm/macs';
import { providerRowState } from '../leanzero-swarm/cloudProviderState';
import type { PlacementPlansRead } from '../leanzero-swarm/usePlacementPlans';
import { ROLES, THIS_MAC, type NodesConfig, type ResolvedNodeDef } from './model';
import type { StateHue } from './hues';
import { placementOfKey, samePlacement } from './nodeDraft';

/**
 * ONE DERIVATION of what a node is doing now, from the stores that already exist
 * (DESIGN-NODES-AND-STRATEGIES.md §4.3). Pure and intl-free: it returns a state and the FACTS each
 * line is said from; NodeCard says them in the person's locale. It never guesses:
 *
 * - Which way serves this Mac's goose, and whether it is THIS node's way, is goosed's
 *   `nodes/residency` — the Rust side matches model identity through `node_names_model` (Q-128),
 *   so a split served under its HF id against an alias still reads Serving, and a remote single
 *   serving while this Mac's engine is idle is never this Mac's single.
 * - Fit, speed and what a way still needs are the placement planner's candidate for the node's
 *   model, goal and way (the one fit rule, fit.rs) — never recomputed here.
 * - The live line and figures are the main-pushed engine glance, which speaks for the one way that
 *   serves (one MLX way at a time).
 * - A load time is the median of MEASURED loads (`nodes/loadHistory`), or "not measured yet".
 * - A fact nobody could read is a named state with its words (`unknown`), never a default.
 */

export type NodeState =
  | 'serving'
  | 'loading'
  | 'waiting'
  | 'ready'
  | 'displaced'
  | 'needsStep'
  | 'cantRun'
  | 'heldByBuild'
  | 'follows'
  | 'unknown'
  | 'cloudReady'
  | 'keyMissing'
  | 'failing';

/** The state chip: a solid state hue, or one of the two outline registers. */
export type StateChipLook = { hue: StateHue } | { outline: 'ink' | 'slate' };

export const STATE_CHIP: Record<NodeState, StateChipLook> = {
  serving: { hue: 'green' },
  loading: { hue: 'amber' },
  waiting: { hue: 'amber' },
  ready: { hue: 'slate' },
  displaced: { hue: 'slate' },
  needsStep: { hue: 'orange' },
  cantRun: { hue: 'red' },
  heldByBuild: { hue: 'red' },
  follows: { outline: 'ink' },
  unknown: { outline: 'slate' },
  cloudReady: { hue: 'green' },
  keyMissing: { hue: 'red' },
  failing: { hue: 'red' },
};

/** The line under a node's name: the facts, each said by NodeCard in the person's words. */
export type GlanceLine =
  | { kind: 'live'; stage: EngineGlance['stage']; hero: EngineFigure | null; chat: string | null }
  | { kind: 'loadPhase'; phase: string | null; progress: { done: number; total: number } | null }
  | { kind: 'words'; text: string }
  | { kind: 'startsIn'; medianMs: number; count: number }
  | { kind: 'firstStart' }
  | { kind: 'planning' }
  | { kind: 'readingState' }
  | { kind: 'planFailed'; error: string }
  | { kind: 'noPlan'; error: string | null }
  | { kind: 'noSuchWay' }
  | { kind: 'loadsUnread'; error: string }
  | { kind: 'notConnected'; mac: string }
  /** `mac` null = this Mac. */
  | { kind: 'copyFirst'; mac: string | null }
  | { kind: 'permissionOff'; mac: string; permission: Permission }
  | { kind: 'outcome'; candidate: PlacementCandidate }
  | { kind: 'heldByBuild'; way: string }
  | { kind: 'follows'; serving: NodesServingWayDto | null }
  | { kind: 'leftPool' }
  | { kind: 'cloudAlways'; provider: string; endpoint: boolean }
  | { kind: 'keyMissing'; provider: string }
  | { kind: 'checking'; provider: string };

export type GlanceAction = 'stop' | 'start' | 'openRunIt' | 'details' | 'pinWay' | 'setUp' | null;

/** Where it runs, as the where chip says it. */
export type GlanceWhere =
  | { kind: 'thisMac' }
  | { kind: 'mac'; name: string }
  | { kind: 'split'; count: number }
  | { kind: 'provider'; name: string };

export interface MemoryRow {
  mac: string;
  /** GB the way needs there (planned), or its live peak (serving). */
  usedGb: number | null;
  budgetGb: number | null;
  kind: 'need' | 'peak';
}

export type GlanceFigures =
  | { kind: 'live'; hero: EngineFigure | null; second: EngineFigure | null }
  | { kind: 'plan'; candidate: PlacementCandidate; goal: PlacementGoal };

export interface NodeGlance {
  state: NodeState;
  line: GlanceLine;
  action: GlanceAction;
  where: GlanceWhere | null;
  figures: GlanceFigures | null;
  memory: MemoryRow[];
  /** The planner's arithmetic in words, for Details. */
  detail: string | null;
  /**
   * What starting this node stops (one MLX way serves this Mac's goose at a time): the node that
   * serves now by name, else the way in goosed's words; null when nothing else serves.
   */
  displaces: string | null;
}

/** A read the page performs: in flight, done, or failed with its words. */
export type Read<T> =
  | { kind: 'reading' }
  | { kind: 'read'; value: T }
  | { kind: 'failed'; error: string };

export interface NodeFacts {
  /** goosed's residency for this node; a failed read is its words. */
  residency: Read<NodeResidency>;
  /** The way serving this Mac's goose now (null = nothing), and the node it belongs to. */
  serving: NodesServingWayDto | null;
  servingNodeName: string | null;
  /** The main-pushed engine glance (null until main built one). */
  glance: EngineGlance | null;
  /** The planner's plans for this node's goal. */
  plans: PlacementPlansRead;
  macs: readonly Mac[];
  /** A Mac's models folder as the page read it (null = not read). */
  modelsOn: (macKey: string) => MlxLocalModel[] | null;
  /** The measured loads of this node's model. */
  loads: Read<NodeLoadGroupDto[]>;
  /** A swarm build holding this Mac's engine, when a holder record says so (S5/S8's holders). */
  buildHolder: { node: string | null; way: string } | null;
  /** Cloud/endpoint: the provider as Cloud Providers reads it (null = no such provider). */
  provider: Read<ProviderDetails | null>;
}

const GIB = 1024 * 1024 * 1024;

export function candidateFor(
  node: ResolvedNodeDef,
  plans: PlacementPlansRead
): PlacementCandidate | null {
  const placement = node.def.placement;
  if (plans.kind !== 'read' || !node.model || !placement || placement.kind === 'follows') {
    return null;
  }
  const plan = plans.plans.get(node.model);
  return plan?.candidates?.find((c) => samePlacement(placement, placementOfKey(c.key))) ?? null;
}

/**
 * The name of a placement Mac: the Link roster's name for a `link:<id>` peer, else the planner's own
 * name for it, else the key itself (a peer the roster does not know is shown by its key, never
 * dropped).
 */
function macNameOf(
  macs: readonly Mac[],
  mac: string,
  candidate: PlacementCandidate | null,
  index: number
): string {
  return macForPlacementNode(macs, mac)?.name ?? candidate?.nodeNames[index] ?? mac;
}

/**
 * Where a node follows to now: a follows node runs wherever this Mac's engine serves — split across
 * the split's Macs, on the peer of a remote single, else this Mac (Q-303: "This Mac" sat above
 * "Split across 2 Macs").
 */
function followedWhere(serving: NodesServingWayDto | null): GlanceWhere {
  if (serving?.kind === 'split') {
    return { kind: 'split', count: serving.macNames.length || (serving.macs?.length ?? 0) };
  }
  if (serving?.kind === 'remoteSingle' && serving.macNames[0]) {
    return { kind: 'mac', name: serving.macNames[0] };
  }
  return { kind: 'thisMac' };
}

function whereOf(
  node: ResolvedNodeDef,
  macs: readonly Mac[],
  providerName: string | null,
  candidate: PlacementCandidate | null,
  serving: NodesServingWayDto | null = null
): GlanceWhere | null {
  const def = node.def;
  if (def.kind !== 'mlx') return providerName ? { kind: 'provider', name: providerName } : null;
  const placement = def.placement;
  if (!placement || placement.kind === 'follows') return followedWhere(serving);
  if (placement.kind !== 'single') return { kind: 'split', count: placement.macs.length };
  const mac = placement.macs[0] ?? THIS_MAC;
  if (mac === THIS_MAC) return { kind: 'thisMac' };
  return { kind: 'mac', name: macNameOf(macs, mac, candidate, 0) };
}

function memoryOf(candidate: PlacementCandidate | null): MemoryRow[] {
  return (candidate?.fit.nodes ?? []).map((n) => ({
    mac: n.name,
    usedGb: n.needBytes / GIB,
    budgetGb: n.budgetBytes / GIB,
    kind: 'need',
  }));
}

function liveMemory(glance: EngineGlance | null): MemoryRow[] {
  return (glance?.nodes ?? []).map((n) => ({
    mac: n.name,
    usedGb: n.peakGb,
    budgetGb: n.budgetGb,
    kind: 'peak',
  }));
}

function liveLine(glance: EngineGlance | null): GlanceLine {
  return {
    kind: 'live',
    stage: glance?.stage ?? 'running',
    hero: glance?.busy ? (glance.hero ?? null) : null,
    chat: glance?.chat?.name ?? null,
  };
}

function loadLine(phase: string | null, glance: EngineGlance | null): GlanceLine {
  const progress =
    glance &&
    glance.progress &&
    glance.progress !== 'indeterminate' &&
    glance.progress.unit === 'bytes'
      ? { done: glance.progress.done, total: glance.progress.total }
      : null;
  return { kind: 'loadPhase', phase, progress };
}

/**
 * The median of this node's MEASURED loads on its own way, or null when none is measured (or the
 * loads are not read) — never an estimate. The card's start line and strategyFit's swap rows both
 * say it from here.
 */
export function measuredStart(
  node: ResolvedNodeDef,
  loads: Read<NodeLoadGroupDto[]>
): { medianMs: number; count: number } | null {
  const placement = node.def.placement;
  if (loads.kind !== 'read' || !placement || placement.kind === 'follows') return null;
  const group = loads.value.find((g) => samePlacement(placement, placementOfKey(g.placement)));
  return group?.medianTotalMs != null && group.count > 0
    ? { medianMs: group.medianTotalMs, count: group.count }
    : null;
}

/** The measured median start of this node's way, or "not measured yet" — never an estimate. */
function startLine(node: ResolvedNodeDef, loads: Read<NodeLoadGroupDto[]>): GlanceLine {
  if (loads.kind === 'failed') return { kind: 'loadsUnread', error: loads.error };
  const measured = measuredStart(node, loads);
  return measured ? { kind: 'startsIn', ...measured } : { kind: 'firstStart' };
}

/** The Mac key useMacs keys this placement Mac by (`self` for this Mac). */
function macKeyOf(macs: readonly Mac[], placementMac: string): string | null {
  if (placementMac === THIS_MAC) return SELF_KEY;
  return macForPlacementNode(macs, placementMac)?.key ?? null;
}

/**
 * What stands between a pinned way and a start, in precedence: a Mac not connected, then the
 * planner's refusals (unsupported, too big), then a step (a model to copy, a permission off, the
 * planner's unavailable action). null = nothing the facts show.
 */
function blockerOf(
  node: ResolvedNodeDef,
  facts: NodeFacts,
  candidate: PlacementCandidate | null
): { state: 'cantRun' | 'needsStep'; line: GlanceLine; action: GlanceAction } | null {
  const placement = node.def.placement;
  if (!placement || placement.kind === 'follows') return null;
  // Only a Link peer (`link:<id>`) is judged against the roster; a peer the planner reaches by
  // its ssh host is judged by the plan itself.
  for (const [index, mac] of placement.macs.entries()) {
    if (!mac.startsWith('link:')) continue;
    const known = macForPlacementNode(facts.macs, mac);
    if (!known || !known.online) {
      return {
        state: 'cantRun',
        line: { kind: 'notConnected', mac: macNameOf(facts.macs, mac, candidate, index) },
        action: 'details',
      };
    }
  }
  if (candidate && (!candidate.supported || candidate.fit.status === 'short')) {
    return { state: 'cantRun', line: { kind: 'outcome', candidate }, action: 'details' };
  }
  for (const mac of placement.macs) {
    const key = macKeyOf(facts.macs, mac);
    const models = key ? facts.modelsOn(key) : null;
    if (models != null && node.model && !models.some((m) => m.id === node.model && m.complete)) {
      const name =
        key === SELF_KEY
          ? null
          : macNameOf(facts.macs, mac, candidate, placement.macs.indexOf(mac));
      return { state: 'needsStep', line: { kind: 'copyFirst', mac: name }, action: 'openRunIt' };
    }
  }
  const permission: Permission = placement.kind === 'single' ? 'chat' : 'split';
  for (const mac of placement.macs) {
    const known = macForPlacementNode(facts.macs, mac);
    if (known && peerRefuses(known, permission)) {
      return {
        state: 'needsStep',
        line: { kind: 'permissionOff', mac: known.name, permission },
        action: 'openRunIt',
      };
    }
  }
  if (candidate?.action.kind === 'unavailable') {
    return {
      state: 'needsStep',
      line: { kind: 'words', text: candidate.action.reason },
      action: 'openRunIt',
    };
  }
  return null;
}

function providerGlance(node: ResolvedNodeDef, facts: NodeFacts): NodeGlance {
  const providerId = node.provider ?? null;
  const details = facts.provider.kind === 'read' ? facts.provider.value : null;
  const name = details?.metadata.display_name ?? providerId ?? '—';
  const base = {
    where: whereOf(node, facts.macs, name, null),
    figures: null,
    memory: [],
    detail: null,
    displaces: null,
  };
  if (facts.provider.kind === 'reading') {
    return { ...base, state: 'unknown', line: { kind: 'checking', provider: name }, action: null };
  }
  if (facts.provider.kind === 'failed') {
    return {
      ...base,
      state: 'unknown',
      line: { kind: 'words', text: facts.provider.error },
      action: null,
    };
  }
  const row = details ? providerRowState(details) : 'not-set-up';
  if (row === 'not-set-up') {
    return {
      ...base,
      state: 'keyMissing',
      line: { kind: 'keyMissing', provider: name },
      action: 'setUp',
    };
  }
  if (row === 'failed') {
    return {
      ...base,
      state: 'failing',
      line: { kind: 'words', text: details?.connection_error ?? name },
      action: 'details',
    };
  }
  return {
    ...base,
    state: 'cloudReady',
    line: { kind: 'cloudAlways', provider: name, endpoint: node.def.kind === 'endpoint' },
    action: null,
  };
}

export function nodeGlance(node: ResolvedNodeDef, facts: NodeFacts): NodeGlance {
  if (node.def.kind !== 'mlx') return providerGlance(node, facts);

  const candidate = candidateFor(node, facts.plans);
  const where = whereOf(node, facts.macs, null, candidate, facts.serving);
  const glance = (state: NodeState, line: GlanceLine, action: GlanceAction): NodeGlance => ({
    state,
    line,
    action,
    where,
    figures: candidate && node.def.goal ? { kind: 'plan', candidate, goal: node.def.goal } : null,
    memory: memoryOf(candidate),
    detail: candidate?.fit.detail ?? null,
    displaces: null,
  });

  if (node.modelFrom.kind === 'poolUnreadable') {
    return glance('unknown', { kind: 'words', text: node.modelFrom.error }, null);
  }
  if (node.modelFrom.kind === 'leftPool') return glance('cantRun', { kind: 'leftPool' }, null);

  const residency = facts.residency;
  if (residency.kind === 'reading') return glance('unknown', { kind: 'readingState' }, null);
  if (residency.kind === 'failed')
    return glance('unknown', { kind: 'words', text: residency.error }, null);
  const r = residency.value;

  const servingHere: NodeGlance = {
    ...glance('serving', liveLine(facts.glance), 'stop'),
    figures: facts.glance
      ? { kind: 'live', hero: facts.glance.hero, second: facts.glance.second }
      : null,
    memory: liveMemory(facts.glance),
  };

  if (node.def.placement?.kind === 'follows' || !node.def.placement) {
    // A pool node follows whatever this Mac's engine serves: its chip says so in every state; its
    // figures are the engine's while that serves.
    const follows = glance('follows', { kind: 'follows', serving: facts.serving }, 'pinWay');
    return r.kind === 'serving'
      ? { ...follows, figures: servingHere.figures, memory: servingHere.memory }
      : follows;
  }

  switch (r.kind) {
    case 'unknown':
      return glance('unknown', { kind: 'words', text: r.reason }, null);
    case 'serving':
      return servingHere;
    case 'loading':
      return glance(
        'loading',
        loadLine(r.phase ?? facts.serving?.loadPhase ?? null, facts.glance),
        null
      );
    case 'waiting':
      return glance('waiting', { kind: 'words', text: r.reason }, null);
    case 'refusedLastTime':
      return glance('cantRun', { kind: 'words', text: r.reason }, 'details');
    case 'alwaysReady':
    case 'notRunning':
      break;
  }

  if (facts.buildHolder && facts.buildHolder.node !== node.def.id) {
    return glance('heldByBuild', { kind: 'heldByBuild', way: facts.buildHolder.way }, null);
  }
  const blocker = blockerOf(node, facts, candidate);
  if (blocker) return glance(blocker.state, blocker.line, blocker.action);

  const displaced = r.kind === 'notRunning' && r.otherWay != null;
  const state: NodeState = displaced ? 'displaced' : 'ready';
  const startable = (line: GlanceLine): NodeGlance => ({
    ...glance(state, line, 'start'),
    displaces: displaced ? (facts.servingNodeName ?? r.otherWay ?? null) : null,
  });
  switch (facts.plans.kind) {
    case 'reading':
    case 'idle':
      return startable({ kind: 'planning' });
    case 'failed':
      return startable({ kind: 'planFailed', error: facts.plans.error });
    case 'read': {
      const plan = node.model ? facts.plans.plans.get(node.model) : undefined;
      if (!plan || plan.error) return startable({ kind: 'noPlan', error: plan?.error ?? null });
      if (!candidate) return glance('cantRun', { kind: 'noSuchWay' }, 'details');
      if (candidate.fit.status === 'unknown') return startable({ kind: 'outcome', candidate });
      return startable(startLine(node, facts.loads));
    }
  }
}

export interface UsedBy {
  role: NodeRole;
  /** 1 = the chain's 1st. */
  rank: number;
  strategyId: string;
  strategyName: string;
}

/** Every strategy role whose chain names this node, in the strategy's and the roles' order. */
export function usedByOf(config: NodesConfig, nodeId: string): UsedBy[] {
  const out: UsedBy[] = [];
  for (const strategy of config.strategies ?? []) {
    for (const role of ROLES) {
      const entry = strategy.roles?.[role];
      const at = entry?.chain.findIndex((link) => link.node === nodeId) ?? -1;
      if (at >= 0) {
        out.push({ role, rank: at + 1, strategyId: strategy.id, strategyName: strategy.name });
      }
    }
  }
  return out;
}
