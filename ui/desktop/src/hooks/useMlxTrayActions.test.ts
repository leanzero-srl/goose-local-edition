import { beforeEach, describe, expect, it, vi } from 'vitest';

const mount = vi.fn();
const unmount = vi.fn();
const status = vi.fn();
const settingsRead = vi.fn();

vi.mock('../acp/mlx-engine', () => ({
  mlxEngineMount: (...a: unknown[]) => mount(...a),
  mlxEngineUnmount: (...a: unknown[]) => unmount(...a),
  mlxEngineStatus: (...a: unknown[]) => status(...a),
  mlxEngineSettingsRead: (...a: unknown[]) => settingsRead(...a),
}));
vi.mock('../components/leanzero-swarm/mlxLiveStats', () => ({ MLX_STATUS_POLL_MS: 0 }));

import { runMlxTrayAction } from './useMlxTrayActions';

describe('runMlxTrayAction — the tray’s Mount/Unmount through the renderer’s ACP client', () => {
  beforeEach(() => {
    mount.mockReset().mockResolvedValue(undefined);
    unmount.mockReset().mockResolvedValue(undefined);
    status.mockReset();
    settingsRead.mockReset();
  });

  it('mounts the configured model and reads status until it leaves "mounting" (each read reaches main)', async () => {
    settingsRead.mockResolvedValue({ modelId: 'org/qwen' });
    status
      .mockResolvedValueOnce({ state: 'mounting' })
      .mockResolvedValueOnce({ state: 'mounting' })
      .mockResolvedValueOnce({ state: 'running' });
    await runMlxTrayAction('mount');
    expect(mount).toHaveBeenCalledWith('org/qwen');
    expect(status).toHaveBeenCalledTimes(3);
  });

  it('a mount that fails ends on the failed read — the loop is bounded by the state, not a clock', async () => {
    settingsRead.mockResolvedValue({ modelId: 'org/qwen' });
    status.mockResolvedValueOnce({ state: 'mounting' }).mockResolvedValueOnce({ state: 'failed' });
    await runMlxTrayAction('mount');
    expect(status).toHaveBeenCalledTimes(2);
  });

  it('no configured model is a named refusal, and nothing is mounted', async () => {
    settingsRead.mockResolvedValue({});
    await expect(runMlxTrayAction('mount')).rejects.toThrow('no-model');
    expect(mount).not.toHaveBeenCalled();
  });

  it('unmount unmounts and reads the status once so main sees the engine go', async () => {
    status.mockResolvedValue({ state: 'stopped' });
    await runMlxTrayAction('unmount');
    expect(unmount).toHaveBeenCalledTimes(1);
    expect(status).toHaveBeenCalledTimes(1);
  });
});
