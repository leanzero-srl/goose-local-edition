import { describe, expect, it, vi } from 'vitest';
import type { MlxEngineStatus } from '../../acp/mlx-engine';
import type { MlxRemoteSingleStatus } from '../../acp/mlx-remote-single';
import type { MlxDistributedStatus } from '../../acp/mlx-distributed';
import { poolEngines, swarmPoolContextLimit } from './swarmContextLimit';

/**
 * The composer's `swarm` token limit reads the engines the POOL runs on. Measured 2026-09-05 on an
 * MLX-only machine: the LM Studio-only read answered null and the composer showed the generic 128k
 * while the sidecar served a model with a 32k window.
 */
const MLX_ONLY = {
  devices: [
    {
      id: 'workhorse-mlx',
      model_id: 'workhorse-qwen3.5-9b-4bit-mlx',
      weight: 1,
      enabled: true,
      engine: 'mlx-sidecar',
    },
  ],
};
const MIXED = {
  devices: [
    { id: 'workhorse-27b', model_id: 'workhorse-qwen3.8-27b', weight: 2, enabled: true },
    ...MLX_ONLY.devices,
  ],
};
const OFF = { state: 'off' } as MlxRemoteSingleStatus;
const running = (contextWindow?: number): MlxEngineStatus => ({
  state: 'running',
  restartRequired: false,
  availableMemoryGb: 30,
  totalMemoryGb: 64,
  contextWindow,
});

describe('poolEngines', () => {
  it('an MLX-only pool has no LM Studio engine; a mixed pool has both; no devices is the legacy LM Studio pool', () => {
    expect(poolEngines(MLX_ONLY)).toEqual({ lmStudio: false, localMlx: true });
    expect(poolEngines(MIXED)).toEqual({ lmStudio: true, localMlx: true });
    expect(poolEngines({ devices: [] })).toEqual({ lmStudio: true, localMlx: false });
    expect(poolEngines(null)).toEqual({ lmStudio: true, localMlx: false });
  });

  it('a disabled device does not count, and a REMOTE sidecar is not the local engine', () => {
    expect(
      poolEngines({ devices: [{ ...MIXED.devices[0], enabled: false }, MLX_ONLY.devices[0]] })
    ).toEqual({ lmStudio: false, localMlx: true });
    expect(poolEngines({ devices: [{ ...MLX_ONLY.devices[0], host: 'gabee.local' }] })).toEqual({
      lmStudio: false,
      localMlx: false,
    });
  });
});

describe('swarmPoolContextLimit', () => {
  it('MLX-only pool: the sidecar engine status supplies the window and LM Studio is never asked', async () => {
    const lmStudioLimit = vi.fn(async () => 131072);
    const limit = await swarmPoolContextLimit({
      readConfig: async () => MLX_ONLY,
      lmStudioLimit,
      mlxStatus: async () => running(32768),
      remoteStatus: async () => OFF,
      distributedStatus: async () => null,
    });
    expect(limit).toBe(32768);
    expect(lmStudioLimit).not.toHaveBeenCalled();
  });

  it('mixed pool: the MIN across engines', async () => {
    const limit = await swarmPoolContextLimit({
      readConfig: async () => MIXED,
      lmStudioLimit: async () => 131072,
      mlxStatus: async () => running(32768),
      remoteStatus: async () => OFF,
      distributedStatus: async () => null,
    });
    expect(limit).toBe(32768);
    const other = await swarmPoolContextLimit({
      readConfig: async () => MIXED,
      lmStudioLimit: async () => 16384,
      mlxStatus: async () => running(32768),
      remoteStatus: async () => OFF,
      distributedStatus: async () => null,
    });
    expect(other).toBe(16384);
  });

  it('an engine that does not answer contributes nothing — never a fabricated number', async () => {
    const stopped = await swarmPoolContextLimit({
      readConfig: async () => MLX_ONLY,
      lmStudioLimit: async () => null,
      mlxStatus: async () => ({ ...running(32768), state: 'stopped' }),
      remoteStatus: async () => OFF,
      distributedStatus: async () => null,
    });
    expect(stopped).toBeNull();
    const noWindow = await swarmPoolContextLimit({
      readConfig: async () => MIXED,
      lmStudioLimit: async () => 131072,
      mlxStatus: async () => running(undefined),
      remoteStatus: async () => OFF,
      distributedStatus: async () => null,
    });
    expect(noWindow).toBe(131072);
    const thrown = await swarmPoolContextLimit({
      readConfig: async () => MIXED,
      lmStudioLimit: async () => 131072,
      mlxStatus: async () => {
        throw new Error('engine unreachable');
      },
      remoteStatus: async () => OFF,
      distributedStatus: async () => null,
    });
    expect(thrown).toBe(131072);
  });

  it('an unreadable config keeps the LM Studio read (it cannot prove the pool has no LM Studio node)', async () => {
    const mlxStatus = vi.fn(async () => running(32768));
    const limit = await swarmPoolContextLimit({
      readConfig: async () => {
        throw new Error('config unreadable');
      },
      lmStudioLimit: async () => 131072,
      mlxStatus,
      remoteStatus: async () => OFF,
      distributedStatus: async () => null,
    });
    expect(limit).toBe(131072);
    expect(mlxStatus).not.toHaveBeenCalled();
  });
});

describe('the MLX window is the engine chat reaches — the router’s own choice', () => {
  it('chat routed to Work’s Mac Studio: its 262,144 window, not the stopped local engine nor a 128k default', async () => {
    const limit = await swarmPoolContextLimit({
      readConfig: async () => MLX_ONLY,
      lmStudioLimit: async () => null,
      mlxStatus: async () => ({ ...running(), state: 'stopped' }),
      remoteStatus: async () =>
        ({ state: 'ready', contextWindow: 262144 }) as MlxRemoteSingleStatus,
      distributedStatus: async () => null,
    });
    expect(limit).toBe(262144);
  });

  it('a route still mounting on the peer says nothing yet — the local engine is not asked in its place', async () => {
    const mlxStatus = vi.fn(async () => running(32768));
    const limit = await swarmPoolContextLimit({
      readConfig: async () => MLX_ONLY,
      lmStudioLimit: async () => null,
      mlxStatus,
      remoteStatus: async () => ({ state: 'mounting' }) as MlxRemoteSingleStatus,
      distributedStatus: async () => null,
    });
    expect(limit).toBeNull();
    expect(mlxStatus).not.toHaveBeenCalled();
  });

  it('the split owning this Mac: its own limit', async () => {
    const limit = await swarmPoolContextLimit({
      readConfig: async () => MLX_ONLY,
      lmStudioLimit: async () => null,
      mlxStatus: async () => running(32768),
      remoteStatus: async () => OFF,
      distributedStatus: async () =>
        ({
          mode: 'distributed',
          state: 'ready',
          contextLimit: 65536,
        }) as unknown as MlxDistributedStatus,
    });
    expect(limit).toBe(65536);
  });
});
