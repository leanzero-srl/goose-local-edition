import { describe, it, expect } from 'vitest';
import {
  shouldRefuseShortcut,
  isBenchmarkViewUrl,
  isSwarmRunStampAlive,
  shortcutRefusalReason,
} from '../shortcutGuard';
import type { GuardedShortcutAction } from '../shortcutGuard';
import { SWARM_HEARTBEAT_STALE_MS } from '../../components/swarm/swarmRunLiveness';

const actions: GuardedShortcutAction[] = ['spawn', 'close', 'quit', 'navigate', 'reload'];

describe('shouldRefuseShortcut', () => {
  it('refuses nothing when no benchmark is running', () => {
    for (const action of actions) {
      expect(
        shouldRefuseShortcut({
          action,
          benchmarkRunning: false,
          triggeredByAccelerator: true,
          onBenchmarkView: true,
        })
      ).toBe(false);
    }
  });

  it('never refuses a mouse click on the menu item, even mid-run on the benchmark window', () => {
    for (const action of actions) {
      expect(
        shouldRefuseShortcut({
          action,
          benchmarkRunning: true,
          triggeredByAccelerator: false,
          onBenchmarkView: true,
        })
      ).toBe(false);
    }
  });

  it('refuses spawn and quit accelerators from any window while a run is live', () => {
    for (const action of ['spawn', 'quit'] as const) {
      for (const onBenchmarkView of [true, false]) {
        expect(
          shouldRefuseShortcut({
            action,
            benchmarkRunning: true,
            triggeredByAccelerator: true,
            onBenchmarkView,
          })
        ).toBe(true);
      }
    }
  });

  it('refuses close, navigate and reload accelerators only on the window showing the run', () => {
    for (const action of ['close', 'navigate', 'reload'] as const) {
      expect(
        shouldRefuseShortcut({
          action,
          benchmarkRunning: true,
          triggeredByAccelerator: true,
          onBenchmarkView: true,
        })
      ).toBe(true);
      expect(
        shouldRefuseShortcut({
          action,
          benchmarkRunning: true,
          triggeredByAccelerator: true,
          onBenchmarkView: false,
        })
      ).toBe(false);
    }
  });
});

describe('isBenchmarkViewUrl', () => {
  it('recognises the hash-router benchmark route with or without a query or subpath', () => {
    expect(isBenchmarkViewUrl('file:///Applications/Goose.app/index.html#/benchmark')).toBe(true);
    expect(isBenchmarkViewUrl('http://localhost:5173/#/benchmark?tier=sb-7')).toBe(true);
    expect(isBenchmarkViewUrl('file:///x/index.html#/benchmark/live')).toBe(true);
  });

  it('rejects every other route, including ones that merely contain the word', () => {
    expect(isBenchmarkViewUrl('file:///x/index.html#/')).toBe(false);
    expect(isBenchmarkViewUrl('file:///x/index.html#/settings')).toBe(false);
    expect(isBenchmarkViewUrl('file:///x/index.html#/benchmarks')).toBe(false);
    expect(isBenchmarkViewUrl('file:///x/benchmark/index.html')).toBe(false);
    expect(isBenchmarkViewUrl('')).toBe(false);
  });
});

/**
 * U-H1 (branch review, 2026-09-01): the guard fed on `activeBenchRun` only, so a SESSION-driven swarm
 * run — `goose swarm run` under the window's goose serve lease — was unguarded: Cmd+N opened a second
 * backend and Cmd+W released the lease, whose cleanup signals goosed's process group and KILLS the run.
 * `close` was also benchmark-window-only, so it needed the #/benchmark hash a session window is never
 * on. The corrected feed is the per-run heartbeat stamp main caches from the renderer's own poll.
 */
describe('shouldRefuseShortcut — a session-driven run is protected by the same guard', () => {
  const noBench = { benchmarkRunning: false, onBenchmarkView: false } as const;

  it('refuses spawn and quit accelerators from any window while a session run is live', () => {
    for (const action of ['spawn', 'quit'] as const) {
      for (const windowHoldsLiveRun of [true, false]) {
        expect(
          shouldRefuseShortcut({
            ...noBench,
            action,
            triggeredByAccelerator: true,
            sessionRunLive: true,
            windowHoldsLiveRun,
          })
        ).toBe(true);
      }
    }
  });

  it('refuses close ONLY on the window whose renderer holds the live run', () => {
    expect(
      shouldRefuseShortcut({
        ...noBench,
        action: 'close',
        triggeredByAccelerator: true,
        sessionRunLive: true,
        windowHoldsLiveRun: true,
      })
    ).toBe(true);
    expect(
      shouldRefuseShortcut({
        ...noBench,
        action: 'close',
        triggeredByAccelerator: true,
        sessionRunLive: true,
        windowHoldsLiveRun: false,
      })
    ).toBe(false);
  });

  it('leaves navigate and reload alone on a session window: neither releases the lease', () => {
    for (const action of ['navigate', 'reload'] as const) {
      expect(
        shouldRefuseShortcut({
          ...noBench,
          action,
          triggeredByAccelerator: true,
          sessionRunLive: true,
          windowHoldsLiveRun: true,
        })
      ).toBe(false);
    }
  });

  it('never refuses a mouse click, even on the window holding the run', () => {
    for (const action of actions) {
      expect(
        shouldRefuseShortcut({
          ...noBench,
          action,
          triggeredByAccelerator: false,
          sessionRunLive: true,
          windowHoldsLiveRun: true,
        })
      ).toBe(false);
    }
  });

  it('fails OPEN when the session feed is absent (legacy callers pass neither flag)', () => {
    for (const action of actions) {
      expect(
        shouldRefuseShortcut({ ...noBench, action, triggeredByAccelerator: true })
      ).toBe(false);
    }
  });

  it('names which run a refusal protects, so the notice can say so', () => {
    expect(shortcutRefusalReason(true)).toBe('benchmark');
    expect(shortcutRefusalReason(false)).toBe('session-run');
  });
});

describe('isSwarmRunStampAlive — the cached stamp decays with the poll that wrote it', () => {
  const NOW = 1_800_000_000_000;

  it('a fresh heartbeat stamp is alive', () => {
    expect(isSwarmRunStampAlive({ heartbeat: NOW - 5_000, heartbeatExited: false }, NOW)).toBe(true);
  });

  it('a stamp older than the liveness window is dead — the same window as the banner, no new literal', () => {
    expect(
      isSwarmRunStampAlive(
        { heartbeat: NOW - SWARM_HEARTBEAT_STALE_MS - 1, heartbeatExited: false },
        NOW
      )
    ).toBe(false);
    expect(
      isSwarmRunStampAlive({ heartbeat: NOW - SWARM_HEARTBEAT_STALE_MS, heartbeatExited: false }, NOW)
    ).toBe(true);
  });

  it('an EXITED stamp is dead at once, however fresh', () => {
    expect(isSwarmRunStampAlive({ heartbeat: NOW - 1_000, heartbeatExited: true }, NOW)).toBe(false);
  });

  it('no stamp, or a run with no heartbeat file, is not a live run', () => {
    expect(isSwarmRunStampAlive(undefined, NOW)).toBe(false);
    expect(isSwarmRunStampAlive({ heartbeat: null, heartbeatExited: false }, NOW)).toBe(false);
  });
});
