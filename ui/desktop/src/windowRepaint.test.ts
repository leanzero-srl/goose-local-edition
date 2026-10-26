import { describe, expect, it, vi } from 'vitest';
import { keepRepainted, REPAINT_INTERVAL_MS, repaintWindow, type RepaintableWindow } from './windowRepaint';

function fakeWindow(state: { destroyed?: boolean; visible?: boolean; minimized?: boolean } = {}) {
  const listeners = new Map<string, Array<() => void>>();
  const invalidate = vi.fn();
  const win: RepaintableWindow = {
    on: (event, listener) => listeners.set(event, [...(listeners.get(event) ?? []), listener]),
    isDestroyed: () => state.destroyed ?? false,
    isVisible: () => state.visible ?? true,
    isMinimized: () => state.minimized ?? false,
    webContents: { invalidate },
  };
  const emit = (event: string) => (listeners.get(event) ?? []).forEach((l) => l());
  return { win, invalidate, emit, state };
}

describe('repaintWindow', () => {
  it('repaints a window that can be seen', () => {
    const { win, invalidate } = fakeWindow();
    expect(repaintWindow(win)).toBe(true);
    expect(invalidate).toHaveBeenCalledTimes(1);
  });

  it('leaves a destroyed, hidden or minimized window alone', () => {
    for (const s of [{ destroyed: true }, { visible: false }, { minimized: true }]) {
      const { win, invalidate } = fakeWindow(s);
      expect(repaintWindow(win)).toBe(false);
      expect(invalidate).not.toHaveBeenCalled();
    }
  });
});

describe('keepRepainted', () => {
  it('repaints on focus, show and restore, and on every interval tick', () => {
    let tick: () => void = () => {};
    const every = vi.fn((fn: () => void, ms: number) => {
      expect(ms).toBe(REPAINT_INTERVAL_MS);
      tick = fn;
      return { unref: vi.fn() };
    });
    const { win, invalidate, emit } = fakeWindow();
    keepRepainted(win, every, vi.fn());
    emit('focus');
    emit('show');
    emit('restore');
    tick();
    expect(invalidate).toHaveBeenCalledTimes(4);
  });

  it('stops its timer when the window closes, and never keeps the app alive', () => {
    const timer = { unref: vi.fn() };
    const stop = vi.fn();
    const { win, emit } = fakeWindow();
    keepRepainted(win, () => timer, stop);
    expect(timer.unref).toHaveBeenCalled();
    emit('closed');
    expect(stop).toHaveBeenCalledWith(timer);
  });

  it('does not repaint a minimized window on a tick', () => {
    let tick: () => void = () => {};
    const { win, invalidate, state } = fakeWindow();
    keepRepainted(win, (fn) => ((tick = fn), {}), vi.fn());
    state.minimized = true;
    tick();
    expect(invalidate).not.toHaveBeenCalled();
  });
});
