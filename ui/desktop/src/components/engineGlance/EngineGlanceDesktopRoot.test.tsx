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

describe('EngineGlanceDesktopRoot — Q-426: the X closes it, from the window, in one click', () => {
  for (const collapsed of [false, true]) {
    it(`${collapsed ? 'pill' : 'card'}: the X asks main to close it — and writes no setting`, () => {
      mount(withPrefs(writing, { desktopHintSeen: true, desktopCollapsed: collapsed }));
      expect(screen.getByTestId('engine-glance').dataset.collapsed).toBe(String(collapsed));
      fireEvent.click(screen.getByTestId('engine-glance-close'));
      expect(actionsSent()).toEqual(['close']);
      // No second store: the dismissal is main's session state, never a setting.
      expect(prefsSet).not.toHaveBeenCalled();
    });
  }

  it('the stack opens away from the remembered corner', () => {
    mount(withPrefs(writing, { desktopPlace: { displayId: 1, corner: 'top-left' } }));
    expect(screen.getByTestId('engine-glance-stack').className).toContain('items-start');
  });
});

describe('EngineGlanceDesktopRoot — Q-224/Q-426: the one-time hint', () => {
  it('never seen: shown; main marking it seen as the window appears does not take it away', () => {
    mount(writing);
    expect(screen.getByTestId('engine-glance-hint').textContent).toContain(
      'Close it here until you bring it back'
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
});
