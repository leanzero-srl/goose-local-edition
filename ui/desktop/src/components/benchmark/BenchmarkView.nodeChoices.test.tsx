import { render, screen, waitFor, within, cleanup } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { IntlTestWrapper } from '../../i18n/test-utils';

/**
 * The node choice is capped by the CONFIGURED pool. Measured 2026-09-05 on an MLX-only machine (one
 * `mlx-sidecar` device): the view offered 1/2/3 with 3 preselected — a run asking for nodes the pool
 * does not have. The data-value/role hooks bench_dispatch.mjs drives over CDP stay byte-identical for
 * every choice that remains, and a pool of ≥3 devices changes nothing.
 */
vi.mock('../swarm/SwarmRunPanel', async () => {
  const React = await import('react');
  const Stub = () => React.createElement('div', { 'data-testid': 'swarm-panel-stub' });
  return { SwarmRunPanel: Stub, default: Stub };
});
vi.mock('../swarm/useSamplingDefaults', () => ({ useSaveSamplingDefaults: () => () => {} }));
const mockReadConfig = vi.fn();
vi.mock('../../acp/config', () => ({ acpReadConfig: (...a: unknown[]) => mockReadConfig(...a) }));

import BenchmarkView, { nodeCapFor } from './BenchmarkView';

type ElectronMock = Record<string, unknown>;
const electron = () => (window as unknown as { electron: ElectronMock }).electron;

const MLX = { id: 'workhorse-mlx', model_id: 'workhorse-qwen3.5-9b-4bit-mlx', weight: 1, enabled: true, engine: 'mlx-sidecar' };
const LMS = (n: string) => ({ id: `${n}-27b`, model_id: `${n}-qwen3.6-27b`, weight: 2, enabled: true });

function mockElectron() {
  const e = electron();
  e.benchmarkStatus = vi.fn(async () => ({ running: false }));
  e.benchmarkRead = vi.fn(async () => null);
  e.benchmarkIdentity = vi.fn(async () => ({ handle: 'mihai' }));
  e.benchmarkShots = vi.fn(async () => []);
  e.readSwarmRun = vi.fn(async () => null);
  e.fleetStatus = vi.fn(async () => ({}));
  e.on = vi.fn();
  e.off = vi.fn();
}

const nodeButtons = async () => {
  const group = await screen.findByRole('group', { name: 'Nodes' });
  return within(group).getAllByRole('button');
};
const values = (bs: HTMLElement[]) => bs.map((b) => b.getAttribute('data-value'));
const pressed = (bs: HTMLElement[]) => bs.find((b) => b.getAttribute('aria-pressed') === 'true')?.getAttribute('data-value');

describe('nodeCapFor', () => {
  it('counts ENABLED devices, floors at 1, caps at 3, and keeps 3 for the legacy empty pool', () => {
    expect(nodeCapFor({ devices: [MLX] })).toBe(1);
    expect(nodeCapFor({ devices: [MLX, LMS('gabee')] })).toBe(2);
    expect(nodeCapFor({ devices: [MLX, LMS('gabee'), LMS('mihai'), LMS('x')] })).toBe(3);
    expect(nodeCapFor({ devices: [{ ...MLX, enabled: false }] })).toBe(1);
    expect(nodeCapFor({ devices: [] })).toBe(3);
    expect(nodeCapFor(null)).toBe(3);
  });
});

describe('BenchmarkView — the node choice follows the configured pool', () => {
  beforeEach(() => {
    vi.spyOn(console, 'error').mockImplementation(() => {});
    mockElectron();
    mockReadConfig.mockReset();
  });
  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  it('MLX-only pool: the only choice is 1 and it is selected', async () => {
    mockReadConfig.mockResolvedValue({ devices: [MLX] });
    render(<IntlTestWrapper><BenchmarkView /></IntlTestWrapper>);
    await waitFor(async () => expect(values(await nodeButtons())).toEqual(['1']));
    expect(pressed(await nodeButtons())).toBe('1');
  });

  it('two devices: 1 and 2 offered, 2 selected', async () => {
    mockReadConfig.mockResolvedValue({ devices: [MLX, LMS('gabee')] });
    render(<IntlTestWrapper><BenchmarkView /></IntlTestWrapper>);
    await waitFor(async () => expect(values(await nodeButtons())).toEqual(['1', '2']));
    expect(pressed(await nodeButtons())).toBe('2');
  });

  it('three or more devices: nothing changes — 1/2/3 with 3 selected, the hooks bench_dispatch.mjs drives', async () => {
    mockReadConfig.mockResolvedValue({ devices: [LMS('gabee'), LMS('mihai'), LMS('workhorse')] });
    render(<IntlTestWrapper><BenchmarkView /></IntlTestWrapper>);
    await waitFor(() => expect(mockReadConfig).toHaveBeenCalled());
    const bs = await nodeButtons();
    expect(values(bs)).toEqual(['1', '2', '3']);
    expect(pressed(bs)).toBe('3');
    expect(bs.every((b) => b.getAttribute('role') === null)).toBe(true);
  });

  it('an unreadable config keeps every choice', async () => {
    mockReadConfig.mockRejectedValue(new Error('no config'));
    render(<IntlTestWrapper><BenchmarkView /></IntlTestWrapper>);
    await waitFor(() => expect(mockReadConfig).toHaveBeenCalled());
    const bs = await nodeButtons();
    expect(values(bs)).toEqual(['1', '2', '3']);
    expect(pressed(bs)).toBe('3');
  });
});
