import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, render } from '@testing-library/react';
import { IntlTestWrapper } from '../../i18n/test-utils';
import { useGlanceTurnedOffNotice } from './useGlanceTurnedOffNotice';
import { ENGINE_GLANCE_TURNED_OFF_CHANNEL } from '../../utils/engineGlance';

const toastSuccess = vi.fn();
vi.mock('../../toasts', () => ({
  toastSuccess: (...args: unknown[]) => toastSuccess(...args),
}));

type Listener = (event: unknown, ...args: unknown[]) => void;
let listeners: Map<string, Set<Listener>>;
const electron = window.electron as unknown as Record<string, unknown>;
const saved: Record<string, unknown> = {};

beforeEach(() => {
  listeners = new Map();
  toastSuccess.mockClear();
  saved.on = electron.on;
  saved.off = electron.off;
  Object.assign(electron, {
    on: (channel: string, fn: Listener) => {
      if (!listeners.has(channel)) listeners.set(channel, new Set());
      listeners.get(channel)!.add(fn);
    },
    off: (channel: string, fn: Listener) => listeners.get(channel)?.delete(fn),
  });
});

afterEach(() => {
  Object.assign(electron, saved);
});

function Host() {
  useGlanceTurnedOffNotice();
  return null;
}

describe('useGlanceTurnedOffNotice — Q-224: the app says it, with the way back', () => {
  it('main says the window was turned off: one toast naming where it comes back from', () => {
    render(
      <IntlTestWrapper>
        <Host />
      </IntlTestWrapper>
    );
    expect(toastSuccess).not.toHaveBeenCalled();
    act(() => {
      for (const fn of listeners.get(ENGINE_GLANCE_TURNED_OFF_CHANNEL) ?? []) fn({});
    });
    expect(toastSuccess).toHaveBeenCalledOnce();
    const [{ title, msg }] = toastSuccess.mock.calls[0] as [{ title: string; msg: string }];
    expect(title).toBe('Floating window turned off');
    expect(msg).toBe('Turn it back on in Settings › App.');
  });

  it('unmounted: it stops listening', () => {
    const view = render(
      <IntlTestWrapper>
        <Host />
      </IntlTestWrapper>
    );
    view.unmount();
    expect(listeners.get(ENGINE_GLANCE_TURNED_OFF_CHANNEL)?.size ?? 0).toBe(0);
  });
});
