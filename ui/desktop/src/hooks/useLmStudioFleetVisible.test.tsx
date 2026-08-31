import { render, screen, waitFor, act } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import {
  useLmStudioFleetVisible,
  LMSTUDIO_FLEET_SETTING_CHANGED,
} from './useLmStudioFleetVisible';

type ElectronMock = Record<string, unknown>;
const electron = () => (window as unknown as { electron: ElectronMock }).electron;

function Probe() {
  return <div data-testid="probe">{useLmStudioFleetVisible() ? 'visible' : 'hidden'}</div>;
}

beforeEach(() => {
  electron().getSetting = vi.fn(async () => false);
});

describe('useLmStudioFleetVisible', () => {
  it('is hidden by default (setting false or absent)', async () => {
    electron().getSetting = vi.fn(async () => undefined);
    render(<Probe />);
    await waitFor(() => expect(screen.getByTestId('probe')).toHaveTextContent('hidden'));
  });

  it('is visible when the setting is on', async () => {
    electron().getSetting = vi.fn(async () => true);
    render(<Probe />);
    await waitFor(() => expect(screen.getByTestId('probe')).toHaveTextContent('visible'));
  });

  it('re-reads when the settings toggle announces a change', async () => {
    let value = false;
    electron().getSetting = vi.fn(async () => value);
    render(<Probe />);
    await waitFor(() => expect(screen.getByTestId('probe')).toHaveTextContent('hidden'));
    value = true;
    act(() => {
      window.dispatchEvent(new CustomEvent(LMSTUDIO_FLEET_SETTING_CHANGED));
    });
    await waitFor(() => expect(screen.getByTestId('probe')).toHaveTextContent('visible'));
  });
});
