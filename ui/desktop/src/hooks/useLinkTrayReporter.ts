import { useEffect } from 'react';
import type { IpcRendererEvent } from 'electron';
import { leanzeroLinkConnect, leanzeroLinkStatus, linkBannerText } from '../acp/leanzero-link';
import type { LinkState } from '../acp/leanzero-link';
import { toastError } from '../toasts';

/** The Link tab's own cadence, used while the state is still moving (a launch reconnect). */
export const LINK_TRAY_SETTLING_POLL_MS = 3000;
/** The steady cadence once the state has settled; focus and every tray-menu open read at once. */
export const LINK_TRAY_POLL_MS = 30000;

/** Still moving: a connect in flight, or a launch reconnect that has not reported an outcome. */
export function linkStateSettling(state: LinkState): boolean {
  if (state.auth.state === 'connecting') return true;
  if (state.reconnect?.state === 'reconnecting') return true;
  // goosed runs the launch reconnect in the background: a connected intent on a signed-in node
  // that has no outcome yet will have one in a moment.
  return (
    state.auth.state === 'loggedIn' &&
    state.intent?.intent === 'connected' &&
    state.reconnect?.state === 'idle' &&
    !state.lastError
  );
}

/**
 * Keep MAIN's LeanZero Link line current for the menu-bar tray — from app launch, not only while
 * the Link tab is open, because the launch reconnect runs with no window on that tab and its
 * failure must not be silent. Every read reaches main inside `leanzeroLinkStatus`. The tray's
 * Retry / Connect lands here (`link-tray-action`) and makes the SAME connect the Link tab makes.
 */
export function useLinkTrayReporter(enabled: boolean): void {
  useEffect(() => {
    if (!enabled) return undefined;
    let disposed = false;
    let reading = false;
    let timer: ReturnType<typeof setTimeout> | null = null;
    const schedule = (ms: number) => {
      if (disposed) return;
      if (timer) clearTimeout(timer);
      timer = setTimeout(() => {
        timer = null;
        void read();
      }, ms);
    };
    const read = async () => {
      if (reading || disposed) return;
      reading = true;
      let settling = false;
      try {
        settling = linkStateSettling(await leanzeroLinkStatus());
      } catch {
        // No state to report; main keeps the last line and the next read tries again.
      } finally {
        reading = false;
      }
      schedule(settling ? LINK_TRAY_SETTLING_POLL_MS : LINK_TRAY_POLL_MS);
    };
    void read();
    const onWake = () => void read();
    const onAction = (_event: IpcRendererEvent, ...args: unknown[]) => {
      if (args[0] !== 'connect') return;
      leanzeroLinkConnect()
        .catch((error: unknown) =>
          toastError({ title: 'LeanZero Link could not connect', msg: linkBannerText(error) })
        )
        .finally(() => void read());
    };
    window.electron.on('mlx-distributed-wake', onWake);
    window.electron.on('link-tray-action', onAction);
    window.addEventListener('focus', onWake);
    return () => {
      disposed = true;
      if (timer) clearTimeout(timer);
      window.electron.off('mlx-distributed-wake', onWake);
      window.electron.off('link-tray-action', onAction);
      window.removeEventListener('focus', onWake);
    };
  }, [enabled]);
}
