import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, fireEvent, render, screen } from '@testing-library/react';
import { IntlTestWrapper } from '../../i18n/test-utils';
import { EngineGlanceDesktopRoot } from './EngineGlanceDesktopRoot';
import { resetEngineGlanceForTests } from './glanceStore';
import { glancePush, runningSnapshot } from '../../utils/engineGlance.fixtures';
import { ENGINE_GLANCE_CHANNEL, type GlancePrefs, type GlancePush } from '../../utils/engineGlance';
import type { GlancePipAction } from '../../engineGlanceDesktop';
import { GENERATING_STATUS } from '../leanzero-swarm/mlxLiveStatus.fixtures';

const writing = glancePush(runningSnapshot(GENERATING_STATUS));
const withPrefs = (push: GlancePush, prefs: Partial<GlancePrefs>): GlancePush => ({
  ...push,
  prefs: { ...push.prefs, ...prefs },
});

type Listener = (event: unknown, ...args: unknown[]) => void;
let listeners: Map<string, Set<Listener>>;
let pip: ReturnType<typeof vi.fn<(action: GlancePipAction) => void>>;
let prefsSet: ReturnType<typeof vi.fn<(prefs: GlancePrefs) => Promise<void>>>;
const electron = window.electron as unknown as Record<string, unknown>;
const saved: Record<string, unknown> = {};

beforeEach(() => {
  listeners = new Map();
  pip = vi.fn();
  prefsSet = vi.fn(async () => undefined);
  for (const key of ['on', 'off', 'engineGlancePip', 'engineGlancePrefsSet', 'engineGlanceRead']) {
    saved[key] = electron[key];
  }
  Object.assign(electron, {
    on: (channel: string, fn: Listener) => {
      if (!listeners.has(channel)) listeners.set(channel, new Set());
      listeners.get(channel)!.add(fn);
    },
    off: (channel: string, fn: Listener) => listeners.get(channel)?.delete(fn),
    engineGlancePip: pip,
    engineGlancePrefsSet: prefsSet,
    engineGlanceRead: undefined,
  });
});

afterEach(() => {
  resetEngineGlanceForTests(null);
  Object.assign(electron, saved);
});

function mount(push: GlancePush) {
  resetEngineGlanceForTests(push);
  return render(
    <IntlTestWrapper>
      <EngineGlanceDesktopRoot />
    </IntlTestWrapper>
  );
}

/** main's next glance, as it reaches the window. */
function pushFromMain(push: GlancePush) {
  act(() => {
    for (const fn of listeners.get(ENGINE_GLANCE_CHANNEL) ?? []) fn({}, push);
  });
}

function actionsSent(): GlancePipAction['type'][] {
  return pip.mock.calls.map(([action]) => action.type).filter((t) => t !== 'size');
}

describe('EngineGlanceDesktopRoot — Q-224: two ways to hide it, both from the window', () => {
  it('"Hide for now": the window asks main to snooze it — and writes no setting', () => {
    mount(withPrefs(writing, { desktopHintSeen: true }));
    fireEvent.click(screen.getByTestId('engine-glance-close'));
    fireEvent.click(screen.getByTestId('engine-glance-hide-for-now'));
    expect(actionsSent()).toEqual(['close']);
    expect(prefsSet).not.toHaveBeenCalled();
  });

  it('"Turn off the floating window": the window asks main to turn it off (the setting is main’s one save)', () => {
    mount(withPrefs(writing, { desktopHintSeen: true }));
    fireEvent.click(screen.getByTestId('engine-glance-close'));
    fireEvent.click(screen.getByTestId('engine-glance-turn-off'));
    expect(actionsSent()).toEqual(['turn-off']);
    // No second store: the window never writes the prefs itself.
    expect(prefsSet).not.toHaveBeenCalled();
  });

  it('the pill offers the same two choices', () => {
    mount(withPrefs(writing, { desktopHintSeen: true, desktopCollapsed: true }));
    expect(screen.getByTestId('engine-glance').dataset.collapsed).toBe('true');
    fireEvent.click(screen.getByTestId('engine-glance-close'));
    fireEvent.click(screen.getByTestId('engine-glance-turn-off'));
    expect(actionsSent()).toEqual(['turn-off']);
  });

  it('the stack opens away from the remembered corner', () => {
    mount(
      withPrefs(writing, {
        desktopHintSeen: true,
        desktopPlace: { displayId: 1, corner: 'top-left' },
      })
    );
    fireEvent.click(screen.getByTestId('engine-glance-close'));
    expect(screen.getByTestId('engine-glance-stack').className).toContain('items-start');
  });
});

describe('EngineGlanceDesktopRoot — Q-224: the one-time hint', () => {
  it('never seen: shown; main marking it seen as the window appears does not take it away', () => {
    mount(writing);
    expect(screen.getByTestId('engine-glance-hint').textContent).toContain(
      'You can turn this off from here'
    );
    pushFromMain(withPrefs(writing, { desktopHintSeen: true }));
    expect(screen.getByTestId('engine-glance-hint')).toBeTruthy();
  });

  it('"Got it" dismisses it, and a later glance does not bring it back', () => {
    mount(writing);
    fireEvent.click(screen.getByTestId('engine-glance-hint-dismiss'));
    expect(screen.queryByTestId('engine-glance-hint')).toBeNull();
    pushFromMain(withPrefs(writing, { desktopHintSeen: false }));
    expect(screen.queryByTestId('engine-glance-hint')).toBeNull();
    expect(actionsSent()).toEqual([]);
  });

  it('seen once (stored): a new window never shows it again', () => {
    const first = mount(writing);
    expect(screen.getByTestId('engine-glance-hint')).toBeTruthy();
    first.unmount();
    resetEngineGlanceForTests(null);
    mount(withPrefs(writing, { desktopHintSeen: true }));
    expect(screen.queryByTestId('engine-glance-hint')).toBeNull();
  });

  it('opening the X counts as found: the hint goes and the choices take its place', () => {
    mount(writing);
    fireEvent.click(screen.getByTestId('engine-glance-close'));
    expect(screen.queryByTestId('engine-glance-hint')).toBeNull();
    fireEvent.click(screen.getByTestId('engine-glance-close'));
    expect(screen.queryByTestId('engine-glance-hint')).toBeNull();
  });
});
