import { useCallback, useEffect, useRef, useState } from 'react';
import { mlxDistributedStatus, type MlxDistributedStatus } from '../../acp/mlx-distributed';
import { mlxErrorMessage } from './mlxErrorMessage';
import { MLX_STATUS_POLL_MS } from './mlxLiveStats';

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
  const [status, setStatus] = useState<MlxDistributedStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const enabledRef = useRef(enabled);
  useEffect(() => {
    enabledRef.current = enabled;
  }, [enabled]);
  const inFlight = useRef(false);

  const refresh = useCallback(async () => {
    if (!enabledRef.current || inFlight.current) return;
    inFlight.current = true;
    try {
      const next = await mlxDistributedStatus();
      if (!enabledRef.current) return;
      setStatus(next);
      setError(null);
    } catch (e) {
      if (!enabledRef.current) return;
      setStatus(null);
      setError(mlxErrorMessage(e, 'Could not read the distributed engine status.'));
    } finally {
      inFlight.current = false;
    }
  }, []);

  useEffect(() => {
    if (!enabled) {
      setStatus(null);
      setError(null);
      return undefined;
    }
    let timer: ReturnType<typeof setInterval> | null = null;
    const start = () => {
      if (timer != null) return;
      void refresh();
      timer = setInterval(() => void refresh(), MLX_STATUS_POLL_MS);
    };
    const stop = () => {
      if (timer != null) {
        clearInterval(timer);
        timer = null;
      }
    };
    const onVisibility = () => {
      if (document.visibilityState === 'visible') start();
      else stop();
    };
    onVisibility();
    document.addEventListener('visibilitychange', onVisibility);
    return () => {
      stop();
      document.removeEventListener('visibilitychange', onVisibility);
    };
  }, [enabled, refresh]);

  return { status, error, refresh };
}
