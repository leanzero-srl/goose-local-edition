import { act, renderHook, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { resetFoldCache, useSwarmRun } from './useSwarmRun';
import { engineLiveness, isEngineSilent, SWARM_HEARTBEAT_STALE_MS } from './swarmRunLiveness';

/**
 * U-M5 (branch review, 2026-09-01): `sourceMissing` was set and rendered by nothing, while the
 * heartbeat read from the vanished files stayed in state and kept aging — so 45s after a run's files
 * were archived or deleted the panel raised the "No heartbeat… most likely hard-killed; nothing
 * discarded" banner and relabelled running rows "interrupted". A vanished run must read as liveness
 * UNKNOWN with its own distinct state, never as a hard-killed engine.
 */

type Delta = { workingDir: string };
type RunPayload = {
  runId: string;
  events: Array<Record<string, unknown>>;
  activity: Record<string, unknown>;
  activityMtimes: Record<string, number>;
  heartbeat: number | null;
  heartbeatExited: boolean;
};

const electron = () => (window as unknown as { electron: Record<string, unknown> }).electron;

const RUN_TS = '2026-08-29T10:00:00.000Z';
const payload = (heartbeat: number): RunPayload => ({
  runId: 'r1',
  events: [
    { event: 'run_started', ts: RUN_TS, pool: [] },
    { event: 'task_dispatched', ts: RUN_TS, task_id: 't0', model: 'mihai-qwen' },
  ],
  activity: {},
  activityMtimes: {},
  heartbeat,
  heartbeatExited: false,
});

describe('useSwarmRun — a run whose files vanished is not a hard-killed engine', () => {
  beforeEach(() => resetFoldCache());
  afterEach(() => {
    delete electron().readSwarmRun;
    electron().onSwarmDelta = vi.fn(() => () => {});
  });

  const withDelta = () => {
    let pushDelta: ((d: Delta) => void) | undefined;
    electron().onSwarmDelta = vi.fn((cb: (d: Delta) => void) => {
      pushDelta = cb;
      return () => {};
    });
    return () => pushDelta;
  };

  it('nulls the heartbeat with sourceMissing, so liveness reads unknown — not silent — however long ago it was read', async () => {
    const readAt = Date.now();
    let next: RunPayload | null = payload(readAt - 2_000);
    electron().readSwarmRun = vi.fn(async () => next);
    const delta = withDelta();

    const { result } = renderHook(() => useSwarmRun('/tmp/gone', 10_000_000));
    await waitFor(() => expect(result.current.present).toBe(true));
    expect(engineLiveness(result.current, readAt).state).toBe('alive');

    next = null;
    await act(async () => delta()?.({ workingDir: '/tmp/gone' }));
    await waitFor(() => expect(result.current.sourceMissing).toBe(true));

    // The distinct state is set, the run is still held on screen…
    expect(result.current.present).toBe(true);
    expect(result.current.totals.tasks).toBe(1);
    // …and the stale heartbeat claim is gone WITH the files: a minute later the old code said 'silent'.
    const later = readAt + SWARM_HEARTBEAT_STALE_MS + 60_000;
    expect(result.current.heartbeat).toBeNull();
    expect(engineLiveness(result.current, later).state).toBe('unknown');
    expect(isEngineSilent(result.current, later)).toBe(false);
  });

  it('a run that resolves again restores its heartbeat and clears the state', async () => {
    let next: RunPayload | null = payload(Date.now() - 1_000);
    electron().readSwarmRun = vi.fn(async () => next);
    const delta = withDelta();

    const { result } = renderHook(() => useSwarmRun('/tmp/gone', 10_000_000));
    await waitFor(() => expect(result.current.present).toBe(true));
    next = null;
    await act(async () => delta()?.({ workingDir: '/tmp/gone' }));
    await waitFor(() => expect(result.current.sourceMissing).toBe(true));

    const back = Date.now() - 500;
    next = payload(back);
    await act(async () => delta()?.({ workingDir: '/tmp/gone' }));
    await waitFor(() => expect(result.current.sourceMissing).toBe(false));
    expect(result.current.heartbeat).toBe(back);
    expect(engineLiveness(result.current, Date.now()).state).toBe('alive');
  });
});
