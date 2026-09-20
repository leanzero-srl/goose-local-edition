import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';
import { BenchmarkRuntimeSetup } from './BenchmarkRuntimeSetup';
afterEach(cleanup);
it('never downloads automatically, shows size, installs only on click and becomes ready after verification', async () => {
  const ready = vi.fn();
  const install = vi.fn(async () => {});
  const status = vi
    .fn()
    .mockResolvedValueOnce({ state: 'missing', downloadBytes: 49092883 })
    .mockResolvedValueOnce({ state: 'ready', downloadBytes: 49092883 });
  Object.assign(window.electron, {
    benchmarkRuntimeStatus: status,
    benchmarkRuntimeInstall: install,
  });
  render(<BenchmarkRuntimeSetup onReady={ready} disabled={false} />);
  expect(await screen.findByText(/46.8 MiB download/)).toBeInTheDocument();
  expect(install).not.toHaveBeenCalled();
  expect(ready).toHaveBeenLastCalledWith(false);
  fireEvent.click(screen.getByRole('button', { name: 'Install benchmark tools' }));
  expect(await screen.findByText('Benchmark tools ready')).toBeInTheDocument();
  expect(install).toHaveBeenCalledOnce();
  expect(ready).toHaveBeenLastCalledWith(true);
});
it('shows progress and offers explicit retry after failure without enabling launch', async () => {
  let fail!: (reason: Error) => void;
  const install = vi.fn(
    () =>
      new Promise<void>((_resolve, reject) => {
        fail = reject;
      })
  );
  const ready = vi.fn();
  const handlers = new Map<string, (event: unknown, payload: unknown) => void>();
  Object.assign(window.electron, {
    benchmarkRuntimeStatus: vi.fn(async () => ({ state: 'missing', downloadBytes: 49092883 })),
    benchmarkRuntimeInstall: install,
    on: vi.fn((channel: string, cb: (event: unknown, payload: unknown) => void) =>
      handlers.set(channel, cb)
    ),
  });
  render(<BenchmarkRuntimeSetup onReady={ready} disabled={false} />);
  fireEvent.click(await screen.findByRole('button', { name: 'Install benchmark tools' }));
  await act(async () =>
    handlers.get('benchmark-runtime-progress')?.(null, {
      phase: 'downloading',
      receivedBytes: 1024 * 1024,
      totalBytes: 49092883,
    })
  );
  expect(screen.getByRole('status')).toHaveTextContent('downloading · 1.0 MiB / 46.8 MiB');
  await act(async () => fail(new Error('checksum mismatch')));
  expect(await screen.findByRole('alert')).toHaveTextContent('checksum mismatch');
  await waitFor(() =>
    expect(screen.getByRole('button', { name: 'Retry installation' })).toBeEnabled()
  );
  expect(ready).toHaveBeenLastCalledWith(false);
});
