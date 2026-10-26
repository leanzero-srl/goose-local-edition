import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { IntlTestWrapper } from '../../../i18n/test-utils';
import { MemfreeSettings } from './MemfreeSettings';
import {
  LINK_ALLOW_REMOTE_APP_QUIT_KEY,
  MEMFREE_ENABLED_KEY,
  MEMFREE_NEVER_TOUCH_KEY,
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

const census = vi.hoisted(() => ({ holders: vi.fn() }));
vi.mock('../../../acp/memfree', () => ({ memoryHolders: census.holders }));

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
  census.holders.mockReset().mockResolvedValue({ groups: [] });
  (window as unknown as { electron: Record<string, unknown> }).electron = {
    ...((window as unknown as { electron?: Record<string, unknown> }).electron ?? {}),
    listInstalledApps: vi.fn().mockResolvedValue([]),
  };
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

  it('never-touch: shows the built-in defaults, adds and removes the person’s own entries', async () => {
    config.stored = { [MEMFREE_NEVER_TOUCH_KEY]: ['Photoshop'] };
    mount();
    await waitFor(() => expect(enabled()).not.toBeDisabled());
    expect(config.read).toHaveBeenCalledWith(MEMFREE_NEVER_TOUCH_KEY, false);
    const defaults = screen.getByTestId('memfree-never-touch-defaults');
    expect(defaults.textContent).toContain('com.microsoft.teams2');
    expect(defaults.textContent).toContain('dev.warp.Warp-Stable');
    expect(screen.getByTestId('memfree-never-touch-extras').textContent).toContain('Photoshop');
    const input = screen.getByTestId('memfree-never-touch-input');
    fireEvent.change(input, { target: { value: ' Final Cut Pro ' } });
    fireEvent.click(screen.getByTestId('memfree-never-touch-add'));
    await waitFor(() =>
      expect(config.upsert).toHaveBeenCalledWith(
        MEMFREE_NEVER_TOUCH_KEY,
        ['Photoshop', 'Final Cut Pro'],
        false
      )
    );
    // A default typed again is not stored twice.
    fireEvent.change(input, { target: { value: 'US.ZOOM.XOS' } });
    fireEvent.click(screen.getByTestId('memfree-never-touch-add'));
    await waitFor(() => expect(config.upsert).toHaveBeenCalledTimes(2));
    expect(config.upsert).toHaveBeenLastCalledWith(
      MEMFREE_NEVER_TOUCH_KEY,
      ['Photoshop', 'Final Cut Pro'],
      false
    );
    fireEvent.click(screen.getByTestId('memfree-never-touch-remove-Photoshop'));
    await waitFor(() =>
      expect(config.upsert).toHaveBeenLastCalledWith(
        MEMFREE_NEVER_TOUCH_KEY,
        ['Final Cut Pro'],
        false
      )
    );
  });
  it('C7: each added entry says what it matches, or that it matches no running or installed app', async () => {
    config.stored = { [MEMFREE_NEVER_TOUCH_KEY]: ['Teams', 'Final Cut', 'Photoshop'] };
    census.holders.mockResolvedValue({
      groups: [
        {
          id: 'app:com.microsoft.teams2',
          label: 'Microsoft Teams',
          category: 'userApp',
          bytes: 1,
          pids: [],
          bundleId: 'com.microsoft.teams2',
          quittable: true,
          reclaimableByGoose: false,
        },
      ],
    });
    (
      window as unknown as { electron: { listInstalledApps: ReturnType<typeof vi.fn> } }
    ).electron.listInstalledApps.mockResolvedValue([
      { name: 'Final Cut Pro', bundleId: 'com.apple.FinalCut' },
    ]);
    mount();
    await waitFor(() =>
      expect(screen.getByTestId('memfree-never-touch-match-Photoshop')).toBeInTheDocument()
    );
    expect(census.holders).toHaveBeenCalledWith({});
    const teams = screen.getByTestId('memfree-never-touch-match-Teams');
    expect(teams.dataset.matched).toBe('yes');
    expect(teams.textContent).toContain('Microsoft Teams');
    expect(screen.getByTestId('memfree-never-touch-match-Final Cut').textContent).toContain(
      'Final Cut Pro'
    );
    const none = screen.getByTestId('memfree-never-touch-match-Photoshop');
    expect(none.dataset.matched).toBe('no');
    expect(none.textContent).toContain('matches no running or installed app');
  });

  it('C7: no census, no hint (never a wrong "matches nothing")', async () => {
    config.stored = { [MEMFREE_NEVER_TOUCH_KEY]: ['Photoshop'] };
    census.holders.mockRejectedValue(new Error('engine down'));
    mount();
    await waitFor(() => expect(enabled()).not.toBeDisabled());
    expect(screen.getByTestId('memfree-never-touch-extras').textContent).toContain('Photoshop');
    expect(screen.queryByTestId('memfree-never-touch-match-Photoshop')).toBeNull();
  });
});
