import { EventEmitter } from 'node:events';
import { describe, expect, it, vi } from 'vitest';
import type { GooseServeResult, GooseServeStop, Logger } from './gooseServe';
import {
  GOOSE_SERVE_EXITED_USER_MESSAGE,
  GooseServeLeaseRegistry,
} from './gooseServeLeaseRegistry';

function createLogger(): Logger {
  return {
    info: vi.fn(),
    error: vi.fn(),
  };
}

function createGooseServeResult(
  overrides: Partial<Pick<GooseServeResult, 'cleanup' | 'hasExited' | 'getExitDetails'>> = {}
): GooseServeResult {
  return {
    acpUrl: 'ws://127.0.0.1:1234/acp?token=test',
    workingDir: '/tmp',
    process: new EventEmitter() as GooseServeResult['process'],
    errorLog: [],
    certFingerprint: null,
    cleanup: vi.fn(async () => 'exited' as const),
    hasExited: () => false,
    getExitDetails: () => ({ code: null, signal: null }),
    startupDiagnosticsPath: null,
    getStartupDiagnostics: () => null,
    recordStartupEvent: () => undefined,
    ...overrides,
  };
}

describe('GooseServeLeaseRegistry', () => {
  it('returns the ACP URL for an attached live lease', () => {
    const store = new GooseServeLeaseRegistry(createLogger());
    const lease = store.create(createGooseServeResult(), 'local-secret');

    store.attachWindow(1, lease);

    expect(store.getAcpUrl(1)).toBe('ws://127.0.0.1:1234/acp?token=test');
    expect(store.getSecretKey(1)).toBe('local-secret');
  });

  it('throws a recovery message after the process exits', () => {
    const logger = createLogger();
    const store = new GooseServeLeaseRegistry(logger);
    const result = createGooseServeResult();
    const lease = store.create(result, 'local-secret');
    store.attachWindow(1, lease);

    result.process.emit('exit', 1, null);

    expect(() => store.getAcpUrl(1)).toThrow(GOOSE_SERVE_EXITED_USER_MESSAGE);
    expect(() => store.getSecretKey(1)).toThrow(GOOSE_SERVE_EXITED_USER_MESSAGE);
    expect(logger.error).toHaveBeenCalledWith(
      'Goose ACP server exited unexpectedly',
      expect.objectContaining({ code: 1, signal: null, windowIds: [1] })
    );
  });

  it('uses the current child exit state when creating the lease', () => {
    const store = new GooseServeLeaseRegistry(createLogger());
    const lease = store.create(
      createGooseServeResult({
        hasExited: () => true,
        getExitDetails: () => ({ code: null, signal: 'SIGTERM' }),
      }),
      'local-secret'
    );

    store.attachWindow(1, lease);

    expect(() => store.getAcpUrl(1)).toThrow(GOOSE_SERVE_EXITED_USER_MESSAGE);
  });

  it('cleans up once after the last attached window is released', async () => {
    const cleanup = vi.fn(async () => 'exited' as const);
    const store = new GooseServeLeaseRegistry(createLogger());
    const lease = store.create(createGooseServeResult({ cleanup }), 'local-secret');
    store.attachWindow(1, lease);
    store.attachWindow(2, lease);

    await store.releaseWindow(1);
    expect(cleanup).not.toHaveBeenCalled();
    expect(store.getAcpUrl(2)).toBe('ws://127.0.0.1:1234/acp?token=test');
    expect(store.getSecretKey(2)).toBe('local-secret');

    await store.releaseWindow(2);
    expect(cleanup).toHaveBeenCalledTimes(1);
    expect(store.getAcpUrl(2)).toBeNull();
    expect(store.getSecretKey(2)).toBeNull();
  });

  it('creates an external ACP lease without process cleanup', async () => {
    const store = new GooseServeLeaseRegistry(createLogger());
    const lease = store.createExternal('wss://example.com/goose/acp?token=test', 'external-secret');

    store.attachWindow(1, lease);

    expect(store.getAcpUrl(1)).toBe('wss://example.com/goose/acp?token=test');
    expect(store.getSecretKey(1)).toBe('external-secret');

    await store.releaseWindow(1);
    expect(store.getAcpUrl(1)).toBeNull();
    expect(store.getSecretKey(1)).toBeNull();
  });

  it('cleans up external leases after the last attached window is released', async () => {
    const cleanup = vi.fn(async () => undefined);
    const store = new GooseServeLeaseRegistry(createLogger());
    const lease = store.createExternal(
      'wss://example.com/goose/acp?token=test',
      'external-secret',
      cleanup
    );
    store.attachWindow(1, lease);
    store.attachWindow(2, lease);

    await store.releaseWindow(1);
    expect(cleanup).not.toHaveBeenCalled();

    await store.releaseWindow(2);
    expect(cleanup).toHaveBeenCalledTimes(1);
  });

  // Q-223: on quit the window's `closed` releases its lease first (not awaited — it cannot be) and
  // that removes the lease from the map; the old will-quit then counted zero leases and let the app
  // exit while goosed was still tearing down. The quit must find the stop already under way.
  it('makes the quit wait for a stop a window release already started', async () => {
    let exitGoosed!: () => void;
    const goosedExited = new Promise<void>((resolve) => {
      exitGoosed = resolve;
    });
    const store = new GooseServeLeaseRegistry(createLogger());
    const cleanup = () => goosedExited.then(() => 'exited' as const);
    const lease = store.create(createGooseServeResult({ cleanup }), 's');
    store.attachWindow(1, lease);

    void store.releaseWindow(1);
    expect(store.activeLeaseCount()).toBe(0);
    expect(store.hasBackendsToStop()).toBe(true);

    let quitMayProceed = false;
    const quit = store.stopAllAndWait().then(() => {
      quitMayProceed = true;
    });
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(quitMayProceed).toBe(false);

    exitGoosed();
    await quit;
    expect(quitMayProceed).toBe(true);
    expect(store.hasBackendsToStop()).toBe(false);
  });

  it('stops attached backends on quit and waits for each to exit', async () => {
    const exits: ((ended: GooseServeStop) => void)[] = [];
    const cleanups = [0, 1].map(() =>
      vi.fn(
        () =>
          new Promise<GooseServeStop>((resolve) => {
            exits.push(resolve);
          })
      )
    );
    const store = new GooseServeLeaseRegistry(createLogger());
    cleanups.forEach((cleanup, windowId) =>
      store.attachWindow(windowId, store.create(createGooseServeResult({ cleanup }), 's'))
    );

    let done = false;
    const quit = store.stopAllAndWait().then((outcome) => {
      done = true;
      return outcome;
    });
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(cleanups.map((c) => c.mock.calls.length)).toEqual([1, 1]);
    exits[0]('exited');
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(done).toBe(false);
    exits[1]('abandoned');
    // A stop that gave up is counted, so the quit's log never claims every backend exited.
    await expect(quit).resolves.toEqual({ abandoned: 1 });
  });

  it('a failed stop is logged and never holds the quit forever', async () => {
    const logger = createLogger();
    const store = new GooseServeLeaseRegistry(logger);
    const lease = store.create(
      createGooseServeResult({ cleanup: async () => Promise.reject(new Error('EPERM')) }),
      's'
    );
    store.attachWindow(1, lease);
    await expect(store.stopAllAndWait()).resolves.toEqual({ abandoned: 1 });
    expect(logger.error).toHaveBeenCalledWith(
      'Failed to cleanup goose serve backend:',
      expect.any(Error)
    );
    expect(store.hasBackendsToStop()).toBe(false);
  });
});

// Q-257: every window once spawned its own goosed, and a second goosed on the Mac is refused the
// LeanZero Link mesh while the first holds it — so window 2 could not load the split. One goosed per
// app now: windows share the live local lease; a new one starts only when none is live.
describe('GooseServeLeaseRegistry — one local goosed per app (Q-257)', () => {
  it('a second window reuses the live local lease; its close leaves goosed standing', async () => {
    const cleanup = vi.fn(async () => 'exited' as const);
    const store = new GooseServeLeaseRegistry(createLogger());
    const start = vi.fn(async () => store.create(createGooseServeResult({ cleanup }), 's'));

    const first = await store.acquireLocal(start);
    store.attachWindow(1, first!);
    const second = await store.acquireLocal(start);
    store.attachWindow(2, second!);

    expect(start).toHaveBeenCalledTimes(1);
    expect(second).toBe(first);

    await store.releaseWindow(1);
    expect(cleanup).not.toHaveBeenCalled();
    expect(store.liveLocal()).toBe(first);

    await store.releaseWindow(2);
    expect(cleanup).toHaveBeenCalledTimes(1);
    expect(store.liveLocal()).toBeNull();
  });

  it('windows asking while the first goosed is still starting share that one start', async () => {
    const store = new GooseServeLeaseRegistry(createLogger());
    let finish!: () => void;
    const started = new Promise<void>((resolve) => {
      finish = resolve;
    });
    const start = vi.fn(async () => {
      await started;
      return store.create(createGooseServeResult(), 's');
    });

    const a = store.acquireLocal(start);
    const b = store.acquireLocal(start);
    finish();
    const [leaseA, leaseB] = await Promise.all([a, b]);

    expect(start).toHaveBeenCalledTimes(1);
    expect(leaseA).not.toBeNull();
    expect(leaseB).toBe(leaseA);
  });

  it('an exited goosed sends the next window to a fresh start', async () => {
    const store = new GooseServeLeaseRegistry(createLogger());
    const firstResult = createGooseServeResult();
    const first = await store.acquireLocal(async () => store.create(firstResult, 's'));
    store.attachWindow(1, first!);

    firstResult.process.emit('exit', 1, null);
    expect(store.liveLocal()).toBeNull();

    const start = vi.fn(async () => store.create(createGooseServeResult(), 's'));
    const second = await store.acquireLocal(start);
    expect(start).toHaveBeenCalledTimes(1);
    expect(second).not.toBe(first);
    expect(store.liveLocal()).toBe(second);
    // The window left on the dead goosed still reads the recovery message.
    expect(() => store.getAcpUrl(1)).toThrow(GOOSE_SERVE_EXITED_USER_MESSAGE);
  });

  it('a new goosed starts only after the one still tearing down has exited (it holds the mesh)', async () => {
    let exitOld!: () => void;
    const oldExited = new Promise<void>((resolve) => {
      exitOld = resolve;
    });
    const store = new GooseServeLeaseRegistry(createLogger());
    const old = store.create(
      createGooseServeResult({ cleanup: () => oldExited.then(() => 'exited' as const) }),
      's'
    );
    store.attachWindow(1, old);
    void store.releaseWindow(1);
    expect(store.liveLocal()).toBeNull();

    const start = vi.fn(async () => store.create(createGooseServeResult(), 's'));
    const next = store.acquireLocal(start);
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(start).not.toHaveBeenCalled();

    exitOld();
    await expect(next).resolves.not.toBeNull();
    expect(start).toHaveBeenCalledTimes(1);
  });

  it('a failed start is not remembered: the next window tries again', async () => {
    const store = new GooseServeLeaseRegistry(createLogger());
    await expect(store.acquireLocal(async () => null)).resolves.toBeNull();
    const start = vi.fn(async () => store.create(createGooseServeResult(), 's'));
    await expect(store.acquireLocal(start)).resolves.not.toBeNull();
    expect(start).toHaveBeenCalledTimes(1);
  });

  it('an external lease is never offered as the local goosed', () => {
    const store = new GooseServeLeaseRegistry(createLogger());
    store.attachWindow(1, store.createExternal('wss://example.com/acp', 's'));
    expect(store.liveLocal()).toBeNull();
  });

  // Refuter D2: two windows wait on one start; the first one's BrowserWindow throws before the
  // second has attached. Giving the lease back must not stop the goosed the second is about to use.
  it('a window whose creation failed never stops the goosed a sibling window is about to attach', async () => {
    const cleanup = vi.fn(async () => 'exited' as const);
    const store = new GooseServeLeaseRegistry(createLogger());
    const start = vi.fn(async () => store.create(createGooseServeResult({ cleanup }), 's'));
    const [failed, sibling] = await Promise.all([
      store.acquireLocal(start),
      store.acquireLocal(start),
    ]);

    await store.releaseUnattached(failed!);
    expect(cleanup).not.toHaveBeenCalled();
    store.attachWindow(2, sibling!);
    expect(store.getAcpUrl(2)).toBe('ws://127.0.0.1:1234/acp?token=test');

    await store.releaseWindow(2);
    expect(cleanup).toHaveBeenCalledTimes(1);
  });

  // Q-490's lead, walked: main.log at 11:58:11 said "Window shares the app's goose serve backend
  // (pid 83194, 1 window(s) attached)" with two windows on screen. The count is read BEFORE the new
  // window attaches, so the 1 is the main window; closing the new one leaves goosed serving it.
  it('the count a joining window logs is the OTHER windows; closing the joiner keeps goosed (Q-490)', async () => {
    const cleanup = vi.fn(async () => 'exited' as const);
    const store = new GooseServeLeaseRegistry(createLogger());
    const start = vi.fn(async () => store.create(createGooseServeResult({ cleanup }), 's'));
    const main = await store.acquireLocal(start);
    store.attachWindow(1, main!);

    const reused = store.liveLocal();
    const joiner = await store.acquireLocal(start);
    expect(reused?.windowIds.size).toBe(1);
    store.attachWindow(2, joiner!);

    await store.releaseWindow(2);
    expect(cleanup).not.toHaveBeenCalled();
    expect(store.getAcpUrl(1)).toBe('ws://127.0.0.1:1234/acp?token=test');
  });

  // The rule releaseUnattached already kept, now kept by releaseWindow too (Q-490): a window being
  // made holds the goosed it was handed, so the last ATTACHED window closing does not stop it.
  it('the last attached window closing never stops the goosed a window being made was handed', async () => {
    const cleanup = vi.fn(async () => 'exited' as const);
    const logger = createLogger();
    const store = new GooseServeLeaseRegistry(logger);
    const start = vi.fn(async () => store.create(createGooseServeResult({ cleanup }), 's'));
    const first = await store.acquireLocal(start);
    store.attachWindow(1, first!);
    const coming = await store.acquireLocal(start);

    await store.releaseWindow(1);
    expect(cleanup).not.toHaveBeenCalled();
    expect(store.liveLocal()).toBe(coming);

    store.attachWindow(2, coming!);
    await store.releaseWindow(2);
    expect(cleanup).toHaveBeenCalledTimes(1);
    expect(logger.info).toHaveBeenCalledWith(
      'Window 2 was the last one using goose serve (pid ?); stopping it'
    );
  });

  it('a lease handed out to one window and given back unused is stopped', async () => {
    const cleanup = vi.fn(async () => 'exited' as const);
    const store = new GooseServeLeaseRegistry(createLogger());
    const lease = await store.acquireLocal(async () =>
      store.create(createGooseServeResult({ cleanup }), 's')
    );
    await store.releaseUnattached(lease!);
    expect(cleanup).toHaveBeenCalledTimes(1);
    expect(store.liveLocal()).toBeNull();
  });

  // Refuter D3's other half: a quit that lands between a goosed's start and its window attaching
  // still finds that goosed and waits for it.
  it('a quit stops the local goosed even before its first window has attached', async () => {
    const cleanup = vi.fn(async () => 'exited' as const);
    const store = new GooseServeLeaseRegistry(createLogger());
    await store.acquireLocal(async () => store.create(createGooseServeResult({ cleanup }), 's'));

    expect(store.hasBackendsToStop()).toBe(true);
    await expect(store.stopAllAndWait()).resolves.toEqual({ abandoned: 0 });
    expect(cleanup).toHaveBeenCalledTimes(1);
    expect(store.hasBackendsToStop()).toBe(false);
  });
});
