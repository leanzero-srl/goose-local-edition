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
  /**
   * failed only: what goose already served when the restore gave up (`servingKey`s). An engine
   * serving now that is NOT in this list is one the owner chose afterwards, and it supersedes the
   * failure (`restoreSuperseded`). Absent = nothing was read, so any serving engine supersedes.
   */
  servingAtFailure?: string[];
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
    strOrNull(r.reason) &&
    (r.servingAtFailure === undefined ||
      (Array.isArray(r.servingAtFailure) &&
        r.servingAtFailure.every((k: unknown) => typeof k === 'string')))
  );
}

/** One serving engine of goose's, the same words on both sides of the IPC. */
export function servingKey(
  kind: 'single' | 'remoteSingle' | 'split',
  modelId: string | null
): string {
  return `${kind}:${modelId ?? ''}`;
}

/**
 * Q-166: a failed restore is superseded by ANY engine serving now that was not serving when the
 * restore gave up — the owner chose it afterwards, and "Could not restore …" beside it is a claim the
 * tile contradicts (owner defect #5 of 2026-09-25). Not only the same model: a line that clears only
 * when the model it names serves stayed up 20 minutes above a serving 27B split (3.0.57). The
 * renderer's line (mlxRestore.ts `settleRestoreLine`) and the tray (mlxTray.ts) both decide here.
 */
export function restoreSuperseded(
  report: Pick<MlxRestoreReport, 'phase' | 'servingAtFailure'>,
  servingNow: readonly string[]
): boolean {
  if (report.phase !== 'failed') return false;
  const before = report.servingAtFailure ?? [];
  return servingNow.some((key) => !before.includes(key));
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
