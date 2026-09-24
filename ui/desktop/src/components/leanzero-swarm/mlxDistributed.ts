import type {
  MlxDistributedConfigDto,
  MlxDistributedNodeConfigDto,
  MlxDistributedRankPlanDto,
  MlxDistributedStatusDto,
} from '@aaif/goose-sdk';
import type { Tone } from '../lz/tokens';

/**
 * Pure derivations over the distributed engine's DTOs, shared by the Engine tab's Distributed
 * section, the state tile and the menu-bar tray (main imports this file, so nothing here touches
 * React, intl or the DOM). Every figure is one the backend reported; an absent one stays absent.
 */

export const GIB = 1024 * 1024 * 1024;

/** Which engine owns this Mac — the backend computes it (`mode`), the UI never re-derives it. */
export function ownsTheMac(status: Pick<MlxDistributedStatusDto, 'mode'> | null): boolean {
  return status?.mode === 'distributed';
}

/** The transport's proper name. An unknown value is shown as sent, never guessed at. */
export function backendName(backend: string | null | undefined): string | null {
  if (!backend) return null;
  if (backend === 'jaccl') return 'JACCL';
  return backend;
}

export type MlxModeSummary =
  | { mode: 'single' }
  | { mode: 'distributed'; nodeNames: string[]; backend: string | null };

/** What the mode line says: the running nodes while distributed, else the single engine. */
export function modeSummary(
  status: Pick<MlxDistributedStatusDto, 'mode' | 'nodes' | 'backend' | 'config'> | null
): MlxModeSummary {
  if (!status || !ownsTheMac(status)) return { mode: 'single' };
  const running = status.nodes.map((n) => n.name);
  const nodeNames = running.length > 0 ? running : (status.config?.nodes ?? []).map((n) => n.name);
  return {
    mode: 'distributed',
    nodeNames,
    backend: backendName(status.backend ?? status.config?.backend),
  };
}

/**
 * The Distributed section's own headline: the engine as it is configured (or, while it owns the
 * Mac, as it runs) — `null` when nothing is configured, so the section never borrows the single
 * engine's words.
 */
export function configuredModeSummary(
  status: Pick<MlxDistributedStatusDto, 'mode' | 'nodes' | 'backend' | 'config'> | null
): MlxModeSummary | null {
  if (!status) return null;
  if (ownsTheMac(status)) return modeSummary(status);
  const config = status.config;
  if (!config) return null;
  return {
    mode: 'distributed',
    nodeNames: config.nodes.map((n) => n.name),
    backend: backendName(config.backend),
  };
}

export type LayerSpan =
  | { kind: 'layers'; first: number; last: number; count: number }
  | { kind: 'shard'; index: number; count: number };

interface SpanFields {
  layerStart?: number | null;
  layerEnd?: number | null;
  shardIndex?: number | null;
  shardCount?: number | null;
}

/**
 * What one rank holds. Tensor split: its shard of every layer (1-based for people — the backend's
 * `shardIndex` is the rank). Pipeline split: layers [layerStart, layerEnd) — shown inclusive, L0–19.
 */
export function layerSpan(fields: SpanFields | null | undefined): LayerSpan | null {
  if (!fields) return null;
  const { layerStart, layerEnd, shardIndex, shardCount } = fields;
  if (shardIndex != null && shardCount != null) {
    return { kind: 'shard', index: shardIndex + 1, count: shardCount };
  }
  if (layerStart != null && layerEnd != null && layerEnd > layerStart) {
    return { kind: 'layers', first: layerStart, last: layerEnd - 1, count: layerEnd - layerStart };
  }
  return null;
}

/** "L0–19" / "shard 1/2" — the compact form the tray and the node strip share. */
export function layerSpanShort(span: LayerSpan | null): string | null {
  if (!span) return null;
  return span.kind === 'layers'
    ? `L${span.first}–${span.last}`
    : `shard ${span.index}/${span.count}`;
}

/** The rank's plan from the last preflight — the only place a memory BUDGET is reported. */
export function planForRank(
  status: Pick<MlxDistributedStatusDto, 'lastPreflight'> | null,
  rank: number
): MlxDistributedRankPlanDto | null {
  return status?.lastPreflight?.nodes.find((n) => n.rank === rank)?.plan ?? null;
}

export function gib(bytes: number): number {
  return bytes / GIB;
}

/** One decimal, the precision the recorded peaks carry (61.0 / 83.4). */
export function gb1(value: number): string {
  return value.toFixed(1);
}

// The run: slate at rest (ready), blue while in flight (preflight/starting/stopping), green while
// it serves, red when it failed — the tile's own register.
export function runStateTone(state: string): Tone {
  switch (state) {
    case 'serving':
      return 'ok';
    case 'preflight':
    case 'starting':
    case 'stopping':
      return 'accent';
    case 'failed':
      return 'err';
    default:
      return 'stopped';
  }
}

export function runStateInFlight(state: string): boolean {
  return state === 'preflight' || state === 'starting' || state === 'stopping';
}

export function nodeStateTone(state: string): Tone {
  switch (state) {
    case 'serving':
      return 'ok';
    case 'preflight':
    case 'loading':
      return 'accent';
    case 'failed':
      return 'err';
    default:
      return 'stopped';
  }
}

export function verdictTone(verdict: string): Tone {
  if (verdict === 'pass') return 'ok';
  if (verdict === 'warn') return 'warn';
  return 'err';
}

export function pressureTone(pressure: string | null | undefined): Tone | null {
  if (pressure === 'normal') return 'ok';
  if (pressure === 'warn') return 'warn';
  if (pressure === 'critical') return 'err';
  return null;
}

/** Events that mean something went wrong (a death, a hang, a memory stop) — red. */
const ALARM_EVENTS = new Set([
  'startFailed',
  'rankDied',
  'rankFrozen',
  'hang',
  'streamWithoutDone',
  'breakerOpen',
  'watchdogCritical',
]);
/** Events that mean the supervisor had to act or could not see (a restart, a repair, a hold). */
const NOTICE_EVENTS = new Set([
  'restart',
  'linkRepaired',
  'watchdogWarn',
  'watchdogBlind',
  'admissionClosed',
  'orphanReclaimed',
]);
const GOOD_EVENTS = new Set(['ready', 'admissionOpened', 'launched']);

export function eventTone(kind: string): Tone {
  if (ALARM_EVENTS.has(kind)) return 'err';
  if (NOTICE_EVENTS.has(kind)) return 'warn';
  if (GOOD_EVENTS.has(kind)) return 'ok';
  return 'stopped';
}

export function isAlarmOrNotice(kind: string): boolean {
  return ALARM_EVENTS.has(kind) || NOTICE_EVENTS.has(kind);
}

/**
 * Stable row keys for the bounded event window (the backend drops the OLDEST at its limit, so an
 * index would re-key every row once the window slides). Identity = the event's own fields; a
 * repeat of an identical event in the same millisecond gets its occurrence number.
 */
export function eventKeys(
  events: ReadonlyArray<{ atMs: number; kind: string; node?: string | null; message: string }>
): string[] {
  const seen = new Map<string, number>();
  return events.map((e) => {
    const base = `${e.atMs}|${e.kind}|${e.node ?? ''}|${e.message}`;
    const n = seen.get(base) ?? 0;
    seen.set(base, n + 1);
    return n === 0 ? base : `${base}#${n}`;
  });
}

// ---------------------------------------------------------------------------
// Config drafts
// ---------------------------------------------------------------------------

export type NodeTextField =
  | 'name'
  | 'ssh'
  | 'tbIp'
  | 'tbNetmask'
  | 'tbInterface'
  | 'tbService'
  | 'rdmaDevice'
  | 'python'
  | 'pipelinePython'
  | 'modelDir';

/**
 * The node fields the backend refuses empty (config.rs `validate`); ssh is required on peers and
 * the RDMA device only under JACCL (ring uses none).
 */
export const REQUIRED_NODE_FIELDS: readonly NodeTextField[] = [
  'name',
  'tbIp',
  'tbNetmask',
  'tbInterface',
  'tbService',
  'python',
  'modelDir',
];

export interface MissingField {
  /** null = a config-level field. */
  node: number | null;
  field: NodeTextField | 'modelId' | 'backend' | 'port' | 'coordinatorPort' | 'nodes';
}

/**
 * The empties the backend would refuse, so Save / Preflight / Start are not offered on a draft that
 * cannot pass. Everything else (IPv4 syntax, port clashes, the model on each node) is the backend's
 * to judge — its message is shown verbatim.
 */
export function missingFields(config: MlxDistributedConfigDto): MissingField[] {
  const missing: MissingField[] = [];
  if (!config.modelId.trim()) missing.push({ node: null, field: 'modelId' });
  if (config.backend !== 'jaccl' && config.backend !== 'ring') {
    missing.push({ node: null, field: 'backend' });
  }
  if (!config.port) missing.push({ node: null, field: 'port' });
  if (!config.coordinatorPort) missing.push({ node: null, field: 'coordinatorPort' });
  if (config.nodes.length < 2) missing.push({ node: null, field: 'nodes' });
  config.nodes.forEach((node, i) => {
    for (const field of REQUIRED_NODE_FIELDS) {
      const value = node[field];
      if (typeof value !== 'string' || !value.trim()) missing.push({ node: i, field });
    }
    if (config.backend === 'jaccl' && !node.rdmaDevice?.trim()) {
      missing.push({ node: i, field: 'rdmaDevice' });
    }
    if (i > 0 && !node.ssh?.trim()) missing.push({ node: i, field: 'ssh' });
  });
  return missing;
}

/**
 * Point the draft at another model. The model DIRECTORY is per node (paths differ per Mac): a
 * node whose directory ends in the old model id is moved to the same place under the new id; any
 * other directory is left exactly as the owner wrote it, and preflight's `model` check says whether
 * it holds the model.
 */
export function withModel(
  config: MlxDistributedConfigDto,
  modelId: string
): MlxDistributedConfigDto {
  const old = config.modelId;
  return {
    ...config,
    modelId,
    nodes: config.nodes.map((node) =>
      old && node.modelDir.endsWith(`/${old}`)
        ? { ...node, modelDir: `${node.modelDir.slice(0, -old.length)}${modelId}` }
        : node
    ),
  };
}

/** Order-insensitive deep equality over plain JSON (drafts are spread copies of the wire object). */
export function sameConfig(a: unknown, b: unknown): boolean {
  return canonical(a) === canonical(b);
}

function canonical(value: unknown): string {
  if (Array.isArray(value)) return `[${value.map(canonical).join(',')}]`;
  if (value && typeof value === 'object') {
    const entries = Object.entries(value as Record<string, unknown>)
      .filter(([, v]) => v !== undefined && v !== null)
      .sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0));
    return `{${entries.map(([k, v]) => `${JSON.stringify(k)}:${canonical(v)}`).join(',')}}`;
  }
  return JSON.stringify(value);
}

/** A peer the owner adds: every field empty, so nothing reaches the backend that he did not type. */
export function emptyNode(): MlxDistributedNodeConfigDto {
  return {
    name: '',
    ssh: '',
    tbIp: '',
    tbNetmask: '',
    tbInterface: '',
    tbService: '',
    rdmaDevice: '',
    python: '',
    modelDir: '',
  };
}

/** The wire shape of a node: optional fields that are blank are OMITTED, never sent as "". */
export function cleanNode(
  node: MlxDistributedNodeConfigDto,
  rank: number
): MlxDistributedNodeConfigDto {
  const next: MlxDistributedNodeConfigDto = { ...node };
  const ssh = node.ssh?.trim();
  if (rank === 0 || !ssh) delete next.ssh;
  else next.ssh = ssh;
  const pipeline = node.pipelinePython?.trim();
  if (!pipeline) delete next.pipelinePython;
  return next;
}

export function cleanConfig(config: MlxDistributedConfigDto): MlxDistributedConfigDto {
  const next: MlxDistributedConfigDto = {
    ...config,
    modelId: config.modelId.trim(),
    nodes: config.nodes.map((n, i) => cleanNode(n, i)),
  };
  if (next.context == null) delete next.context;
  return next;
}
