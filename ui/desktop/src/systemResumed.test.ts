import { EventEmitter } from 'node:events';
import { describe, expect, it, vi } from 'vitest';
import {
  broadcastSystemResumed,
  registerSystemResumed,
  SYSTEM_RESUMED_CHANNEL,
  type ResumedWindow,
} from './systemResumed';

/** Q-228 (L10): the wake reaches every goose window and never the floating engine glance. */

function fakeWindow(id: number, destroyed = false): ResumedWindow & { sent: string[] } {
  const sent: string[] = [];
  return {
    sent,
    isDestroyed: () => destroyed,
    webContents: { id, send: (channel: string) => sent.push(channel) },
  };
}

describe('broadcastSystemResumed', () => {
  it('sends system-resumed once to every live goose window and never to the glance', () => {
    const first = fakeWindow(11);
    const second = fakeWindow(12);
    const glance = fakeWindow(40);

    const reached = broadcastSystemResumed([first, glance, second], 40);

    expect(reached).toEqual([11, 12]);
    expect(first.sent).toEqual([SYSTEM_RESUMED_CHANNEL]);
    expect(second.sent).toEqual([SYSTEM_RESUMED_CHANNEL]);
    expect(glance.sent).toEqual([]);
  });

  it('skips a destroyed window, and with no glance open every goose window hears it', () => {
    const live = fakeWindow(11);
    const closed = fakeWindow(12, true);

    expect(broadcastSystemResumed([live, closed], null)).toEqual([11]);
    expect(closed.sent).toEqual([]);
  });
});

describe('registerSystemResumed', () => {
  it("on the system's resume, reads the windows then — a window opened after ready hears it", async () => {
    const powerMonitor = new EventEmitter();
    const windows: ResumedWindow[] = [];
    let glanceId: number | null = null;

    await registerSystemResumed({
      app: { whenReady: () => Promise.resolve() },
      powerMonitor,
      gooseWindows: () => windows,
      glanceWebContentsId: () => glanceId,
    });

    const first = fakeWindow(11);
    const glance = fakeWindow(40);
    windows.push(first, glance);
    glanceId = 40;
    powerMonitor.emit('resume');

    expect(first.sent).toEqual([SYSTEM_RESUMED_CHANNEL]);
    expect(glance.sent).toEqual([]);

    const second = fakeWindow(12);
    windows.push(second);
    powerMonitor.emit('resume');

    expect(first.sent).toEqual([SYSTEM_RESUMED_CHANNEL, SYSTEM_RESUMED_CHANNEL]);
    expect(second.sent).toEqual([SYSTEM_RESUMED_CHANNEL]);
    expect(glance.sent).toEqual([]);
  });

  it('attaches nothing before the app is ready', async () => {
    const on = vi.fn();
    let ready!: () => void;
    const registered = registerSystemResumed({
      app: { whenReady: () => new Promise<void>((resolve) => (ready = resolve)) },
      powerMonitor: { on },
      gooseWindows: () => [],
      glanceWebContentsId: () => null,
    });

    expect(on).not.toHaveBeenCalled();
    ready();
    await registered;
    expect(on).toHaveBeenCalledTimes(1);
    expect(on).toHaveBeenCalledWith('resume', expect.any(Function));
  });
});
