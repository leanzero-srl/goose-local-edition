/**
 * What the renderer hands MAIN while it brings back what served before the relaunch, so the
 * menu-bar tray says it too (main owns no ACP client). Validated on arrival — IPC is a trust
 * boundary. `null` = nothing is being restored and nothing failed to be.
 */
export interface MlxRestoreReport {
  phase: 'restoring' | 'failed';
  /** "single" | "remoteSingle" | "split"; null when the record itself could not be read. */
  kind: string | null;
  modelId: string | null;
  /** The linked Mac a remote single runs on, by its one name. */
  peerName: string | null;
  /**
   * Why it could not be restored, in goose's words; while restoring, what the restore waits on
   * (the previous split still shutting down), else null.
   */
  reason: string | null;
}

const strOrNull = (v: unknown) => v === null || typeof v === 'string';

export function isMlxRestoreReport(value: unknown): value is MlxRestoreReport | null {
  if (value === null) return true;
  if (typeof value !== 'object' || value == null) return false;
  const r = value as Record<string, unknown>;
  return (
    (r.phase === 'restoring' || r.phase === 'failed') &&
    strOrNull(r.kind) &&
    strOrNull(r.modelId) &&
    strOrNull(r.peerName) &&
    strOrNull(r.reason)
  );
}

function where(report: MlxRestoreReport): string {
  const model = report.modelId ? report.modelId.split('/').pop() || report.modelId : 'the model';
  if (report.kind === 'remoteSingle') return `${model} on ${report.peerName ?? 'the other Mac'}`;
  if (report.kind === 'split') return `${model} across your Macs`;
  return `${model} on this Mac`;
}

/** The tray's line: what is coming back and where, or why it did not. */
export function restoreTrayLine(report: MlxRestoreReport): string {
  if (report.phase === 'restoring') return report.reason ?? `Restoring ${where(report)}…`;
  if (report.kind == null) {
    return `Could not read what served before the relaunch: ${report.reason ?? 'no reason given'}`;
  }
  return `Could not restore ${where(report)}: ${report.reason ?? 'no reason given'}`;
}
