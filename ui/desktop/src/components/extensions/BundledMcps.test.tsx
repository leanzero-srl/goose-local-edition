import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { BundledMcps, mergeMcpSettings } from './BundledMcps';
import type { FixedExtensionEntry } from '../ConfigContext';
const { add, inspect, upsert, state } = vi.hoisted(() => ({
  add: vi.fn(),
  upsert: vi.fn(),
  inspect: vi.fn(),
  state: { extensionsList: [] as FixedExtensionEntry[] },
}));
vi.mock('../ConfigContext', () => ({
  useConfig: () => ({ ...state, addExtension: add, upsert, setExtensionEnabled: vi.fn() }),
}));
vi.mock('../../acp/extensions', () => ({ inspectConfigExtension: inspect }));
const entry = {
  name: 'LeanZero Web Search',
  description: 'Search',
  type: 'stdio' as const,
  cmd: '/bundle/node',
  args: ['/bundle/search.js'],
  envs: { MCP_CLIENT_TYPE: 'agent' },
  timeout: 300,
};
beforeEach(() => {
  vi.clearAllMocks();
  state.extensionsList = [];
  window.electron.bundledMcps = vi.fn().mockResolvedValue([entry]);
  inspect.mockResolvedValue({ settings: { OUTPUT_DIR: '/research' }, tools: [], savedFile: null });
  add.mockResolvedValue(undefined);
});
describe('bundled MCP setup', () => {
  it('preserves unrelated keys, refreshes bundle paths, and removes only explicitly cleared references', () => {
    const saved = {
      ...entry,
      enabled: true,
      cmd: '/old/node',
      env_keys: ['SERPER_API_KEY', 'OTHER_KEY'],
      envs: { OTHER_VALUE: 'kept' },
    };
    const result = mergeMcpSettings(entry, saved, { SERPER_API_KEY: '', OUTPUT_DIR: '/research' });
    expect(result.env_keys).toEqual(['OTHER_KEY']);
    expect(result.envs).toMatchObject({ OUTPUT_DIR: '/research', OTHER_VALUE: 'kept' });
    expect(result.cmd).toBe('/bundle/node');
  });
  it.each(['~/corpus', 'relative/corpus'])(
    'rejects a folder that the MCP would resolve differently: %s',
    async (folder) => {
      render(<BundledMcps />);
      fireEvent.change(await screen.findByLabelText(/Research corpus folder/), {
        target: { value: folder },
      });
      fireEvent.click(screen.getByRole('button', { name: 'Save and enable' }));
      expect(await screen.findByRole('alert')).toHaveTextContent('full absolute path');
      expect(add).not.toHaveBeenCalled();
    }
  );
  it('saves actual entered settings and does not claim an unsaved connection was tested', async () => {
    render(<BundledMcps />);
    fireEvent.change(await screen.findByLabelText(/Search API key/), {
      target: { value: 'test-secret' },
    });
    fireEvent.change(screen.getByLabelText(/Research corpus folder/), {
      target: { value: '/research' },
    });
    expect(screen.getByRole('button', { name: 'Test connection' })).toBeDisabled();
    fireEvent.click(screen.getByRole('button', { name: 'Save and enable' }));
    await waitFor(() =>
      expect(add).toHaveBeenCalledWith(
        entry.name,
        expect.objectContaining({
          envs: expect.objectContaining({ SERPER_API_KEY: 'test-secret', OUTPUT_DIR: '/research' }),
        }),
        true
      )
    );
  });
  it('loads public settings without returning credentials and preserves saved secrets on an unrelated edit', async () => {
    state.extensionsList = [
      { ...entry, enabled: true, env_keys: ['SERPER_API_KEY', 'OUTPUT_DIR'] },
    ];
    render(<BundledMcps />);
    await waitFor(() =>
      expect(screen.getByLabelText(/Research corpus folder/)).toHaveValue('/research')
    );
    expect(screen.getByLabelText(/Search API key/)).toHaveValue('');
    fireEvent.change(screen.getByLabelText(/Research corpus folder/), {
      target: { value: '/updated' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Save settings' }));
    await waitFor(() =>
      expect(add).toHaveBeenCalledWith(
        entry.name,
        expect.objectContaining({ env_keys: expect.arrayContaining(['SERPER_API_KEY']) }),
        true
      )
    );
  });
});

it('offers fresh hosted setup, requires its own access token, and stores it only as a secret reference', async () => {
  render(<BundledMcps />);
  fireEvent.click(await screen.findByRole('radio', { name: 'LeanZero hosted' }));
  expect(screen.queryByLabelText(/Research corpus folder/)).not.toBeInTheDocument();
  expect(screen.getByRole('link', { name: 'Get a Web Search access token' })).toHaveAttribute(
    'href',
    'https://leanzero.net/portfolio/mcp-web-search#get-key'
  );
  fireEvent.click(screen.getByRole('button', { name: 'Save and enable' }));
  expect(await screen.findByRole('alert')).toHaveTextContent('access token');
  expect(add).not.toHaveBeenCalled();
  fireEvent.change(screen.getByLabelText(/^Web Search access token/), {
    target: { value: 'fresh-tenant-token' },
  });
  fireEvent.change(screen.getByLabelText(/^Search API key/), {
    target: { value: 'fresh-serper-key' },
  });
  fireEvent.click(screen.getByRole('button', { name: 'Save and enable' }));
  await waitFor(() => expect(add).toHaveBeenCalled());
  expect(upsert).toHaveBeenCalledWith(
    'LEANZERO_HOSTED_WEB_ACCESS_TOKEN',
    'fresh-tenant-token',
    true
  );
  expect(upsert).toHaveBeenCalledWith(
    'LEANZERO_HOSTED_WEB_SERPER_API_KEY',
    'fresh-serper-key',
    true
  );
  const config = add.mock.calls[0][1];
  expect(config.type).toBe('streamable_http');
  expect(config.headers.Authorization).toBe('Bearer ${LEANZERO_HOSTED_WEB_ACCESS_TOKEN}');
  expect(JSON.stringify(config)).not.toContain('fresh-tenant-token');
  expect(JSON.stringify(config)).not.toContain('fresh-serper-key');
});

it('does not turn successful discovery into a feature-readiness claim', async () => {
  state.extensionsList = [{ ...entry, enabled: true, env_keys: ['OUTPUT_DIR'] }];
  render(<BundledMcps />);
  await waitFor(() =>
    expect(screen.getByLabelText(/Research corpus folder/)).toHaveValue('/research')
  );
  fireEvent.click(screen.getByRole('button', { name: 'Test connection' }));
  expect(await screen.findByRole('status')).toHaveTextContent(
    'provider credentials have not been tested'
  );
  fireEvent.change(screen.getByLabelText('Search query'), { target: { value: 'MDN JavaScript' } });
  expect(screen.getByRole('button', { name: 'Run test search' })).toBeDisabled();
});

it('reloads hosted configuration without exposing saved tokens or rewriting it as a local process', async () => {
  state.extensionsList = [
    {
      name: entry.name,
      type: 'streamable_http',
      uri: 'https://example.org/mcp',
      headers: { Authorization: 'Bearer ${SAVED_TOKEN}', 'X-Output-Dir': 'research' },
      env_keys: ['SAVED_TOKEN'],
      enabled: true,
    },
  ];
  render(<BundledMcps />);
  await waitFor(() => expect(screen.getByLabelText(/^MCP endpoint/)).toHaveValue('https://example.org/mcp'));
  expect(screen.getByLabelText(/^Web Search access token/)).toHaveValue('');
  fireEvent.change(screen.getByLabelText(/^Hosted folder name/), { target: { value: 'updated' } });
  fireEvent.click(screen.getByRole('button', { name: 'Save settings' }));
  await waitFor(() => expect(add).toHaveBeenCalled());
  expect(add.mock.calls[0][1]).toMatchObject({
    type: 'streamable_http',
    headers: { Authorization: 'Bearer ${SAVED_TOKEN}', 'X-Output-Dir': 'updated' },
  });
  expect(upsert).not.toHaveBeenCalled();
  expect(inspect).not.toHaveBeenCalled();
});
