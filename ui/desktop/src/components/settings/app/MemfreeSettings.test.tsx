import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { IntlTestWrapper } from '../../../i18n/test-utils';
import { MemfreeSettings } from './MemfreeSettings';
import {
  LINK_ALLOW_REMOTE_APP_QUIT_KEY,
  MEMFREE_ENABLED_KEY,
  MEMFREE_SHOW_APPS_MIN_MB_KEY,
} from '../../memfree/memfree';

const config = vi.hoisted(() => ({
  stored: {} as Record<string, unknown>,
  read: vi.fn(),
  upsert: vi.fn(),
}));

vi.mock('../../../acp/config', () => ({
  acpReadConfig: config.read,
  acpUpsertConfig: config.upsert,
}));

const forget = vi.hoisted(() => vi.fn());
vi.mock('../../memfree/useMemfreeEnabled', () => ({ forgetMemfreeEnabled: forget }));

const mount = () =>
  render(
    <IntlTestWrapper>
      <MemfreeSettings />
    </IntlTestWrapper>
  );

const enabled = () => screen.getByTestId('memfree-enabled-switch');
const remoteQuit = () => screen.getByTestId('memfree-remote-quit-switch');
const minMb = () => screen.getByTestId('memfree-min-mb-input') as HTMLInputElement;

beforeEach(() => {
  config.stored = {};
  config.read.mockReset().mockImplementation(async (key: string) => config.stored[key] ?? null);
  config.upsert.mockReset().mockImplementation(async (key: string, value: unknown) => {
    config.stored[key] = value;
  });
  forget.mockReset();
});

describe('Settings > App — Free up memory', () => {
  it('reads the three keys; with nothing stored: on, 200 MB, remote quit off', async () => {
    mount();
    await waitFor(() => expect(enabled()).not.toBeDisabled());
    expect(config.read).toHaveBeenCalledWith(MEMFREE_ENABLED_KEY, false);
    expect(config.read).toHaveBeenCalledWith(MEMFREE_SHOW_APPS_MIN_MB_KEY, false);
    expect(config.read).toHaveBeenCalledWith(LINK_ALLOW_REMOTE_APP_QUIT_KEY, false);
    expect(enabled().getAttribute('data-state')).toBe('checked');
    expect(minMb().value).toBe('200');
    expect(remoteQuit().getAttribute('data-state')).toBe('unchecked');
  });

  it('shows the stored values', async () => {
    config.stored = {
      [MEMFREE_ENABLED_KEY]: false,
      [MEMFREE_SHOW_APPS_MIN_MB_KEY]: 512,
      [LINK_ALLOW_REMOTE_APP_QUIT_KEY]: true,
    };
    mount();
    await waitFor(() => expect(minMb().value).toBe('512'));
    expect(enabled().getAttribute('data-state')).toBe('unchecked');
    expect(remoteQuit().getAttribute('data-state')).toBe('checked');
  });

  it('turning it off saves false and drops the cached enabled read', async () => {
    mount();
    await waitFor(() => expect(enabled()).not.toBeDisabled());
    fireEvent.click(enabled());
    await waitFor(() =>
      expect(config.upsert).toHaveBeenCalledWith(MEMFREE_ENABLED_KEY, false, false)
    );
    expect(forget).toHaveBeenCalled();
    expect(enabled().getAttribute('data-state')).toBe('unchecked');
  });

  it('remote quit saves its own key and leaves the enabled cache alone', async () => {
    mount();
    await waitFor(() => expect(remoteQuit()).not.toBeDisabled());
    fireEvent.click(remoteQuit());
    await waitFor(() =>
      expect(config.upsert).toHaveBeenCalledWith(LINK_ALLOW_REMOTE_APP_QUIT_KEY, true, false)
    );
    expect(forget).not.toHaveBeenCalled();
  });

  it('the minimum MB saves on blur as a number; junk falls back to 200', async () => {
    mount();
    await waitFor(() => expect(minMb()).not.toBeDisabled());
    fireEvent.change(minMb(), { target: { value: '750' } });
    expect(config.upsert).not.toHaveBeenCalled();
    fireEvent.blur(minMb());
    await waitFor(() =>
      expect(config.upsert).toHaveBeenCalledWith(MEMFREE_SHOW_APPS_MIN_MB_KEY, 750, false)
    );
    fireEvent.change(minMb(), { target: { value: 'abc' } });
    fireEvent.blur(minMb());
    await waitFor(() =>
      expect(config.upsert).toHaveBeenLastCalledWith(MEMFREE_SHOW_APPS_MIN_MB_KEY, 200, false)
    );
    expect(minMb().value).toBe('200');
  });

  it('a failed save puts the switch back and says why', async () => {
    config.upsert.mockImplementation(async () => {
      throw new Error('config.yaml is read-only');
    });
    mount();
    await waitFor(() => expect(enabled()).not.toBeDisabled());
    fireEvent.click(enabled());
    expect((await screen.findByRole('alert')).textContent).toContain('config.yaml is read-only');
    expect(enabled().getAttribute('data-state')).toBe('checked');
  });

  it('a failed read says why and keeps the rows disabled', async () => {
    config.read.mockImplementation(async () => {
      throw new Error('engine not reachable');
    });
    mount();
    expect((await screen.findByRole('alert')).textContent).toContain('engine not reachable');
    expect(enabled()).toBeDisabled();
  });
});
