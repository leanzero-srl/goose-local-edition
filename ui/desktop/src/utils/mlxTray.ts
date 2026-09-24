import {
  MLX_STATUS_POLL_MS,
  compactTokens,
  formatElapsed,
  formatRate,
  liveDecodeTps,
  measuredPrefillTps,
  mlxActivity,
  type MlxLiveStats,
} from '../components/leanzero-swarm/mlxLiveStats';
import { backendName, gb1, layerSpanShort } from '../components/leanzero-swarm/mlxDistributed';
import type { MlxEngineSnapshot } from './mlxEngineMonitor';
import type {
  MlxDistributedReport,
  MlxDistributedReportHosting,
  MlxDistributedReportNode,
} from './mlxDistributedReport';
import type { MlxClient, MlxServing } from './mlxServing';
import { remoteTrayLine, type MlxRemoteReport } from './mlxRemoteReport';

/**
 * The menu-bar presence of the local LeanZero MLX engine, as a PURE function of main's snapshot:
 * the short title beside the tray icon (macOS `Tray.setTitle`) and the menu's engine section. main.ts
 * turns the descriptors into Electron menu items; nothing here touches Electron, so every state is
 * tested as data. Every figure is one the monitor measured — the same derivations as the state tile.
 */

export type MlxTrayAction = 'open-providers' | 'mount' | 'unmount' | 'stop-distributed';

export type MlxTrayItem =
  | { type: 'info'; label: string }
  | { type: 'action'; label: string; action: MlxTrayAction; enabled: boolean }
  | { type: 'separator' };

export interface MlxTrayModel {
  /** The text beside the icon; empty when there is no engine to speak of. */
  title: string;
  items: MlxTrayItem[];
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
}

function remoteTrayTitle(report: MlxRemoteReport): string {
  if (report.state === 'ready') {
    return report.generationTps != null && report.generationTps > 0
      ? `Remote · ${formatRate(report.generationTps)} tok/s`
      : 'Remote';
  }
  return report.state === 'failed' ? 'Remote failed' : `Remote · ${report.state}`;
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
    const elapsed = reading.elapsedS != null ? ` for ${formatElapsed(reading.elapsedS)}` : '';
    items.push({
      type: 'info',
      label: `Reading a ${compactTokens(reading.promptTokens)}-token prompt${cached}${elapsed}`,
    });
  }
  if (prefill > 0) {
    items.push({ type: 'info', label: `Read the last prompt at ${formatRate(prefill)} tok/s` });
  }
  if (activity !== 'generating' && decode === 0 && prefill === 0) {
    const { decodeTps, prefillTps } = snapshot.last;
    if (decodeTps != null || prefillTps != null) {
      const parts = [
        decodeTps != null ? `wrote ${formatRate(decodeTps)} tok/s` : null,
        prefillTps != null ? `read ${formatRate(prefillTps)} tok/s` : null,
      ].filter(Boolean);
      items.push({ type: 'info', label: `Last run: ${parts.join(', ')}` });
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

export function distributedNodeLine(node: MlxDistributedReportNode, runState: string): string {
  const head = node.state === runState ? node.name : `${node.name} (${node.state})`;
  if (node.memoryError) return clip(`${head}: memory unread — ${node.memoryError}`);
  const parts = [
    layerSpanShort(node.layers),
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

/** The title while the distributed engine owns this Mac. */
export function distributedTrayTitle(d: NonNullable<MlxTrayOptions['distributed']>): string {
  const { report } = d;
  if (distributedStale(d)) return 'Dist · stale';
  if (!report.admissionOpen) return 'Dist · held';
  if (report.state === 'serving') {
    return report.inflight != null ? `Dist · ${report.inflight} in flight` : 'Dist · serving';
  }
  return `Dist · ${report.state}`;
}

function distributedItems(d: NonNullable<MlxTrayOptions['distributed']>): MlxTrayItem[] {
  const { report } = d;
  const items: MlxTrayItem[] = [
    { type: 'info', label: `LeanZero MLX: distributed, ${report.state}` },
    { type: 'info', label: distributedModeLine(report) },
  ];
  if (report.modelId) items.push({ type: 'info', label: clip(`Model: ${report.modelId}`) });
  for (const node of report.nodes) {
    items.push({ type: 'info', label: distributedNodeLine(node, report.state) });
  }
  if (report.state === 'serving' || report.state === 'ready') {
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

export function buildMlxTrayModel(
  snapshot: MlxEngineSnapshot,
  options: MlxTrayOptions
): MlxTrayModel {
  const distributed = options.distributed;
  const hosting = distributed?.report.mode === 'single' ? distributed.report.hosting : null;
  if (hosting && distributed) {
    // This Mac serves a rank of ANOTHER Mac's engine over LeanZero Link: the single engine is
    // refused meanwhile (goose's `hostingRank`), so no Mount is offered; the run is stopped from
    // the Mac that started it.
    return {
      title: distributedStale(distributed)
        ? 'Rank · stale'
        : `Rank ${hosting.rank} · ${hosting.state}`,
      items: [
        { type: 'info', label: `LeanZero MLX: serving a rank, ${hosting.state}` },
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
    // the menu speaks for the distributed run and offers its Stop instead of Mount.
    return {
      title: distributedTrayTitle(distributed),
      items: [
        ...distributedItems(distributed),
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
  const remote = options.remote ?? null;
  const items: MlxTrayItem[] = [];
  if (remote) {
    // Chat goes to a peer's engine: that is the line that matters; this Mac's own engine follows.
    items.push({ type: 'info', label: clip(remoteTrayLine(remote)) });
    if (remote.lastError) items.push({ type: 'info', label: clip(`Error: ${remote.lastError}`) });
  }
  items.push({ type: 'info', label: headline(snapshot) });
  if (distributed) items.push({ type: 'info', label: 'Single · this Mac' });
  if (snapshot.modelId && snapshot.mode !== 'off') {
    items.push({ type: 'info', label: clip(`Model: ${snapshot.modelId}`) });
  }
  if (snapshot.mode === 'running') items.push(...runningItems(snapshot));
  if (snapshot.mode === 'failed' && snapshot.failedError) {
    items.push({ type: 'info', label: clip(`Error: ${snapshot.failedError}`) });
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
  if (snapshot.mode === 'running' || snapshot.mode === 'mounting') {
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
  const single = mlxTrayTitle(snapshot);
  if (remote) return { title: remoteTrayTitle(remote), items };
  return { title: single || (distributedFailed ? 'Dist failed' : ''), items };
}
