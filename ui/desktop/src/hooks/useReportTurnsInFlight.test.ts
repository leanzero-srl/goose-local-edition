import { afterEach, describe, expect, it, vi } from 'vitest';
import { renderHook } from '@testing-library/react';

const store = vi.hoisted(() => ({
  attempts: new Map<string, string | null>(),
  watch: vi.fn(),
}));
vi.mock('../acp/chatSessionStore', () => ({
  acpChatSessionStore: {
    getSnapshot: (sessionId: string) =>
      store.attempts.has(sessionId)
        ? { activePromptAttemptId: store.attempts.get(sessionId) }
        : undefined,
  },
  watchPromptsInFlight: (onChange: (p: unknown[]) => void) => {
    store.watch(onChange);
    onChange([{ sessionId: '20260929_15', sessionName: 'Coffee' }]);
    return () => undefined;
  },
}));
const controller = vi.hoisted(() => ({ stop: vi.fn() }));
vi.mock('../acp/chatSessionController', () => ({ acpChatSessionController: controller }));

import { stopTurnForAnotherWindow, useReportTurnsInFlight } from './useReportTurnsInFlight';
import { STOP_TURN_CHANNEL } from '../utils/runningElsewhere';

/**
 * Q-500: goosed's cancel reaches a prompt only through the connection that sent it (acp/server.rs
 * `on_cancel`), so a Stop pressed in another window is relayed by main to the window holding the
 * prompt, which stops it exactly as its own Stop would.
 */
describe('Stop from another window (Q-500)', () => {
  const original = (window as unknown as { electron: unknown }).electron;
  afterEach(() => {
    store.attempts.clear();
    controller.stop.mockReset();
    (window as unknown as { electron: unknown }).electron = original;
  });

  it('stops the prompt this window holds for that chat', () => {
    store.attempts.set('20260929_15', 'attempt-1');
    stopTurnForAnotherWindow('20260929_15');
    expect(controller.stop).toHaveBeenCalledWith('20260929_15');
  });

  it('leaves a chat alone whose turn here already ended, or one it never ran', () => {
    store.attempts.set('20260929_15', null);
    stopTurnForAnotherWindow('20260929_15');
    stopTurnForAnotherWindow('20260928_19');
    stopTurnForAnotherWindow(42);
    expect(controller.stop).not.toHaveBeenCalled();
  });

  it('the window listens for main’s relay while it reports its prompts', () => {
    const listeners = new Map<string, (event: unknown, ...args: unknown[]) => void>();
    const report = vi.fn();
    (window as unknown as { electron: unknown }).electron = {
      reportTurnsInFlight: report,
      on: (channel: string, fn: (event: unknown, ...args: unknown[]) => void) =>
        listeners.set(channel, fn),
      off: (channel: string) => listeners.delete(channel),
    };
    store.attempts.set('20260929_15', 'attempt-1');
    const hook = renderHook(() => useReportTurnsInFlight());
    expect(report).toHaveBeenCalledWith([{ sessionId: '20260929_15', sessionName: 'Coffee' }]);
    listeners.get(STOP_TURN_CHANNEL)?.({}, '20260929_15');
    expect(controller.stop).toHaveBeenCalledWith('20260929_15');
    hook.unmount();
    expect(listeners.has(STOP_TURN_CHANNEL)).toBe(false);
  });
});
