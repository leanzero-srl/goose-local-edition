import { describe, expect, it, vi, beforeEach } from 'vitest';
import { renderHook } from '@testing-library/react';

const mockSet = vi.fn();
vi.mock('../../acp/nodes', () => ({ nodesSetChatNodes: (...a: unknown[]) => mockSet(...a) }));
const mockNote = vi.fn();
vi.mock('../ModelAndProviderContext', () => ({
  noteSessionProviderModel: (...a: unknown[]) => mockNote(...a),
}));
const mockRefresh = vi.fn();
vi.mock('../engineGlance/glanceStore', () => ({ refreshGlanceNodes: () => mockRefresh() }));

import { useChatNodes } from './useChatNodes';

beforeEach(() => {
  mockSet.mockReset();
  mockNote.mockReset();
  mockRefresh.mockReset();
});

describe('useChatNodes — the chip’s one door to a chat’s nodes (Q-359)', () => {
  it('mirrors the model goosed set, into the store and the composer, and reads the nodes again', async () => {
    mockSet.mockResolvedValueOnce({
      write: { written: true, refusals: [], read: {} },
      model: 'strategy:chat-7',
    });
    const onModelChanged = vi.fn();
    const { result } = renderHook(() => useChatNodes('7', onModelChanged));
    const got = await result.current(['a', 'b'], true);
    expect(mockSet).toHaveBeenCalledWith('7', ['a', 'b'], true);
    expect(got).toEqual({ applied: true, model: 'strategy:chat-7' });
    expect(mockNote).toHaveBeenCalledWith('7', 'swarm', 'strategy:chat-7');
    expect(onModelChanged).toHaveBeenCalledWith({ model: 'strategy:chat-7', provider: 'swarm' });
    expect(mockRefresh).toHaveBeenCalled();
  });

  it('a refused set comes back in goosed’s words and moves nothing', async () => {
    mockSet.mockResolvedValueOnce({
      write: { written: false, refusals: [{ code: 'unknownNode', message: 'no node ghost' }] },
    });
    const onModelChanged = vi.fn();
    const { result } = renderHook(() => useChatNodes('7', onModelChanged));
    expect(await result.current(['ghost'], false)).toEqual({
      applied: false,
      refusals: ['no node ghost'],
    });
    expect(mockNote).not.toHaveBeenCalled();
    expect(onModelChanged).not.toHaveBeenCalled();
  });

  it('removing a set that is not there is no refusal, and moves nothing', async () => {
    mockSet.mockResolvedValueOnce({ write: { written: false, refusals: [] } });
    const { result } = renderHook(() => useChatNodes('7'));
    expect(await result.current([], false)).toEqual({ applied: true, model: null });
    expect(mockNote).not.toHaveBeenCalled();
  });
});
