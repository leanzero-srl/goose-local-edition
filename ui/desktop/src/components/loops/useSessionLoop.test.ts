import { act, renderHook, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { LoopsChangedNotification_unstable } from '@aaif/goose-sdk';
import { loopRecord, waitingRecord } from './railFixtures';
import { useSessionLoop } from './useSessionLoop';

const loops = vi.hoisted(() => ({ get: vi.fn(), control: vi.fn() }));
const changed = vi.hoisted(() => new Set<(c: LoopsChangedNotification_unstable) => void>());

vi.mock('../../acp/loops', () => ({ loopsGet: loops.get, loopsControl: loops.control }));
vi.mock('../../acp/acpConnection', () => ({
  onLoopsChanged: (listener: (c: LoopsChangedNotification_unstable) => void) => {
    changed.add(listener);
    return () => changed.delete(listener);
  },
}));

beforeEach(() => {
  loops.get.mockReset();
  loops.control.mockReset();
  changed.clear();
});

describe('useSessionLoop', () => {
  it('reads no loop as none, and a loop with the status goosed derived for it now', async () => {
    loops.get.mockResolvedValueOnce({});
    const { result, rerender } = renderHook(({ key }) => useSessionLoop('s1', key), {
      initialProps: { key: 'a' },
    });
    await waitFor(() => expect(result.current.state).toEqual({ kind: 'none' }));
    loops.get.mockResolvedValueOnce({
      loop: loopRecord(),
      effectiveStatus: 'paused',
      effectiveReason: { kind: 'closed' },
    });
    rerender({ key: 'b' });
    await waitFor(() => expect(result.current.state.kind).toBe('loop'));
    expect(result.current.state).toMatchObject({ status: 'paused', reason: { kind: 'closed' } });
  });

  it('never reads an unreadable record or a failed read as no loop', async () => {
    loops.get.mockResolvedValueOnce({ error: 'expected value at line 1' });
    const { result, rerender } = renderHook(({ key }) => useSessionLoop('s1', key), {
      initialProps: { key: 'a' },
    });
    await waitFor(() =>
      expect(result.current.state).toEqual({
        kind: 'unreadable',
        error: 'expected value at line 1',
      })
    );
    loops.get.mockRejectedValueOnce(new Error('method not found'));
    rerender({ key: 'b' });
    await waitFor(() =>
      expect(result.current.state).toEqual({ kind: 'unreadable', error: 'method not found' })
    );
  });

  it('returns a refusal as a refusal and leaves the loop as it was', async () => {
    loops.get.mockResolvedValue({ loop: loopRecord() });
    loops.control.mockResolvedValue({
      refusal: { code: 'runner_absent', reason: 'The loop runner is not in this build' },
    });
    const { result } = renderHook(() => useSessionLoop('s1', 'k'));
    await waitFor(() => expect(result.current.state.kind).toBe('loop'));
    let got: unknown;
    await act(async () => {
      got = await result.current.control('pause');
    });
    expect(got).toEqual({
      kind: 'refused',
      refusal: { code: 'runner_absent', reason: 'The loop runner is not in this build' },
    });
    expect(result.current.state).toMatchObject({ status: 'running' });
  });

  it('follows loops/changed for its own chat only', async () => {
    loops.get.mockResolvedValue({ loop: loopRecord() });
    const { result } = renderHook(() => useSessionLoop('s1', 'k'));
    await waitFor(() => expect(result.current.state.kind).toBe('loop'));
    act(() => {
      for (const listener of changed) listener({ sessionId: 's2', loop: waitingRecord() });
    });
    expect(result.current.state).toMatchObject({ status: 'running' });
    act(() => {
      for (const listener of changed) listener({ sessionId: 's1', loop: waitingRecord() });
    });
    expect(result.current.state).toMatchObject({ status: 'waiting' });
  });
});
