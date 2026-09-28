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
      'You closed it — hidden until you bring it back.'
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
      'Its close button hides it for the rest of this session.'
    );
  });
});
