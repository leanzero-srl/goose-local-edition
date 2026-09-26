import { describe, expect, it, vi, beforeEach } from 'vitest';
import { render, waitFor } from '@testing-library/react';
import { ConfigProvider } from './ConfigContext';
import { addConfigExtension } from '../acp/extensions';
import { acpReadConfig, acpUpsertConfig } from '../acp/config';

const APP = '/Applications/Goose Swarm.app/Contents/Resources';
const DEV = '/Users/mihaiperdum/Projects/goose/ui';

vi.mock('../acp/extensions', () => ({
  getConfiguredExtensions: vi.fn(async () => ({
    extensions: [
      {
        type: 'stdio',
        name: 'LeanZero Web Search',
        description: 'old',
        cmd: `${DEV}/node_modules/electron/dist/Electron.app/Contents/MacOS/Electron`,
        args: [`${DEV}/desktop/bundled-mcps/leanzero-web-search/dist/index.js`],
        env_keys: ['ELECTRON_RUN_AS_NODE', 'OUTPUT_DIR'],
        timeout: 300,
        enabled: true,
      },
    ],
    warnings: [],
  })),
  addConfigExtension: vi.fn(async () => {}),
  removeConfigExtension: vi.fn(),
  setConfigExtensionEnabled: vi.fn(),
}));
vi.mock('../acp/config', () => ({
  acpReadAllConfig: vi.fn(async () => ({})),
  acpReadConfig: vi.fn(async () => null),
  acpRemoveConfig: vi.fn(),
  acpUpsertConfig: vi.fn(async () => {}),
}));
vi.mock('../acp/providers', () => ({ acpListProviderDetails: vi.fn(async () => []) }));
vi.mock('./settings/extensions', () => ({
  syncBundledExtensions: vi.fn(async () => {}),
  pruneDeprecatedBundledExtensions: vi.fn(async (extensions: unknown) => extensions),
}));

describe('ConfigProvider reconciles the bundled MCP servers at startup', () => {
  beforeEach(() => {
    vi.mocked(addConfigExtension).mockClear();
    window.electron.logInfo = vi.fn();
    window.electron.bundledMcps = vi.fn().mockResolvedValue([
      {
        name: 'LeanZero Web Search',
        description: 'new',
        type: 'stdio',
        cmd: `${APP}/bin/node`,
        args: [`${APP}/bundled-mcps/leanzero-web-search/dist/index.js`],
        envs: { MCP_CLIENT_TYPE: 'agent' },
        timeout: 300,
        bundleEntry: 'bundled-mcps/leanzero-web-search/dist/index.js',
        managedEnvKeys: ['MCP_CLIENT_TYPE', 'ELECTRON_RUN_AS_NODE'],
        packaged: true,
      },
    ]);
  });

  it('rewrites a dev-tree entry to the installed bundle, records it and logs it once', async () => {
    render(<ConfigProvider>{null}</ConfigProvider>);
    await waitFor(() => expect(acpUpsertConfig).toHaveBeenCalled());
    expect(acpReadConfig).toHaveBeenCalledWith('bundled_mcp_paths', false);
    expect(addConfigExtension).toHaveBeenCalledTimes(1);
    expect(addConfigExtension).toHaveBeenCalledWith(
      expect.objectContaining({
        cmd: `${APP}/bin/node`,
        args: [`${APP}/bundled-mcps/leanzero-web-search/dist/index.js`],
        env_keys: ['OUTPUT_DIR'],
        envs: { MCP_CLIENT_TYPE: 'agent' },
      }),
      true
    );
    expect(acpUpsertConfig).toHaveBeenCalledWith(
      'bundled_mcp_paths',
      expect.objectContaining({
        'LeanZero Web Search': expect.objectContaining({ cmd: `${APP}/bin/node` }),
      }),
      false
    );
    expect(window.electron.logInfo).toHaveBeenCalledTimes(1);
    expect(vi.mocked(window.electron.logInfo).mock.calls[0][0]).toContain(
      'corrected "LeanZero Web Search"'
    );
  });
});
