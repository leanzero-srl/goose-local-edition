import type { MlxRemoteSingleStatusDto } from '@aaif/goose-sdk';

/**
 * What the renderer hands MAIN about REMOTE SINGLE after every status read (main owns no ACP
 * client): where this Mac's MLX chat goes. Validated on arrival — IPC is a trust boundary.
 * `null` = no route (chat stays on this Mac's own engine).
 */
export interface MlxRemoteReport {
  /** "mounting" | "ready" | "failed". */
  state: string;
  peerHostname: string;
  modelId: string | null;
  /** The peer engine's own decode rate over its last generation. */
  generationTps: number | null;
  activeRequests: number | null;
  lastError: string | null;
}

export function toMlxRemoteReport(status: MlxRemoteSingleStatusDto | null): MlxRemoteReport | null {
  if (!status || status.state === 'off' || !status.peerHostname) return null;
  return {
    state: status.state,
    peerHostname: status.peerHostname,
    modelId: status.modelId ?? null,
    generationTps: status.generationTps ?? null,
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
    isStr(r.peerHostname) &&
    strOrNull(r.modelId) &&
    numOrNull(r.generationTps) &&
    numOrNull(r.activeRequests) &&
    strOrNull(r.lastError)
  );
}

/** "Serving from <peer>" — the one phrase every surface uses for a remote route. */
export function servingFromLabel(peerHostname: string): string {
  return `Serving from ${peerHostname}`;
}

/** The tray's line for a live route: where, which model, and how it is doing. */
export function remoteTrayLine(report: MlxRemoteReport): string {
  const model = report.modelId ? ` · ${report.modelId.split('/').pop()}` : '';
  const state =
    report.state === 'ready'
      ? report.generationTps != null && report.generationTps > 0
        ? ` · ${report.generationTps.toFixed(1)} tok/s last reply`
        : ' · ready'
      : ` · ${report.state}`;
  return `${servingFromLabel(report.peerHostname)}${model}${state}`;
}
