import { useEffect } from 'react';
import type { IpcRendererEvent } from 'electron';
import { defineMessages, useIntl } from '../i18n';
import {
  mlxEngineMount,
  mlxEngineSettingsRead,
  mlxEngineStatus,
  mlxEngineUnmount,
} from '../acp/mlx-engine';
import {
  foreignOwner,
  latestMlxDistributedStatus,
  mlxDistributedStatus,
  mlxDistributedStop,
  subscribeMlxDistributedStatus,
} from '../acp/mlx-distributed';
import { dropRoute } from '../components/leanzero-swarm/routeSwitch';
import { MLX_STATUS_POLL_MS } from '../components/leanzero-swarm/mlxLiveStats';
import { useFeatures } from '../contexts/FeaturesContext';
import { useMlxRemoteReporter } from './useMlxRemoteReporter';
import { toastError } from '../toasts';
import { errorMessage } from '../utils/conversionUtils';

const i18n = defineMessages({
  mountFailed: {
    id: 'mlxTray.mountFailed',
    defaultMessage: 'Could not mount the MLX engine',
  },
  unmountFailed: {
    id: 'mlxTray.unmountFailed',
    defaultMessage: 'Could not unmount the MLX engine',
  },
  noModel: {
    id: 'mlxTray.noModel',
    defaultMessage: 'No model is configured to mount. Pick one in Providers › LeanZero MLX.',
  },
  stopFailed: {
    id: 'mlxTray.distributedStopFailed',
    defaultMessage: 'Could not stop the distributed engine',
  },
  stopUnverified: {
    id: 'mlxTray.distributedStopUnverified',
    defaultMessage: 'The distributed engine stop was not verified: {steps}',
  },
  remoteStopFailed: {
    id: 'mlxTray.remoteStopFailed',
    defaultMessage: 'Could not stop serving from the other Mac',
  },
});

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

export type MlxTrayRendererAction =
  | 'mount'
  | 'unmount'
  | 'stop-distributed'
  | 'stop-remote'
  | 'run-here'
  | 'stop-waiting';

const RENDERER_ACTIONS: readonly string[] = [
  'mount',
  'unmount',
  'stop-distributed',
  'stop-remote',
  'run-here',
  'stop-waiting',
];

/** A stop whose ranks were not all observed gone — the steps say which pid was left. */
export class DistributedStopNotVerified extends Error {
  constructor(readonly steps: string[]) {
    super('distributed-stop-not-verified');
  }
}

/**
 * The menu-bar tray's Mount / Unmount / Stop (utils/mlxTray.ts): main owns no ACP client, so it asks
 * this window to make the SAME calls the Providers view makes. Mount returns at once with the engine
 * "mounting"; the status is read on the view's cadence until it leaves that state — each read goes
 * to main (mlxEngineStatus reports it), so the tray follows the mount to running or failed and its
 * own loop stops on the real outcome, not on a guess. The distributed stop returns after the
 * verified stop, and its status goes to main the same way.
 */
export async function runMlxTrayAction(action: MlxTrayRendererAction): Promise<void> {
  if (action === 'stop-remote') {
    // The route's Stop in Run it (routeSwitch.ts, the one path): withdrawn on this Mac, at once when
    // its Mac is not answering; a peer that keeps its model is the quiet PeerHeldLine, not a toast.
    await dropRoute().routeGone;
    return;
  }
  if (action === 'stop-waiting' || action === 'run-here') {
    // Offered only while the route's Mac is gone (mlxTray `peerGoneModel`): withdrawn here, that
    // Mac never asked. "Run on this Mac instead" then brings this Mac's engine up unless it is.
    await dropRoute('gone').routeGone;
    if (action === 'stop-waiting') return;
    const here = await mlxEngineStatus();
    if (here.state === 'running' || here.state === 'mounting') return;
  }
  if (action === 'stop-distributed') {
    const { stop } = await mlxDistributedStop();
    if (!stop.verified) throw new DistributedStopNotVerified(stop.steps);
    return;
  }
  if (action === 'unmount') {
    await mlxEngineUnmount();
    await mlxEngineStatus();
    return;
  }
  const settings = await mlxEngineSettingsRead();
  if (!settings.modelId) throw new Error('no-model');
  await mlxEngineMount(settings.modelId);
  while ((await mlxEngineStatus()).state === 'mounting') {
    await sleep(MLX_STATUS_POLL_MS);
  }
}

/**
 * Keep MAIN's copy of the distributed engine current for the tray. One read on start and on every
 * tray-menu open (`mlx-distributed-wake`); while the run owns this Mac, one read per view poll.
 * Each read reaches main inside `mlxDistributedStatus`. A failed read reports nothing — main's copy
 * then ages into "stale" in the menu instead of being shown as live — and the loop keeps trying
 * while the last good read said the run owns the Mac.
 */
export function useMlxDistributedReporter(enabled: boolean): void {
  useEffect(() => {
    if (!enabled) return undefined;
    let disposed = false;
    let reading = false;
    let owned = false;
    let timer: ReturnType<typeof setTimeout> | null = null;
    const read = async () => {
      if (reading || disposed) return;
      reading = true;
      if (timer) {
        clearTimeout(timer);
        timer = null;
      }
      try {
        const status = await mlxDistributedStatus();
        // Another window's run is followed too, so its stop reaches this window's readiness; so is a
        // rank this Mac serves for another Mac, so the tray follows it until it ends.
        owned =
          status.mode === 'distributed' || foreignOwner(status) != null || status.hosting != null;
      } catch {
        // Nothing to report; `owned` keeps the last good read's word.
      } finally {
        reading = false;
      }
      if (!disposed && owned) timer = setTimeout(() => void read(), MLX_STATUS_POLL_MS);
    };
    void read();
    const onWake = () => void read();
    window.electron.on('mlx-distributed-wake', onWake);
    // Another window may have started a run while this one was in the background.
    window.addEventListener('focus', onWake);
    // A run started from the Engine tab is first seen by ITS read: join the loop then, so the
    // latest status (the composer's readiness reads it) stays current after that view closes.
    const unsubscribe = subscribeMlxDistributedStatus(() => {
      const latest = latestMlxDistributedStatus();
      if (
        disposed ||
        reading ||
        timer ||
        (latest?.mode !== 'distributed' && foreignOwner(latest) == null && latest?.hosting == null)
      ) {
        return;
      }
      owned = true;
      timer = setTimeout(() => void read(), MLX_STATUS_POLL_MS);
    });
    return () => {
      disposed = true;
      if (timer) clearTimeout(timer);
      unsubscribe();
      window.removeEventListener('focus', onWake);
      window.electron.off('mlx-distributed-wake', onWake);
    };
  }, [enabled]);
}

export function useMlxTrayActions(): void {
  const intl = useIntl();
  const { mlxDistributed } = useFeatures();
  useMlxDistributedReporter(mlxDistributed);
  useMlxRemoteReporter();
  useEffect(() => {
    const onAction = (_event: IpcRendererEvent, ...args: unknown[]) => {
      const action = args[0];
      if (typeof action !== 'string' || !RENDERER_ACTIONS.includes(action)) return;
      runMlxTrayAction(action as MlxTrayRendererAction).catch((error: unknown) => {
        if (error instanceof DistributedStopNotVerified) {
          toastError({
            title: intl.formatMessage(i18n.stopFailed),
            msg: intl.formatMessage(i18n.stopUnverified, { steps: error.steps.join('; ') }),
          });
          return;
        }
        const noModel = error instanceof Error && error.message === 'no-model';
        const title =
          action === 'mount' || action === 'run-here'
            ? i18n.mountFailed
            : action === 'unmount'
              ? i18n.unmountFailed
              : action === 'stop-remote' || action === 'stop-waiting'
                ? i18n.remoteStopFailed
                : i18n.stopFailed;
        toastError({
          title: intl.formatMessage(title),
          msg: noModel ? intl.formatMessage(i18n.noModel) : errorMessage(error, String(error)),
        });
      });
    };
    window.electron.on('mlx-tray-action', onAction);
    return () => window.electron.off('mlx-tray-action', onAction);
  }, [intl]);
}
