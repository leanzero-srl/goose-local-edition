import { describe, expect, it, vi } from 'vitest';
import type { App, IpcMain, PowerSaveBlocker } from 'electron';
import { registerKeepAwake } from './keepAwake';

type Handler = (event: unknown, ...args: unknown[]) => unknown;

function fakePower() {
  const live = new Set<number>();
  let next = 7;
  return {
    live,
    start: vi.fn((_type: 'prevent-app-suspension' | 'prevent-display-sleep') => {
      const id = next++;
      live.add(id);
      return id;
    }),
    stop: vi.fn((id: number) => live.delete(id)),
    isStarted: vi.fn((id: number) => live.has(id)),
  };
}

function setup({ saved = false, power = fakePower() } = {}) {
  const handlers = new Map<string, Handler>();
  const quitListeners: Array<() => void> = [];
  let enabled = saved;
  const ipcMain = {
    handle: (channel: string, listener: Handler) => handlers.set(channel, listener),
  } as unknown as Pick<IpcMain, 'handle'>;
  const app = {
    whenReady: () => Promise.resolve(),
    on: (event: string, listener: () => void) => {
      if (event === 'will-quit') quitListeners.push(listener);
    },
  } as unknown as Pick<App, 'whenReady' | 'on'>;
  const keepAwake = registerKeepAwake({
    ipcMain,
    app,
    power: power as unknown as PowerSaveBlocker,
    readEnabled: () => enabled,
    saveEnabled: (value) => {
      enabled = value;
    },
  });
  const invoke = (channel: string, ...args: unknown[]) => handlers.get(channel)!({}, ...args);
  const quit = () => quitListeners.forEach((listener) => listener());
  return { power, keepAwake, invoke, quit, saved: () => enabled };
}

describe('Prevent Sleep (Q-230): the setting holds one real power save blocker', () => {
  it('switching on starts exactly one prevent-app-suspension blocker and saves the choice', async () => {
    const { power, keepAwake, invoke, saved } = setup();
    await keepAwake.restored;
    expect(power.start).not.toHaveBeenCalled();

    const state = await invoke('set-wakelock', true);

    expect(power.start).toHaveBeenCalledExactlyOnceWith('prevent-app-suspension');
    expect(power.live.size).toBe(1);
    expect(state).toEqual({ enabled: true, holding: true, error: null });
    expect(saved()).toBe(true);
    expect(await invoke('get-wakelock-state')).toEqual({
      enabled: true,
      holding: true,
      error: null,
    });
  });

  it('switching on twice is idempotent — still one blocker', async () => {
    const { power, keepAwake, invoke } = setup();
    await keepAwake.restored;
    await invoke('set-wakelock', true);
    await invoke('set-wakelock', true);
    expect(power.start).toHaveBeenCalledTimes(1);
    expect(power.live.size).toBe(1);
  });

  it('switching off stops that blocker', async () => {
    const { power, keepAwake, invoke, saved } = setup();
    await keepAwake.restored;
    await invoke('set-wakelock', true);
    const id = power.start.mock.results[0].value as number;

    const state = await invoke('set-wakelock', false);

    expect(power.stop).toHaveBeenCalledExactlyOnceWith(id);
    expect(power.live.size).toBe(0);
    expect(state).toEqual({ enabled: false, holding: false, error: null });
    expect(saved()).toBe(false);
  });

  it('restores the blocker at startup when the setting was saved on', async () => {
    const { power, keepAwake, invoke } = setup({ saved: true });
    expect(await keepAwake.restored).toEqual({ enabled: true, holding: true, error: null });
    expect(power.start).toHaveBeenCalledExactlyOnceWith('prevent-app-suspension');
    expect(await invoke('get-wakelock-state')).toEqual({
      enabled: true,
      holding: true,
      error: null,
    });
  });

  it('does not start a blocker at startup when the setting was saved off', async () => {
    const { power, keepAwake } = setup({ saved: false });
    await keepAwake.restored;
    expect(power.start).not.toHaveBeenCalled();
  });

  it('quitting the app stops the blocker', async () => {
    const { power, keepAwake, quit } = setup({ saved: true });
    await keepAwake.restored;
    const id = power.start.mock.results[0].value as number;
    quit();
    expect(power.stop).toHaveBeenCalledExactlyOnceWith(id);
    expect(power.live.size).toBe(0);
  });

  it('a blocker that cannot start is reported, not claimed', async () => {
    const power = fakePower();
    power.start.mockImplementation(() => {
      throw new Error('IOPMAssertionCreate failed');
    });
    const { keepAwake, invoke } = setup({ power });
    await keepAwake.restored;
    expect(await invoke('set-wakelock', true)).toEqual({
      enabled: true,
      holding: false,
      error: 'IOPMAssertionCreate failed',
    });
    expect(await invoke('get-wakelock-state')).toMatchObject({
      holding: false,
      error: 'IOPMAssertionCreate failed',
    });
  });

  it('a blocker the system never started is reported, and a later switch-on retries', async () => {
    const power = fakePower();
    power.isStarted.mockReturnValueOnce(false);
    const { keepAwake, invoke } = setup({ saved: true, power });
    const restored = await keepAwake.restored;
    expect(restored.holding).toBe(false);
    expect(restored.error).toMatch(/did not start power save blocker/);

    expect(await invoke('set-wakelock', true)).toEqual({
      enabled: true,
      holding: true,
      error: null,
    });
  });

  it('refuses a non-boolean switch value instead of saving it', async () => {
    const { power, invoke, saved } = setup();
    expect(() => invoke('set-wakelock', 'yes')).toThrow(/expects a boolean/);
    expect(power.start).not.toHaveBeenCalled();
    expect(saved()).toBe(false);
  });
});
