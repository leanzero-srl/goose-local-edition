import { renderHook, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { deriveFleet, DIGEST_OPEN_CALL_FRESH_MS } from './useSwarmRun';
import type { TurnLane } from './useSwarmRun';

const mockReadConfig = vi.fn();
vi.mock('../../acp/config', () => ({
  acpReadConfig: (...a: unknown[]) => mockReadConfig(...a),
}));
const mockMlxStatus = vi.fn();
vi.mock('../../acp/mlx-engine', () => ({
  mlxEngineStatus: (...a: unknown[]) => mockMlxStatus(...a),
}));

import { useFleetCorroboration } from './useFleetCorroboration';

const electron = () => (window as unknown as { electron: Record<string, unknown> }).electron;

/** The live config's shape (2026-09-01): one local sidecar device beside the LM Studio nodes. */
const SIDECAR_CFG = {
  endpoint: 'http://localhost:1234',
  devices: [
    { id: 'gabee-27b', model_id: 'gabee-qwen3.6-27b', weight: 2, enabled: true },
    {
      id: 'workhorse-mlx',
      model_id: 'workhorse-qwen3.5-9b-4bit-mlx',
      weight: 1,
      enabled: true,
      engine: 'mlx-sidecar',
    },
  ],
};
const RUNNING = { state: 'running', servedModelId: 'workhorse-qwen3.5-9b-4bit-mlx', restartRequired: false };

/**
 * U-H2: the dead-lane corroboration must not depend on the showLmStudioFleet DISPLAY toggle, and a
 * LeanZero MLX sidecar device (never in `lms ps`) must be able to reach `reportedNodes` at all.
 */
describe('useFleetCorroboration — truth is fed regardless of display, from polls that already exist', () => {
  beforeEach(() => {
    electron().fleetStatus = vi.fn(async () => ({
      'gabee-qwen3.6-27b': 'idle',
      'mihai-qwen3.6-27b': 'generating',
    }));
    mockReadConfig.mockReset();
    mockMlxStatus.mockReset();
  });
  afterEach(() => {
    delete electron().fleetStatus;
  });

  it('reports every lms node with no visibility input at all — the hook has no toggle to be off', async () => {
    mockReadConfig.mockResolvedValue({ endpoint: 'http://localhost:1234', devices: [] });
    const { result } = renderHook(() => useFleetCorroboration(10_000_000));
    await waitFor(() => expect(result.current.reportedNodes).toEqual(['gabee', 'mihai']));
    expect(result.current.busyNodes).toEqual(['mihai']);
    expect(result.current.mlxNodes).toEqual([]);
    // No sidecar declared → the MLX status is never polled: no chatter for a device that does not exist.
    expect(mockMlxStatus).not.toHaveBeenCalled();
  });

  it('a running local sidecar device is REPORTED under its short node name, never busy', async () => {
    mockReadConfig.mockResolvedValue(SIDECAR_CFG);
    mockMlxStatus.mockResolvedValue(RUNNING);
    const { result } = renderHook(() => useFleetCorroboration(10_000_000));
    await waitFor(() => expect(result.current.mlxNodes).toEqual(['workhorse']));
    expect(result.current.reportedNodes).toEqual(['gabee', 'mihai', 'workhorse']);
    expect(result.current.busyNodes).toEqual(['mihai']);
  });

  it('a stopped or probe-failed engine takes the sidecar OUT of reported — fail safe, no demotion', async () => {
    mockReadConfig.mockResolvedValue(SIDECAR_CFG);
    mockMlxStatus.mockResolvedValue({ ...RUNNING, state: 'stopped' });
    const { result } = renderHook(() => useFleetCorroboration(10_000_000));
    await waitFor(() => expect(mockMlxStatus).toHaveBeenCalled());
    await waitFor(() => expect(result.current.reportedNodes).toEqual(['gabee', 'mihai']));
    expect(result.current.mlxNodes).toEqual([]);
  });

  it('a REMOTE sidecar (host set) is not corroborated by the local engine and stays out', async () => {
    mockReadConfig.mockResolvedValue({
      devices: [{ ...SIDECAR_CFG.devices[1], id: 'gabee-mlx', model_id: 'gabee-qwen-mlx', host: 'gabee.local' }],
    });
    const { result } = renderHook(() => useFleetCorroboration(10_000_000));
    await waitFor(() => expect(result.current.reportedNodes).toEqual(['gabee', 'mihai']));
    expect(mockMlxStatus).not.toHaveBeenCalled();
  });

  it('an unreadable swarm config means no sidecar feed — nothing fabricated', async () => {
    mockReadConfig.mockRejectedValue(new Error('config unreadable'));
    const { result } = renderHook(() => useFleetCorroboration(10_000_000));
    await waitFor(() => expect(result.current.reportedNodes).toEqual(['gabee', 'mihai']));
    expect(mockMlxStatus).not.toHaveBeenCalled();
  });
});

/** The contract the feed exists for: with the sidecar REPORTED, a dead sidecar lane can finally demote. */
describe('deriveFleet × the MLX feed', () => {
  const lane = (device: string, taskId = `t-${device}`): TurnLane => ({
    taskId,
    device,
    status: 'running',
    seq: 0,
  });
  const NOW = 1_800_000_000_000;
  const staleOpenCall = {
    digests: { 't-workhorse': { calls: [{ ok: null }] } },
    digestMtimes: { 't-workhorse': NOW - DIGEST_OPEN_CALL_FRESH_MS - 1 },
  };

  it('a sidecar lane with a digest past the open-call window demotes once the engine is reported', () => {
    const fleet = deriveFleet({
      pool: ['gabee', 'workhorse'],
      laneSources: [lane('workhorse')],
      ...staleOpenCall,
      now: NOW,
      busyNodes: [],
      reportedNodes: ['gabee', 'workhorse'],
    });
    expect(fleet.workingByDevice.has('workhorse')).toBe(false);
  });

  it('the same lane stays working while the sidecar is NOT reported (engine down: no evidence)', () => {
    const fleet = deriveFleet({
      pool: ['gabee', 'workhorse'],
      laneSources: [lane('workhorse')],
      ...staleOpenCall,
      now: NOW,
      busyNodes: [],
      reportedNodes: ['gabee'],
    });
    expect(fleet.workingByDevice.has('workhorse')).toBe(true);
  });
});
