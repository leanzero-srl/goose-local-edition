import type { MlxDistributedStatusDto } from '@aaif/goose-sdk';
import {
  gib,
  layerSpan,
  modeSummary,
  nodeLoadProgress,
  nodeStartWord,
  planForRank,
  isAlarmOrNotice,
  type LayerSpan,
  type LoadProgress,
} from '../components/leanzero-swarm/mlxDistributed';

/**
 * What the renderer hands MAIN about the distributed engine after every ACP status read (main owns
 * no ACP client). A projection of `distributedStatus` — only the facts the menu-bar tray prints —
 * validated field by field on arrival, because IPC is a trust boundary.
 */

export interface MlxDistributedReportNode {
  name: string;
  rank: number;
  state: string;
  layers: LayerSpan | null;
  peakGb: number | null;
  availableGb: number | null;
  totalGb: number | null;
  /** From the last preflight's plan for this rank — the only reported budget. */
  budgetGb: number | null;
  pressure: string | null;
  memoryError: string | null;
  /** While the rank loads: the backend's measured progress (null = no figure reported). */
  load: LoadProgress | null;
  /** `nodeStartWord`: makingRoom / warming while it starts, else `state`. */
  startWord: string;
}

/** The rank THIS Mac serves for another Mac's distributed engine over LeanZero Link. */
export interface MlxDistributedReportHosting {
  rank: number;
  requester: string;
  modelId: string;
  backend: string | null;
  /** "loading" | "serving". */
  state: string;
  load: LoadProgress | null;
}

export interface MlxDistributedReport {
  mode: 'single' | 'distributed';
  state: string;
  backend: string | null;
  modelId: string | null;
  /** The running nodes while distributed, else the configured ones. */
  nodeNames: string[];
  nodes: MlxDistributedReportNode[];
  admissionOpen: boolean;
  inflight: number | null;
  restarts: number;
  lastError: string | null;
  /** The newest event that needed the supervisor (a restart, a hang, a repair, a hold). */
  lastAlarm: { kind: string; node: string | null; message: string } | null;
  /** Set while this Mac serves a rank of another Mac's engine (then `mode` is 'single'). */
  hosting: MlxDistributedReportHosting | null;
}

export function toMlxDistributedReport(status: MlxDistributedStatusDto): MlxDistributedReport {
  const summary = modeSummary(status);
  const alarm = [...status.events].reverse().find((e) => isAlarmOrNotice(e.kind)) ?? null;
  return {
    mode: status.mode === 'distributed' ? 'distributed' : 'single',
    state: status.state,
    backend: status.backend ?? status.config?.backend ?? null,
    modelId: status.modelId ?? null,
    nodeNames:
      summary.mode === 'distributed'
        ? summary.nodeNames
        : (status.config?.nodes ?? []).map((n) => n.name),
    nodes: status.nodes.map((n) => {
      const plan = planForRank(status, n.rank);
      return {
        name: n.name,
        rank: n.rank,
        state: n.state,
        layers: layerSpan(n),
        peakGb: n.peakMemoryGb ?? null,
        availableGb: n.availableMemoryGb ?? null,
        totalGb: n.totalMemoryGb ?? null,
        budgetGb: plan ? gib(plan.budgetBytes) : null,
        pressure: n.pressure ?? null,
        memoryError: n.memoryError ?? null,
        load: nodeLoadProgress(n),
        startWord: nodeStartWord(status, n),
      };
    }),
    admissionOpen: status.admissionOpen,
    inflight: status.inflight ?? null,
    restarts: status.restarts,
    lastError: status.lastError ?? null,
    lastAlarm: alarm
      ? { kind: alarm.kind, node: alarm.node ?? null, message: alarm.message }
      : null,
    hosting: status.hosting
      ? {
          rank: status.hosting.rank,
          requester: status.hosting.requesterName,
          modelId: status.hosting.modelId,
          backend: status.hosting.backend ?? null,
          state: status.hosting.state,
          load: nodeLoadProgress(status.hosting),
        }
      : null,
  };
}

const isStr = (v: unknown): v is string => typeof v === 'string';
const isNum = (v: unknown): v is number => typeof v === 'number' && Number.isFinite(v);
const strOrNull = (v: unknown) => v === null || isStr(v);
const numOrNull = (v: unknown) => v === null || isNum(v);

function isLayerSpan(v: unknown): boolean {
  if (v === null) return true;
  if (typeof v !== 'object' || v === undefined) return false;
  const s = v as Record<string, unknown>;
  if (s.kind === 'layers') return isNum(s.first) && isNum(s.last) && isNum(s.count);
  if (s.kind === 'shard') return isNum(s.index) && isNum(s.count);
  return false;
}

function isLoad(v: unknown): boolean {
  if (v === null) return true;
  if (v == null || typeof v !== 'object') return false;
  const l = v as Record<string, unknown>;
  return l.unit === 'bytes' && isNum(l.done) && isNum(l.total);
}

function isReportNode(v: unknown): boolean {
  if (v == null || typeof v !== 'object') return false;
  const n = v as Record<string, unknown>;
  return (
    isStr(n.name) &&
    isNum(n.rank) &&
    isStr(n.state) &&
    isLayerSpan(n.layers) &&
    numOrNull(n.peakGb) &&
    numOrNull(n.availableGb) &&
    numOrNull(n.totalGb) &&
    numOrNull(n.budgetGb) &&
    strOrNull(n.pressure) &&
    strOrNull(n.memoryError) &&
    isLoad(n.load) &&
    isStr(n.startWord)
  );
}

function isHosting(v: unknown): boolean {
  if (v === null) return true;
  if (v == null || typeof v !== 'object') return false;
  const h = v as Record<string, unknown>;
  return (
    isNum(h.rank) &&
    isStr(h.requester) &&
    isStr(h.modelId) &&
    strOrNull(h.backend) &&
    isStr(h.state) &&
    isLoad(h.load)
  );
}

/** An IPC payload is a report only if every field is what `toMlxDistributedReport` builds. */
export function isMlxDistributedReport(value: unknown): value is MlxDistributedReport {
  if (value == null || typeof value !== 'object') return false;
  const r = value as Record<string, unknown>;
  const alarm = r.lastAlarm as Record<string, unknown> | null | undefined;
  return (
    isHosting(r.hosting) &&
    (r.mode === 'single' || r.mode === 'distributed') &&
    isStr(r.state) &&
    strOrNull(r.backend) &&
    strOrNull(r.modelId) &&
    Array.isArray(r.nodeNames) &&
    r.nodeNames.every(isStr) &&
    Array.isArray(r.nodes) &&
    r.nodes.every(isReportNode) &&
    typeof r.admissionOpen === 'boolean' &&
    numOrNull(r.inflight) &&
    isNum(r.restarts) &&
    strOrNull(r.lastError) &&
    (alarm === null ||
      (alarm != null &&
        typeof alarm === 'object' &&
        isStr(alarm.kind) &&
        strOrNull(alarm.node) &&
        isStr(alarm.message)))
  );
}
