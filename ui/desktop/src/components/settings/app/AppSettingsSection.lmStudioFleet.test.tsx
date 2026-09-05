import { render, screen, waitFor, fireEvent } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import AppSettingsSection from './AppSettingsSection';
import { IntlTestWrapper } from '../../../i18n/test-utils';

/**
 * Pass E + follow-up — Settings > App:
 *  - the 'showLmStudioFleet' toggle lives here (default FALSE) and persists; it now governs the
 *    LM Studio surfaces elsewhere (nodes-tab discovered rows, run-panel lms dots, recipe wizard);
 *  - the legacy "Goose Swarm" card — edition switcher + LM Studio fan-in preview — is GONE from
 *    this page (SHOW_EDITION_CARD), so no fleet discovery ever runs from App settings, toggle on
 *    or off.
 */

let fleetCalls: Array<boolean | undefined> = [];
vi.mock('../../swarm/useFleet', () => ({
  useFleet: (_pollMs?: number, _endpoint?: string, enabled?: boolean) => {
    fleetCalls.push(enabled);
    return enabled
      ? {
          lanes: [{ device: 'workhorse', action: 'workhorse-model · live', status: 'done' }],
          models: ['workhorse-model'],
          online: true,
          loading: false,
          endpoint: 'http://127.0.0.1:1234/api/v0/models',
        }
      : { lanes: [], models: [], online: false, loading: false, endpoint: '' };
  },
}));
vi.mock('./UpdateSection', () => ({ default: () => null }));
vi.mock('./TelemetrySettings', () => ({ default: () => null }));
vi.mock('../../GooseSidebar/ThemeSelector', () => ({ default: () => null }));
vi.mock('../../GooseSidebar/EditionSelector', () => ({
  default: () => <div data-testid="edition-selector" />,
}));

type ElectronMock = Record<string, unknown>;
const electron = () => (window as unknown as { electron: ElectronMock }).electron;

const mount = () =>
  render(
    <IntlTestWrapper>
      <AppSettingsSection />
    </IntlTestWrapper>
  );

beforeEach(() => {
  fleetCalls = [];
  const e = electron();
  e.getMenuBarIconState = vi.fn(async () => true);
  e.getWakelockState = vi.fn(async () => false);
  e.getDockIconState = vi.fn(async () => true);
  e.openNotificationsSettings = vi.fn(async () => {});
  (window as unknown as { appConfig: { get: (k: string) => unknown } }).appConfig = {
    get: (k: string) => (k === 'GOOSE_VERSION' ? '9.9.9' : undefined),
  };
});

describe('Settings > App (pass E follow-up)', () => {
  it('has the LM Studio toggle, default off, and NO legacy Goose Swarm card', async () => {
    mount();
    await waitFor(() =>
      expect(screen.getByTestId('show-lmstudio-fleet-toggle')).toBeInTheDocument()
    );
    expect(
      screen.getByTestId('show-lmstudio-fleet-toggle').getAttribute('data-state')
    ).toBe('unchecked');
    // The edition card and its fan-in preview are gone wholesale.
    expect(screen.queryByText('Goose Swarm')).toBeNull();
    expect(screen.queryByText('Goose Swarm — the fan-in view')).toBeNull();
    expect(screen.queryByTestId('edition-selector')).toBeNull();
    expect(screen.queryByText(/workhorse-model · live/)).toBeNull();
    // No fleet discovery runs from this page.
    expect(fleetCalls.every((v) => v === false)).toBe(true);
  });

  it('flipping the toggle persists the setting but never resurrects the fleet preview here', async () => {
    mount();
    const toggle = await screen.findByTestId('show-lmstudio-fleet-toggle');
    fireEvent.click(toggle);
    await waitFor(() => {
      const setSetting = electron().setSetting as ReturnType<typeof vi.fn>;
      expect(setSetting).toHaveBeenCalledWith('showLmStudioFleet', true);
    });
    expect(screen.queryByText(/workhorse-model · live/)).toBeNull();
    expect(screen.queryByText('Goose Swarm — the fan-in view')).toBeNull();
    expect(fleetCalls.every((v) => v === false)).toBe(true);
  });
});
