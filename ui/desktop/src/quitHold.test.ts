import { EventEmitter } from 'node:events';
import { describe, expect, it, vi } from 'vitest';
import type { GooseServeResult, GooseServeStop, Logger } from './gooseServe';
import { GooseServeLeaseRegistry } from './gooseServeLeaseRegistry';
import { QuitHold, type QuitDoor } from './quitHold';

/**
 * Q-241: the quit sequence as Electron runs it — `before-quit`, then every window closes (each close
 * releasing its goosed), then `will-quit` once the list is empty, then the process ends — driven over
 * the real lease registry with fake goosed processes whose exit the test controls.
 */

function logger(): Logger & { lines: string[] } {
  const lines: string[] = [];
  return {
    lines,
    info: vi.fn((...args: unknown[]) => lines.push(`info ${args.join(' ')}`)),
    error: vi.fn((...args: unknown[]) => lines.push(`error ${args.join(' ')}`)),
  };
}

/** A goosed whose stop resolves only when the test says it exited. */
function fakeGoosed() {
  const process = new EventEmitter() as GooseServeResult['process'];
  let exit: (ended: GooseServeStop) => void = () => {};
  const stopped = new Promise<GooseServeStop>((resolve) => {
    exit = resolve;
  });
  const cleanup = vi.fn(() => stopped);
  const result: GooseServeResult = {
    acpUrl: 'ws://127.0.0.1:1/acp',
    workingDir: '/tmp',
    process,
    errorLog: [],
    certFingerprint: null,
    cleanup,
    hasExited: () => false,
    getExitDetails: () => ({ code: null, signal: null }),
    startupDiagnosticsPath: null,
    getStartupDiagnostics: () => null,
    recordStartupEvent: () => undefined,
  };
  return { result, cleanup, exit: (ended: GooseServeStop = 'exited') => exit(ended) };
}

type Step = string;

/**
 * Electron's quit, reduced to what the hold sees: `quit()` emits before-quit; unless prevented, each
 * window closes (its `closed` releases its lease — fire and forget, as main.ts does); then will-quit;
 * unless prevented, the process ends. `dieAtFirstClose` is the 3.0.65 measurement: the process ended
 * 4 ms into the first window's close, before `closed` or will-quit could run.
 */
function electronQuit(opts: {
  registry: GooseServeLeaseRegistry;
  hold: () => QuitHold;
  windows: number[];
  dieAtFirstClose?: boolean;
}) {
  const steps: Step[] = [];
  let ended = false;
  const event = () => {
    let prevented = false;
    return { preventDefault: () => (prevented = true), prevented: () => prevented };
  };
  const emit = (door: QuitDoor) => {
    const e = event();
    steps.push(`${door}:${opts.hold().onQuitEvent(door, e)}`);
    return e.prevented();
  };
  const quit = () => {
    if (ended) return;
    if (emit('before-quit')) return;
    for (const id of opts.windows.splice(0)) {
      steps.push(`close ${id}`);
      if (opts.dieAtFirstClose) {
        ended = true;
        steps.push('process ended');
        return;
      }
      void opts.registry.releaseWindow(id);
      steps.push(`closed ${id}`);
    }
    if (emit('will-quit')) return;
    ended = true;
    steps.push('process ended');
  };
  return { steps, quit, ended: () => ended };
}

const flush = () => new Promise((resolve) => setTimeout(resolve, 0));

function setup({ closeWouldAsk = false } = {}) {
  const log = logger();
  const registry = new GooseServeLeaseRegistry(log);
  const goosed = fakeGoosed();
  registry.attachWindow(1, registry.create(goosed.result, 'secret'));
  let hold: QuitHold;
  const app = electronQuit({ registry, hold: () => hold, windows: [1] });
  hold = new QuitHold({
    backends: registry,
    logger: log,
    quit: app.quit,
    closeWouldAsk: () => closeWouldAsk,
  });
  return { log, registry, goosed, app, hold: () => hold };
}

describe('QuitHold — Q-241: goosed has exited before the app does', () => {
  it('holds at before-quit, before any window closes, and quits once goosed exits', async () => {
    const { app, goosed, log } = setup();

    app.quit();
    expect(app.steps).toEqual(['before-quit:held']);
    expect(goosed.cleanup).toHaveBeenCalledTimes(1);
    expect(app.ended()).toBe(false);

    goosed.exit();
    await flush();

    expect(app.steps).toEqual([
      'before-quit:held',
      'before-quit:passed',
      'close 1',
      'closed 1',
      'will-quit:passed',
      'process ended',
    ]);
    expect(log.lines[0]).toContain('App quitting (before-quit): waiting for 1 attached backend(s)');
    expect(log.lines).toContain('info App quitting: every goose serve backend has exited');
  });

  it('the 3.0.65 quit: the process ends at the first window close — goosed was already stopped', async () => {
    const log = logger();
    const registry = new GooseServeLeaseRegistry(log);
    const goosed = fakeGoosed();
    registry.attachWindow(1, registry.create(goosed.result, 'secret'));
    let hold: QuitHold;
    const app = electronQuit({ registry, hold: () => hold, windows: [1], dieAtFirstClose: true });
    hold = new QuitHold({
      backends: registry,
      logger: log,
      quit: app.quit,
      closeWouldAsk: () => false,
    });

    app.quit();
    // Nothing closes while goosed tears down: the window close that ended 3.0.65 has not happened.
    expect(app.steps).toEqual(['before-quit:held']);
    expect(app.ended()).toBe(false);

    goosed.exit();
    await flush();
    expect(app.steps).toEqual([
      'before-quit:held',
      'before-quit:passed',
      'close 1',
      'process ended',
    ]);
    // goosed's stop was asked (SIGTERM) and ANSWERED (its exit) while the app was whole.
    expect(goosed.cleanup).toHaveBeenCalledTimes(1);
    expect(log.lines).toContain('info App quitting: every goose serve backend has exited');
  });

  it('a second quit during the hold (Cmd+Q again, SIGTERM) waits on the same stop', async () => {
    const { app, goosed } = setup();

    app.quit();
    app.quit();
    expect(app.steps).toEqual(['before-quit:held', 'before-quit:held']);
    expect(goosed.cleanup).toHaveBeenCalledTimes(1);

    goosed.exit();
    await flush();
    expect(app.ended()).toBe(true);
  });

  it('a window whose close guard would ask goes first: no goosed is stopped before the question', async () => {
    const { app, goosed } = setup({ closeWouldAsk: true });

    app.quit();
    // before-quit passes; the window's close releases its lease (the run was confirmed away), and
    // will-quit waits for THAT stop.
    expect(app.steps).toEqual(['before-quit:passed', 'close 1', 'closed 1', 'will-quit:held']);
    expect(app.ended()).toBe(false);

    goosed.exit();
    await flush();
    expect(app.steps.slice(-3)).toEqual([
      'before-quit:passed',
      'will-quit:passed',
      'process ended',
    ]);
  });

  it('a goosed that never exits is abandoned by its own stop — loudly — and never holds the quit forever', async () => {
    const { app, goosed, log } = setup();

    app.quit();
    goosed.exit('abandoned');
    await flush();

    expect(app.ended()).toBe(true);
    expect(
      log.lines.some((l) =>
        l.startsWith('error App quitting: 1 goose serve backend(s) did not exit')
      )
    ).toBe(true);
  });

  it('with no backend the quit is never held', () => {
    const log = logger();
    const registry = new GooseServeLeaseRegistry(log);
    let hold: QuitHold;
    const app = electronQuit({ registry, hold: () => hold, windows: [] });
    hold = new QuitHold({
      backends: registry,
      logger: log,
      quit: app.quit,
      closeWouldAsk: () => false,
    });

    app.quit();
    expect(app.steps).toEqual(['before-quit:passed', 'will-quit:passed', 'process ended']);
  });

  it('a quit refused after the hold let it go holds again the next time', async () => {
    const { app, goosed, hold, registry, log } = setup();
    app.quit();
    goosed.exit();
    await flush();

    // A later window with a new goosed; the previous quit was refused by a close guard.
    const next = fakeGoosed();
    registry.attachWindow(2, registry.create(next.result, 'secret'));
    hold().quitRefused();
    const event = { preventDefault: vi.fn() };
    expect(hold().onQuitEvent('before-quit', event)).toBe('held');
    expect(event.preventDefault).toHaveBeenCalled();
    expect(log.lines.filter((l) => l.includes('waiting for'))).toHaveLength(2);
  });
});

// Q-257: every window shares the app's one goosed. The quit must stop that one goosed ONCE, wait for
// its exit with the app whole — the 3.0.65 shape (the process ends at the first window close) included
// — and the close guard must know a quit is under way, so a window sharing goosed still asks.
describe('QuitHold — one goosed shared by every window (Q-257)', () => {
  function sharedSetup(dieAtFirstClose = false) {
    const log = logger();
    const registry = new GooseServeLeaseRegistry(log);
    const goosed = fakeGoosed();
    const lease = registry.create(goosed.result, 'secret');
    registry.attachWindow(1, lease);
    registry.attachWindow(2, lease);
    let hold: QuitHold;
    const app = electronQuit({ registry, hold: () => hold, windows: [1, 2], dieAtFirstClose });
    hold = new QuitHold({
      backends: registry,
      logger: log,
      quit: app.quit,
      closeWouldAsk: () => false,
    });
    return { log, goosed, app, hold: () => hold };
  }

  it('two windows, one goosed: one stop, waited for before any window closes', async () => {
    const { app, goosed, log } = sharedSetup();

    app.quit();
    expect(app.steps).toEqual(['before-quit:held']);
    expect(goosed.cleanup).toHaveBeenCalledTimes(1);
    expect(log.lines[0]).toContain('waiting for 1 attached backend(s)');

    goosed.exit();
    await flush();
    expect(app.steps).toEqual([
      'before-quit:held',
      'before-quit:passed',
      'close 1',
      'closed 1',
      'close 2',
      'closed 2',
      'will-quit:passed',
      'process ended',
    ]);
    expect(goosed.cleanup).toHaveBeenCalledTimes(1);
  });

  it('the 3.0.65 shape with two windows: the shared goosed exited before the first close', async () => {
    const { app, goosed } = sharedSetup(true);

    app.quit();
    goosed.exit();
    await flush();
    expect(app.steps).toEqual([
      'before-quit:held',
      'before-quit:passed',
      'close 1',
      'process ended',
    ]);
    expect(goosed.cleanup).toHaveBeenCalledTimes(1);
  });

  it('isQuitting is set by the quit and cleared by a refused one', () => {
    const { app, hold } = sharedSetup();
    expect(hold().isQuitting()).toBe(false);
    app.quit();
    expect(hold().isQuitting()).toBe(true);
  });

  it('a quit the close guard refused is no longer a quit', () => {
    const log = logger();
    const registry = new GooseServeLeaseRegistry(log);
    const hold = new QuitHold({
      backends: registry,
      logger: log,
      quit: () => undefined,
      closeWouldAsk: () => true,
    });
    hold.onQuitEvent('before-quit', { preventDefault: vi.fn() });
    expect(hold.isQuitting()).toBe(true);
    hold.quitRefused();
    expect(hold.isQuitting()).toBe(false);
  });
});
