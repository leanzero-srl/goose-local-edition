import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { cleanup, render as rtlRender, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { IntlTestWrapper } from '../../i18n/test-utils';
import CloudProviderSetupDialog from './CloudProviderSetupDialog';
import type { ProviderDetails } from '../../types/providers';

const mockSave = vi.fn();
const mockLive = vi.fn();
const mockSaveDefault = vi.fn();
const mockRead = vi.fn();
const mockDelete = vi.fn();
vi.mock('../../acp/providers', () => ({
  acpSaveProviderConfig: (...a: unknown[]) => mockSave(...a),
  acpListProviderLiveModels: (...a: unknown[]) => mockLive(...a),
  acpSaveProviderDefaultModel: (...a: unknown[]) => mockSaveDefault(...a),
  acpReadProviderConfig: (...a: unknown[]) => mockRead(...a),
  acpDeleteProviderConfig: (...a: unknown[]) => mockDelete(...a),
}));

function provider(configured: boolean, defaultModel: string | null = null): ProviderDetails {
  return {
    name: 'openai',
    is_configured: configured,
    default_model: defaultModel,
    provider_type: 'Native',
    metadata: {
      name: 'openai',
      display_name: 'OpenAI',
      description: '',
      default_model: 'gpt-4o',
      model_doc_link: '',
      model_selection_hint: null,
      config_keys: [
        {
          name: 'OPENAI_API_KEY',
          required: true,
          secret: true,
          default: null,
          oauth_flow: false,
          device_code_flow: false,
          primary: true,
        },
      ],
      known_models: [{ name: 'gpt-known', context_limit: 0 }],
      setup_steps: [],
    },
  } as unknown as ProviderDetails;
}

const render = (p: ProviderDetails, onSaved = vi.fn().mockResolvedValue(undefined)) =>
  rtlRender(<CloudProviderSetupDialog provider={p} onClose={vi.fn()} onSaved={onSaved} />, {
    wrapper: IntlTestWrapper,
  });

beforeEach(() => {
  vi.clearAllMocks();
  mockRead.mockResolvedValue([]);
});
afterEach(() => cleanup());

describe('CloudProviderSetupDialog', () => {
  it('key → the provider’s own model list → a chosen default, saved through the engine', async () => {
    mockSave.mockResolvedValue(undefined);
    mockLive.mockResolvedValue(['gpt-5', 'gpt-5-mini', 'o4']);
    mockSaveDefault.mockResolvedValue(undefined);
    const onSaved = vi.fn().mockResolvedValue(undefined);
    render(provider(false), onSaved);

    expect(screen.queryByText(/Connection test model/)).not.toBeInTheDocument();
    await userEvent.click(screen.getByTestId('cloud-provider-connect'));
    expect(screen.getByText('OPENAI_API_KEY is required')).toBeInTheDocument();
    expect(mockSave).not.toHaveBeenCalled();

    await userEvent.type(screen.getByLabelText('OPENAI_API_KEY'), 'sk-test');
    await userEvent.click(screen.getByTestId('cloud-provider-connect'));
    await waitFor(() => {
      expect(mockSave).toHaveBeenCalledWith('openai', [
        { key: 'OPENAI_API_KEY', value: 'sk-test' },
      ]);
    });
    await waitFor(() => {
      expect(screen.getByTestId('cloud-model-gpt-5-mini')).toBeInTheDocument();
    });
    expect(mockLive).toHaveBeenCalledWith('openai');
    expect(screen.getByText('3 models your key can run')).toBeInTheDocument();

    await userEvent.type(screen.getByLabelText('Filter models'), 'mini');
    expect(screen.queryByTestId('cloud-model-o4')).not.toBeInTheDocument();
    await userEvent.click(screen.getByTestId('cloud-model-gpt-5-mini'));
    await userEvent.click(screen.getByTestId('cloud-provider-save-default'));
    await waitFor(() => {
      expect(mockSaveDefault).toHaveBeenCalledWith('openai', 'gpt-5-mini');
    });
    expect(onSaved).toHaveBeenCalledTimes(2);
  });

  it('a rejected key shows the engine’s words verbatim and never reaches the model step', async () => {
    mockSave.mockRejectedValue(
      new Error('Connection check failed; previous settings retained. OpenAI 401')
    );
    render(provider(false));
    await userEvent.type(screen.getByLabelText('OPENAI_API_KEY'), 'bad');
    await userEvent.click(screen.getByTestId('cloud-provider-connect'));
    await waitFor(() => {
      expect(screen.getByText(/OpenAI 401/)).toBeInTheDocument();
    });
    expect(mockLive).not.toHaveBeenCalled();
    expect(screen.queryByTestId('cloud-provider-save-default')).not.toBeInTheDocument();
  });

  it('a configured provider opens on its models with the current default first and marked', async () => {
    mockLive.mockResolvedValue(['a-model', 'gpt-5', 'z-model']);
    render(provider(true, 'gpt-5'));
    await waitFor(() => {
      expect(screen.getByTestId('cloud-model-gpt-5')).toBeInTheDocument();
    });
    const options = screen.getAllByRole('option');
    expect(options[0]).toHaveTextContent('gpt-5');
    expect(options[0]).toHaveTextContent('current default');
    expect(options[0]).toHaveAttribute('aria-selected', 'true');
    expect(mockSave).not.toHaveBeenCalled();
    expect(screen.getByRole('button', { name: 'Replace key' })).toBeInTheDocument();
  });

  it('a provider with no listing offers goose’s known ids and SAYS so', async () => {
    mockLive.mockResolvedValue([]);
    render(provider(true, null));
    await waitFor(() => {
      expect(screen.getByTestId('cloud-model-gpt-known')).toBeInTheDocument();
    });
    expect(screen.getByText(/OpenAI has no model listing/)).toBeInTheDocument();
  });

  it('a failed listing is an error with a retry, and a typed model id still saves', async () => {
    mockLive
      .mockRejectedValueOnce(new Error('could not list models: 429'))
      .mockResolvedValueOnce(['gpt-5']);
    mockSaveDefault.mockResolvedValue(undefined);
    render(provider(true, null));
    await waitFor(() => {
      expect(screen.getByText(/429/)).toBeInTheDocument();
    });
    await userEvent.type(screen.getByLabelText('Or type a model id'), 'gpt-5-custom');
    await userEvent.click(screen.getByTestId('cloud-provider-save-default'));
    await waitFor(() => {
      expect(mockSaveDefault).toHaveBeenCalledWith('openai', 'gpt-5-custom');
    });
  });

  it('remove asks in the dialog itself, then deletes through the engine', async () => {
    mockLive.mockResolvedValue(['gpt-5']);
    mockDelete.mockResolvedValue(undefined);
    render(provider(true, 'gpt-5'));
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Remove' })).toBeInTheDocument();
    });
    await userEvent.click(screen.getByRole('button', { name: 'Remove' }));
    expect(screen.getByText(/Remove the OpenAI key/)).toBeInTheDocument();
    expect(mockDelete).not.toHaveBeenCalled();
    await userEvent.click(screen.getByTestId('cloud-provider-remove'));
    await waitFor(() => {
      expect(mockDelete).toHaveBeenCalledWith('openai');
    });
  });
});
