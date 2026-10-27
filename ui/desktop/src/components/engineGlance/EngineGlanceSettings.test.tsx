import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';
import { IntlTestWrapper } from '../../i18n/test-utils';
import { EngineGlanceSettings } from './EngineGlanceSettings';
import { resetEngineGlanceForTests } from './glanceStore';
import { glancePush, runningSnapshot } from '../../utils/engineGlance.fixtures';
import type { GlancePush } from '../../utils/engineGlance';
import { GENERATING_STATUS } from '../leanzero-swarm/mlxLiveStatus.fixtures';

const writing = glancePush(runningSnapshot(GENERATING_STATUS));
const electron = window.electron as unknown as Record<string, unknown>;
const saved: Record<string, unknown> = {};
let showDesktop: ReturnType<typeof vi.fn>;

beforeEach(() => {
  showDesktop = vi.fn();
  for (const key of ['on', 'off', 'engineGlanceRead', 'engineGlanceShowDesktop']) {
    saved[key] = electron[key];
  }
  Object.assign(electron, {
    on: () => undefined,
    off: () => undefined,
    engineGlanceRead: undefined,
    engineGlanceShowDesktop: showDesktop,
  });
});

afterEach(() => {
  resetEngineGlanceForTests(null);
  Object.assign(electron, saved);
});

function mount(push: GlancePush) {
  resetEngineGlanceForTests(push);
  render(
    <IntlTestWrapper>
      <EngineGlanceSettings />
    </IntlTestWrapper>
  );
}

describe('EngineGlanceSettings — Q-426: the way back after the floating window was closed', () => {
  it('closed this session: says so, and "Show it again" asks main to bring it back', () => {
    mount({ ...writing, desktopDismissed: true });
    expect(screen.getByTestId('engine-glance-desktop-dismissed').textContent).toContain(
      'You closed it — hidden until something new starts.'
    );
    fireEvent.click(screen.getByTestId('engine-glance-desktop-show-again'));
    expect(showDesktop).toHaveBeenCalledOnce();
  });

  it('negative control: not closed — no line', () => {
    mount(writing);
    expect(screen.queryByTestId('engine-glance-desktop-dismissed')).toBeNull();
  });

  it('negative control: turned off (the switch itself is the way back)', () => {
    mount({ ...writing, desktopDismissed: true, prefs: { ...writing.prefs, desktop: 'off' } });
    expect(screen.queryByTestId('engine-glance-desktop-dismissed')).toBeNull();
  });

  it('the description says what the close does now', () => {
    mount(writing);
    expect(screen.getByTestId('engine-glance-settings').textContent).toContain(
      'Only its ⤢ button opens goose; dragging just moves it, and its ✕ hides it until a new job, run or split starts.'
    );
  });
});

describe('EngineGlanceSettings — owner 2026-10-27: the floating pill is opt-in', () => {
  let setPrefs: ReturnType<typeof vi.fn>;
  beforeEach(() => {
    setPrefs = vi.fn(async () => undefined);
    saved.engineGlancePrefsSet = electron.engineGlancePrefsSet;
    Object.assign(electron, { engineGlancePrefsSet: setPrefs });
  });

  it('not ticked: the switch is off, no "when" choice, and no way back offered for a close', () => {
    mount({ ...writing, desktopDismissed: true, prefs: { ...writing.prefs, desktopOptIn: false } });
    expect(screen.getByTestId('engine-glance-desktop-optin').getAttribute('aria-checked')).toBe(
      'false'
    );
    expect(screen.queryByTestId('engine-glance-desktop-away')).toBeNull();
    expect(screen.queryByTestId('engine-glance-desktop-dismissed')).toBeNull();
  });

  it('ticking stores the opt-in (a stored "off" becomes "while in the background")', () => {
    mount({ ...writing, prefs: { ...writing.prefs, desktopOptIn: false, desktop: 'off' } });
    fireEvent.click(screen.getByTestId('engine-glance-desktop-optin'));
    expect(setPrefs).toHaveBeenCalledWith(
      expect.objectContaining({ desktopOptIn: true, desktop: 'away' })
    );
  });

  it('negative control: ticked, the "when" choice shows and unticking stores the opt-out', () => {
    mount(writing);
    expect(screen.getByTestId('engine-glance-desktop-away')).toBeTruthy();
    fireEvent.click(screen.getByTestId('engine-glance-desktop-optin'));
    expect(setPrefs).toHaveBeenCalledWith(expect.objectContaining({ desktopOptIn: false }));
  });
});
