import { EventEmitter } from 'node:events';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { GooseServeResult } from './gooseServe';
import { GooseServeLeaseRegistry } from './gooseServeLeaseRegistry';
import { DesktopBackendLifetime, parseDesktopLifetime } from './desktopBackendLifetime';
import { decideClose } from './utils/closeGuard';

function fixture(read: ConstructorParameters<typeof DesktopBackendLifetime>[2]) {
  const logger = { info: vi.fn(), error: vi.fn() };
  const cleanup = vi.fn(async () => 'exited' as const);
  const registry = new GooseServeLeaseRegistry(logger);
  const result = {
    process: new EventEmitter(),
    acpUrl: 'wss://127.0.0.1/acp',
    cleanup,
    hasExited: () => false,
  } as unknown as GooseServeResult;
  const lease = registry.create(result, 'secret');
  registry.attachWindow(1, lease);
  const monitor = new DesktopBackendLifetime(lease, registry, read, logger);
  monitor.start();
  return { cleanup, registry, lease, monitor, logger };
}
afterEach(() => vi.useRealTimers());
describe('application-owned backend lifetime', () => {
  it('an idle sample cannot release the backend when a prompt is admitted just after it', async () => {
    vi.useFakeTimers();
    const read = vi
      .fn()
      .mockResolvedValue({ link_host_enabled: false, active_run_ids: [], detached_runs: true });
    const { cleanup, registry, lease, monitor } = fixture(read);
    await vi.advanceTimersByTimeAsync(0);
    expect(monitor.canDetach()).toBe(true);
    await registry.releaseWindow(1);
    expect(cleanup).not.toHaveBeenCalled();
    read.mockResolvedValue({
      link_host_enabled: false,
      active_run_ids: ['just-admitted'],
      detached_runs: true,
    });
    await vi.advanceTimersByTimeAsync(1000);
    expect(lease.applicationHolders).toEqual(new Set(['controller-runtime', 'run:just-admitted']));
    read.mockResolvedValue({ link_host_enabled: false, active_run_ids: [], detached_runs: true });
    await vi.advanceTimersByTimeAsync(1000);
    expect(cleanup).not.toHaveBeenCalled();
    await registry.stopAllAndWait();
    expect(cleanup).toHaveBeenCalledTimes(1);
    monitor.stop();
  });
  it('failed reads retain the owner and never manufacture a detached capability', async () => {
    vi.useFakeTimers();
    const read = vi.fn().mockRejectedValue(new Error('offline'));
    const { cleanup, registry, monitor, logger } = fixture(read);
    await vi.advanceTimersByTimeAsync(0);
    await registry.releaseWindow(1);
    await vi.advanceTimersByTimeAsync(1000);
    expect(cleanup).not.toHaveBeenCalled();
    expect(monitor.canDetach()).toBe(false);
    expect(logger.error).toHaveBeenCalledTimes(1);
    read.mockResolvedValue({ link_host_enabled: true, active_run_ids: [], detached_runs: true });
    await vi.advanceTimersByTimeAsync(1000);
    expect(monitor.canDetach()).toBe(true);
    read.mockResolvedValue('legacy');
    await vi.advanceTimersByTimeAsync(1000);
    expect(cleanup).not.toHaveBeenCalled();
    monitor.stop();
  });
  it('a legacy backend retains its old close behavior and cleanup', async () => {
    vi.useFakeTimers();
    const { cleanup, registry, monitor } = fixture(async () => 'legacy');
    await vi.advanceTimersByTimeAsync(0);
    expect(monitor.canDetach()).toBe(false);
    await registry.releaseWindow(1);
    expect(cleanup).toHaveBeenCalledTimes(1);
  });
  it('distinguishes an ordinary detached Close from explicit Quit', () => {
    const state = {
      confirmed: false,
      rendererCanAnswer: true,
      windowHoldsLiveTurn: true,
      windowHoldsLiveRun: true,
    };
    expect(decideClose({ ...state, ownerSurvivesClose: true })).toBe('pass');
    expect(decideClose({ ...state, ownerSurvivesClose: false })).toBe('ask');
    expect(decideClose({ ...state, confirmed: true })).toBe('pass');
  });
  it('rejects a malformed lifetime response rather than treating it as idle', () => {
    expect(() => parseDesktopLifetime({ detached_runs: true, active_run_ids: [] })).toThrow(
      'invalid lifetime'
    );
    expect(() =>
      parseDesktopLifetime({ detached_runs: true, active_run_ids: [null], link_host_enabled: true })
    ).toThrow('invalid lifetime');
  });
});
