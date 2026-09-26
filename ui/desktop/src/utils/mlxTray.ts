import {
  bookSpreads,
  MLX_STATUS_POLL_MS,
  compactTokens,
  formatElapsed,
  formatRate,
  liveDecodeTps,
  measuredPrefillTps,
  mlxActivity,
  readingNowTps,
  type MlxLiveStats,
  type RateSpread,
} from '../components/leanzero-swarm/mlxLiveStats';
import {
  backendName,
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
import { restoreTrayLine, type MlxRestoreReport } from './mlxRestoreReport';

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
  | { type: 'info'; label: string; phase?: EnginePhase }
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

/** The composer bar's steady words for a Mac whose goose is gone (Q-111), in the tray's English. */
export function peerGoneText(mac: string): string {
  return `${mac}’s goose isn’t running`;
}

/**
 * The route's Mac is gone, not a blip (routeContact.ts `routePeerGone`): a steady state with the
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
  const why =
    gone.because === 'said-quit'
      ? `${mac} quit goose`
      : `No answer for ${formatElapsed(gone.lostForMs / 1000)}`;
  const items: MlxTrayItem[] = [
    { type: 'info', label: clip(peerGoneText(mac)), phase },
    { type: 'info', label: clip(`${why} — open goose there, or run chat on this Mac`) },
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
  return { title: peerGoneText(mac), phase, items };
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
    {
      type: 'action',
      label: clip(`Stop serving from ${remote.peerName}`),
      action: 'stop-remote',
      enabled: canAct,
    }
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

function plural(n: number, one: string, many: string): string {
  return `${n.toLocaleString()} ${n === 1 ? one : many}`;
}

/** The request still reading its prompt that has waited longest — what the title names. */
function readingRequest(stats: MlxLiveStats) {
  return stats.requests
    .filter((r) => r.status !== 'waiting' && r.phase === 'prefill')
    .sort((a, b) => (b.elapsedS ?? 0) - (a.elapsedS ?? 0))[0];
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
  switch (mlxActivity(stats)) {
    case 'generating': {
      const rate = liveDecodeTps(stats);
      return rate > 0 ? `${formatRate(rate)} tok/s` : 'Writing';
    }
    case 'prefill': {
      const r = readingRequest(stats);
      return r?.promptTokens != null ? `Reading ${compactTokens(r.promptTokens)}` : 'Reading';
    }
    case 'queued':
      return `Queued ${stats.requests.length}`;
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
    idle: 'idle',
    not_loaded: 'running, no model loaded',
  }[mlxActivity(snapshot.stats)];
  return `LeanZero MLX: ${word}`;
}

export function clientLabel(client: MlxClient): string {
  const times = client.count > 1 ? ` (×${client.count})` : '';
  switch (client.kind) {
    case 'chat':
      return clip(`Serving chat: ${client.sessionName || client.sessionId}${times}`);
    case 'external':
      return clip(`Serving an external client via /v1: ${client.model}${times}`);
    case 'session': {
      const type = client.sessionType ? client.sessionType.replace(/_/g, ' ') : 'goose';
      const name = client.sessionName || client.sessionId || 'no session';
      return clip(`Serving a ${type} session: ${name}${times}`);
    }
  }
}

function servingItems(serving: MlxServing | null): MlxTrayItem[] {
  if (!serving) return [];
  const items: MlxTrayItem[] = serving.clients.map((c) => ({
    type: 'info' as const,
    label: clientLabel(c),
  }));
  if (serving.unattributed > 0) {
    items.push({
      type: 'info',
      label: `${plural(serving.unattributed, 'request', 'requests')} not from this app's chats or /v1`,
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
    const { writing, reading } = bookSpreads(snapshot.rates);
    const spread = (s: RateSpread) =>
      s.max > s.min
        ? `${formatRate(s.median)} tok/s (${formatRate(s.min)}–${formatRate(s.max)})`
        : `${formatRate(s.median)} tok/s`;
    const runs = Math.max(writing?.runs ?? 0, reading?.runs ?? 0);
    if (writing || reading) {
      const parts = [
        writing ? `writes ${spread(writing)}` : null,
        reading ? `reads ${spread(reading)}` : null,
      ].filter(Boolean);
      const over = runs >= 2 ? `Median of ${runs} runs` : '1 run';
      items.push({ type: 'info', label: `${over}: ${parts.join(', ')}` });
    }
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

/** "Distributed · MacBook Pro + workhorse · JACCL" — the mode line the tile and the tab say too. */
export function distributedModeLine(report: MlxDistributedReport): string {
  const nodes =
    report.nodeNames.length > 0 ? report.nodeNames.join(' + ') : `${report.nodes.length} nodes`;
  const backend = backendName(report.backend);
  return clip(['Distributed', nodes, backend].filter(Boolean).join(' · '));
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
        ? `peak ${gb1(node.peakGb)} of ${gb1(node.budgetGb)} GiB budget`
        : `peak ${gb1(node.peakGb)} GiB`
      : node.availableGb != null
        ? `${gb1(node.availableGb)} GiB available`
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

/** The title while the distributed engine owns this Mac. */
export function distributedTrayTitle(
  d: NonNullable<MlxTrayOptions['distributed']>,
  live: MlxEngineSnapshot | null = null
): string {
  const { report } = d;
  if (distributedStale(d)) return 'Dist · stale';
  if (!report.admissionOpen) return 'Dist · held';
  if (live) return `Dist · ${mlxTrayTitle(live)}`;
  if (report.state === 'serving') {
    return report.inflight != null ? `Dist · ${report.inflight} in flight` : 'Dist · serving';
  }
  return `Dist · ${report.state}`;
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
      label: `LeanZero MLX: distributed, ${report.state}`,
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
  if (report.lastAlarm) {
    const where = report.lastAlarm.node ? ` on ${report.lastAlarm.node}` : '';
    items.push({
      type: 'info',
      label: clip(`Last: ${report.lastAlarm.kind}${where} — ${report.lastAlarm.message}`),
    });
  }
  if (report.lastError) items.push({ type: 'info', label: clip(`Error: ${report.lastError}`) });
  if (distributedStale(d)) {
    items.push({
      type: 'info',
      label: `Not refreshed for ${ageText(d.ageMs)} — open goose to read it again`,
    });
  }
  return items;
}

/** "Rank 1 of MacBook Pro's distributed engine · JACCL" (the model has its own line below). */
export function hostingLine(hosting: MlxDistributedReportHosting): string {
  const backend = backendName(hosting.backend);
  return clip(
    [`Rank ${hosting.rank} of ${hosting.requester}'s distributed engine`, backend]
      .filter(Boolean)
      .join(' · ')
  );
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
  const restore = options.restore ?? null;
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
        {
          type: 'action',
          label: 'Stop the distributed engine',
          action: 'stop-distributed',
          enabled: options.canAct,
        },
      ],
    };
  }
  if (options.remote) {
    return remoteModel(snapshot, options.remote, options.canAct, options.mountModelId);
  }
  // A read of the distributed rank 0 never speaks for the single engine (a run that just stopped).
  const singleSnap = snapshot.engine === 'single' ? snapshot : INITIAL_SNAPSHOT;
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
      label: clip(
        `Distributed engine failed: ${distributed.report.lastError ?? 'no error was reported'}`
      ),
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
    items.push({
      type: 'action',
      label: 'Unmount the MLX engine',
      action: 'unmount',
      enabled: options.canAct,
    });
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
    ? { title: 'Dist failed', phase: 'failed', items }
    : { title: '', phase: null, items };
}
