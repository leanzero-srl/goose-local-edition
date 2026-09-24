import { useEffect, useRef } from 'react';
import type { IpcRendererEvent } from 'electron';
import type { IntlShape } from 'react-intl';
import { useIntl } from '../i18n';
import {
  leanzeroLinkConnect,
  leanzeroLinkNodes,
  leanzeroLinkStatus,
  linkBannerText,
} from '../acp/leanzero-link';
import type { LinkState } from '../acp/leanzero-link';
import { mlxEngineStatus } from '../acp/mlx-engine';
import { toastError } from '../toasts';
import { macTarget, macsFrom, peerRefuses } from '../components/leanzero-swarm/macs';
import {
  macTrayText,
  macsTrayOpenLabel,
  summarizeMac,
} from '../components/leanzero-swarm/macSummary';
import { mlxErrorMessage } from '../components/leanzero-swarm/mlxErrorMessage';
import type { MacsTrayReport } from '../utils/macsTrayReport';

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

function sendMacs(report: MacsTrayReport | null): void {
  const send = (
    window as unknown as { electron?: { macsReport?: (r: MacsTrayReport | null) => void } }
  ).electron?.macsReport;
  send?.(report);
}

/**
 * One tray line per linked Mac, read the way My Macs reads it: the roster (names, switches), then
 * each reachable Mac's engine status. A Mac whose owner turned model management off is not asked —
 * its line says Off. Not connected: null, and the tray keeps the Link line.
 */
export async function readMacsTrayReport(
  intl: IntlShape,
  state: LinkState
): Promise<MacsTrayReport | null> {
  if (state.auth.state !== 'connected') return null;
  const macs = macsFrom(await leanzeroLinkNodes(), '');
  const lines = await Promise.all(
    macs.map(async (mac) => {
      let status = null;
      let statusError: string | null = null;
      if (mac.online && !peerRefuses(mac, 'manage')) {
        try {
          status = await mlxEngineStatus(macTarget(mac));
        } catch (e) {
          statusError = mlxErrorMessage(e, String(e));
        }
      }
      const summary = summarizeMac(mac, { status, statusError, activity: null, decodeTps: null });
      return { name: mac.name, phase: summary.phase, text: macTrayText(intl, mac, summary) };
    })
  );
  return { lines, openLabel: macsTrayOpenLabel(intl) };
}

/**
 * Keep MAIN's LeanZero Link line current for the menu-bar tray — from app launch, not only while
 * the Link tab is open, because the launch reconnect runs with no window on that tab and its
 * failure must not be silent. Every read reaches main inside `leanzeroLinkStatus`. The tray's
 * Retry / Connect lands here (`link-tray-action`) and makes the SAME connect the Link tab makes.
 */
export function useLinkTrayReporter(enabled: boolean): void {
  const intl = useIntl();
  const intlRef = useRef(intl);
  useEffect(() => {
    intlRef.current = intl;
  }, [intl]);
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
        const state = await leanzeroLinkStatus();
        settling = linkStateSettling(state);
        try {
          sendMacs(await readMacsTrayReport(intlRef.current, state));
        } catch {
          // The roster did not answer: main keeps the last Mac lines and the next read tries again.
        }
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
