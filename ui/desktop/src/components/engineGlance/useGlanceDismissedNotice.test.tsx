import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, render } from '@testing-library/react';
import { IntlTestWrapper } from '../../i18n/test-utils';
import { useGlanceDismissedNotice } from './useGlanceDismissedNotice';
import { ENGINE_GLANCE_DISMISSED_CHANNEL } from '../../utils/engineGlance';

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
  useGlanceDismissedNotice();
  return null;
}

function mount() {
  return render(
    <IntlTestWrapper>
      <Host />
    </IntlTestWrapper>
  );
}

function mainSays(notice: unknown) {
  act(() => {
    for (const fn of listeners.get(ENGINE_GLANCE_DISMISSED_CHANNEL) ?? []) fn({}, notice);
  });
}

function toasted(): { title: string; msg: string } {
  return (toastSuccess.mock.calls[0] as [{ title: string; msg: string }])[0];
}

describe('useGlanceDismissedNotice — Q-426: closed for the session, said once, with the ways back', () => {
  it('a menu-bar icon: the toast names it, and Settings › App', () => {
    mount();
    expect(toastSuccess).not.toHaveBeenCalled();
    mainSays({ tray: true });
    expect(toastSuccess).toHaveBeenCalledOnce();
    expect(toasted().title).toBe('Floating window hidden for this session');
    expect(toasted().msg).toBe(
      'Bring it back from the goose icon in the menu bar or Settings › App, where you can also turn it off.'
    );
  });

  it('no menu-bar icon: the toast never points at one', () => {
    mount();
    mainSays({ tray: false });
    expect(toasted().msg).toBe(
      'Bring it back from Settings › App, where you can also turn it off.'
    );
  });

  it('a notice main did not shape: the way that always exists', () => {
    mount();
    mainSays(undefined);
    expect(toasted().msg).not.toContain('menu bar');
  });

  it('unmounted: it stops listening', () => {
    const view = mount();
    view.unmount();
    expect(listeners.get(ENGINE_GLANCE_DISMISSED_CHANNEL)?.size ?? 0).toBe(0);
  });
});
