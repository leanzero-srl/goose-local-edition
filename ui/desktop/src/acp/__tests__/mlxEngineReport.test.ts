import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { getAcpClient } from '../acpConnection';
import { mlxEngineStatus } from '../mlx-engine';

vi.mock('../acpConnection', () => ({
  getAcpClient: vi.fn(),
}));

type Bridge = { mlxEngineReport?: (r: unknown) => void };

describe('mlxEngineStatus tells main what goose said about the LOCAL engine', () => {
  const bridge = window.electron as unknown as Bridge;
  const report = vi.fn();

  beforeEach(() => {
    report.mockReset();
    bridge.mlxEngineReport = report;
    vi.mocked(getAcpClient).mockResolvedValue({
      extMethod: vi.fn().mockResolvedValue({
        status: {
          state: 'running',
          modelId: 'org/qwen',
          servedModelId: 'qwen',
          baseUrl: 'http://127.0.0.1:8090',
          restartRequired: false,
          availableMemoryGb: 60,
          totalMemoryGb: 128,
        },
      }),
    } as never);
  });

  afterEach(() => {
    delete bridge.mlxEngineReport;
  });

  it('a local read is reported with the fields main needs', async () => {
    await mlxEngineStatus();
    expect(report).toHaveBeenCalledWith({
      state: 'running',
      baseUrl: 'http://127.0.0.1:8090',
      modelId: 'org/qwen',
      servedModelId: 'qwen',
      lastError: undefined,
    });
  });

  it("a linked peer's read is never reported: main reads only this machine's engine", async () => {
    await mlxEngineStatus('peer-node-id');
    expect(report).not.toHaveBeenCalled();
  });
});
