import type { BackgroundWorkKind } from '@aaif/goose-sdk';
import {
  engineHeadline,
  mlxActivity,
  requestActivity,
  type MlxActivity,
  type MlxLiveRequest,
} from '../components/leanzero-swarm/mlxLiveStats';
import {
  engineFigures,
  largestPrompt,
  promptProgress,
  type EngineFigure,
  type MeasuredPair,
} from '../components/leanzero-swarm/engineFigures';
import type { LoadProgress, MlxModeSummary } from '../components/leanzero-swarm/mlxDistributed';
import {
  activityPhase,
  hostingPhase,
  nodePhase,
  remotePhase,
  runPhase,
} from '../components/leanzero-swarm/mlxPhase';
import type { EnginePhase } from '../components/lz/tokens';
import { INITIAL_SNAPSHOT, type MlxEngineSnapshot } from './mlxEngineMonitor';
import type { MlxDistributedReport } from './mlxDistributedReport';
import type { MlxRemoteReport } from './mlxRemoteReport';
import { measuredFigure, type MlxMeasuredRead } from './mlxMeasuredRuns';
import { leaveCause } from './leaveCause';
import { routeContactLost, routePeerGone } from './routeContact';
import { MLX_DISTRIBUTED_STALE_MS, snapshotPhase } from './mlxTray';
import { isNodeSwap, swapStopsEngine, type NodeSwap } from './nodeSwap';

/**
 * THE ENGINE GLANCE — the Engine tab's live state tile made small enough to carry everywhere: the
 * card at the foot of the sidebar, and a floating mini window on the desktop while goose is in the
 * background. (The in-app card that floated over the content is deleted, Q-217.)
 *
 * ONE source: main's engine read (utils/mlxEngineMonitor.ts — the loop the menu-bar tray and the
 * composer's "served by" already read, which keeps reading while goose is hidden or minimized) and the
 * split/route reports main already holds for the tray. Built ONCE, in main, and pushed to every
 * window on `ENGINE_GLANCE_CHANNEL`, so the sidebar card and the desktop window can never disagree.
 * Every figure is the tile's own fact (engineFigures.ts); every colour is the tile's engine phase
 * (mlxPhase.ts). Pure and React-free: it carries facts, and each surface says them in its locale.
 */

export const ENGINE_GLANCE_CHANNEL = 'engine-glance';
/**
 * main → the focused goose window: the person turned the desktop window off from the window itself
 * (Q-224), so the app says it happened and where it comes back from.
 */
export const ENGINE_GLANCE_TURNED_OFF_CHANNEL = 'engine-glance-turned-off';

/**
 * What the engine is doing, as the glance headlines it. The tile's activity words while it is up
 * and read; otherwise the state that explains why there is no activity to show.
 */
export type GlanceStage =
  | MlxActivity
  /**
   * The engine holds only rows whose answers already ended (Q-231 `leaving`): nobody's work, so not
   * busy — said as that, never "Reading" (Q-246).
   */
  | 'leaving'
  /** Up, not read yet (or the read failed — `detail` says why). */
  | 'running'
  /** Weights going in, a split starting, a route mounting on its Mac. */
  | 'loading'
  /**
   * The node loader stopped this way to load another node (utils/nodeSwap.ts): the stopped way's
   * "failed · exit 143" or "not mounted" is the swap's doing, never said as either (Q-254).
   */
  | 'swapping'
  | 'failed'
  | 'reconnecting'
  /** The route's Mac is away, not a blip (routeContact.ts `routePeerGone`). */
  | 'away'
  /** The split serves, and no live read of its rank 0 says what it does. */
  | 'serving'
  /** This Mac serves a rank of ANOTHER Mac's split. */
  | 'hosting'
  /** The split's admission is held by the memory watchdog. */
  | 'held'
  /** The split's report is older than three polls: nothing live is claimed. */
  | 'stale'
  | 'off';

/** Which engine serves chat, in the tile's mode vocabulary (mlxModeLabel.ts formats it). */
export type GlanceEngine = MlxModeSummary | { mode: 'remote'; peerName: string };

export interface GlanceNode {
  name: string;
  phase: EnginePhase;
  peakGb: number | null;
  budgetGb: number | null;
  load: LoadProgress | null;
}

/** A node definition as the glance names it (the Nodes page opens it by `id`). */
export interface GlanceNodeRef {
  id: string;
  name: string;
}

/**
 * The way that serves this Mac's goose as goosed's `nodes/residency` read it, and the nodes that
 * name it — the chat's own served node first, then pinned nodes, then nodes that follow this Mac's
 * engine. A window reads it when its glance changes and reports it with its sessions; main applies
 * it only to the glance of the SAME way and model (`glanceServedBy`), so a read that raced a switch
 * never names the wrong node. `error`: the read failed or goosed could not tell which way serves.
 */
export type GlanceServingReport =
  | {
      way: { kind: 'single' | 'remoteSingle' | 'split'; modelId: string; servedModelId: string };
      nodes: GlanceNodeRef[];
    }
  | { error: string };

/** What the glance says about the node that serves: its node(s), or why that is not known. */
export type GlanceServedBy = { nodes: GlanceNodeRef[] } | { error: string };

export interface EngineGlance {
  /** There is an engine to speak of (not off, not unknown). */
  present: boolean;
  /** Reading, writing, queued, loading or reconnecting — work a person waits on. */
  busy: boolean;
  phase: EnginePhase;
  stage: GlanceStage;
  engine: GlanceEngine;
  modelId: string | null;
  hero: EngineFigure | null;
  second: EngineFigure | null;
  /** How far the read / the load is; `indeterminate` when it moves with no measured figure. */
  progress: { done: number; total: number; unit: 'tokens' | 'bytes' } | 'indeterminate' | null;
  /** Requests the engine holds waiting; null = no live read to count them from. */
  waiting: number | null;
  /** The split's in-flight count when no live read of its rank 0 exists. */
  inflight: number | null;
  /**
   * The chat of this app the engine serves (it opens that session): its turn, else goose's own call
   * for it (`work`, Q-185 — the fact check after the reply is named as that, never as the chat).
   */
  chat: { sessionId: string; name: string; work: BackgroundWorkKind | null } | null;
  /**
   * goose's own calls running beside the chat line's work — the fact check, a title, a tool label
   * (Q-185) — by their kind, each once. They are named, never counted as other clients and never
   * read as the chat's turn (Q-218).
   */
  side: BackgroundWorkKind[];
  /** Everyone else it serves: other clients, and requests from another app. */
  otherClients: number;
  ranges: {
    writing: { low: number; high: number } | null;
    reading: { low: number; high: number } | null;
  };
  nodes: GlanceNode[];
  /** The engine's own words for a failure, a stale read or a lost Mac; null = nothing to say. */
  detail: string | null;
  /**
   * The node definition(s) naming the way that serves (design §7.3, `nodes.glanceNode`); null = no
   * window has reported a node for this way and model — nothing is guessed.
   */
  servedBy: GlanceServedBy | null;
  /** While `stage` is `swapping`: the node the loader is loading, by its name. */
  swapTo?: string;
}

export interface GlanceNeedsYou {
  sessionId: string;
  sessionName: string;
  question: string;
}

/** The session-state store's running / needs-you, as the window that holds it reported them. */
export interface GlanceSessions {
  running: number;
  needsYou: GlanceNeedsYou[];
  /**
   * A window's report only (never the merged push): the node its goosed says serves, read when the
   * glance changed (glanceStore.ts). Absent = this window has not read it.
   */
  serving?: GlanceServingReport | null;
  /**
   * A window's report only: the swap its goosed's node loader is making (utils/nodeSwap.ts); null =
   * none. Absent = this window has not read it.
   */
  swap?: NodeSwap | null;
}

export type GlanceDesktopMode = 'off' | 'away' | 'busy';
export type GlanceCorner = 'top-left' | 'top-right' | 'bottom-left' | 'bottom-right';

/** The person's choices, stored in settings.json under `engineGlance`. */
export interface GlancePrefs {
  /**
   * The card at the foot of the sidebar. false = hidden by the person (the card's hide control, or
   * Settings › App); the sidebar then offers one row that brings it back (Q-218).
   */
  inApp: boolean;
  /**
   * The desktop mini window: never; while goose is in the background and something is live
   * (the default); or whenever something is live.
   */
  desktop: GlanceDesktopMode;
  /** The desktop window shows as a pill (stage + rate) instead of the card. */
  desktopCollapsed: boolean;
  /** Where the desktop window sits: a corner of a display, by the display's id. */
  desktopPlace: { displayId: number; corner: GlanceCorner } | null;
  /**
   * The desktop window has appeared at least once, so its one-time "you can turn this off from
   * here" hint has had its showing (Q-224). Set by main the first time it shows the window.
   */
  desktopHintSeen: boolean;
}

/** The corner the desktop window takes before the person drags it anywhere. */
export const DEFAULT_GLANCE_CORNER: GlanceCorner = 'bottom-right';

export const DEFAULT_GLANCE_PREFS: GlancePrefs = {
  inApp: true,
  desktop: 'away',
  desktopCollapsed: false,
  desktopPlace: null,
  desktopHintSeen: false,
};

export interface GlancePush {
  engine: EngineGlance;
  sessions: GlanceSessions;
  prefs: GlancePrefs;
}

export interface EngineGlanceOptions {
  distributed: { report: MlxDistributedReport; ageMs: number } | null;
  remote: MlxRemoteReport | null;
  /** Every window's serving report (`servingReportsOf`); the glance names a node only from these. */
  served: readonly GlanceServingReport[];
  /** The swap a window's loader reports (`swapOfReports`); null = none in progress. */
  swap?: NodeSwap | null;
}

type EngineParts = Omit<EngineGlance, 'servedBy'>;

const NO_RANGES: EngineGlance['ranges'] = { writing: null, reading: null };

export function measuredPairOf(read: MlxMeasuredRead): MeasuredPair {
  if (read.kind !== 'read') return { writing: null, reading: null };
  return {
    writing: measuredFigure(read.answer.writing),
    reading: measuredFigure(read.answer.reading),
  };
}

function rangesOf(pair: MeasuredPair): EngineGlance['ranges'] {
  return { writing: pair.writing?.spread ?? null, reading: pair.reading?.spread ?? null };
}

const BUSY_STAGES: ReadonlySet<GlanceStage> = new Set<GlanceStage>([
  'generating',
  'prefill',
  'queued',
  'loading',
  'swapping',
  'reconnecting',
  'serving',
  'held',
]);

type ServingParts = Pick<EngineGlance, 'chat' | 'side' | 'otherClients'> & {
  /** The chat line is a chat's own TURN (not goose's call for it): the card leads with it. */
  turn: boolean;
};

function servingOf(snapshot: MlxEngineSnapshot): ServingParts {
  const serving = snapshot.serving;
  if (!serving) return { chat: null, side: [], otherClients: 0, turn: false };
  const chats = serving.clients.filter((c) => c.kind === 'chat');
  const chatClient = chats.find((c) => c.work == null) ?? chats[0];
  const chat =
    chatClient && chatClient.kind === 'chat'
      ? {
          sessionId: chatClient.sessionId,
          name: chatClient.sessionName || chatClient.sessionId,
          work: chatClient.work,
        }
      : null;
  const side: BackgroundWorkKind[] = [];
  let sideClients = 0;
  for (const c of serving.clients) {
    if (c === chatClient || c.kind === 'external' || c.work == null) continue;
    sideClients += 1;
    if (!side.includes(c.work)) side.push(c.work);
  }
  return {
    chat,
    side,
    turn: chat != null && chat.work == null,
    otherClients: serving.clients.length - (chat ? 1 : 0) - sideClients + serving.unattributed,
  };
}

/**
 * The request the card leads with while a chat's turn is on the engine: the largest prompt — the
 * turn carries the whole conversation (engineFigures.ts `largestPrompt`, the composer's rule). Q-218:
 * without it the card named the engine's longest-read prompt, a 174-token side call, as "Reading
 * prompt" beside the chat's own 77k. With no turn on the engine the card speaks for the engine.
 */
function leadRequest(requests: readonly MlxLiveRequest[], turn: boolean): MlxLiveRequest | null {
  return turn ? (largestPrompt(requests) ?? null) : null;
}

/** The glance of an engine that is up and read: the tile's figures, activity and phase. */
function liveParts(
  snapshot: MlxEngineSnapshot,
  pair: MeasuredPair
): Pick<
  EngineGlance,
  'stage' | 'hero' | 'second' | 'progress' | 'waiting' | 'chat' | 'side' | 'otherClients'
> & { activity: MlxActivity | null } {
  const stats = snapshot.stats;
  const { turn, ...named } = servingOf(snapshot);
  if (!stats) {
    return {
      activity: null,
      stage: 'running',
      hero: pair.writing
        ? { kind: 'writingMedian', median: pair.writing.median, runs: pair.writing.runs }
        : null,
      second: null,
      progress: null,
      waiting: null,
      ...named,
    };
  }
  const lead = leadRequest(stats.requests, turn);
  const activity = lead ? requestActivity(lead) : mlxActivity(stats);
  const { hero, second } = engineFigures(stats, pair, lead);
  const read = activity === 'prefill' ? promptProgress(stats, lead) : null;
  return {
    activity,
    stage: lead ? activity : engineHeadline(stats),
    hero,
    second,
    progress: read ? { ...read, unit: 'tokens' } : null,
    waiting: stats.numWaiting ?? stats.requests.filter((r) => r.status === 'waiting').length,
    ...named,
  };
}

function glance(parts: Omit<EngineParts, 'busy'>): EngineParts {
  return { ...parts, busy: parts.present && BUSY_STAGES.has(parts.stage) };
}

/**
 * The glance while the loader swaps: amber, the node it loads by name, the model it loads — none of
 * the stopped way's figures or words (they describe an engine on its way out).
 */
function swapping(swap: NodeSwap, engine: GlanceEngine, pair: MeasuredPair): EngineParts {
  return glance({
    present: true,
    phase: 'loading',
    stage: 'swapping',
    engine,
    modelId: swap.target.modelId,
    hero: null,
    second: null,
    progress: 'indeterminate',
    waiting: null,
    inflight: null,
    chat: null,
    side: [],
    otherClients: 0,
    ranges: rangesOf(pair),
    nodes: [],
    detail: null,
    swapTo: swap.target.name,
  });
}

/** The split's start: every rank's measured load summed, or indeterminate while any is unmeasured. */
function splitLoad(report: MlxDistributedReport): EngineGlance['progress'] {
  const loads = report.nodes.map((n) => n.load);
  if (loads.length === 0 || loads.some((l) => l == null)) return 'indeterminate';
  const done = loads.reduce((sum, l) => sum + (l as LoadProgress).done, 0);
  const total = loads.reduce((sum, l) => sum + (l as LoadProgress).total, 0);
  return total > 0 ? { done, total, unit: 'bytes' } : 'indeterminate';
}

export function buildEngineGlance(
  snapshot: MlxEngineSnapshot,
  options: EngineGlanceOptions
): EngineGlance {
  const parts = engineParts(snapshot, options);
  return { ...parts, servedBy: glanceServedBy(parts, options.served) };
}

/** The glance's way in `nodes/residency`'s words; null = no way of this Mac's goose (a hosted rank). */
function wayKindOf(engine: GlanceEngine): 'single' | 'remoteSingle' | 'split' | null {
  switch (engine.mode) {
    case 'single':
      return 'single';
    case 'remote':
      return 'remoteSingle';
    case 'distributed':
      return 'split';
    case 'hosting':
      return null;
  }
}

/**
 * The node(s) serving the glance's way, from the windows' reports: the first report of the SAME
 * way and model (either id the engine goes by) names them. A report of another way is a read that
 * raced a switch and says nothing about this one. With no matching report, a report that could not
 * read the way is said as that; otherwise null — never a node guessed from a model name.
 */
export function glanceServedBy(
  engine: Pick<EngineGlance, 'present' | 'engine' | 'modelId'>,
  reports: readonly GlanceServingReport[]
): GlanceServedBy | null {
  const kind = wayKindOf(engine.engine);
  if (!engine.present || kind == null || engine.modelId == null) return null;
  let failed: string | null = null;
  for (const report of reports) {
    if ('error' in report) {
      failed ??= report.error;
      continue;
    }
    const { way } = report;
    if (way.kind !== kind) continue;
    if (way.modelId !== engine.modelId && way.servedModelId !== engine.modelId) continue;
    return report.nodes.length > 0 ? { nodes: report.nodes } : null;
  }
  return failed != null ? { error: failed } : null;
}

/**
 * Every window's serving report, in the order main holds the windows, then `closed` — the last
 * node read of a window that has since closed (`servingKeptOf`). Q-237: the floating window is the
 * one surface left when every goose window is closed (the app lives on in the tray), and it is
 * main's windows that read the nodes, so without it the float lost its node line exactly then. A
 * live window's report always comes first; the kept one names a node only through
 * `glanceServedBy`'s same-way-and-model match, so a switch made since it was read says nothing.
 */
export function servingReportsOf(
  reports: Iterable<GlanceSessions>,
  closed: GlanceServingReport | null = null
): GlanceServingReport[] {
  const out: GlanceServingReport[] = [];
  for (const r of reports) if (r.serving) out.push(r.serving);
  if (closed) out.push(closed);
  return out;
}

/**
 * What main keeps when a window closes: its node read when it named a way (the node definitions
 * change only in a goose window, which reports afresh), else what was kept before. A failed read is
 * never kept — with no window left to read again it would stand as a stale failure.
 */
export function servingKeptOf(
  closing: GlanceSessions | undefined,
  kept: GlanceServingReport | null
): GlanceServingReport | null {
  const serving = closing?.serving;
  return serving && !('error' in serving) ? serving : kept;
}

/**
 * The Nodes nav row's chip (design §5.1): "Loading" while the glance shows a load or a swap, "Failed"
 * while it shows the engine failed, nothing otherwise — a permanent "ready" count would be noise
 * (Q-8). A way a swap stopped is the swap's "Loading", never "Failed" (Q-254).
 */
export function nodesNavChip(engine: EngineGlance | null | undefined): 'loading' | 'failed' | null {
  if (!engine?.present) return null;
  if (engine.stage === 'loading' || engine.stage === 'swapping') return 'loading';
  if (engine.stage === 'failed') return 'failed';
  return null;
}

function engineParts(snapshot: MlxEngineSnapshot, options: EngineGlanceOptions): EngineParts {
  const pair = measuredPairOf(snapshot.measured);
  const distributed = options.distributed;
  const stale = distributed != null && distributed.ageMs > MLX_DISTRIBUTED_STALE_MS;
  const hosting = distributed?.report.mode === 'single' ? distributed.report.hosting : null;

  if (hosting) {
    const loading = hosting.state === 'loading';
    return glance({
      present: true,
      phase: stale ? 'idle' : hostingPhase(hosting.state),
      stage: stale ? 'stale' : loading ? 'loading' : 'hosting',
      engine: {
        mode: 'hosting',
        rank: hosting.rank,
        requester: hosting.requester,
        modelId: hosting.modelId,
        backend: hosting.backend,
      },
      modelId: hosting.modelId,
      hero: null,
      second: null,
      progress: loading && !stale ? (hosting.load ?? 'indeterminate') : null,
      waiting: null,
      inflight: null,
      chat: null,
      side: [],
      otherClients: 0,
      ranges: NO_RANGES,
      nodes: [],
      detail: null,
    });
  }

  if (distributed?.report.mode === 'distributed') {
    const report = distributed.report;
    const up = report.state === 'ready' || report.state === 'serving';
    const live =
      up && snapshot.engine === 'distributed' && snapshot.mode === 'running' && snapshot.stats
        ? liveParts(snapshot, pair)
        : null;
    const nodes: GlanceNode[] = report.nodes.map((n) => ({
      name: n.name,
      phase: nodePhase(n.startWord),
      peakGb: n.peakGb,
      budgetGb: n.budgetGb,
      load: n.state === 'loading' && n.startWord !== 'makingRoom' ? n.load : null,
    }));
    const starting = ['preflight', 'starting'].includes(report.state);
    // The split stopping — or failing as its ranks take the stop — while the loader loads another
    // node is the swap, never "Failed" (Q-254); the split's own start failing stays Failed.
    const swap =
      !stale &&
      (report.state === 'stopping' || report.state === 'failed') &&
      swapStopsEngine(options.swap, {
        way: 'split',
        modelId: report.modelId,
        failed: report.state === 'failed',
      })
        ? options.swap
        : null;
    if (swap)
      return swapping(
        swap,
        { mode: 'distributed', nodeNames: report.nodeNames, backend: report.backend },
        pair
      );
    const stage: GlanceStage = stale
      ? 'stale'
      : report.state === 'failed'
        ? 'failed'
        : up && !report.admissionOpen
          ? 'held'
          : live
            ? live.stage
            : starting || report.state === 'stopping'
              ? 'loading'
              : report.state === 'serving'
                ? 'serving'
                : report.state === 'ready'
                  ? 'running'
                  : 'off';
    return glance({
      present: report.state !== 'stopped',
      phase: stale ? 'idle' : runPhase(report.state, report.admissionOpen, live?.activity ?? null),
      stage,
      engine: { mode: 'distributed', nodeNames: report.nodeNames, backend: report.backend },
      modelId: report.modelId,
      hero: live?.hero ?? null,
      second: live?.second ?? null,
      progress: live ? live.progress : starting && !stale ? splitLoad(report) : null,
      waiting: live?.waiting ?? null,
      inflight: live ? null : up ? report.inflight : null,
      chat: live?.chat ?? null,
      side: live?.side ?? [],
      otherClients: live?.otherClients ?? 0,
      ranges: rangesOf(pair),
      nodes,
      detail: report.state === 'failed' ? report.lastError : null,
    });
  }

  const route = options.remote;
  if (route) {
    // The composer bar's rule (routeContact.ts): main's own read of the route decides "back"; a
    // lagging "reconnecting" mark while main reads the Mac answering is the route up (Q-64).
    const lost =
      route.state === 'ready' || route.state === 'reconnecting'
        ? routeContactLost(route, null, snapshot)
        : null;
    const state = route.state === 'reconnecting' && !lost ? 'ready' : route.state;
    const cause = lost ? leaveCause(route.lastError ?? snapshot.statusDetail) : null;
    const gone = lost
      ? routePeerGone(snapshot.engine === 'remote' ? snapshot.contact : null, cause)
      : null;
    const live =
      !lost && state === 'ready' && snapshot.engine === 'remote' && snapshot.mode === 'running'
        ? liveParts(snapshot, pair)
        : null;
    const stage: GlanceStage = gone
      ? 'away'
      : lost
        ? 'reconnecting'
        : state === 'mounting'
          ? 'loading'
          : state === 'failed'
            ? 'failed'
            : (live?.stage ?? 'running');
    return glance({
      present: true,
      phase: gone ? 'held' : lost ? 'loading' : remotePhase(state, live?.activity ?? null),
      stage,
      engine: { mode: 'remote', peerName: route.peerName },
      modelId: route.modelId ?? snapshot.modelId,
      hero: live?.hero ?? null,
      second: live?.second ?? null,
      progress: live ? live.progress : state === 'mounting' ? 'indeterminate' : null,
      waiting: live?.waiting ?? null,
      inflight: null,
      chat: live?.chat ?? null,
      side: live?.side ?? [],
      otherClients: live?.otherClients ?? 0,
      ranges: rangesOf(pair),
      nodes: [],
      detail: lost ? (lost.why ?? null) : state === 'failed' ? route.lastError : null,
    });
  }

  // A read of the split's rank 0 never speaks for the single engine (a run that just stopped).
  const single = snapshot.engine === 'single' ? snapshot : INITIAL_SNAPSHOT;
  const phase = snapshotPhase(single) ?? 'unloaded';
  const base = {
    engine: { mode: 'single' } as GlanceEngine,
    modelId: single.modelId,
    inflight: null,
    ranges: rangesOf(pair),
    nodes: [],
  };
  if (single.mode === 'running') {
    const live = liveParts(single, pair);
    return glance({
      ...base,
      present: true,
      // The lead request's own colour: reading blue while the turn reads, whatever runs beside it.
      phase: live.activity ? activityPhase(live.activity) : phase,
      stage: live.stage,
      hero: live.hero,
      second: live.second,
      progress: live.progress,
      waiting: live.waiting,
      chat: live.chat,
      side: live.side,
      otherClients: live.otherClients,
      detail: single.stats ? null : single.statusDetail,
    });
  }
  const stage: GlanceStage =
    single.mode === 'mounting'
      ? 'loading'
      : single.mode === 'failed'
        ? 'failed'
        : single.mode === 'reconnecting'
          ? 'reconnecting'
          : 'off';
  // This Mac's engine stopped (or killed by the stop: "exit status: 143") while the loader loads
  // another node is the swap's doing (Q-254); the target's own mount failing stays Failed.
  if (
    (stage === 'failed' || stage === 'off') &&
    swapStopsEngine(options.swap, {
      way: 'single',
      modelId: single.modelId,
      failed: stage === 'failed',
    })
  ) {
    return swapping(options.swap, base.engine, pair);
  }
  return glance({
    ...base,
    present: stage !== 'off',
    phase,
    stage,
    hero: null,
    second: null,
    progress: stage === 'loading' ? 'indeterminate' : null,
    waiting: null,
    chat: null,
    side: [],
    otherClients: 0,
    detail: single.mode === 'failed' ? single.failedError : null,
  });
}

const DESKTOP_MODES: ReadonlySet<string> = new Set<GlanceDesktopMode>(['off', 'away', 'busy']);
const CORNERS: ReadonlySet<string> = new Set<GlanceCorner>([
  'top-left',
  'top-right',
  'bottom-left',
  'bottom-right',
]);

/** A stored value is the person's prefs only if every field is one this build writes. */
export function isGlancePrefs(value: unknown): value is GlancePrefs {
  if (value == null || typeof value !== 'object') return false;
  const v = value as Record<string, unknown>;
  const place = v.desktopPlace as Record<string, unknown> | null | undefined;
  return (
    typeof v.inApp === 'boolean' &&
    typeof v.desktop === 'string' &&
    DESKTOP_MODES.has(v.desktop) &&
    typeof v.desktopCollapsed === 'boolean' &&
    typeof v.desktopHintSeen === 'boolean' &&
    (place === null ||
      (place != null &&
        typeof place === 'object' &&
        typeof place.displayId === 'number' &&
        typeof place.corner === 'string' &&
        CORNERS.has(place.corner)))
  );
}

/** The stored prefs, each field that is not one this build writes read as its default. */
export function glancePrefsOf(stored: unknown): GlancePrefs {
  if (isGlancePrefs(stored)) return stored;
  if (stored == null || typeof stored !== 'object') return DEFAULT_GLANCE_PREFS;
  const merged = { ...DEFAULT_GLANCE_PREFS, ...(stored as Partial<GlancePrefs>) };
  return isGlancePrefs(merged) ? merged : DEFAULT_GLANCE_PREFS;
}

function isNodeRef(value: unknown): value is GlanceNodeRef {
  if (value == null || typeof value !== 'object') return false;
  const v = value as Record<string, unknown>;
  return typeof v.id === 'string' && typeof v.name === 'string';
}

const WAY_KINDS: ReadonlySet<string> = new Set(['single', 'remoteSingle', 'split']);

export function isGlanceServingReport(value: unknown): value is GlanceServingReport {
  if (value == null || typeof value !== 'object') return false;
  const v = value as Record<string, unknown>;
  if ('error' in v) return typeof v.error === 'string';
  const way = v.way as Record<string, unknown> | null | undefined;
  return (
    way != null &&
    typeof way === 'object' &&
    typeof way.kind === 'string' &&
    WAY_KINDS.has(way.kind) &&
    typeof way.modelId === 'string' &&
    typeof way.servedModelId === 'string' &&
    Array.isArray(v.nodes) &&
    v.nodes.every(isNodeRef)
  );
}

export function isGlanceSessions(value: unknown): value is GlanceSessions {
  if (value == null || typeof value !== 'object') return false;
  const v = value as Record<string, unknown>;
  return (
    (v.serving === undefined || v.serving === null || isGlanceServingReport(v.serving)) &&
    (v.swap === undefined || v.swap === null || isNodeSwap(v.swap)) &&
    typeof v.running === 'number' &&
    Number.isFinite(v.running) &&
    Array.isArray(v.needsYou) &&
    v.needsYou.every(
      (n) =>
        n != null &&
        typeof n === 'object' &&
        typeof (n as GlanceNeedsYou).sessionId === 'string' &&
        typeof (n as GlanceNeedsYou).sessionName === 'string' &&
        typeof (n as GlanceNeedsYou).question === 'string'
    )
  );
}

export const NO_SESSIONS: GlanceSessions = { running: 0, needsYou: [] };

/**
 * Each window reports its own ACP connection's turns (one goosed serves every window since Q-257,
 * but every connection keeps its own busy set — acp/server/needs_you.rs busy_sessions): its
 * sessions add up, a question asked twice counts once.
 */
export function mergeGlanceSessions(reports: Iterable<GlanceSessions>): GlanceSessions {
  let running = 0;
  const needsYou = new Map<string, GlanceNeedsYou>();
  for (const r of reports) {
    running += r.running;
    for (const n of r.needsYou) {
      const key = `${n.sessionId}\n${n.question}`;
      if (!needsYou.has(key)) needsYou.set(key, n);
    }
  }
  return { running, needsYou: [...needsYou.values()] };
}

export function isGlancePush(value: unknown): value is GlancePush {
  if (value == null || typeof value !== 'object') return false;
  const v = value as Record<string, unknown>;
  const engine = v.engine as Record<string, unknown> | undefined;
  return (
    engine != null &&
    typeof engine === 'object' &&
    typeof engine.present === 'boolean' &&
    typeof engine.stage === 'string' &&
    typeof engine.phase === 'string' &&
    isGlanceSessions(v.sessions) &&
    isGlancePrefs(v.prefs)
  );
}
