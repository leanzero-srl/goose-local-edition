import { act, render } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { MlxEngineStatus } from '../../acp/mlx-engine';
import type { MlxDistributedStatus } from '../../acp/mlx-distributed';
import { useMlxEngineStatusPoll } from './useMlxEngineStatus';
import { useMlxDistributedStatus } from './useMlxDistributedStatus';
import { createSharedPoll, useSharedPoll } from './sharedStatusPoll';

const engineStatus = vi.fn();
vi.mock('../../acp/mlx-engine', () => ({
  mlxEngineStatus: (...args: unknown[]) => engineStatus(...args),
}));
const distributedStatus = vi.fn();
vi.mock('../../acp/mlx-distributed', () => ({
  mlxDistributedStatus: (...args: unknown[]) => distributedStatus(...args),
}));

const RUNNING = {
  state: 'running',
  modelId: 'mlx-community/Qwen3-30B-A3B-4bit',
  servedModelId: 'qwen3-30b-served',
  restartRequired: false,
  availableMemoryGb: 40,
  totalMemoryGb: 64,
} as MlxEngineStatus;
const SPLIT = { mode: 'distributed', state: 'serving' } as unknown as MlxDistributedStatus;

async function advance(ms: number) {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(ms);
  });
}

beforeEach(() => {
  vi.useFakeTimers();
  engineStatus.mockReset().mockResolvedValue(RUNNING);
  distributedStatus.mockReset().mockResolvedValue(SPLIT);
});

afterEach(() => {
  vi.useRealTimers();
});

/**
 * Q-208's proof: the surfaces a window mounts at once — every open chat's composer (the served-by
 * chip at 2 s, the model bar at 5 s, the no-node notice at 2 s), the Nodes table, the fleet — each
 * ran its own timer on the same engine status. N watchers must cost ONE read per tick.
 */
describe('one engine-status read per tick, however many surfaces watch it', () => {
  function ComposerSurfaces({ copies }: { copies: number }) {
    return (
      <>
        {Array.from({ length: copies }, (_, i) => (
          <Composer key={i} />
        ))}
      </>
    );
  }
  function Composer() {
    useMlxEngineStatusPoll(true, 2000);
    useMlxEngineStatusPoll(true, 5000);
    useMlxEngineStatusPoll(true, 2000);
    return null;
  }

  it('three open chats (nine watchers) read the engine once per 2 s tick', async () => {
    const view = render(<ComposerSurfaces copies={3} />);
    await advance(0);
    expect(engineStatus).toHaveBeenCalledTimes(1);
    await advance(10_000);
    // The first read plus one per 2-second tick — the shortest interval any watcher asked for.
    expect(engineStatus).toHaveBeenCalledTimes(6);
    view.unmount();
    await advance(10_000);
    expect(engineStatus).toHaveBeenCalledTimes(6);
  });

  it('watchers at 5 s only are read at 5 s, and a 2 s watcher joining speeds the one read up', async () => {
    function Slow() {
      useMlxEngineStatusPoll(true, 5000);
      useMlxEngineStatusPoll(true, 5000);
      return null;
    }
    function Fast({ on }: { on: boolean }) {
      useMlxEngineStatusPoll(on, 2000);
      return null;
    }
    const view = render(
      <>
        <Slow />
        <Fast on={false} />
      </>
    );
    await advance(10_000);
    expect(engineStatus).toHaveBeenCalledTimes(3);
    view.rerender(
      <>
        <Slow />
        <Fast on />
      </>
    );
    await advance(0);
    // The faster watcher is not served by the running reads: it gets one now.
    expect(engineStatus).toHaveBeenCalledTimes(4);
    await advance(4_000);
    expect(engineStatus).toHaveBeenCalledTimes(6);
  });

  it('every watcher sees the same answer, and a failed read clears it for all of them', async () => {
    const seen: Array<{ status: MlxEngineStatus | null; error: string | null }> = [];
    function Probe({ at }: { at: number }) {
      seen[at] = useMlxEngineStatusPoll(true, at === 0 ? 2000 : 5000);
      return null;
    }
    render(
      <>
        <Probe at={0} />
        <Probe at={1} />
      </>
    );
    await advance(0);
    expect(seen.map((s) => s.status?.servedModelId)).toEqual([
      'qwen3-30b-served',
      'qwen3-30b-served',
    ]);
    engineStatus.mockRejectedValue(new Error('agent connection lost'));
    await advance(2_000);
    expect(seen.map((s) => s.status)).toEqual([null, null]);
    expect(seen.every((s) => s.error?.includes('agent connection lost'))).toBe(true);
  });

  it('the Engine tab’s fit rides the one read; the other watchers read that same answer', async () => {
    const poll = createSharedPoll<string, string>(async (fit) => `read:${fit ?? 'none'}`);
    const seen: Array<string | null> = [];
    function Watch({ at, fit, ms }: { at: number; fit?: string; ms: number }) {
      seen[at] = useSharedPoll(poll, { intervalMs: ms, arg: fit ?? null })?.value ?? null;
      return null;
    }
    render(
      <>
        <Watch at={0} ms={5000} />
        <Watch at={1} ms={2000} fit="qwen" />
      </>
    );
    await advance(0);
    expect(seen).toEqual(['read:qwen', 'read:qwen']);
    expect(poll.readsStarted()).toBe(1);
    await advance(2_000);
    expect(poll.readsStarted()).toBe(2);
  });
});

describe('one split-status read per tick for the Engine tab and My Macs', () => {
  it('two watchers of the split read it once per tick', async () => {
    function Both() {
      useMlxDistributedStatus(true);
      useMlxDistributedStatus(true);
      return null;
    }
    render(<Both />);
    await advance(0);
    expect(distributedStatus).toHaveBeenCalledTimes(1);
    await advance(4_000);
    expect(distributedStatus).toHaveBeenCalledTimes(3);
  });
});

describe('the shared store keeps the old hooks’ truth rules', () => {
  it('a read overtaken by a newer one is dropped, never shown after it', async () => {
    let resolveFirst: (v: string) => void = () => undefined;
    const reads = [
      new Promise<string>((r) => {
        resolveFirst = r;
      }),
      Promise.resolve('second'),
    ];
    const poll = createSharedPoll<string>(() => reads.shift() ?? Promise.resolve('later'));
    const seen: Array<string | null> = [];
    function Watch() {
      seen.push(useSharedPoll(poll, { intervalMs: 60_000 })?.value ?? null);
      return null;
    }
    render(<Watch />);
    // The mount's read takes the slow answer; the refresh after it takes the fast one.
    await advance(0);
    await act(async () => {
      await poll.refresh();
    });
    expect(seen[seen.length - 1]).toBe('second');
    await act(async () => {
      resolveFirst('first');
    });
    expect(seen[seen.length - 1]).toBe('second');
  });

  it('the last watcher leaving forgets the answer: the next one never sees a read nobody took', async () => {
    let asked = 0;
    const poll = createSharedPoll<number>(async () => ++asked);
    let value: number | null = null;
    function Watch() {
      value = useSharedPoll(poll, { intervalMs: 1000 })?.value ?? null;
      return null;
    }
    const first = render(<Watch />);
    await advance(0);
    expect(value).toBe(1);
    first.unmount();
    render(<Watch />);
    expect(value).toBeNull();
    await advance(0);
    expect(value).toBe(2);
  });

  it('a hidden window stops the reads unless a watcher keeps them going (the tray reporter)', async () => {
    let asked = 0;
    const poll = createSharedPoll<number>(async () => ++asked);
    const hide = (hidden: boolean) => {
      Object.defineProperty(document, 'visibilityState', {
        configurable: true,
        get: () => (hidden ? 'hidden' : 'visible'),
      });
      document.dispatchEvent(new Event('visibilitychange'));
    };
    function Watch({ whileHidden }: { whileHidden: boolean }) {
      useSharedPoll(poll, { intervalMs: 1000, whileHidden });
      return null;
    }
    try {
      const view = render(<Watch whileHidden={false} />);
      await advance(0);
      hide(true);
      const before = asked;
      await advance(5_000);
      expect(asked).toBe(before);
      view.rerender(<Watch whileHidden />);
      await advance(3_000);
      expect(asked).toBeGreaterThan(before);
    } finally {
      hide(false);
    }
  });
});
