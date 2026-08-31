import { useEffect, useState } from 'react';
import { mlxEngineStatus, type MlxEngineStatus } from '../../acp/mlx-engine';
import { errorMessage } from '../../utils/conversionUtils';

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
  const [status, setStatus] = useState<MlxEngineStatus | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!enabled) {
      setStatus(null);
      setError(null);
      return undefined;
    }

    let disposed = false;
    let timer: ReturnType<typeof setInterval> | null = null;

    const tick = async () => {
      try {
        const next = await mlxEngineStatus();
        if (disposed) return;
        setStatus(next);
        setError(null);
      } catch (e) {
        if (disposed) return;
        setStatus(null);
        setError(errorMessage(e, 'Could not read the MLX engine status.'));
      }
    };

    const start = () => {
      if (timer != null) return;
      void tick();
      timer = setInterval(() => void tick(), intervalMs);
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
      disposed = true;
      stop();
      document.removeEventListener('visibilitychange', onVisibility);
    };
  }, [enabled, intervalMs]);

  return { status, error };
}
