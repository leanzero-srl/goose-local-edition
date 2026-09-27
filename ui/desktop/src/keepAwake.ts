import type { App, IpcMain, PowerSaveBlocker } from 'electron';

/** What the Prevent Sleep setting is doing right now: the saved choice, whether the app holds its
 * power assertion, and why not when it should. */
export type KeepAwakeState = { enabled: boolean; holding: boolean; error: string | null };

type Power = Pick<PowerSaveBlocker, 'start' | 'stop' | 'isStarted'>;

type KeepAwakeDeps = {
  ipcMain: Pick<IpcMain, 'handle'>;
  app: Pick<App, 'whenReady' | 'on'>;
  power: Power;
  readEnabled: () => boolean;
  saveEnabled: (enabled: boolean) => void;
};

const describe = (error: unknown) => (error instanceof Error ? error.message : String(error));

/**
 * ONE assertion for the whole app, not one per window: the setting is app-wide and so is sleep.
 * `prevent-app-suspension` keeps the system from idle-sleeping (macOS PreventUserIdleSystemSleep)
 * while still letting the display turn off and lock — exactly what the setting's text promises;
 * `prevent-display-sleep` would also keep the screen lit, which it does not promise.
 */
export function createKeepAwake(power: Power) {
  let blockerId: number | null = null;
  let error: string | null = null;

  const holding = () => blockerId !== null && power.isStarted(blockerId);

  const hold = () => {
    if (holding()) return;
    blockerId = null;
    try {
      const id = power.start('prevent-app-suspension');
      if (power.isStarted(id)) {
        blockerId = id;
        error = null;
      } else {
        error = `the system did not start power save blocker ${id}`;
      }
    } catch (e) {
      error = describe(e);
    }
  };

  const release = () => {
    if (blockerId === null) {
      error = null;
      return;
    }
    try {
      power.stop(blockerId);
      blockerId = null;
      error = null;
    } catch (e) {
      error = `could not release power save blocker ${blockerId}: ${describe(e)}`;
    }
  };

  const state = (enabled: boolean): KeepAwakeState => {
    const isHolding = holding();
    const problem = error ?? (enabled && !isHolding ? 'no power save blocker is running' : null);
    return { enabled, holding: isHolding, error: problem };
  };

  return {
    apply(enabled: boolean): KeepAwakeState {
      if (enabled) hold();
      else release();
      return state(enabled);
    },
    state,
    release,
  };
}

/** The Prevent Sleep setting end to end: its two IPC calls, the restore at startup, the release at quit. */
export function registerKeepAwake({
  ipcMain,
  app,
  power,
  readEnabled,
  saveEnabled,
}: KeepAwakeDeps) {
  const keepAwake = createKeepAwake(power);

  ipcMain.handle('set-wakelock', (_event, enable: unknown) => {
    if (typeof enable !== 'boolean')
      throw new Error(`set-wakelock expects a boolean, got ${typeof enable}`);
    saveEnabled(enable);
    return keepAwake.apply(enable);
  });
  ipcMain.handle('get-wakelock-state', () => keepAwake.state(readEnabled()));

  const restored = app.whenReady().then(() => {
    const state = keepAwake.apply(readEnabled());
    if (state.enabled && !state.holding) {
      console.error(
        `[Main] Prevent Sleep is on but could not keep the computer awake: ${state.error}`
      );
    }
    return state;
  });

  app.on('will-quit', () => keepAwake.release());

  return { ...keepAwake, restored };
}
