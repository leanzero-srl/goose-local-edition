import { render, screen, waitFor, fireEvent } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import AppSettingsSection from './AppSettingsSection';
import { IntlTestWrapper } from '../../../i18n/test-utils';
import type { KeepAwakeState } from '../../../keepAwake';

vi.mock('../../swarm/useFleet', () => ({
  useFleet: () => ({ lanes: [], models: [], online: false, loading: false, endpoint: '' }),
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

const off: KeepAwakeState = { enabled: false, holding: false, error: null };

beforeEach(() => {
  const e = electron();
  e.getMenuBarIconState = vi.fn(async () => true);
  e.getWakelockState = vi.fn(async () => off);
  e.getDockIconState = vi.fn(async () => true);
  (window as unknown as { appConfig: { get: (k: string) => unknown } }).appConfig = {
    get: (k: string) => (k === 'GOOSE_VERSION' ? '9.9.9' : undefined),
  };
});

describe('Settings > App — Prevent Sleep (Q-230)', () => {
  it('says what it does: the computer stays awake while goose is open, the screen may still lock', async () => {
    mount();
    expect(
      await screen.findByText(
        'Keep your computer from going to sleep while goose is open (the screen can still turn off and lock)'
      )
    ).toBeInTheDocument();
  });

  it('shows the saved state from the main process, not an assumed default', async () => {
    electron().getWakelockState = vi.fn(async () => ({
      enabled: true,
      holding: true,
      error: null,
    }));
    mount();
    await waitFor(() =>
      expect(screen.getByTestId('prevent-sleep-toggle').getAttribute('data-state')).toBe('checked')
    );
    expect(screen.queryByRole('alert')).toBeNull();
  });

  it('switching on reflects what the main process actually did', async () => {
    const setWakelock = vi.fn(async () => ({ enabled: true, holding: true, error: null }));
    electron().setWakelock = setWakelock;
    mount();
    const toggle = await screen.findByTestId('prevent-sleep-toggle');
    await waitFor(() => expect(electron().getWakelockState).toHaveBeenCalled());
    fireEvent.click(toggle);
    await waitFor(() => expect(toggle.getAttribute('data-state')).toBe('checked'));
    expect(setWakelock).toHaveBeenCalledWith(true);
    expect(screen.queryByRole('alert')).toBeNull();
  });

  it('a blocker that could not start is said out loud under the switch', async () => {
    electron().setWakelock = vi.fn(async () => ({
      enabled: true,
      holding: false,
      error: 'IOPMAssertionCreate failed',
    }));
    mount();
    const toggle = await screen.findByTestId('prevent-sleep-toggle');
    await waitFor(() => expect(electron().getWakelockState).toHaveBeenCalled());
    fireEvent.click(toggle);
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Prevent Sleep is not working: IOPMAssertionCreate failed'
    );
  });

  it('a failed restore at startup is shown when the page opens', async () => {
    electron().getWakelockState = vi.fn(async () => ({
      enabled: true,
      holding: false,
      error: 'no power save blocker is running',
    }));
    mount();
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Prevent Sleep is not working: no power save blocker is running'
    );
  });
});
