import { useCallback, useEffect, useRef } from 'react';
import { mlxDistributedStatus, type MlxDistributedStatus } from '../../acp/mlx-distributed';
import { mlxErrorMessage } from './mlxErrorMessage';
import { MLX_STATUS_POLL_MS } from './mlxLiveStats';
import { createSharedPoll, useSharedPoll } from './sharedStatusPoll';

/**
 * The split's status (`mlxEngine/distributedStatus`) — ONE read per tick for every watcher in this
 * window (Q-208): the Engine tab, My Macs' own card and the tray reporter while the split owns this
 * Mac used to run three timers on it. Every read also lands in `latestMlxDistributedStatus` (inside
 * `mlxDistributedStatus`), which the passive readers follow.
 */
export const distributedStatusPoll = createSharedPoll<MlxDistributedStatus>(() =>
  mlxDistributedStatus()
);

/**
 * The Engine tab's read of the distributed engine, on the view's own 2-second cadence while the
 * window is visible. Truth rule: a failed read INVALIDATES the previous status — the mode, the
 * nodes and every figure vanish with the fact that ended them, and the error says why.
 */
export function useMlxDistributedStatus(enabled: boolean): {
  status: MlxDistributedStatus | null;
  error: string | null;
  refresh: () => Promise<void>;
} {
  const snap = useSharedPoll(distributedStatusPoll, enabled ? { intervalMs: MLX_STATUS_POLL_MS } : null);
  const enabledRef = useRef(enabled);
  useEffect(() => {
    enabledRef.current = enabled;
  }, [enabled]);
  const refresh = useCallback(
    () => (enabledRef.current ? distributedStatusPoll.refresh() : Promise.resolve()),
    []
  );
  return {
    status: snap?.value ?? null,
    error: snap?.failed ? mlxErrorMessage(snap.error, 'Could not read the split’s status.') : null,
    refresh,
  };
}
