import { mlxEngineStatus, type MlxEngineStatus } from '../../acp/mlx-engine';
import { errorMessage } from '../../utils/conversionUtils';
import { createSharedPoll, useSharedPoll, type PollSnapshot } from './sharedStatusPoll';

/**
 * THIS Mac's engine status (`mlxEngine/status`, no node id) — ONE read per tick for the whole window
 * (Q-208): the composer's chip, model bar and no-node notice, the Nodes table, the fleet's
 * corroboration, the Engine tab and My Macs' own card all watch this store instead of each running
 * a timer. The argument is the Engine tab's `fitModelId`: the fit is an additive field of the same
 * answer, so every other watcher reads the answer unchanged.
 */
export const localEngineStatusPoll = createSharedPoll<MlxEngineStatus, string>((fitModelId) =>
  fitModelId != null ? mlxEngineStatus(undefined, fitModelId) : mlxEngineStatus()
);

/** Read this Mac's engine now, for every watcher (after a mount, a stop, a settings save). */
export function refreshLocalEngineStatus(): Promise<void> {
  return localEngineStatusPoll.refresh();
}

/**
 * Watch this Mac's engine status every `intervalMs` (the store reads at the shortest interval any
 * watcher asks for), with `fitModelId`'s fit in the answer when given; null = not watching.
 */
export function useLocalEngineStatus(
  need: { intervalMs: number; fitModelId?: string | null } | null
): PollSnapshot<MlxEngineStatus> | null {
  return useSharedPoll(
    localEngineStatusPoll,
    need ? { intervalMs: need.intervalMs, arg: need.fitModelId ?? null } : null
  );
}

/**
 * Light MLX engine status poll shared by the chat model selector surfaces.
 *
 * Truth rules: a failed status read INVALIDATES the previous status (state claims are never
 * kept alive past the liveness fact that ended them), and polling stops entirely while the
 * document is hidden or `enabled` is false — no background chatter from closed surfaces.
 */
export function useMlxEngineStatusPoll(
  enabled: boolean,
  intervalMs = 5000
): { status: MlxEngineStatus | null; error: string | null } {
  const snap = useLocalEngineStatus(enabled ? { intervalMs } : null);
  return {
    status: snap?.value ?? null,
    error:
      snap?.failed ? errorMessage(snap.error, 'Could not read the MLX engine status.') : null,
  };
}
