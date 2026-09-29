import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { IntlTestWrapper } from '../../i18n/test-utils';
import { acpReadAllConfig } from '../../acp/config';
import { ConfigProvider } from '../ConfigContext';
import UnreadableConfigBanner from './UnreadableConfigBanner';

const CONFIG_PATH = '/Users/someone/.config/goose/config.yaml';
const REASON =
  "did not find expected ',' or ']' at line 4 column 1, while parsing a flow sequence at line 3 column 13";

const acp = vi.hoisted(() => ({
  unreadable: [] as unknown[],
  moveAside: vi.fn(),
}));

vi.mock('../../acp/config', () => ({
  acpReadAllConfig: vi.fn(async () => ({ config: {}, unreadableFiles: acp.unreadable })),
  acpMoveConfigAside: acp.moveAside,
  acpReadConfig: vi.fn(async () => null),
  acpRemoveConfig: vi.fn(),
  acpUpsertConfig: vi.fn(),
}));
vi.mock('../../acp/extensions', () => ({
  getConfiguredExtensions: vi.fn(async () => ({ extensions: [], warnings: [] })),
  addConfigExtension: vi.fn(),
  removeConfigExtension: vi.fn(),
  setConfigExtensionEnabled: vi.fn(),
}));
vi.mock('../../acp/providers', () => ({ acpListProviderDetails: vi.fn(async () => []) }));
vi.mock('../settings/extensions', () => ({
  syncBundledExtensions: vi.fn(async () => {}),
  pruneDeprecatedBundledExtensions: vi.fn(async (extensions: unknown[]) => extensions),
}));

const revealInFinder = vi.fn(async () => true);

function renderBanner() {
  return render(
    <IntlTestWrapper>
      <ConfigProvider>
        <UnreadableConfigBanner />
      </ConfigProvider>
    </IntlTestWrapper>
  );
}

describe('UnreadableConfigBanner (Q-468)', () => {
  beforeEach(() => {
    acp.unreadable = [{ path: CONFIG_PATH, role: 'config', reason: REASON, line: 4, column: 1 }];
    acp.moveAside.mockReset();
    revealInFinder.mockClear();
    Object.assign(window.electron, {
      revealInFinder,
      bundledMcps: vi.fn(async () => []),
      logInfo: vi.fn(),
    });
  });

  it('names the unreadable file, the parser error and its place, with both ways out', async () => {
    renderBanner();
    const banner = await screen.findByTestId('unreadable-config-banner');
    expect(banner).toHaveAttribute('role', 'alert');
    expect(within(banner).getByText('goose could not read your settings file')).toBeTruthy();
    expect(within(banner).getByTestId('unreadable-config-path').textContent).toBe(CONFIG_PATH);
    const reason = within(banner).getByTestId('unreadable-config-reason').textContent;
    expect(reason).toContain('Line 4, column 1');
    expect(reason).toContain(REASON);
    expect(
      within(banner).getByText(/your providers, models and extensions can look gone/)
    ).toBeTruthy();

    fireEvent.click(within(banner).getByTestId('unreadable-config-show'));
    expect(revealInFinder).toHaveBeenCalledWith(CONFIG_PATH);
    expect(within(banner).getByTestId('unreadable-config-move-aside').textContent).toBe(
      'Move it aside and start fresh'
    );
  });

  it('shows nothing when every settings file reads', async () => {
    acp.unreadable = [];
    renderBanner();
    await waitFor(() => expect(acpReadAllConfig).toHaveBeenCalled());
    expect(screen.queryByTestId('unreadable-config-banner')).toBeNull();
  });

  it('moves the file aside only after the in-app confirm, then reloads and clears the banner', async () => {
    acp.moveAside.mockImplementation(async (path: string) => {
      acp.unreadable = [];
      return `${path}.corrupt-20260929T101500.000Z`;
    });
    renderBanner();
    fireEvent.click(await screen.findByTestId('unreadable-config-move-aside'));
    expect(acp.moveAside).not.toHaveBeenCalled();

    const dialog = await screen.findByRole('dialog');
    expect(within(dialog).getByText('Move config.yaml aside?')).toBeTruthy();
    expect(within(dialog).getByText(/a fresh settings file/)).toBeTruthy();
    fireEvent.click(within(dialog).getByRole('button', { name: 'Move it aside' }));

    await waitFor(() => expect(acp.moveAside).toHaveBeenCalledWith(CONFIG_PATH));
    await waitFor(() => expect(screen.queryByTestId('unreadable-config-banner')).toBeNull());
  });

  it('keeps the banner and says why when goosed refuses the move', async () => {
    acp.moveAside.mockRejectedValue(new Error('permission denied'));
    renderBanner();
    fireEvent.click(await screen.findByTestId('unreadable-config-move-aside'));
    fireEvent.click(
      within(await screen.findByRole('dialog')).getByRole('button', { name: 'Move it aside' })
    );
    expect((await screen.findByTestId('unreadable-config-move-error')).textContent).toContain(
      'permission denied'
    );
    expect(screen.getByTestId('unreadable-config-banner')).toBeTruthy();
  });

  it('names the keys file for secrets.yaml', async () => {
    acp.unreadable = [
      {
        path: '/Users/someone/.config/goose/secrets.yaml',
        role: 'secrets',
        reason: 'expected a mapping of keys',
      },
    ];
    renderBanner();
    const banner = await screen.findByTestId('unreadable-config-banner');
    expect(
      within(banner).getByText('goose could not read the file that holds your keys')
    ).toBeTruthy();
    expect(within(banner).getByTestId('unreadable-config-reason').textContent).toBe(
      'expected a mapping of keys'
    );
  });
});
