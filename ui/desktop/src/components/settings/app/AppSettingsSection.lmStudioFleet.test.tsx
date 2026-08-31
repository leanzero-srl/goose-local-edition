import { render, screen, waitFor, fireEvent } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import AppSettingsSection from './AppSettingsSection';
import { IntlTestWrapper } from '../../../i18n/test-utils';

/**
 * Pass E — LM Studio surfaces are DISABLED, NOT DELETED: the fleet preview's discovered rows hide
 * behind the 'showLmStudioFleet' setting (default FALSE) and the Settings > App toggle brings them
 * back. useFleet itself is untouched; the consumer gates via its `enabled` argument.
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
vi.mock('../../GooseSidebar/EditionSelector', () => ({ default: () => null }));

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

describe('LM Studio fleet behind the switch (pass E)', () => {
  it('hides discovered fleet rows by default and says so honestly', async () => {
    mount();
    await waitFor(() => expect(screen.getByTestId('show-lmstudio-fleet-toggle')).toBeInTheDocument());
    // Discovery is disabled — useFleet was called with enabled=false, no live row rendered.
    expect(fleetCalls.every((v) => v === false)).toBe(true);
    expect(screen.queryByText(/workhorse-model · live/)).toBeNull();
    expect(screen.getByText(/LM Studio fleet hidden \(legacy\)/)).toBeInTheDocument();
  });

  it('brings the rows back when the toggle is flipped, and persists the setting', async () => {
    mount();
    const toggle = await screen.findByTestId('show-lmstudio-fleet-toggle');
    fireEvent.click(toggle);
    await waitFor(() => expect(screen.getByText(/workhorse-model · live/)).toBeInTheDocument());
    expect(electron().setSetting).toBeTruthy();
    const setSetting = electron().setSetting as ReturnType<typeof vi.fn>;
    expect(setSetting).toHaveBeenCalledWith('showLmStudioFleet', true);
    expect(screen.queryByText(/LM Studio fleet hidden \(legacy\)/)).toBeNull();
  });
});
