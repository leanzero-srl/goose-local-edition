/**
 * @vitest-environment jsdom
 */
import { render } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import BenchmarkAutoOpen from './BenchmarkAutoOpen';

const navigate = vi.fn();
let pathname = '/';

vi.mock('react-router-dom', () => ({
  useNavigate: () => navigate,
  useLocation: () => ({ pathname, state: null }),
}));

type Listener = (event: unknown, payload: unknown) => void;

const listeners = new Map<string, Listener[]>();
const benchmarkStatus = vi.fn();
const electron = {
  benchmarkStatus,
  on: vi.fn((channel: string, cb: Listener) => {
    listeners.set(channel, [...(listeners.get(channel) ?? []), cb]);
  }),
  off: vi.fn((channel: string, cb: Listener) => {
    listeners.set(
      channel,
      (listeners.get(channel) ?? []).filter((l) => l !== cb)
    );
  }),
};

const emit = (channel: string) => {
  for (const l of [...(listeners.get(channel) ?? [])]) l(null, {});
};

const flush = () => new Promise((r) => setTimeout(r, 0));

beforeEach(() => {
  pathname = '/';
  listeners.clear();
  navigate.mockClear();
  electron.on.mockClear();
  electron.off.mockClear();
  benchmarkStatus.mockReset().mockResolvedValue({ running: false });
  (window as unknown as { electron: typeof electron }).electron = electron;
});

afterEach(() => {
  vi.clearAllMocks();
});

describe('BenchmarkAutoOpen', () => {
  it('opens the benchmark view for a run that was already in flight at mount', async () => {
    benchmarkStatus.mockResolvedValue({ running: true, workdir: '/w' });

    render(<BenchmarkAutoOpen />);
    await flush();

    expect(navigate).toHaveBeenCalledWith('/benchmark');
  });

  it('stays put when nothing is running', async () => {
    render(<BenchmarkAutoOpen />);
    await flush();

    expect(navigate).not.toHaveBeenCalled();
  });

  it('stays put when the window did not open on the default route', async () => {
    pathname = '/settings';
    benchmarkStatus.mockResolvedValue({ running: true });

    render(<BenchmarkAutoOpen />);
    await flush();

    expect(navigate).not.toHaveBeenCalled();
  });

  it('never yanks a user who walked back to the chat view during a live run', async () => {
    benchmarkStatus.mockResolvedValue({ running: true });

    const { rerender } = render(<BenchmarkAutoOpen />);
    await flush();
    expect(navigate).toHaveBeenCalledTimes(1);

    // The user leaves /benchmark deliberately, then comes back to '/' — the run is still going.
    for (const p of ['/settings', '/', '/pair', '/']) {
      pathname = p;
      rerender(<BenchmarkAutoOpen />);
      await flush();
    }

    expect(navigate).toHaveBeenCalledTimes(1);
    expect(benchmarkStatus).toHaveBeenCalledTimes(1);
  });

  it('opens the benchmark view for a run started while the renderer sat on the default route', async () => {
    render(<BenchmarkAutoOpen />);
    await flush();
    expect(navigate).not.toHaveBeenCalled();

    emit('benchmark-started');

    expect(navigate).toHaveBeenCalledWith('/benchmark');
  });

  it('ignores a run start while the user is somewhere they navigated to', async () => {
    const { rerender } = render(<BenchmarkAutoOpen />);
    await flush();

    pathname = '/settings';
    rerender(<BenchmarkAutoOpen />);
    emit('benchmark-started');

    expect(navigate).not.toHaveBeenCalled();
  });

  it('does not redirect when the status probe resolves after the user has moved on', async () => {
    let resolveStatus: (v: unknown) => void = () => {};
    benchmarkStatus.mockReturnValue(
      new Promise((r) => {
        resolveStatus = r;
      })
    );

    const { rerender } = render(<BenchmarkAutoOpen />);
    pathname = '/settings';
    rerender(<BenchmarkAutoOpen />);
    resolveStatus({ running: true });
    await flush();

    expect(navigate).not.toHaveBeenCalled();
  });

  it('drops its subscription on unmount', async () => {
    const { unmount } = render(<BenchmarkAutoOpen />);
    await flush();

    const handler = electron.on.mock.calls.find(([c]) => c === 'benchmark-started')?.[1];
    expect(handler).toBeDefined();

    unmount();
    expect(electron.off).toHaveBeenCalledWith('benchmark-started', handler);

    emit('benchmark-started');
    expect(navigate).not.toHaveBeenCalled();
  });

  it('survives a build with no benchmark bridge', async () => {
    (window as unknown as { electron: Record<string, unknown> }).electron = {};

    expect(() => render(<BenchmarkAutoOpen />)).not.toThrow();
    await flush();
    expect(navigate).not.toHaveBeenCalled();
  });
});
