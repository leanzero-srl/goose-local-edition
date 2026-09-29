import { describe, expect, it, vi } from 'vitest';
import {
  broadcastToWindows,
  canReach,
  reportFatalError,
  sendToWindow,
  type ReachableWindow,
} from './windowReach';

/**
 * Q-490. Measured on Electron 41.0.0 (the app's version) with a two-window probe: inside the closing
 * window's webContents `destroyed` event, `BrowserWindow.getAllWindows()` still lists it,
 * `win.isDestroyed()` is false, `win.webContents.isDestroyed()` is true, and `webContents.send`
 * throws `TypeError: Object has been destroyed`. The same probe with main.ts's old uncaught-exception
 * handler (a bare send to every window) exited with code 7 — launchd's "exited due to exit(7)" for
 * the installed 3.0.78 at 09:00:56.480Z, 19 ms after the second window's close.
 */
type FakeWindow = ReachableWindow & { sent: [string, unknown[]][] };

function fakeWindow(state: 'live' | 'closing' | 'destroyed'): FakeWindow {
  const sent: [string, unknown[]][] = [];
  return {
    sent,
    isDestroyed: () => state === 'destroyed',
    webContents: {
      isDestroyed: () => state !== 'live',
      send: (channel: string, ...args: unknown[]) => {
        if (state !== 'live') throw new TypeError('Object has been destroyed');
        sent.push([channel, args]);
      },
    },
  };
}

describe('canReach / sendToWindow / broadcastToWindows — a window mid-close is skipped, never thrown on', () => {
  it('a window whose renderer is gone is not reachable though the window is not destroyed yet', () => {
    expect(canReach(fakeWindow('live'))).toBe(true);
    expect(canReach(fakeWindow('closing'))).toBe(false);
    expect(canReach(fakeWindow('destroyed'))).toBe(false);
  });

  it('the glance broadcast from a closing window’s `destroyed` reaches the main window and does not throw', () => {
    const main = fakeWindow('live');
    const closing = fakeWindow('closing');

    let sent = 0;
    expect(() => {
      sent = broadcastToWindows([main, closing], 'engine-glance', { sessions: { running: 1 } });
    }).not.toThrow();
    expect(sent).toBe(1);
    expect(main.sent).toEqual([['engine-glance', [{ sessions: { running: 1 } }]]]);
  });

  it('sendToWindow answers whether it sent', () => {
    expect(sendToWindow(fakeWindow('live'), 'x')).toBe(true);
    expect(sendToWindow(fakeWindow('closing'), 'x')).toBe(false);
  });
});

describe('reportFatalError — the uncaught-exception handler never throws and always logs (Q-490)', () => {
  const deps = (windows: ReachableWindow[]) => ({
    windows: () => windows,
    logError: vi.fn(),
    formatError: (error: unknown) =>
      error instanceof Error ? (error.stack ?? error.message) : String(error),
  });

  it('with a window mid-close listed, the handler returns, logs to main.log and still tells the live window', () => {
    const main = fakeWindow('live');
    const closing = fakeWindow('closing');
    const d = deps([main, closing]);

    expect(() =>
      reportFatalError('uncaughtException', new TypeError('Object has been destroyed'), d)
    ).not.toThrow();

    expect(d.logError).toHaveBeenCalledWith(
      '[main] uncaughtException: Object has been destroyed',
      expect.stringContaining('Object has been destroyed')
    );
    expect(main.sent).toEqual([['fatal-error', ['Object has been destroyed']]]);
  });

  it('a send that throws anyway is logged, not rethrown', () => {
    const lying: ReachableWindow = {
      isDestroyed: () => false,
      webContents: {
        isDestroyed: () => false,
        send: () => {
          throw new Error('renderer went away between the check and the send');
        },
      },
    };
    const d = deps([lying]);
    expect(() => reportFatalError('unhandledRejection', 'plain reason', d)).not.toThrow();
    expect(d.logError).toHaveBeenCalledWith(
      '[main] unhandledRejection: plain reason',
      'plain reason'
    );
    expect(d.logError).toHaveBeenCalledWith(
      '[main] could not show the error in a window',
      expect.stringContaining('renderer went away')
    );
  });

  it('a failing log or window list never escapes either', () => {
    expect(() =>
      reportFatalError('uncaughtException', new Error('x'), {
        windows: () => {
          throw new Error('no windows');
        },
        logError: () => {
          throw new Error('disk full');
        },
        formatError: String,
      })
    ).not.toThrow();
  });
});
