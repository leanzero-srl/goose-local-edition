import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { cleanup, render as rtlRender, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import type { CustomProviderConfigDto } from '@aaif/goose-sdk';
import { IntlTestWrapper } from '../../i18n/test-utils';
import CompatibleEndpointDialog from './CompatibleEndpointDialog';
import type { ProviderDetails } from '../../types/providers';

const mockCreate = vi.fn();
const mockUpdate = vi.fn();
const mockDelete = vi.fn();
const mockLive = vi.fn();
const mockSaveDefault = vi.fn();
vi.mock('../../acp/providers', () => ({
  acpCreateCustomProviderFromRequest: (...a: unknown[]) => mockCreate(...a),
  acpUpdateCustomProviderFromRequest: (...a: unknown[]) => mockUpdate(...a),
  acpDeleteCustomProvider: (...a: unknown[]) => mockDelete(...a),
  acpListProviderLiveModels: (...a: unknown[]) => mockLive(...a),
  acpSaveProviderDefaultModel: (...a: unknown[]) => mockSaveDefault(...a),
}));

const saved: CustomProviderConfigDto = {
  providerId: 'custom_team_gateway',
  engine: 'openai_compatible',
  displayName: 'Team Gateway',
  apiUrl: 'https://gw.corp.example/v1',
  models: ['qwen3-coder', 'llama-4-scout'],
  headers: { 'X-Team': 'platform' },
  requiresAuth: true,
  apiKeySet: true,
  preservesThinking: true,
  supportsStreaming: null,
  basePath: null,
  catalogProviderId: null,
};

const row = {
  name: 'custom_team_gateway',
  is_configured: true,
  provider_type: 'Custom',
  default_model: 'qwen3-coder',
  metadata: { display_name: 'Team Gateway' },
} as unknown as ProviderDetails;

function render(props: Partial<Parameters<typeof CompatibleEndpointDialog>[0]> = {}) {
  const onClose = vi.fn();
  const onSaved = vi.fn().mockResolvedValue(undefined);
  rtlRender(<CompatibleEndpointDialog onClose={onClose} onSaved={onSaved} {...props} />, {
    wrapper: IntlTestWrapper,
  });
  return { onClose, onSaved };
}

beforeEach(() => {
  vi.clearAllMocks();
  mockCreate.mockResolvedValue({ provider_name: 'custom_desk_vllm' });
  mockUpdate.mockResolvedValue(undefined);
  mockDelete.mockResolvedValue(undefined);
  mockSaveDefault.mockResolvedValue(undefined);
});
afterEach(() => cleanup());

describe('CompatibleEndpointDialog', () => {
  it('new endpoint: fields → created with no models → the engine lists → the default runs, then leads the saved list', async () => {
    mockLive.mockResolvedValue(['llama-4-scout', 'qwen3-coder']);
    const { onClose, onSaved } = render();

    await userEvent.click(screen.getByTestId('compatible-endpoint-test'));
    expect(screen.getByText('Name is required')).toBeInTheDocument();
    expect(screen.getByText('Base URL is required')).toBeInTheDocument();
    expect(mockCreate).not.toHaveBeenCalled();

    await userEvent.type(screen.getByLabelText('Name'), 'Desk vLLM');
    await userEvent.type(screen.getByLabelText('Base URL'), 'http://10.0.0.5:8000/v1');
    await userEvent.type(screen.getByLabelText('API key'), 'desk-key');
    await userEvent.click(screen.getByRole('button', { name: /Add header/ }));
    await userEvent.type(screen.getByLabelText('Header name'), 'X-Team');
    await userEvent.type(screen.getByLabelText('Value'), 'platform');
    await userEvent.click(screen.getByTestId('compatible-endpoint-test'));

    await waitFor(() => expect(mockCreate).toHaveBeenCalledTimes(1));
    expect(mockCreate.mock.calls[0][0]).toMatchObject({
      engine: 'openai_compatible',
      display_name: 'Desk vLLM',
      api_url: 'http://10.0.0.5:8000/v1',
      api_key: 'desk-key',
      models: [],
      headers: { 'X-Team': 'platform' },
      requires_auth: true,
    });
    expect(mockLive).toHaveBeenCalledWith('custom_desk_vllm');
    await userEvent.click(await screen.findByTestId('cloud-model-qwen3-coder'));
    await userEvent.click(screen.getByTestId('compatible-endpoint-save'));

    await waitFor(() => expect(onClose).toHaveBeenCalled());
    expect(mockSaveDefault).toHaveBeenCalledWith('custom_desk_vllm', 'qwen3-coder');
    // The stored key is kept (blank key, auth still on) and the chosen default leads the list.
    expect(mockUpdate).toHaveBeenCalledWith(
      'custom_desk_vllm',
      expect.objectContaining({
        api_key: '',
        requires_auth: true,
        models: ['qwen3-coder', 'llama-4-scout'],
      })
    );
    expect(mockSaveDefault.mock.invocationCallOrder[0]).toBeLessThan(
      mockUpdate.mock.invocationCallOrder[0]
    );
    expect(mockDelete).not.toHaveBeenCalled();
    expect(onSaved).toHaveBeenCalled();
  });

  it('a server with no model listing shows its own words and a typed id still saves; keyless means no auth', async () => {
    mockLive.mockRejectedValue(new Error('Failed to fetch provider supported models: 404'));
    render();
    await userEvent.type(screen.getByLabelText('Name'), 'Desk llama.cpp');
    await userEvent.type(screen.getByLabelText('Base URL'), 'http://10.0.0.6:8080');
    await userEvent.click(screen.getByTestId('compatible-endpoint-test'));

    expect(await screen.findByTestId('compatible-endpoint-list-error')).toHaveTextContent(
      'Failed to fetch provider supported models: 404'
    );
    expect(mockCreate.mock.calls[0][0]).toMatchObject({ api_key: '', requires_auth: false });
    await userEvent.type(screen.getByLabelText('Or type a model id'), 'qwen3-8b');
    await userEvent.click(screen.getByTestId('compatible-endpoint-save'));
    await waitFor(() =>
      expect(mockSaveDefault).toHaveBeenCalledWith('custom_desk_vllm', 'qwen3-8b')
    );
    expect(mockUpdate).toHaveBeenCalledWith(
      'custom_desk_vllm',
      expect.objectContaining({ models: ['qwen3-8b'], requires_auth: false })
    );
  });

  it('a default the server refuses to run is not saved, and the error is the engine’s own', async () => {
    mockLive.mockResolvedValue(['llama-4-scout']);
    mockSaveDefault.mockRejectedValue(
      new Error('Connection check failed; previous settings retained. not enabled for this key')
    );
    const { onClose } = render();
    await userEvent.type(screen.getByLabelText('Name'), 'Gateway');
    await userEvent.type(screen.getByLabelText('Base URL'), 'https://gw.example/v1');
    await userEvent.click(screen.getByTestId('compatible-endpoint-test'));
    await userEvent.click(await screen.findByTestId('cloud-model-llama-4-scout'));
    await userEvent.click(screen.getByTestId('compatible-endpoint-save'));
    expect(await screen.findByText(/not enabled for this key/)).toBeInTheDocument();
    expect(mockUpdate).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();
  });

  it('closing a new endpoint that never got a default removes it again', async () => {
    mockLive.mockResolvedValue(['m']);
    const { onClose } = render();
    await userEvent.type(screen.getByLabelText('Name'), 'Typo');
    await userEvent.type(screen.getByLabelText('Base URL'), 'http://10.0.0.7:1/v1');
    await userEvent.click(screen.getByTestId('compatible-endpoint-test'));
    await screen.findByTestId('cloud-model-m');
    await userEvent.keyboard('{Escape}');
    await waitFor(() => expect(mockDelete).toHaveBeenCalledWith('custom_desk_vllm'));
    expect(onClose).toHaveBeenCalled();
  });

  it('an existing endpoint opens on its models with the default chosen; editing keeps the stored key', async () => {
    mockLive.mockResolvedValue(['llama-4-scout', 'qwen3-coder']);
    render({ endpoint: { provider: row, config: saved } });
    expect(await screen.findByTestId('cloud-model-qwen3-coder')).toHaveAttribute(
      'aria-selected',
      'true'
    );
    expect(mockLive).toHaveBeenCalledWith('custom_team_gateway');

    await userEvent.click(screen.getByRole('button', { name: 'Back' }));
    expect(screen.getByLabelText('Name')).toHaveValue('Team Gateway');
    expect(screen.getByLabelText('Base URL')).toHaveValue('https://gw.corp.example/v1');
    expect(screen.getByLabelText('Header name')).toHaveValue('X-Team');
    expect(screen.getByText('saved — leave blank to keep')).toBeInTheDocument();
    await userEvent.clear(screen.getByLabelText('Base URL'));
    await userEvent.type(screen.getByLabelText('Base URL'), 'https://gw2.corp.example/v1');
    await userEvent.click(screen.getByTestId('compatible-endpoint-test'));
    await waitFor(() =>
      expect(mockUpdate).toHaveBeenCalledWith(
        'custom_team_gateway',
        expect.objectContaining({
          api_url: 'https://gw2.corp.example/v1',
          api_key: '',
          requires_auth: true,
          models: ['qwen3-coder', 'llama-4-scout'],
          headers: { 'X-Team': 'platform' },
        })
      )
    );
    expect(mockCreate).not.toHaveBeenCalled();
  });

  it('"needs no key" switches auth off for a saved endpoint; Remove asks first, then deletes', async () => {
    mockLive.mockResolvedValue(['qwen3-coder']);
    const { onClose } = render({ endpoint: { provider: row, config: saved } });
    await screen.findByTestId('cloud-model-qwen3-coder');
    await userEvent.click(screen.getByRole('button', { name: 'Back' }));
    await userEvent.click(screen.getByTestId('compatible-endpoint-no-key'));
    expect(screen.getByLabelText('API key')).toBeDisabled();
    await userEvent.click(screen.getByTestId('compatible-endpoint-test'));
    await waitFor(() =>
      expect(mockUpdate).toHaveBeenCalledWith(
        'custom_team_gateway',
        expect.objectContaining({ requires_auth: false })
      )
    );

    await userEvent.click(await screen.findByRole('button', { name: 'Remove' }));
    expect(mockDelete).not.toHaveBeenCalled();
    await userEvent.click(screen.getByTestId('compatible-endpoint-remove'));
    await waitFor(() => expect(mockDelete).toHaveBeenCalledWith('custom_team_gateway'));
    expect(onClose).toHaveBeenCalled();
  });
});
