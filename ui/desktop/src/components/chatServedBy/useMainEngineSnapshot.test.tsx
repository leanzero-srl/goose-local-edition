import { act, render } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useMainEngineSnapshot } from './useChatServedBy';
import { MLX_STATUS_POLL_MS } from '../leanzero-swarm/mlxLiveStats';
import {
  INITIAL_SNAPSHOT,
  MLX_ENGINE_SNAPSHOT_CHANNEL,
  type MlxEngineSnapshot,
} from '../../utils/mlxEngineMonitor';

const activity = vi.fn();
const pushListeners = new Set<(event: unknown, ...args: unknown[]) => void>();

async function advance(ms: number) {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(ms);
  });
}

beforeEach(() => {
  vi.useFakeTimers();
  activity.mockReset().mockResolvedValue({ ...INITIAL_SNAPSHOT, mode: 'off' });
  pushListeners.clear();
  (window as unknown as { electron: unknown }).electron = {
    mlxEngineActivity: () => activity(),
    on: (channel: string, fn: (event: unknown, ...args: unknown[]) => void) => {
      if (channel === MLX_ENGINE_SNAPSHOT_CHANNEL) pushListeners.add(fn);
    },
    off: (_channel: string, fn: (event: unknown, ...args: unknown[]) => void) => {
      pushListeners.delete(fn);
    },
  };
});

afterEach(() => {
  vi.useRealTimers();
});

/**
 * Q-208: every open chat keeps its composer mounted (ChatSessionsContainer renders each active
 * session, hidden or not), and each composer asked main for the engine snapshot on its own timer.
 */
describe('main’s engine snapshot: one ask per tick for every composer in the window', () => {
  it('four composers ask main once per tick', async () => {
    function Composer() {
      useMainEngineSnapshot(true);
      return null;
    }
    const view = render(
      <>
        <Composer />
        <Composer />
        <Composer />
        <Composer />
      </>
    );
    await advance(0);
    expect(activity).toHaveBeenCalledTimes(1);
    await advance(MLX_STATUS_POLL_MS * 3);
    expect(activity).toHaveBeenCalledTimes(4);
    view.unmount();
    await advance(MLX_STATUS_POLL_MS * 3);
    expect(activity).toHaveBeenCalledTimes(4);
  });

  it('a push from main reaches every composer, and a failed ask says nothing rather than idle', async () => {
    const seen: Array<MlxEngineSnapshot | null> = [];
    function Composer({ at }: { at: number }) {
      seen[at] = useMainEngineSnapshot(true);
      return null;
    }
    render(
      <>
        <Composer at={0} />
        <Composer at={1} />
      </>
    );
    await advance(0);
    expect(seen.map((s) => s?.mode)).toEqual(['off', 'off']);
    const pushed: MlxEngineSnapshot = { ...INITIAL_SNAPSHOT, mode: 'running' };
    await act(async () => {
      for (const fn of pushListeners) fn(undefined, pushed);
    });
    expect(seen.map((s) => s?.mode)).toEqual(['running', 'running']);
    activity.mockRejectedValue(new Error('main is gone'));
    await advance(MLX_STATUS_POLL_MS);
    expect(seen).toEqual([null, null]);
  });
});
