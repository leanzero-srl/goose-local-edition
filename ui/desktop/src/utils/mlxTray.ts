import {
  MLX_STATUS_POLL_MS,
  answeredRequests,
  compactTokens,
  engineHeadline,
  formatElapsed,
  formatRate,
  leavingRowsOf,
  liveDecodeTps,
  measuredPrefillTps,
  mlxActivity,
  readingNowTps,
  type LeavingRows,
} from '../components/leanzero-swarm/mlxLiveStats';
import { readingRequest } from '../components/leanzero-swarm/engineFigures';
import { measuredFigure, type MeasuredFigure, type MlxMeasuredRead } from './mlxMeasuredRuns';
import {
  linkWords,
  gb1,
  gib,
  layerSpanShort,
  type LoadProgress,
} from '../components/leanzero-swarm/mlxDistributed';
import {
  activityPhase,
  hostingPhase,
  nodePhase,
  remotePhase,
  runPhase,
} from '../components/leanzero-swarm/mlxPhase';
import type { EnginePhase } from '../components/lz/tokens';
import { INITIAL_SNAPSHOT, type MlxEngineSnapshot } from './mlxEngineMonitor';
import type {
  MlxDistributedReport,
  MlxDistributedReportHosting,
  MlxDistributedReportNode,
} from './mlxDistributedReport';
import type { MlxClient, MlxServing } from './mlxServing';
import { remoteTrayLine, type MlxRemoteReport } from './mlxRemoteReport';
import { leaveCause, type LeaveCause } from './leaveCause';
import { routeContactLost, routePeerGone, type PeerGone } from './routeContact';
import { swapStopsEngine, type NodeSwap } from './nodeSwap';
import {
  restoreSuperseded,
  restoreTrayLine,
  servingKey,
  type MlxRestoreReport,
} from './mlxRestoreReport';
import { BACKGROUND_WORK_EN, TRAY_ACTION_ENGINES, trayCutLine, workCutBy } from './mlxInFlight';

/**
 * The menu-bar presence of the local LeanZero MLX engine, as a PURE function of main's snapshot:
 * the short title beside the tray icon (macOS `Tray.setTitle`) and the menu's engine section. main.ts
 * turns the descriptors into Electron menu items; nothing here touches Electron, so every state is
 * tested as data. Every figure is one the monitor measured — the same derivations as the state tile.
 * Colour is the engine-phase palette the tile uses (mlxPhase.ts): the title leads with the phase's
 * glyph and each state line carries `phase`, which main draws as a solid dot in PHASE_HEX.
 */

export type MlxTrayAction =
  | 'open-providers'
  | 'mount'
  | 'unmount'
  | 'stop-distributed'
  | 'stop-remote'
  | 'run-here'
  | 'stop-waiting';

export type MlxTrayItem =
  /** `sessionId`: the line is a chat of this app, and clicking it opens that exact session. */
  | { type: 'info'; label: string; phase?: EnginePhase; sessionId?: string }
  | { type: 'action'; label: string; action: MlxTrayAction; enabled: boolean }
  | { type: 'separator' };

export interface MlxTrayModel {
  /** The text beside the icon; empty when there is no engine to speak of. */
  title: string;
  /** The phase the title speaks for; null = no live claim (nothing to show, or a stale read). */
  phase: EnginePhase | null;
  items: MlxTrayItem[];
}

/**
 * The title's colour mark. A menu-bar title is plain text (Electron `Tray.setTitle`), so the phase
 * travels as the one coloured glyph the system font draws in colour — the nearest of each hue to
 * PHASE_HEX; the menu's own dots are drawn in the exact hex by main.
 */
export const PHASE_GLYPH: Record<EnginePhase, string> = {
  unloaded: '⚫',
  idle: '⚪',
  loading: '🟡',
  reading: '🔵',
  writing: '🟢',
  held: '🟠',
  failed: '🔴',
};

/** The title as main sets it: the phase glyph, then the words. */
export function trayTitleText(model: MlxTrayModel): string {
  if (!model.title) return '';
  return model.phase ? `${PHASE_GLYPH[model.phase]} ${model.title}` : model.title;
}

/** The single engine's phase from main's snapshot (`unknown` claims nothing). */
export function snapshotPhase(snapshot: MlxEngineSnapshot): EnginePhase | null {
  switch (snapshot.mode) {
    case 'off':
      return 'unloaded';
    case 'unknown':
      return null;
    case 'mounting':
      return 'loading';
    case 'failed':
      return 'failed';
    case 'reconnecting':
      return 'loading';
    case 'running':
      return snapshot.stats ? activityPhase(mlxActivity(snapshot.stats)) : 'idle';
  }
}

function loadText(load: LoadProgress): string {
  return `loaded ${gb1(gib(load.done))} of ${gb1(gib(load.total))} GB`;
}

export interface MlxTrayOptions {
  /** A window exists to carry the ACP call (mount/unmount/navigate go through a renderer). */
  canAct: boolean;
  /** The model goose would mount (`mlx_engine.model_id`), or null when none is configured. */
  mountModelId: string | null;
  /**
   * The renderer's last read of the DISTRIBUTED engine and how old it is; null when no window has
   * reported one (a backend without the `mlxDistributed` capability, or no window yet).
   */
  distributed: { report: MlxDistributedReport; ageMs: number } | null;
  /**
   * Where this Mac's MLX chat goes when it is routed to a LeanZero Link peer's engine (remote
   * single); null when chat stays here or no window has reported a route.
   */
  remote?: MlxRemoteReport | null;
  /** What the launch is bringing back, or why it could not; null = nothing to say. */
  restore?: MlxRestoreReport | null;
  /**
   * The swap a goose window's node loader is making (utils/nodeSwap.ts): the way it stopped reads
   * "swapping to <node>", never "MLX failed" (Q-254). null = none.
   */
  swap?: NodeSwap | null;
}

/**
 * main's live read of the peer's engine through the relay — the same snapshot and derivations as
 * the single engine — or null while the route mounts, failed, or has not been read yet.
 */
function remoteLive(
  snapshot: MlxEngineSnapshot,
  report: MlxRemoteReport
): MlxEngineSnapshot | null {
  return report.state === 'ready' &&
    snapshot.engine === 'remote' &&
    snapshot.mode === 'running' &&
    snapshot.stats
    ? snapshot
    : null;
}

/**
 * The route is published and its Mac does not answer: the route says so (`reconnecting`), or
 * main's own read of its engine through the relay failed. Chat still goes there — nothing on THIS
 * Mac is read or offered as if it served (Q-48).
 */
function remoteReconnecting(snapshot: MlxEngineSnapshot, report: MlxRemoteReport): boolean {
  // The composer bar's rule (routeContact.ts): main's own read of the route decides "back".
  if (report.state !== 'ready' && report.state !== 'reconnecting') return false;
  return routeContactLost(report, null, snapshot) != null;
}

/**
 * The title names the Mac that serves chat — its one name (`routePeerName`, carried as
 * `peerName`) — never "Remote", which said a route exists without saying where (Q-27).
 * `reconnecting` is why contact is lost (the Mac's own word, or just lost); null = it answers.
 */
function remoteTrayTitle(
  report: MlxRemoteReport,
  live: MlxEngineSnapshot | null,
  reconnecting: LeaveCause | 'lost' | null
): string {
  const mac = report.peerName;
  // A Mac that said it quit is gone (peerGoneModel); one restarting goose is a blip.
  if (reconnecting) {
    return reconnecting === 'restart' ? `${mac} is restarting goose` : `Reconnecting to ${mac}`;
  }
  if (report.state === 'ready') return live ? `${mac} · ${mlxTrayTitle(live)}` : mac;
  return report.state === 'failed' ? `${mac} · failed` : `${mac} · ${report.state}`;
}

/** The clock time contact was lost at, in this Mac's own time format. */
export function lostSinceText(ms: number): string {
  return new Date(ms).toLocaleTimeString(undefined, { hour: 'numeric', minute: '2-digit' });
}

/**
 * The composer bar's steady words for a Mac that is away (Q-111), in the tray's English: "isn't
 * running" ONLY when its goose said it quit; a silence says what is known — no answer since when.
 */
export function peerGoneText(mac: string, gone: PeerGone): string {
  return gone.because === 'said-quit'
    ? `${mac}’s goose isn’t running`
    : `${mac} hasn’t answered since ${lostSinceText(gone.lostSinceMs)}`;
}

/**
 * The route's Mac is away, not a blip (routeContact.ts `routePeerGone`): a steady state with the
 * two ways out — chat on this Mac, or stop waiting for that one — instead of "reconnecting…" for
 * hours. The Mac coming back still restores the route on its own.
 */
function peerGoneModel(
  mac: string,
  gone: PeerGone,
  canAct: boolean,
  mountModelId: string | null
): MlxTrayModel {
  const phase: EnginePhase = 'held';
  const items: MlxTrayItem[] = [
    { type: 'info', label: clip(peerGoneText(mac, gone)), phase },
    ...(gone.because === 'said-quit'
      ? [{ type: 'info' as const, label: clip(`${mac} quit goose`) }]
      : [{ type: 'info' as const, label: 'Its goose may be closed, or it’s offline' }]),
    { type: 'info', label: 'Open goose there, or run chat on this Mac' },
    { type: 'separator' },
  ];
  // This Mac runs chat only with a model to load (or one already up, which the renderer checks).
  if (mountModelId != null) {
    items.push({
      type: 'action',
      label: 'Run on this Mac instead',
      action: 'run-here',
      enabled: canAct,
    });
  }
  items.push(
    { type: 'action', label: 'Stop waiting for it', action: 'stop-waiting', enabled: canAct },
    { type: 'action', label: 'Open Providers', action: 'open-providers', enabled: canAct }
  );
  return { title: peerGoneText(mac, gone), phase, items };
}

/** Chat is served by a linked Mac's engine: the tray speaks for THAT engine, and offers its Stop. */
function remoteModel(
  snapshot: MlxEngineSnapshot,
  report: MlxRemoteReport,
  canAct: boolean,
  mountModelId: string | null
): MlxTrayModel {
  const reconnecting = remoteReconnecting(snapshot, report);
  // The registry's lagging mark while main reads the Mac answering: the route main proves (Q-64).
  const remote: MlxRemoteReport =
    report.state === 'reconnecting' && !reconnecting
      ? { ...report, state: 'ready', lastError: null }
      : report;
  // The composer bar's words (ComposerReadiness), never the raw read (Q-58): a Mac that said it
  // quit or is restarting goose is named with that; otherwise contact is lost and goose keeps
  // trying. The raw reason stays in the app, behind the bar's Details.
  const cause = reconnecting ? leaveCause(remote.lastError ?? snapshot.statusDetail) : null;
  const mac = remote.peerName;
  const gone = reconnecting
    ? routePeerGone(snapshot.engine === 'remote' ? snapshot.contact : null, cause)
    : null;
  if (gone) return peerGoneModel(mac, gone, canAct, mountModelId);
  const live = remoteLive(snapshot, remote);
  const phase = reconnecting
    ? 'loading'
    : remotePhase(remote.state, live?.stats ? mlxActivity(live.stats) : null);
  const items: MlxTrayItem[] = [
    {
      type: 'info',
      label: clip(
        !reconnecting
          ? remoteTrayLine(remote)
          : cause === 'restart'
            ? `${mac} is restarting goose`
            : `Lost contact with ${mac} — reconnecting…`
      ),
      phase,
    },
  ];
  if (reconnecting) {
    items.push({
      type: 'info',
      label: clip(
        cause
          ? 'goose reconnects when it is back'
          : 'goose keeps trying, then checks whether your answer survived'
      ),
    });
  } else if (live) {
    items.push(...runningItems(live));
  } else if (remote.state === 'ready' && snapshot.engine === 'remote' && snapshot.statusDetail) {
    items.push({
      type: 'info',
      label: clip(`Rates unavailable over LeanZero Link: ${snapshot.statusDetail}`),
    });
  }
  if (remote.lastError && !reconnecting) {
    items.push({ type: 'info', label: clip(`Error: ${remote.lastError}`) });
  }
  items.push(
    { type: 'separator' },
    { type: 'action', label: 'Open Providers', action: 'open-providers', enabled: canAct },
    ...stopItems(snapshot, clip(`Stop serving from ${remote.peerName}`), 'stop-remote', canAct)
  );
  return {
    title: remoteTrayTitle(remote, live, reconnecting ? (cause ?? 'lost') : null),
    phase,
    items,
  };
}

/**
 * A distributed report older than three missed renderer polls is STALE: main cannot read the
 * distributed engine itself (ACP lives in the renderer), so past this the tray says it is showing
 * an old read instead of presenting it as live. ratio: three poll intervals.
 */
export const MLX_DISTRIBUTED_STALE_MS = 3 * MLX_STATUS_POLL_MS;

const LABEL_MAX = 80;

function clip(text: string): string {
  return text.length > LABEL_MAX ? `${text.slice(0, LABEL_MAX - 1)}…` : text;
}

/**
 * A stop that would cut work in flight says so on the line above it, and its label ends in "…":
 * the click asks first, in the window (Q-148). Nothing in flight: the action alone, as it was.
 */
function stopItems(
  snapshot: MlxEngineSnapshot,
  label: string,
  action: MlxTrayAction,
  enabled: boolean
): MlxTrayItem[] {
  const work = workCutBy(snapshot, TRAY_ACTION_ENGINES[action] ?? []);
  if (!work) return [{ type: 'action', label, action, enabled }];
  return [
    { type: 'info', label: clip(trayCutLine(work)) },
    { type: 'action', label: `${label}…`, action, enabled },
  ];
}

function plural(n: number, one: string, many: string): string {
  return `${n.toLocaleString()} ${n === 1 ? one : many}`;
}

/**
 * Rows whose answers already ended but still hold the batch (Q-231 `leaving`), in the tray's words —
 * the Engine tile and the glance say the same fact (leavingRowsText.ts). Never "Reading" (Q-246).
 */
function leavingLine(leaving: LeavingRows): string {
  const since =
    leaving.sinceStopS != null ? ` · stopped ${formatElapsed(leaving.sinceStopS)} ago` : '';
  return `${plural(leaving.rows, 'stopped request', 'stopped requests')} still leaving the engine${since}`;
}

export function mlxTrayTitle(snapshot: MlxEngineSnapshot): string {
  switch (snapshot.mode) {
    case 'off':
    case 'unknown':
      return '';
    case 'mounting':
      return 'Mounting';
    case 'failed':
      return 'MLX failed';
    case 'reconnecting':
      return 'Reconnecting';
    case 'running':
      break;
  }
  const stats = snapshot.stats;
  if (!stats) return 'MLX';
  switch (engineHeadline(stats)) {
    case 'generating': {
      const rate = liveDecodeTps(stats);
      return rate > 0 ? `${formatRate(rate)} tok/s` : 'Writing';
    }
    case 'prefill': {
      const r = readingRequest(stats);
      return r?.promptTokens != null ? `Reading ${compactTokens(r.promptTokens)}` : 'Reading';
    }
    case 'queued':
      return `Queued ${answeredRequests(stats.requests).length}`;
    case 'leaving':
      return `${leavingRowsOf(stats.requests)?.rows ?? 0} stopped · leaving`;
    case 'not_loaded':
      return 'No model';
    case 'idle':
      return 'Idle';
  }
}

function headline(snapshot: MlxEngineSnapshot): string {
  switch (snapshot.mode) {
    case 'off':
      return 'LeanZero MLX: not mounted';
    case 'unknown':
      return 'LeanZero MLX: state unknown';
    case 'mounting':
      return 'LeanZero MLX: mounting';
    case 'failed':
      return 'LeanZero MLX: failed';
    case 'reconnecting':
      return 'LeanZero MLX: reconnecting';
    case 'running':
      break;
  }
  if (!snapshot.stats) return 'LeanZero MLX: running';
  const word = {
    generating: 'writing',
    prefill: 'reading a prompt',
    queued: 'requests queued',
    leaving: 'stopped requests leaving',
    idle: 'idle',
    not_loaded: 'running, no model loaded',
  }[engineHeadline(snapshot.stats)];
  return `LeanZero MLX: ${word}`;
}

export function clientLabel(client: MlxClient): string {
  const times = client.count > 1 ? ` (×${client.count})` : '';
  switch (client.kind) {
    case 'chat': {
      const name = client.sessionName || client.sessionId;
      // goose's own call for the chat (Q-185) is named as that, never as the chat's answer.
      return client.work
        ? clip(`Serving: ${BACKGROUND_WORK_EN[client.work]} · ${name}${times}`)
        : clip(`Serving chat: ${name}${times}`);
    }
    case 'external':
      return clip(`Serving an external client via /v1: ${client.model}${times}`);
    case 'session': {
      const type = client.sessionType ? client.sessionType.replace(/_/g, ' ') : 'goose';
      const name = client.sessionName || client.sessionId || 'no session';
      return client.work
        ? clip(`Serving: ${BACKGROUND_WORK_EN[client.work]} · ${name}${times}`)
        : clip(`Serving a ${type} session: ${name}${times}`);
    }
  }
}

/**
 * A measured rate with what it is the median of — the Engine tile's words (MlxStateTile): writing
 * is measured over runs, reading over the prompts it read, so the same 454 is never "454 runs" in
 * the tray beside "454 prompts" on the card (Q-314).
 */
function measuredText(verb: string, f: MeasuredFigure, unit: 'run' | 'prompt'): string {
  const rate = `${verb} ${formatRate(f.median)} tok/s`;
  if (f.runs === 1) return `${rate} · 1 ${unit}`;
  const half = f.spread
    ? `, middle half ${formatRate(f.spread.low)}–${formatRate(f.spread.high)}`
    : '';
  return `${rate} · median of ${f.runs} ${unit}s${half}`;
}

/**
 * goose's measured runs for the way the engine runs — the figures the Engine tile and the Run it card
 * show (the same reader, the same one-run rule): writing, and reading at the chat prompt size. A read
 * still in flight says nothing; one that failed says why; none measured says so.
 */
export function measuredLines(read: MlxMeasuredRead): string[] {
  if (read.kind === 'pending') return [];
  if (read.kind === 'unread') return [clip(`Measured runs unread: ${read.detail}`)];
  const writing = measuredFigure(read.answer.writing);
  const reading = measuredFigure(read.answer.reading);
  if (!writing && !reading) return ['No measured runs on this way yet'];
  return [
    writing ? measuredText('Writes', writing, 'run') : null,
    reading ? measuredText('Reads', reading, 'prompt') : null,
  ].filter((line): line is string => line != null);
}

function servingItems(serving: MlxServing | null): MlxTrayItem[] {
  if (!serving) return [];
  const items: MlxTrayItem[] = serving.clients.map((c) => ({
    type: 'info' as const,
    label: clientLabel(c),
    ...(c.kind === 'chat' ? { sessionId: c.sessionId } : {}),
  }));
  if (serving.unattributed > 0) {
    items.push({
      type: 'info',
      // The engine lists requests by an internal id: the caller's address is not known, so none is
      // shown — plain words, never the attribution rule's internals (Q-22).
      label: `${plural(serving.unattributed, 'request', 'requests')} from another app`,
    });
    if (serving.swarmRuns.length > 0) {
      items.push({ type: 'info', label: clip(`Swarm run live: ${serving.swarmRuns.join(', ')}`) });
    }
  }
  if (serving.error) {
    items.push({ type: 'info', label: clip(`Who is unknown: ${serving.error}`) });
  }
  return items;
}

function runningItems(snapshot: MlxEngineSnapshot): MlxTrayItem[] {
  const stats = snapshot.stats;
  const items: MlxTrayItem[] = [];
  if (!stats) {
    if (snapshot.statusDetail) {
      items.push({ type: 'info', label: clip(`Live stats unavailable: ${snapshot.statusDetail}`) });
    }
    return items;
  }
  const activity = mlxActivity(stats);
  const decode = liveDecodeTps(stats);
  const prefill = measuredPrefillTps(stats);
  if (activity === 'generating' && decode > 0) {
    items.push({ type: 'info', label: `Writing ${formatRate(decode)} tok/s` });
  }
  const leaving = leavingRowsOf(stats.requests);
  if (leaving) items.push({ type: 'info', label: clip(leavingLine(leaving)) });
  const reading = readingRequest(stats);
  if (reading?.promptTokens != null) {
    const cached = reading.cachedTokens ? `, ${compactTokens(reading.cachedTokens)} cached` : '';
    const read =
      reading.prefilledTokens != null ? `, ${compactTokens(reading.prefilledTokens)} read` : '';
    const elapsed = reading.elapsedS != null ? ` for ${formatElapsed(reading.elapsedS)}` : '';
    items.push({
      type: 'info',
      label: `Reading a ${compactTokens(reading.promptTokens)}-token prompt${cached}${read}${elapsed}`,
    });
  }
  if (readingNowTps(stats) > 0) {
    items.push({ type: 'info', label: `Reading at ${formatRate(prefill)} tok/s` });
  } else if (prefill > 0) {
    items.push({ type: 'info', label: `Read the last prompt at ${formatRate(prefill)} tok/s` });
  }
  if (activity !== 'generating' && decode === 0 && prefill === 0) {
    items.push(
      ...measuredLines(snapshot.measured).map((label) => ({ type: 'info' as const, label }))
    );
  }
  if (snapshot.statusDetail) {
    items.push({ type: 'info', label: clip(`Stale: ${snapshot.statusDetail}`) });
  }
  items.push(...servingItems(snapshot.serving));
  if (stats.cacheTokensSaved != null) {
    const hits =
      stats.cacheHitRate != null ? `, ${Math.round(stats.cacheHitRate * 100)}% of lookups hit` : '';
    items.push({
      type: 'info',
      label: `Cache saved ${compactTokens(stats.cacheTokensSaved)} prompt tokens${hits}`,
    });
  }
  if (stats.totalRequests != null) {
    const prompt =
      stats.totalPromptTokens != null ? `, ${compactTokens(stats.totalPromptTokens)} read` : '';
    const written =
      stats.totalCompletionTokens != null
        ? `, ${compactTokens(stats.totalCompletionTokens)} written`
        : '';
    items.push({
      type: 'info',
      label: `Served ${plural(stats.totalRequests, 'request', 'requests')}${prompt}${written}`,
    });
  }
  const facts = [
    stats.uptimeS != null ? `Up ${formatElapsed(stats.uptimeS)}` : null,
    stats.activeMemoryGb != null ? `${stats.activeMemoryGb.toFixed(1)} GB GPU memory` : null,
  ].filter(Boolean);
  if (facts.length > 0) items.push({ type: 'info', label: facts.join(', ') });
  return items;
}

function shortModel(id: string): string {
  return id.split('/').pop() || id;
}

/** "Split across MacBook Pro + workhorse · over Thunderbolt" — the mode line the tile says too. */
export function distributedModeLine(report: MlxDistributedReport): string {
  const nodes =
    report.nodeNames.length > 0 ? report.nodeNames.join(' + ') : `${report.nodes.length} nodes`;
  const backend = linkWords(report.backend);
  return clip([`Split across ${nodes}`, backend].filter(Boolean).join(' · '));
}

function ageText(ms: number): string {
  return formatElapsed(Math.round(ms / 1000));
}

const START_WORDS: Record<string, string> = { makingRoom: 'making room', warming: 'warming up' };

export function distributedNodeLine(node: MlxDistributedReportNode, runState: string): string {
  const said = START_WORDS[node.startWord] ?? node.state;
  const head = node.startWord === runState ? node.name : `${node.name} (${said})`;
  if (node.memoryError) return clip(`${head}: memory unread — ${node.memoryError}`);
  const parts = [
    layerSpanShort(node.layers),
    node.load && node.state === 'loading' && node.startWord !== 'makingRoom'
      ? loadText(node.load)
      : null,
    node.peakGb != null
      ? node.budgetGb != null
        ? `peak ${gb1(node.peakGb)} of ${gb1(node.budgetGb)} GB split budget`
        : `peak ${gb1(node.peakGb)} GB`
      : node.availableGb != null
        ? `${gb1(node.availableGb)} GB available`
        : null,
    node.pressure && node.pressure !== 'normal' ? `pressure ${node.pressure}` : null,
  ].filter(Boolean);
  return clip(parts.length > 0 ? `${head}: ${parts.join(' · ')}` : head);
}

function distributedStale(d: MlxTrayOptions['distributed']): boolean {
  return d != null && d.ageMs > MLX_DISTRIBUTED_STALE_MS;
}

/**
 * main's live read of the distributed run's rank 0 — the same snapshot and derivations as the single
 * engine — or null when main has none for an up run (the renderer's counters speak then).
 */
function distributedLive(
  snapshot: MlxEngineSnapshot,
  d: NonNullable<MlxTrayOptions['distributed']>
): MlxEngineSnapshot | null {
  const up = d.report.state === 'ready' || d.report.state === 'serving';
  return up && snapshot.engine === 'distributed' && snapshot.mode === 'running' && snapshot.stats
    ? snapshot
    : null;
}

/**
 * Where the split runs, as a split node's name says it (nodeDraft `whereWords`: "both Macs" for
 * two, "3 Macs"); a report that lists no Mac yet names none.
 */
function splitWhere(report: MlxDistributedReport): string {
  const count = report.nodes.length || report.nodeNames.length;
  if (count === 0) return 'your Macs';
  return count === 2 ? 'both Macs' : `${count} Macs`;
}

/**
 * What the split is doing now — the ONE word the title and the menu's first line both say, so the
 * title never reads "Idle" over a menu that says "ready" (Q-314): main's live read of rank 0 when
 * it has one, else the run's own state.
 */
function splitStateWord(
  d: NonNullable<MlxTrayOptions['distributed']>,
  live: MlxEngineSnapshot | null
): string {
  const { report } = d;
  if (distributedStale(d)) return 'stale';
  if (!report.admissionOpen) return 'held';
  if (live) return mlxTrayTitle(live);
  if (report.state === 'serving') {
    return report.inflight != null ? `${report.inflight} in flight` : 'serving';
  }
  return report.state;
}

/**
 * The title while the distributed engine owns this Mac: the model, where it runs, what it does —
 * "Qwen3.8-27B-Atlassian-Q8-mlx · both Macs · Idle", the words a split node is named by. A run
 * whose model is not reported yet says where and what only.
 */
export function distributedTrayTitle(
  d: NonNullable<MlxTrayOptions['distributed']>,
  live: MlxEngineSnapshot | null = null
): string {
  const { report } = d;
  return [
    report.modelId ? shortModel(report.modelId) : null,
    splitWhere(report),
    splitStateWord(d, live),
  ]
    .filter((part): part is string => part != null)
    .join(' · ');
}

function lowerFirst(text: string): string {
  return text.charAt(0).toLowerCase() + text.slice(1);
}

function distributedItems(
  d: NonNullable<MlxTrayOptions['distributed']>,
  live: MlxEngineSnapshot | null
): MlxTrayItem[] {
  const { report } = d;
  const stale = distributedStale(d);
  const activity = live?.stats ? mlxActivity(live.stats) : null;
  const items: MlxTrayItem[] = [
    {
      type: 'info',
      label: `LeanZero MLX: split across ${splitWhere(report)}, ${lowerFirst(splitStateWord(d, live))}`,
      ...(stale ? {} : { phase: runPhase(report.state, report.admissionOpen, activity) }),
    },
    { type: 'info', label: distributedModeLine(report) },
  ];
  if (report.modelId) items.push({ type: 'info', label: clip(`Model: ${report.modelId}`) });
  for (const node of report.nodes) {
    items.push({
      type: 'info',
      label: distributedNodeLine(node, report.state),
      ...(stale ? {} : { phase: nodePhase(node.startWord) }),
    });
  }
  if (live && !stale) {
    items.push(...runningItems(live));
  } else if (report.state === 'serving' || report.state === 'ready') {
    items.push({
      type: 'info',
      label: report.inflight != null ? `In flight: ${report.inflight}` : 'In flight: not measured',
    });
  }
  if (!report.admissionOpen) {
    items.push({
      type: 'info',
      label: "Admission closed: a node's memory is low, new requests wait",
    });
  }
  if (report.restarts > 0) {
    items.push({ type: 'info', label: `Restarts: ${report.restarts.toLocaleString()}` });
  }
  if (report.lastAlarm) items.push({ type: 'info', label: clip(alarmLine(report.lastAlarm)) });
  if (report.lastError) items.push({ type: 'info', label: clip(`Error: ${report.lastError}`) });
  if (distributedStale(d)) {
    items.push({
      type: 'info',
      label: `Not refreshed for ${ageText(d.ageMs)} — open goose to read it again`,
    });
  }
  return items;
}

/**
 * The supervisor's event kinds as a person says them. The tray has no Details: the kind's id and the
 * preflight's check ids ("startFailed", "foreignEngines") stay in the split's event log (Q-174). A
 * kind this build does not know is shown as sent, never mapped onto a known one.
 */
const ALARM_WORDS: Record<string, string> = {
  startFailed: 'The split did not start',
  localNetworkBlocked: 'macOS blocked the local network',
  rankDied: 'A Mac in the split stopped',
  rankFrozen: 'A Mac in the split stopped answering',
  hang: 'The split stopped answering',
  streamWithoutDone: 'An answer ended before it finished',
  breakerOpen: 'The split kept failing, so goose stopped restarting it',
  watchdogCritical: 'Memory ran too low',
  runnerUpdateFailed: 'Updating the split’s software failed',
  restart: 'The split restarted',
  linkRepaired: 'goose repaired the Thunderbolt link',
  watchdogWarn: 'Memory is getting low',
  watchdogBlind: 'goose could not read a Mac’s memory',
  admissionClosed: 'New requests wait: a Mac’s memory is low',
  orphanReclaimed: 'goose cleared a split left from before',
  compactionSkipped: 'goose could not make room',
};

/** The preflight's check ids (goose-sidecar preflight.rs `Check::id`), never shown in the tray. */
const PREFLIGHT_CHECK_IDS = [
  'reachable',
  'foreignEngines',
  'memory',
  'modelManifest',
  'model',
  'python',
  'tbIpv4',
  'ping',
  'rdmaGid',
  'portRange',
  'ports',
  'runnerEnv',
  'runner',
  'plan',
  'localNetworkPermission',
  'linkRepair',
  'loadLock',
];
const CHECK_IDS = PREFLIGHT_CHECK_IDS.join('|');
/** A cluster check leads its clause: "memory: …" alone, or after "; ". */
const LEADING_CHECK = new RegExp(`(^|; )(?:${CHECK_IDS}): `, 'g');
/** A node's check follows its name at the head of its clause: "Work’s Mac Studio foreignEngines: …". */
const NODE_CHECK = new RegExp(`(^|; )([^:;]*?) (?:${CHECK_IDS}): `, 'g');

/**
 * The alarm's message without its machine words: "preflight: Work’s Mac Studio foreignEngines:
 * another distri…" → "Work’s Mac Studio: another distri…".
 */
export function plainAlarmMessage(message: string): string {
  return message
    .replace(/^(?:restart )?(?:refused by )?preflight(?: refused the start)?: /, '')
    .replace(LEADING_CHECK, '$1')
    .replace(NODE_CHECK, '$1$2: ');
}

/** "Last: The split did not start — Work’s Mac Studio: another MLX split …". */
export function alarmLine(alarm: { kind: string; node: string | null; message: string }): string {
  const where = alarm.node ? ` on ${alarm.node}` : '';
  return `Last: ${ALARM_WORDS[alarm.kind] ?? alarm.kind}${where} — ${plainAlarmMessage(alarm.message)}`;
}

/** "Rank 1 of MacBook Pro's split · over Thunderbolt" (the model has its own line below). */
export function hostingLine(hosting: MlxDistributedReportHosting): string {
  const backend = linkWords(hosting.backend);
  return clip(
    [`Rank ${hosting.rank} of ${hosting.requester}'s split`, backend].filter(Boolean).join(' · ')
  );
}

/**
 * What serves chat as main reads it, in the restore's `servingKey`s — the same derivation the
 * renderer's line settles by (mlxRestore.ts `servingKeysOf`), from main's own facts.
 */
export function trayServingKeys(snapshot: MlxEngineSnapshot, options: MlxTrayOptions): string[] {
  const keys: string[] = [];
  if (snapshot.engine === 'single' && snapshot.mode === 'running') {
    keys.push(servingKey('single', snapshot.modelId));
  }
  if (options.remote?.state === 'ready')
    keys.push(servingKey('remoteSingle', options.remote.modelId));
  const split = options.distributed?.report;
  if (split?.mode === 'distributed' && (split.state === 'ready' || split.state === 'serving')) {
    keys.push(servingKey('split', split.modelId));
  }
  return keys;
}

/**
 * The restore report the tray should still show: a failed restore an engine chose afterwards has
 * superseded is gone (Q-166 — the tray said "Could not restore …" beside "split across 2 Macs, ready").
 */
export function standingRestore(
  snapshot: MlxEngineSnapshot,
  options: MlxTrayOptions
): MlxRestoreReport | null {
  const restore = options.restore ?? null;
  if (!restore) return null;
  return restoreSuperseded(restore, trayServingKeys(snapshot, options)) ? null : restore;
}

/**
 * The tray model with the restore's line on top: amber while the launch brings back what served,
 * red with goose's reason when it could not. A title that says nothing yet says so.
 */
export function buildMlxTrayModel(
  snapshot: MlxEngineSnapshot,
  options: MlxTrayOptions
): MlxTrayModel {
  const model = buildEngineTrayModel(snapshot, options);
  const restore = standingRestore(snapshot, options);
  if (!restore) return model;
  const phase: EnginePhase = restore.phase === 'restoring' ? 'loading' : 'failed';
  const line: MlxTrayItem = { type: 'info', label: clip(restoreTrayLine(restore)), phase };
  if (model.title) return { ...model, items: [line, ...model.items] };
  return {
    title: restore.phase === 'restoring' ? 'Restoring…' : 'Restore failed',
    phase,
    items: [line, ...model.items],
  };
}

/** The tray while the node loader swaps: the node it loads, never the stopped way's failure. */
function swapTrayModel(node: string, canAct: boolean): MlxTrayModel {
  return {
    title: 'Swapping',
    phase: 'loading',
    items: [
      { type: 'info', label: clip(`LeanZero MLX: swapping to ${node}`), phase: 'loading' },
      { type: 'separator' },
      { type: 'action', label: 'Open Providers', action: 'open-providers', enabled: canAct },
    ],
  };
}

function buildEngineTrayModel(snapshot: MlxEngineSnapshot, options: MlxTrayOptions): MlxTrayModel {
  const distributed = options.distributed;
  const hosting = distributed?.report.mode === 'single' ? distributed.report.hosting : null;
  if (hosting && distributed) {
    // This Mac serves a rank of ANOTHER Mac's engine over LeanZero Link: the single engine is
    // refused meanwhile (goose's `hostingRank`), so no Mount is offered; the run is stopped from
    // the Mac that started it.
    const stale = distributedStale(distributed);
    const phase = stale ? null : hostingPhase(hosting.state);
    return {
      title: stale ? 'Rank · stale' : `Rank ${hosting.rank} · ${hosting.state}`,
      phase,
      items: [
        {
          type: 'info',
          label:
            hosting.state === 'loading'
              ? clip(
                  `LeanZero MLX: loading rank ${hosting.rank} for ${hosting.requester}${
                    hosting.load ? `, ${loadText(hosting.load)}` : ''
                  }`
                )
              : `LeanZero MLX: serving a rank, ${hosting.state}`,
          ...(phase ? { phase } : {}),
        },
        { type: 'info', label: hostingLine(hosting) },
        { type: 'info', label: clip(`Model: ${hosting.modelId}`) },
        {
          type: 'info',
          label: clip(`Single engine: refused while this Mac serves ${hosting.requester}`),
        },
        ...(distributedStale(distributed)
          ? [
              {
                type: 'info' as const,
                label: `Not refreshed for ${ageText(distributed.ageMs)} — open goose to read it again`,
              },
            ]
          : []),
        { type: 'separator' },
        {
          type: 'action',
          label: 'Open Providers',
          action: 'open-providers',
          enabled: options.canAct,
        },
      ],
    };
  }
  if (
    distributed?.report.mode === 'distributed' &&
    !distributedStale(distributed) &&
    (distributed.report.state === 'stopping' || distributed.report.state === 'failed') &&
    swapStopsEngine(options.swap, {
      way: 'split',
      modelId: distributed.report.modelId,
      failed: distributed.report.state === 'failed',
    })
  ) {
    return swapTrayModel(options.swap.target.name, options.canAct);
  }
  if (distributed?.report.mode === 'distributed') {
    // The distributed engine owns this Mac: the single engine cannot mount (goose refuses it), so
    // the menu speaks for the distributed run and offers its Stop instead of Mount. While the run is
    // up, main's read of its rank 0 says what it is doing, through the single engine's derivations.
    const live = distributedLive(snapshot, distributed);
    return {
      title: distributedTrayTitle(distributed, live),
      phase: distributedStale(distributed)
        ? null
        : runPhase(
            distributed.report.state,
            distributed.report.admissionOpen,
            live?.stats ? mlxActivity(live.stats) : null
          ),
      items: [
        ...distributedItems(distributed, live),
        { type: 'separator' },
        {
          type: 'action',
          label: 'Open Providers',
          action: 'open-providers',
          enabled: options.canAct,
        },
        ...stopItems(snapshot, 'Stop the split', 'stop-distributed', options.canAct),
      ],
    };
  }
  if (options.remote) {
    return remoteModel(snapshot, options.remote, options.canAct, options.mountModelId);
  }
  // A read of the distributed rank 0 never speaks for the single engine (a run that just stopped).
  const singleSnap = snapshot.engine === 'single' ? snapshot : INITIAL_SNAPSHOT;
  // This Mac's engine stopped — or killed by the stop ("exit status: 143") — while a window's node
  // loader loads another node is the swap, not a failure (Q-254).
  if (
    (singleSnap.mode === 'failed' || singleSnap.mode === 'off' || singleSnap.mode === 'unknown') &&
    swapStopsEngine(options.swap, {
      way: 'single',
      modelId: singleSnap.modelId,
      failed: singleSnap.mode === 'failed',
    })
  ) {
    return swapTrayModel(options.swap.target.name, options.canAct);
  }
  const items: MlxTrayItem[] = [];
  const singlePhase = snapshotPhase(singleSnap);
  items.push({
    type: 'info',
    label: headline(singleSnap),
    ...(singlePhase ? { phase: singlePhase } : {}),
  });
  if (distributed) items.push({ type: 'info', label: 'Single · this Mac' });
  if (singleSnap.modelId && singleSnap.mode !== 'off') {
    items.push({ type: 'info', label: clip(`Model: ${singleSnap.modelId}`) });
  }
  if (singleSnap.mode === 'running') items.push(...runningItems(singleSnap));
  if (singleSnap.mode === 'failed' && singleSnap.failedError) {
    items.push({ type: 'info', label: clip(`Error: ${singleSnap.failedError}`) });
  }
  const distributedFailed = distributed?.report.state === 'failed';
  if (distributed && distributedFailed) {
    items.push({
      type: 'info',
      label: clip(`The split failed: ${distributed.report.lastError ?? 'no error was reported'}`),
    });
  }
  items.push({ type: 'separator' });
  items.push({
    type: 'action',
    label: 'Open Providers',
    action: 'open-providers',
    enabled: options.canAct,
  });
  if (singleSnap.mode === 'running' || singleSnap.mode === 'mounting') {
    items.push(...stopItems(singleSnap, 'Unmount the MLX engine', 'unmount', options.canAct));
  } else {
    items.push({
      type: 'action',
      label: options.mountModelId
        ? clip(`Mount ${shortModel(options.mountModelId)}`)
        : 'Mount (pick a model in Providers first)',
      action: 'mount',
      enabled: options.canAct && options.mountModelId != null,
    });
  }
  const single = mlxTrayTitle(singleSnap);
  if (single) return { title: single, phase: singlePhase, items };
  return distributedFailed
    ? { title: 'Split failed', phase: 'failed', items }
    : { title: '', phase: null, items };
}
