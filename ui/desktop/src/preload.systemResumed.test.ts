import { EventEmitter } from 'node:events';
import { describe, expect, it, vi } from 'vitest';
import { SYSTEM_RESUMED_CHANNEL } from './systemResumed';

/** Q-228 (L10): the preload bridge hands main's `system-resumed` to the window, and lets go. */

const bridge = vi.hoisted(() => ({
  exposed: new Map<string, unknown>(),
  renderer: null as EventEmitter | null,
}));

vi.mock('electron', async () => {
  const { EventEmitter: Emitter } = await import('node:events');
  const renderer = new Emitter();
  bridge.renderer = renderer;
  return {
    default: {},
    contextBridge: {
      exposeInMainWorld: (key: string, api: unknown) => bridge.exposed.set(key, api),
    },
    ipcRenderer: Object.assign(renderer, {
      invoke: vi.fn(),
      send: vi.fn(),
      sendSync: vi.fn(),
    }),
    webUtils: { getPathForFile: vi.fn() },
  };
});

describe('preload onSystemResumed', () => {
  it('calls back once per system-resumed, and not after its unsubscribe', async () => {
    await import('./preload');
    const api = bridge.exposed.get('electron') as {
      onSystemResumed(callback: () => void): () => void;
    };
    const renderer = bridge.renderer!;
    const heard = vi.fn();

    const off = api.onSystemResumed(heard);
    renderer.emit(SYSTEM_RESUMED_CHANNEL, {});
    renderer.emit(SYSTEM_RESUMED_CHANNEL, {});
    expect(heard).toHaveBeenCalledTimes(2);

    off();
    renderer.emit(SYSTEM_RESUMED_CHANNEL, {});
    expect(heard).toHaveBeenCalledTimes(2);
    expect(renderer.listenerCount(SYSTEM_RESUMED_CHANNEL)).toBe(0);
  });
});
