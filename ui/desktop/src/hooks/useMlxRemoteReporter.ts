import { useEffect } from 'react';
import {
  latestMlxRemoteSingleStatus,
  mlxRemoteSingleStatus,
  remoteRouteUp,
  subscribeMlxRemoteSingleStatus,
} from '../acp/mlx-remote-single';
import { MLX_STATUS_POLL_MS } from '../components/leanzero-swarm/mlxLiveStats';

/**
 * Keep "where MLX chat goes" current for every surface that shows it (the state tile, the composer's
 * readiness, the menu-bar tray through main): one read on start, on focus and on every tray-menu
 * open; while a remote route is up, one read per poll. Each read reaches main and the latest-status
 * listeners inside `mlxRemoteSingleStatus`. A failed read (an older goose without the method, a
 * backend restarting) clears the route claim and stops the loop — no claim outlives its read.
 */
export function useMlxRemoteReporter(): void {
  useEffect(() => {
    let disposed = false;
    let reading = false;
    let timer: ReturnType<typeof setTimeout> | null = null;
    const schedule = () => {
      if (!disposed && !timer && remoteRouteUp(latestMlxRemoteSingleStatus())) {
        timer = setTimeout(() => {
          timer = null;
          void read();
        }, MLX_STATUS_POLL_MS);
      }
    };
    const read = async () => {
      if (reading || disposed) return;
      reading = true;
      try {
        await mlxRemoteSingleStatus();
      } catch {
        // The read recorded why it failed; the last status stands and the next read is scheduled.
      } finally {
        reading = false;
      }
      schedule();
    };
    void read();
    const onWake = () => void read();
    window.electron.on('mlx-distributed-wake', onWake);
    window.addEventListener('focus', onWake);
    // A route started from the placement card is first seen by ITS call: join the loop then.
    const unsubscribe = subscribeMlxRemoteSingleStatus(schedule);
    return () => {
      disposed = true;
      if (timer) clearTimeout(timer);
      unsubscribe();
      window.removeEventListener('focus', onWake);
      window.electron.off('mlx-distributed-wake', onWake);
    };
  }, []);
}
