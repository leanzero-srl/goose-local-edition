import type { MlxRemoteSingleStatusDto } from '@aaif/goose-sdk';
import { routePeerName } from '../components/leanzero-swarm/macs';

/**
 * What the renderer hands MAIN about REMOTE SINGLE after every status read (main owns no ACP
 * client): where this Mac's MLX chat goes. Validated on arrival — IPC is a trust boundary.
 * `null` = no route (chat stays on this Mac's own engine).
 */
export interface MlxRemoteReport {
  /** "mounting" | "ready" | "failed". */
  state: string;
  /** The Mac chat is served from, named the one way (`routePeerName`). */
  peerName: string;
  modelId: string | null;
  /**
   * goosed's loopback relay to the peer's engine: main reads `<baseUrl>/v1/status` exactly as it
   * reads a local engine (the tray's live lines). null = the backend did not hand one over.
   */
  baseUrl: string | null;
  activeRequests: number | null;
  lastError: string | null;
}

export function toMlxRemoteReport(status: MlxRemoteSingleStatusDto | null): MlxRemoteReport | null {
  if (!status || status.state === 'off' || !(status.peerHostname || status.peer)) return null;
  return {
    state: status.state,
    peerName: routePeerName(status),
    modelId: status.modelId ?? null,
    baseUrl: status.baseUrl ?? null,
    activeRequests: status.activeRequests ?? null,
    lastError: status.lastError ?? null,
  };
}

const isStr = (v: unknown): v is string => typeof v === 'string';
const strOrNull = (v: unknown) => v === null || isStr(v);
const numOrNull = (v: unknown) => v === null || (typeof v === 'number' && Number.isFinite(v));

export function isMlxRemoteReport(value: unknown): value is MlxRemoteReport | null {
  if (value === null) return true;
  if (typeof value !== 'object' || value == null) return false;
  const r = value as Record<string, unknown>;
  return (
    isStr(r.state) &&
    isStr(r.peerName) &&
    strOrNull(r.modelId) &&
    strOrNull(r.baseUrl) &&
    numOrNull(r.activeRequests) &&
    strOrNull(r.lastError)
  );
}

/** The base main's loop reads while the route serves; null while it mounts, failed or has none. */
export function remoteLiveBase(report: MlxRemoteReport | null): string | null {
  return report?.state === 'ready' ? report.baseUrl : null;
}

/** "Serving from <peer>" — the one phrase every surface uses for a remote route. */
export function servingFromLabel(peerName: string): string {
  return `Serving from ${peerName}`;
}

/** The tray's line for a route: where, which model, and — until it serves — its state. */
export function remoteTrayLine(report: MlxRemoteReport): string {
  const model = report.modelId ? ` · ${report.modelId.split('/').pop()}` : '';
  const state = report.state === 'ready' ? '' : ` · ${report.state}`;
  return `${servingFromLabel(report.peerName)}${model}${state}`;
}
