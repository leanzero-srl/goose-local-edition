import { describe, it, expect, beforeEach, vi } from 'vitest';
import { renderHook, waitFor, act } from '@testing-library/react';
import { useSwarmRun } from './useSwarmRun';

type ElectronMock = Record<string, unknown>;
const electron = () => (window as unknown as { electron: ElectronMock }).electron;
const reads = () => electron().readSwarmRun as ReturnType<typeof vi.fn>;

let listeners: Array<(d: { workingDir: string; runId: string; source: string }) => void>;
let unsubscribes: number;

beforeEach(() => {
  listeners = [];
  unsubscribes = 0;
  const e = electron();
  e.readSwarmRun = vi.fn(async () => null);
  e.onSwarmDelta = vi.fn(
    (cb: (d: { workingDir: string; runId: string; source: string }) => void) => {
      listeners.push(cb);
      return () => {
        unsubscribes++;
      };
    }
  );
});

const push = async (workingDir: string) => {
  await act(async () => {
    for (const l of listeners) l({ workingDir, runId: 'r1', source: `${workingDir}/.swarm` });
    await Promise.resolve();
  });
};

describe('useSwarmRun — the panel is pushed to, and still polls', () => {
  it('re-reads the moment main pushes a delta for its own run', async () => {
    renderHook(() => useSwarmRun('/tmp/build', 100_000));
    await waitFor(() => expect(reads()).toHaveBeenCalledTimes(1));

    await push('/tmp/build');
    await waitFor(() => expect(reads()).toHaveBeenCalledTimes(2));
  });

  it('ignores a delta for a different working directory', async () => {
    renderHook(() => useSwarmRun('/tmp/build', 100_000));
    await waitFor(() => expect(reads()).toHaveBeenCalledTimes(1));

    await push('/tmp/other');
    await new Promise((r) => setTimeout(r, 30));
    expect(reads()).toHaveBeenCalledTimes(1);
  });

  it('keeps the interval as the safety net when no delta ever arrives', async () => {
    renderHook(() => useSwarmRun('/tmp/build', 20));
    await waitFor(() => expect(reads().mock.calls.length).toBeGreaterThanOrEqual(3));
    expect(listeners).toHaveLength(1);
  });

  it('coalesces a burst of deltas into one queued re-read, and does not lose the last one', async () => {
    let resolveRead: (v: null) => void = () => {};
    reads().mockImplementation(
      () =>
        new Promise<null>((res) => {
          resolveRead = res;
        })
    );
    renderHook(() => useSwarmRun('/tmp/build', 100_000));
    await waitFor(() => expect(reads()).toHaveBeenCalledTimes(1));

    await push('/tmp/build');
    await push('/tmp/build');
    await push('/tmp/build');
    expect(reads()).toHaveBeenCalledTimes(1);

    await act(async () => {
      resolveRead(null);
      await Promise.resolve();
    });
    await waitFor(() => expect(reads()).toHaveBeenCalledTimes(2));
  });

  it('unsubscribes on unmount, so a dead panel cannot be pushed to', async () => {
    const { unmount } = renderHook(() => useSwarmRun('/tmp/build', 100_000));
    await waitFor(() => expect(reads()).toHaveBeenCalledTimes(1));
    unmount();
    expect(unsubscribes).toBe(1);

    const before = reads().mock.calls.length;
    await push('/tmp/build');
    expect(reads().mock.calls.length).toBe(before);
  });
});
