import { useEffect } from 'react';
import type { IpcRendererEvent } from 'electron';
import { defineMessages, useIntl } from '../i18n';
import {
  mlxEngineMount,
  mlxEngineSettingsRead,
  mlxEngineStatus,
  mlxEngineUnmount,
} from '../acp/mlx-engine';
import { MLX_STATUS_POLL_MS } from '../components/leanzero-swarm/mlxLiveStats';
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
});

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

/**
 * The menu-bar tray's Mount / Unmount (utils/mlxTray.ts): main owns no ACP client, so it asks this
 * window to make the SAME calls the Providers view makes. Mount returns at once with the engine
 * "mounting"; the status is read on the view's cadence until it leaves that state — each read goes
 * to main (mlxEngineStatus reports it), so the tray follows the mount to running or failed and its
 * own loop stops on the real outcome, not on a guess.
 */
export async function runMlxTrayAction(action: 'mount' | 'unmount'): Promise<void> {
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

export function useMlxTrayActions(): void {
  const intl = useIntl();
  useEffect(() => {
    const onAction = (_event: IpcRendererEvent, ...args: unknown[]) => {
      const action = args[0];
      if (action !== 'mount' && action !== 'unmount') return;
      runMlxTrayAction(action).catch((error: unknown) => {
        const noModel = error instanceof Error && error.message === 'no-model';
        toastError({
          title: intl.formatMessage(action === 'mount' ? i18n.mountFailed : i18n.unmountFailed),
          msg: noModel ? intl.formatMessage(i18n.noModel) : errorMessage(error, String(error)),
        });
      });
    };
    window.electron.on('mlx-tray-action', onAction);
    return () => window.electron.off('mlx-tray-action', onAction);
  }, [intl]);
}
