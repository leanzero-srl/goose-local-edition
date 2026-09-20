import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { BundledMcps, mergeMcpSettings } from './BundledMcps';
import type { FixedExtensionEntry } from '../ConfigContext';
const { add, inspect, state } = vi.hoisted(() => ({
  add: vi.fn(),
  inspect: vi.fn(),
  state: { extensionsList: [] as FixedExtensionEntry[] },
}));
vi.mock('../ConfigContext', () => ({ useConfig: () => ({ ...state, addExtension: add }) }));
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
