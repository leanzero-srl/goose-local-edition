import { describe, expect, it, vi, beforeEach } from 'vitest';
import { act, render, screen, waitFor } from '@testing-library/react';
import { ConfigProvider, useConfig } from './ConfigContext';
import { AppEvents } from '../constants/events';

const acp = vi.hoisted(() => ({
  list: [{ name: 'developer', enabled: true, type: 'builtin' }] as unknown[],
}));

vi.mock('../acp/extensions', () => ({
  getConfiguredExtensions: vi.fn(async () => ({ extensions: acp.list, warnings: [] })),
  addConfigExtension: vi.fn(),
  removeConfigExtension: vi.fn(),
  setConfigExtensionEnabled: vi.fn(),
}));
vi.mock('../acp/config', () => ({
  acpReadAllConfig: vi.fn(async () => ({})),
  acpReadConfig: vi.fn(),
  acpRemoveConfig: vi.fn(),
  acpUpsertConfig: vi.fn(),
}));
vi.mock('../acp/providers', () => ({ acpListProviderDetails: vi.fn(async () => []) }));
vi.mock('./settings/extensions', () => ({
  syncBundledExtensions: vi.fn(async () => {}),
  pruneDeprecatedBundledExtensions: vi.fn(async () => false),
}));

const Names = () => {
  const { extensionsList } = useConfig();
  return <div data-testid="names">{extensionsList.map((e) => e.name).join(',')}</div>;
};

describe('ConfigProvider re-reads the extension list config.yaml can change behind it', () => {
  beforeEach(() => {
    acp.list = [{ name: 'developer', enabled: true, type: 'builtin' }];
  });

  it('an MCP an agent turn added shows up when the turn ends — no app reload', async () => {
    render(
      <ConfigProvider>
        <Names />
      </ConfigProvider>
    );
    await waitFor(() => expect(screen.getByTestId('names').textContent).toBe('developer'));
    acp.list = [...acp.list, { name: 'fetch', enabled: true, type: 'stdio' }];
    await act(async () => {
      window.dispatchEvent(new CustomEvent(AppEvents.MESSAGE_STREAM_FINISHED));
    });
    await waitFor(() => expect(screen.getByTestId('names').textContent).toBe('developer,fetch'));
  });

  it('and when the window regains focus', async () => {
    render(
      <ConfigProvider>
        <Names />
      </ConfigProvider>
    );
    await waitFor(() => expect(screen.getByTestId('names').textContent).toBe('developer'));
    acp.list = [];
    await act(async () => {
      window.dispatchEvent(new Event('focus'));
    });
    await waitFor(() => expect(screen.getByTestId('names').textContent).toBe(''));
  });
});
