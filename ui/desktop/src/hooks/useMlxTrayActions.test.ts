import { beforeEach, describe, expect, it, vi } from 'vitest';

const mount = vi.fn();
const unmount = vi.fn();
const status = vi.fn();
const settingsRead = vi.fn();

vi.mock('../acp/mlx-engine', () => ({
  mlxEngineMount: (...a: unknown[]) => mount(...a),
  mlxEngineUnmount: (...a: unknown[]) => unmount(...a),
  mlxEngineStatus: (...a: unknown[]) => status(...a),
  mlxEngineSettingsRead: (...a: unknown[]) => settingsRead(...a),
}));
vi.mock('../components/leanzero-swarm/mlxLiveStats', () => ({ MLX_STATUS_POLL_MS: 0 }));
const distributedStop = vi.fn();
const distributedStatus = vi.fn();
const store = vi.hoisted(() => {
  let latest: { mode: string } | null = null;
  const listeners = new Set<() => void>();
  return {
    latest: () => latest,
    subscribe: (fn: () => void) => {
      listeners.add(fn);
      return () => listeners.delete(fn);
    },
    publish: (status: { mode: string } | null) => {
      latest = status;
      for (const fn of listeners) fn();
    },
  };
});
vi.mock('../acp/mlx-distributed', () => ({
  mlxDistributedStop: (...a: unknown[]) => distributedStop(...a),
  mlxDistributedStatus: (...a: unknown[]) => distributedStatus(...a),
  latestMlxDistributedStatus: () => store.latest(),
  subscribeMlxDistributedStatus: (fn: () => void) => store.subscribe(fn),
}));

import { renderHook, waitFor } from '@testing-library/react';
import {
  DistributedStopNotVerified,
  runMlxTrayAction,
  useMlxDistributedReporter,
} from './useMlxTrayActions';

describe('runMlxTrayAction — the tray’s Mount/Unmount through the renderer’s ACP client', () => {
  beforeEach(() => {
    mount.mockReset().mockResolvedValue(undefined);
    unmount.mockReset().mockResolvedValue(undefined);
    status.mockReset();
    settingsRead.mockReset();
  });

  it('mounts the configured model and reads status until it leaves "mounting" (each read reaches main)', async () => {
    settingsRead.mockResolvedValue({ modelId: 'org/qwen' });
    status
      .mockResolvedValueOnce({ state: 'mounting' })
      .mockResolvedValueOnce({ state: 'mounting' })
      .mockResolvedValueOnce({ state: 'running' });
    await runMlxTrayAction('mount');
    expect(mount).toHaveBeenCalledWith('org/qwen');
    expect(status).toHaveBeenCalledTimes(3);
  });

  it('a mount that fails ends on the failed read — the loop is bounded by the state, not a clock', async () => {
    settingsRead.mockResolvedValue({ modelId: 'org/qwen' });
    status.mockResolvedValueOnce({ state: 'mounting' }).mockResolvedValueOnce({ state: 'failed' });
    await runMlxTrayAction('mount');
    expect(status).toHaveBeenCalledTimes(2);
  });

  it('no configured model is a named refusal, and nothing is mounted', async () => {
    settingsRead.mockResolvedValue({});
    await expect(runMlxTrayAction('mount')).rejects.toThrow('no-model');
    expect(mount).not.toHaveBeenCalled();
  });

  it('unmount unmounts and reads the status once so main sees the engine go', async () => {
    status.mockResolvedValue({ state: 'stopped' });
    await runMlxTrayAction('unmount');
    expect(unmount).toHaveBeenCalledTimes(1);
    expect(status).toHaveBeenCalledTimes(1);
  });

  it('stop-distributed runs the verified stop once; an unverified stop is an error with its steps', async () => {
    distributedStop.mockResolvedValueOnce({ stop: { steps: ['rank 0 gone'], verified: true } });
    await runMlxTrayAction('stop-distributed');
    expect(distributedStop).toHaveBeenCalledTimes(1);
    expect(unmount).not.toHaveBeenCalled();

    distributedStop.mockResolvedValueOnce({
      stop: { steps: ['SIGTERM rank 1 pid 5521 → STILL ALIVE'], verified: false },
    });
    const error = await runMlxTrayAction('stop-distributed').catch((e: unknown) => e);
    expect(error).toBeInstanceOf(DistributedStopNotVerified);
    expect((error as DistributedStopNotVerified).steps).toEqual([
      'SIGTERM rank 1 pid 5521 → STILL ALIVE',
    ]);
  });
});

describe('useMlxDistributedReporter — keeps main’s copy of the distributed engine current', () => {
  const listeners = new Map<string, () => void>();
  beforeEach(() => {
    listeners.clear();
    distributedStatus.mockReset();
    (window as unknown as { electron: unknown }).electron = {
      on: (channel: string, fn: () => void) => listeners.set(channel, fn),
      off: (channel: string) => listeners.delete(channel),
    };
  });

  it('reads while the run owns the Mac, stops when it does not, and reads again on a tray wake', async () => {
    distributedStatus
      .mockResolvedValueOnce({ mode: 'distributed' })
      .mockResolvedValueOnce({ mode: 'distributed' })
      .mockResolvedValue({ mode: 'single' });
    const { unmount } = renderHook(() => useMlxDistributedReporter(true));
    await waitFor(() => expect(distributedStatus).toHaveBeenCalledTimes(3));
    await new Promise((r) => setTimeout(r, 20));
    expect(distributedStatus).toHaveBeenCalledTimes(3);
    listeners.get('mlx-distributed-wake')?.();
    await waitFor(() => expect(distributedStatus).toHaveBeenCalledTimes(4));
    unmount();
    expect(listeners.has('mlx-distributed-wake')).toBe(false);
  });

  it('a run another read saw first (the Engine tab started it) pulls the reporter into its loop', async () => {
    distributedStatus.mockResolvedValue({ mode: 'single' });
    renderHook(() => useMlxDistributedReporter(true));
    await waitFor(() => expect(distributedStatus).toHaveBeenCalledTimes(1));
    await new Promise((r) => setTimeout(r, 20));
    expect(distributedStatus).toHaveBeenCalledTimes(1);
    distributedStatus
      .mockResolvedValueOnce({ mode: 'distributed' })
      .mockResolvedValue({ mode: 'single' });
    store.publish({ mode: 'distributed' });
    await waitFor(() => expect(distributedStatus).toHaveBeenCalledTimes(3));
    await new Promise((r) => setTimeout(r, 20));
    expect(distributedStatus).toHaveBeenCalledTimes(3);
  });

  it('without the capability nothing is read', async () => {
    renderHook(() => useMlxDistributedReporter(false));
    await new Promise((r) => setTimeout(r, 10));
    expect(distributedStatus).not.toHaveBeenCalled();
  });
});
