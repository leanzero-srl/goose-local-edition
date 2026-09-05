import { describe, expect, it, vi } from 'vitest';
import type { MlxEngineStatus } from '../../acp/mlx-engine';
import { poolEngines, swarmPoolContextLimit } from './swarmContextLimit';

/**
 * The composer's `swarm` token limit reads the engines the POOL runs on. Measured 2026-09-05 on an
 * MLX-only machine: the LM Studio-only read answered null and the composer showed the generic 128k
 * while the sidecar served a model with a 32k window.
 */
const MLX_ONLY = {
  devices: [
    { id: 'workhorse-mlx', model_id: 'workhorse-qwen3.5-9b-4bit-mlx', weight: 1, enabled: true, engine: 'mlx-sidecar' },
  ],
};
const MIXED = {
  devices: [
    { id: 'workhorse-27b', model_id: 'workhorse-qwen3.8-27b', weight: 2, enabled: true },
    ...MLX_ONLY.devices,
  ],
};
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
    });
    expect(limit).toBe(32768);
    expect(lmStudioLimit).not.toHaveBeenCalled();
  });

  it('mixed pool: the MIN across engines', async () => {
    const limit = await swarmPoolContextLimit({
      readConfig: async () => MIXED,
      lmStudioLimit: async () => 131072,
      mlxStatus: async () => running(32768),
    });
    expect(limit).toBe(32768);
    const other = await swarmPoolContextLimit({
      readConfig: async () => MIXED,
      lmStudioLimit: async () => 16384,
      mlxStatus: async () => running(32768),
    });
    expect(other).toBe(16384);
  });

  it('an engine that does not answer contributes nothing — never a fabricated number', async () => {
    const stopped = await swarmPoolContextLimit({
      readConfig: async () => MLX_ONLY,
      lmStudioLimit: async () => null,
      mlxStatus: async () => ({ ...running(32768), state: 'stopped' }),
    });
    expect(stopped).toBeNull();
    const noWindow = await swarmPoolContextLimit({
      readConfig: async () => MIXED,
      lmStudioLimit: async () => 131072,
      mlxStatus: async () => running(undefined),
    });
    expect(noWindow).toBe(131072);
    const thrown = await swarmPoolContextLimit({
      readConfig: async () => MIXED,
      lmStudioLimit: async () => 131072,
      mlxStatus: async () => {
        throw new Error('engine unreachable');
      },
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
    });
    expect(limit).toBe(131072);
    expect(mlxStatus).not.toHaveBeenCalled();
  });
});
