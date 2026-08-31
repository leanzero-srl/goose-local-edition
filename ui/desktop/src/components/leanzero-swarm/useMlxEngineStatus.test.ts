import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { renderHook, waitFor } from '@testing-library/react';
import { useMlxEngineStatusPoll } from './useMlxEngineStatus';
import type { MlxEngineStatus } from '../../acp/mlx-engine';

const mockStatus = vi.fn();
vi.mock('../../acp/mlx-engine', () => ({
  mlxEngineStatus: (...args: unknown[]) => mockStatus(...args),
}));

const RUNNING: MlxEngineStatus = {
  state: 'running',
  modelId: 'mlx-community/Qwen3-30B-A3B-4bit',
  servedModelId: 'qwen3-30b-served',
  restartRequired: false,
  availableMemoryGb: 40,
  totalMemoryGb: 64,
};

beforeEach(() => {
  vi.clearAllMocks();
  mockStatus.mockResolvedValue(RUNNING);
});

afterEach(() => {
  vi.useRealTimers();
});

describe('useMlxEngineStatusPoll', () => {
  it('disabled: never calls the engine and reports no status', async () => {
    const { result } = renderHook(() => useMlxEngineStatusPoll(false));
    await new Promise((r) => setTimeout(r, 20));
    expect(mockStatus).not.toHaveBeenCalled();
    expect(result.current.status).toBeNull();
    expect(result.current.error).toBeNull();
  });

  it('enabled: reads immediately and exposes the status', async () => {
    const { result } = renderHook(() => useMlxEngineStatusPoll(true));
    await waitFor(() => {
      expect(result.current.status?.state).toBe('running');
    });
    expect(result.current.status?.servedModelId).toBe('qwen3-30b-served');
    expect(result.current.error).toBeNull();
  });

  it('a failed read INVALIDATES the previous status instead of keeping the stale claim', async () => {
    const { result } = renderHook(() => useMlxEngineStatusPoll(true, 10));
    await waitFor(() => {
      expect(result.current.status?.state).toBe('running');
    });
    mockStatus.mockRejectedValue(new Error('agent connection lost'));
    await waitFor(() => {
      expect(result.current.status).toBeNull();
    });
    expect(result.current.error).toContain('agent connection lost');
  });

  it('flipping enabled off clears the status and stops polling', async () => {
    const { result, rerender } = renderHook(({ enabled }) => useMlxEngineStatusPoll(enabled, 10), {
      initialProps: { enabled: true },
    });
    await waitFor(() => {
      expect(result.current.status?.state).toBe('running');
    });
    rerender({ enabled: false });
    await waitFor(() => {
      expect(result.current.status).toBeNull();
    });
    const calls = mockStatus.mock.calls.length;
    await new Promise((r) => setTimeout(r, 50));
    expect(mockStatus.mock.calls.length).toBe(calls);
  });
});
