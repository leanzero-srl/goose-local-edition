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

import { lmStudioFeedWanted, localSidecarNames, useFleetCorroboration } from './useFleetCorroboration';

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
/** The run's pool map (`poolNodeMap`) for the mixed pool measured 2026-09-02 — engine 748084b97 names the
 *  sidecar on the LM Studio host `workhorse-mlx`, a SECOND node beside `workhorse`. */
const MIXED_NODES: Record<string, string> = {
  'gabee-27b': 'gabee',
  'gabee-qwen3.6-27b': 'gabee',
  'workhorse-27b': 'workhorse',
  'workhorse-qwen3.8-27b': 'workhorse',
  'workhorse-mlx': 'workhorse-mlx',
  'workhorse-qwen3.5-9b-4bit-mlx': 'workhorse-mlx',
};

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

  it('a running local sidecar whose status carries NO count is REPORTED, not busy — the engine did not say', async () => {
    mockReadConfig.mockResolvedValue(SIDECAR_CFG);
    mockMlxStatus.mockResolvedValue(RUNNING);
    const { result } = renderHook(() => useFleetCorroboration(10_000_000));
    await waitFor(() => expect(result.current.mlxNodes).toEqual(['workhorse']));
    expect(result.current.reportedNodes).toEqual(['gabee', 'mihai', 'workhorse']);
    expect(result.current.busyNodes).toEqual(['mihai']);
  });

  // Q1 (2026-09-02): `activeRequests` is Rapid-MLX's own /v1/status num_running + num_waiting, read by
  // the sidecar. Measured on the real engine: idle is an explicit 0; a 10-stream burst read 8.
  it('a local sidecar whose engine reports activeRequests > 0 is BUSY under its short node name', async () => {
    mockReadConfig.mockResolvedValue(SIDECAR_CFG);
    mockMlxStatus.mockResolvedValue({ ...RUNNING, activeRequests: 1 });
    const { result } = renderHook(() => useFleetCorroboration(10_000_000));
    await waitFor(() => expect(result.current.busyNodes).toEqual(['mihai', 'workhorse']));
    expect(result.current.reportedNodes).toEqual(['gabee', 'mihai', 'workhorse']);
    expect(result.current.mlxNodes).toEqual(['workhorse']);
  });

  it('activeRequests 0 is an explicit idle: reported, not busy', async () => {
    mockReadConfig.mockResolvedValue(SIDECAR_CFG);
    mockMlxStatus.mockResolvedValue({ ...RUNNING, activeRequests: 0 });
    const { result } = renderHook(() => useFleetCorroboration(10_000_000));
    await waitFor(() => expect(result.current.mlxNodes).toEqual(['workhorse']));
    expect(result.current.busyNodes).toEqual(['mihai']);
  });

  it('a refused /v1/status probe (activeRequestsError, no count) keeps the device reported and never busy', async () => {
    mockReadConfig.mockResolvedValue(SIDECAR_CFG);
    mockMlxStatus.mockResolvedValue({
      ...RUNNING,
      activeRequestsError: 'GET http://127.0.0.1:8090/v1/status returned HTTP 401',
    });
    const { result } = renderHook(() => useFleetCorroboration(10_000_000));
    await waitFor(() => expect(result.current.mlxNodes).toEqual(['workhorse']));
    expect(result.current.reportedNodes).toEqual(['gabee', 'mihai', 'workhorse']);
    expect(result.current.busyNodes).toEqual(['mihai']);
  });

  it('a running sidecar with NO count is BUSY-UNKNOWN — named, with the engine\'s reason, never read as idle', async () => {
    mockReadConfig.mockResolvedValue(SIDECAR_CFG);
    mockMlxStatus.mockResolvedValue({
      ...RUNNING,
      activeRequestsError: 'GET http://127.0.0.1:8090/v1/status returned HTTP 401',
    });
    const { result } = renderHook(() => useFleetCorroboration(10_000_000));
    await waitFor(() => expect(result.current.busyUnknownNodes).toEqual(['workhorse']));
    expect(result.current.busyUnknownReason).toBe('GET http://127.0.0.1:8090/v1/status returned HTTP 401');
    expect(result.current.busyNodes).toEqual(['mihai']);
  });

  it('a count of any kind (0 or more) clears busy-unknown', async () => {
    mockReadConfig.mockResolvedValue(SIDECAR_CFG);
    mockMlxStatus.mockResolvedValue({ ...RUNNING, activeRequests: 0 });
    const { result } = renderHook(() => useFleetCorroboration(10_000_000));
    await waitFor(() => expect(result.current.mlxNodes).toEqual(['workhorse']));
    expect(result.current.busyUnknownNodes).toEqual([]);
    expect(result.current.busyUnknownReason).toBeUndefined();
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
      devices: [
        SIDECAR_CFG.devices[0],
        { ...SIDECAR_CFG.devices[1], id: 'gabee-mlx', model_id: 'gabee-qwen-mlx', host: 'gabee.local' },
      ],
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

/**
 * The mixed pool: the feeds key their rows by the run's canonical node names, so the sidecar and the LM
 * Studio model on ONE host are two nodes with their own reported/busy facts — not one `workhorse` whose
 * busy came from whichever feed answered last.
 */
describe('useFleetCorroboration × the run\'s pool map — the sidecar is `workhorse-mlx`', () => {
  beforeEach(() => {
    electron().fleetStatus = vi.fn(async () => ({
      'gabee-qwen3.6-27b': 'idle',
      'workhorse-qwen3.8-27b': 'generating',
    }));
    mockReadConfig.mockReset();
    mockMlxStatus.mockReset();
    mockReadConfig.mockResolvedValue(SIDECAR_CFG);
  });
  afterEach(() => {
    delete electron().fleetStatus;
  });

  it('localSidecarNames: the map names the sidecar device; without it the prefix collides with LM Studio', () => {
    const rows = [SIDECAR_CFG.devices[1]];
    expect(localSidecarNames(rows, MIXED_NODES)).toEqual(['workhorse-mlx']);
    expect(localSidecarNames(rows)).toEqual(['workhorse']);
  });

  it('LM Studio busy on `workhorse`, the sidecar idle: two rows, two answers', async () => {
    mockMlxStatus.mockResolvedValue({ ...RUNNING, activeRequests: 0 });
    const { result } = renderHook(() => useFleetCorroboration(10_000_000, MIXED_NODES));
    await waitFor(() => expect(result.current.mlxNodes).toEqual(['workhorse-mlx']));
    expect(result.current.reportedNodes).toEqual(['gabee', 'workhorse', 'workhorse-mlx']);
    expect(result.current.busyNodes).toEqual(['workhorse']);
    expect(result.current.nodeStatus).toEqual({ gabee: 'idle', workhorse: 'generating' });
  });

  it('the sidecar busy is filed under `workhorse-mlx`, never under LM Studio\'s `workhorse`', async () => {
    electron().fleetStatus = vi.fn(async () => ({
      'gabee-qwen3.6-27b': 'idle',
      'workhorse-qwen3.8-27b': 'idle',
    }));
    mockMlxStatus.mockResolvedValue({ ...RUNNING, activeRequests: 2 });
    const { result } = renderHook(() => useFleetCorroboration(10_000_000, MIXED_NODES));
    await waitFor(() => expect(result.current.busyNodes).toEqual(['workhorse-mlx']));
    expect(result.current.reportedNodes).toEqual(['gabee', 'workhorse', 'workhorse-mlx']);
  });

  it('the map arriving after the config read (the first fold lands later) re-keys the sidecar', async () => {
    mockMlxStatus.mockResolvedValue({ ...RUNNING, activeRequests: 0 });
    const { result, rerender } = renderHook(
      ({ nodes }: { nodes?: Record<string, string> }) => useFleetCorroboration(10_000_000, nodes),
      { initialProps: { nodes: undefined as Record<string, string> | undefined } }
    );
    await waitFor(() => expect(result.current.mlxNodes).toEqual(['workhorse']));
    rerender({ nodes: MIXED_NODES });
    await waitFor(() => expect(result.current.mlxNodes).toEqual(['workhorse-mlx']));
  });

  it('without a map (no run open) the old collision is what it was: one `workhorse`', async () => {
    mockMlxStatus.mockResolvedValue({ ...RUNNING, activeRequests: 0 });
    const { result } = renderHook(() => useFleetCorroboration(10_000_000));
    await waitFor(() => expect(result.current.mlxNodes).toEqual(['workhorse']));
    expect(result.current.reportedNodes).toEqual(['gabee', 'workhorse']);
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

  // Q1's contract: the engine's own count is the second independent signal. A lane whose digest is
  // stale past the open-call window is NOT demoted while the sidecar reports requests in flight.
  it('a sidecar lane past the open-call window stays WORKING while its engine reports it busy', () => {
    const fleet = deriveFleet({
      pool: ['gabee', 'workhorse'],
      laneSources: [lane('workhorse')],
      ...staleOpenCall,
      now: NOW,
      busyNodes: ['workhorse'],
      reportedNodes: ['gabee', 'workhorse'],
    });
    expect(fleet.workingByDevice.has('workhorse')).toBe(true);
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

/**
 * The `lms ps` probe runs only when the pool has an LM Studio node to report on. Measured 2026-09-05
 * on an MLX-only machine: `lms ps` spawned every 1.5 s for a fleet with zero LM Studio devices.
 */
describe('useFleetCorroboration — `lms ps` is spawned only for a pool that has an LM Studio node', () => {
  const MLX_ONLY_CFG = { devices: [SIDECAR_CFG.devices[1]] };
  beforeEach(() => {
    electron().fleetStatus = vi.fn(async () => ({ 'gabee-qwen3.6-27b': 'idle' }));
    mockReadConfig.mockReset();
    mockMlxStatus.mockReset();
    mockMlxStatus.mockResolvedValue({ ...RUNNING, activeRequests: 0 });
  });
  afterEach(() => {
    delete electron().fleetStatus;
  });

  it('lmStudioFeedWanted: false only when every enabled device is an mlx-sidecar', () => {
    expect(lmStudioFeedWanted(MLX_ONLY_CFG)).toBe(false);
    expect(lmStudioFeedWanted(SIDECAR_CFG)).toBe(true);
    expect(lmStudioFeedWanted({ devices: [] })).toBe(true);
    expect(lmStudioFeedWanted(null)).toBe(true);
    expect(
      lmStudioFeedWanted({ devices: [{ ...SIDECAR_CFG.devices[0], enabled: false }, SIDECAR_CFG.devices[1]] })
    ).toBe(false);
  });

  it('MLX-only pool: the sidecar is reported and `lms ps` is never asked', async () => {
    mockReadConfig.mockResolvedValue(MLX_ONLY_CFG);
    const { result } = renderHook(() => useFleetCorroboration(10_000_000));
    await waitFor(() => expect(result.current.mlxNodes).toEqual(['workhorse']));
    expect(result.current.reportedNodes).toEqual(['workhorse']);
    expect(electron().fleetStatus).not.toHaveBeenCalled();
  });

  it('mixed pool: `lms ps` runs beside the sidecar poll', async () => {
    mockReadConfig.mockResolvedValue(SIDECAR_CFG);
    const { result } = renderHook(() => useFleetCorroboration(10_000_000));
    await waitFor(() => expect(result.current.reportedNodes).toEqual(['gabee', 'workhorse']));
    expect(electron().fleetStatus).toHaveBeenCalled();
  });
});
