import { act, renderHook, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { resetFoldCache, useSwarmRun } from './useSwarmRun';

/**
 * THE PANEL MAY NEVER GO BACKWARDS.
 *
 * `useSwarmRun` awaits an IPC round trip and then setStates what it read. Reads are driven by a 500ms
 * interval AND by main's fs.watch deltas, so with nothing serialising them an older read can resolve LAST
 * and the panel regresses — done rows back to running, a lane count back down, the ribbon back a stage —
 * off data that was already stale when it landed. The hook's single-flight latch is what makes that
 * unrepresentable, and this test is why it may not be "simplified" away: it fails the moment two reads are
 * allowed to be in flight at once.
 *
 * A long `pollMs` keeps the safety-net interval out of these tests; every read here is triggered explicitly.
 */

type Delta = { workingDir: string };
type RunPayload = {
  runId: string;
  events: Array<Record<string, unknown>>;
  activity: Record<string, unknown>;
  activityMtimes: Record<string, number>;
};

const electron = () => (window as unknown as { electron: Record<string, unknown> }).electron;

const RUN_TS = '2026-08-29T10:00:00.000Z';
const started = Date.parse(RUN_TS);

/** A run with `tasks` dispatched tasks — `totals.tasks` is the lane count the panel renders. */
const payload = (tasks: number, extra?: Partial<RunPayload>): RunPayload => ({
  runId: 'r1',
  events: [
    { event: 'run_started', ts: RUN_TS, pool: [] },
    ...Array.from({ length: tasks }, (_, i) => ({
      event: 'task_dispatched',
      ts: RUN_TS,
      task_id: `t${i}`,
      model: 'mihai-qwen',
    })),
  ],
  activity: {},
  activityMtimes: {},
  ...extra,
});

describe('useSwarmRun — reads are single-flight, so the panel never regresses', () => {
  beforeEach(() => resetFoldCache());
  afterEach(() => {
    delete electron().readSwarmRun;
    electron().onSwarmDelta = vi.fn(() => () => {});
  });

  it('refuses to start a second read while one is in flight, however many deltas arrive', async () => {
    const resolvers: Array<(p: RunPayload) => void> = [];
    let inFlight = 0;
    let maxInFlight = 0;
    electron().readSwarmRun = vi.fn(() => {
      inFlight += 1;
      maxInFlight = Math.max(maxInFlight, inFlight);
      return new Promise<RunPayload>((resolve) => {
        resolvers.push((p) => {
          inFlight -= 1;
          resolve(p);
        });
      });
    });
    let pushDelta: ((d: Delta) => void) | undefined;
    electron().onSwarmDelta = vi.fn((cb: (d: Delta) => void) => {
      pushDelta = cb;
      return () => {};
    });

    const { result } = renderHook(() => useSwarmRun('/tmp/run', 10_000_000));
    expect(resolvers).toHaveLength(1);

    // A burst of fs.watch deltas while read #1 is still out. Each one must coalesce, not race.
    await act(async () => {
      pushDelta?.({ workingDir: '/tmp/run' });
      pushDelta?.({ workingDir: '/tmp/run' });
      pushDelta?.({ workingDir: '/tmp/run' });
    });
    expect(maxInFlight, 'two reads must never be in flight at once').toBe(1);
    expect(resolvers).toHaveLength(1);

    // Read #1 lands with four lanes.
    await act(async () => {
      resolvers[0](payload(4));
    });
    await waitFor(() => expect(result.current.totals.tasks).toBe(4));

    // The coalesced burst is not dropped: exactly one queued read follows, and it carries the newer state.
    await waitFor(() => expect(resolvers).toHaveLength(2));
    await act(async () => {
      resolvers[1](payload(6));
    });
    await waitFor(() => expect(result.current.totals.tasks).toBe(6));
    expect(maxInFlight).toBe(1);
  });

  it('drops a previous run’s leftover digests instead of minting lanes from them', async () => {
    // `.swarm/activity/` is not cleared between runs — the engine truncates only `.swarm/prereview` — and
    // main globs the whole directory, so a second run in the same directory inherits the last run's digests.
    electron().readSwarmRun = vi.fn(async () =>
      payload(0, {
        activity: {
          'slice-old': { phase: 'processing', thinking_chars: 900 },
          'slice-live': { phase: 'processing', thinking_chars: 900 },
        },
        activityMtimes: { 'slice-old': started - 60_000, 'slice-live': started + 60_000 },
      })
    );
    electron().onSwarmDelta = vi.fn(() => () => {});

    const { result } = renderHook(() => useSwarmRun('/tmp/run', 10_000_000));
    await waitFor(() => expect(result.current.present).toBe(true));
    expect(result.current.sliceLanes.map((l) => l.taskId)).toStrictEqual(['slice-live']);
    expect(Object.keys(result.current.activityDigests)).toStrictEqual(['slice-live']);
    expect(Object.keys(result.current.activityMtimes)).toStrictEqual(['slice-live']);
  });
});
