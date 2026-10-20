import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { IntlTestWrapper } from '../../../i18n/test-utils';
import {
  MlxRestoreSettings,
  RESTORE_AT_LAUNCH_KEY,
  restoreAtLaunchFrom,
} from './MlxRestoreSettings';

const config = vi.hoisted(() => ({
  stored: undefined as unknown,
  read: vi.fn(),
  upsert: vi.fn(),
}));

vi.mock('../../../acp/config', () => ({
  acpReadConfig: config.read,
  acpUpsertConfig: config.upsert,
}));

const mount = () =>
  render(
    <IntlTestWrapper>
      <MlxRestoreSettings />
    </IntlTestWrapper>
  );

const toggle = () => screen.getByTestId('mlx-restore-at-launch-switch');

beforeEach(() => {
  config.stored = null;
  config.read.mockReset().mockImplementation(async () => config.stored);
  config.upsert.mockReset().mockImplementation(async (_key: string, value: unknown) => {
    config.stored = value;
  });
});

describe('Settings > App — Restore the local model at launch', () => {
  it('is off when goose config holds nothing: an absent key starts no model at launch', async () => {
    mount();
    await waitFor(() => expect(toggle()).not.toBeDisabled());
    expect(toggle().getAttribute('data-state')).toBe('unchecked');
    expect(config.read).toHaveBeenCalledWith(RESTORE_AT_LAUNCH_KEY, false);
  });

  it('shows the saved value and writes the owner’s choice to goose config', async () => {
    config.stored = true;
    mount();
    await waitFor(() => expect(toggle().getAttribute('data-state')).toBe('checked'));
    fireEvent.click(toggle());
    await waitFor(() =>
      expect(config.upsert).toHaveBeenCalledWith(RESTORE_AT_LAUNCH_KEY, false, false)
    );
    expect(toggle().getAttribute('data-state')).toBe('unchecked');
  });

  it('a failed save puts the switch back and says why', async () => {
    config.upsert.mockImplementation(async () => {
      throw new Error('config.yaml is read-only');
    });
    mount();
    await waitFor(() => expect(toggle()).not.toBeDisabled());
    fireEvent.click(toggle());
    expect(await screen.findByRole('alert')).toHaveTextContent('config.yaml is read-only');
    expect(toggle().getAttribute('data-state')).toBe('unchecked');
  });

  it('reads only a real true as on', () => {
    expect(restoreAtLaunchFrom(true)).toBe(true);
    expect(restoreAtLaunchFrom('true')).toBe(true);
    for (const off of [null, undefined, false, 'false', 1, 'yes']) {
      expect(restoreAtLaunchFrom(off)).toBe(false);
    }
  });
});
