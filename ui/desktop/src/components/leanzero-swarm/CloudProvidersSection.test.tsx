import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { cleanup, render as rtlRender, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { IntlTestWrapper } from '../../i18n/test-utils';
import CloudProvidersSection from './CloudProvidersSection';
import type { ProviderDetails } from '../../types/providers';

const dialogSpy = vi.fn();
vi.mock('./CloudProviderSetupDialog', () => ({
  default: (props: { provider: ProviderDetails }) => {
    dialogSpy(props);
    return <div data-testid="setup-dialog">{props.provider.name}</div>;
  },
}));

const endpointDialogSpy = vi.fn();
vi.mock('./CompatibleEndpointDialog', () => ({
  default: (props: { endpoint?: { provider: ProviderDetails } }) => {
    endpointDialogSpy(props);
    return (
      <div data-testid="endpoint-dialog">{props.endpoint?.provider.name ?? 'new endpoint'}</div>
    );
  },
}));

const mockList = vi.fn();
const mockRecheck = vi.fn();
const mockReadConfig = vi.fn();
const mockGetCustom = vi.fn();
vi.mock('../../acp/providers', () => ({
  acpListProviderDetails: (...a: unknown[]) => mockList(...a),
  acpRecheckProviderConnections: (...a: unknown[]) => mockRecheck(...a),
  acpReadProviderConfig: (...a: unknown[]) => mockReadConfig(...a),
  acpGetCustomProvider: (...a: unknown[]) => mockGetCustom(...a),
}));

function provider(
  name: string,
  configured: boolean,
  extra: Partial<ProviderDetails> = {}
): ProviderDetails {
  return {
    name,
    is_configured: configured,
    provider_type: 'Native',
    metadata: {
      name,
      display_name: name,
      description: '',
      default_model: '',
      model_doc_link: '',
      model_selection_hint: null,
      config_keys: [],
      known_models: [],
      setup_steps: [],
    },
    ...extra,
  } as unknown as ProviderDetails;
}

const render = () => rtlRender(<CloudProvidersSection />, { wrapper: IntlTestWrapper });

/** A cloud family whose saved key passed this session's check — what the engine reports for it. */
const connected = (name: string, extra: Partial<ProviderDetails> = {}) =>
  provider(name, true, { credentials_saved: true, connection_checked: true, ...extra });

beforeEach(() => {
  vi.clearAllMocks();
  mockReadConfig.mockResolvedValue([]);
});
afterEach(() => cleanup());

describe('CloudProvidersSection', () => {
  it('lists every registry cloud provider as a row, hides local engines, shows the default model', async () => {
    mockList.mockResolvedValue([
      connected('anthropic', { default_model: 'claude-sonnet-5' }),
      provider('openai', false),
      provider('lmstudio', true),
      provider('ollama', false),
      provider('omlx', true),
      provider('swarm', true),
      connected('aws_bedrock', { default_model: null }),
      provider('zai', false),
      connected('google', { default_model: 'gemini-3-pro' }),
      provider('custom_deepseek', false),
    ]);
    render();
    await waitFor(() => {
      expect(screen.getByTestId('cloud-provider-anthropic')).toBeInTheDocument();
    });
    for (const allowed of [
      'anthropic',
      'openai',
      'aws_bedrock',
      'zai',
      'google',
      'custom_deepseek',
    ]) {
      expect(screen.getByTestId(`cloud-provider-${allowed}`)).toBeInTheDocument();
    }
    for (const hidden of ['lmstudio', 'ollama', 'omlx', 'swarm']) {
      expect(screen.queryByTestId(`cloud-provider-${hidden}`)).not.toBeInTheDocument();
    }
    const configured = screen.getByRole('region', { name: 'Configured' });
    expect(configured).toHaveTextContent('anthropic');
    expect(configured).toHaveTextContent('claude-sonnet-5');
    expect(configured).not.toHaveTextContent('openai');
    // A configured provider with no chosen default says so instead of showing a blank.
    expect(
      within(screen.getByTestId('cloud-provider-aws_bedrock')).getByText('no default model yet')
    ).toBeInTheDocument();
    expect(screen.getByRole('region', { name: 'Available to set up' })).toHaveTextContent('openai');
    expect(screen.getByText('3 of 6 configured')).toBeInTheDocument();
    expect(screen.getAllByRole('button', { name: /Set up/ })).toHaveLength(3);
    expect(screen.getAllByRole('button', { name: /Change/ })).toHaveLength(3);
    expect(within(configured).getAllByText('Connected')).toHaveLength(3);
  });

  it('the header count is exactly the Configured list — a failed or unchecked key is counted where it is shown', async () => {
    // The reported bug: OpenAI sat under CONFIGURED (its saved key answered 401) while the chip said
    // "0 of 14 configured", because the chip counted is_configured and the list did not.
    mockList.mockResolvedValue([
      provider('openai', false, {
        credentials_saved: true,
        connection_checked: true,
        connection_error:
          'OpenAI could not run model gpt-5: 401 Incorrect API key provided: lm-studio',
      }),
      provider('mistral', false, { credentials_saved: true }),
      provider('google', false),
      provider('xai', false),
    ]);
    render();
    const configured = await screen.findByRole('region', { name: 'Configured' });
    const rows = within(configured).getAllByTestId(/^cloud-provider-(?!open-|warning-)/);
    expect(rows.map((r) => r.getAttribute('data-testid'))).toEqual([
      'cloud-provider-mistral',
      'cloud-provider-openai',
    ]);
    expect(screen.getByText('2 of 4 configured')).toBeInTheDocument();
    expect(within(configured).getByText('Not checked yet')).toBeInTheDocument();
    expect(within(configured).getByRole('alert')).toHaveTextContent('lm-studio');
  });

  it('flags the official OpenAI tile loudly when saved settings send it to another server', async () => {
    mockList.mockResolvedValue([connected('openai', { default_model: 'gpt-5' })]);
    mockReadConfig.mockResolvedValue([
      { key: 'OPENAI_API_KEY', value: 'sk-p...wxyz' },
      { key: 'OPENAI_HOST', value: 'http://192.168.8.220:1234' },
      { key: 'OPENAI_BASE_PATH', value: 'v1/chat/completions' },
    ]);
    render();
    const warning = await screen.findByTestId('cloud-provider-warning-openai');
    expect(warning).toHaveTextContent('Not the official API');
    expect(warning).toHaveTextContent('OPENAI_HOST=http://192.168.8.220:1234');
    expect(warning).not.toHaveTextContent('OPENAI_BASE_PATH');
    expect(mockReadConfig).toHaveBeenCalledWith('openai');
    await userEvent.click(screen.getByTestId('cloud-provider-open-openai'));
    expect(dialogSpy).toHaveBeenLastCalledWith(
      expect.objectContaining({
        endpointOverrides: [{ key: 'OPENAI_HOST', value: 'http://192.168.8.220:1234' }],
      })
    );
  });

  it('an OpenAI tile pointing at api.openai.com carries no warning; an unreadable endpoint says so', async () => {
    mockList.mockResolvedValue([connected('openai')]);
    mockReadConfig.mockResolvedValueOnce([{ key: 'OPENAI_HOST', value: 'https://api.openai.com' }]);
    render();
    await screen.findByTestId('cloud-provider-openai');
    expect(screen.queryByTestId('cloud-provider-warning-openai')).not.toBeInTheDocument();
    cleanup();
    mockReadConfig.mockRejectedValueOnce(new Error('config.yaml is not valid YAML'));
    render();
    expect(await screen.findByTestId('cloud-provider-warning-openai')).toHaveTextContent(
      'config.yaml is not valid YAML'
    );
  });

  it('each saved OpenAI-compatible endpoint is its own Configured row, counted, chat-only, and opens its own editor', async () => {
    mockList.mockResolvedValue([
      connected('anthropic'),
      provider('custom_team_gateway', true, {
        provider_type: 'Custom',
        default_model: 'qwen3-coder',
        metadata: { ...provider('x', true).metadata, display_name: 'Team Gateway' },
      }),
      provider('custom_desk_vllm', true, {
        provider_type: 'Custom',
        connection_checked: true,
        connection_error: 'Desk vLLM could not run model llama: connection refused',
        metadata: { ...provider('x', true).metadata, display_name: 'Desk vLLM' },
      }),
      provider('custom_deepseek', false, { provider_type: 'Declarative' }),
    ]);
    mockGetCustom.mockImplementation(async (id: string) => ({
      provider: {
        providerId: id,
        engine: 'openai_compatible',
        displayName: id,
        apiUrl:
          id === 'custom_team_gateway' ? 'https://gw.corp.example/v1' : 'http://10.0.0.5:8000/v1',
        models: [],
        requiresAuth: false,
        apiKeySet: false,
        preservesThinking: true,
      },
      editable: true,
    }));
    render();
    const configured = await screen.findByRole('region', { name: 'Configured' });
    const gateway = await within(configured).findByTestId('cloud-provider-custom_team_gateway');
    await waitFor(() => expect(gateway).toHaveTextContent('https://gw.corp.example/v1'));
    expect(gateway).toHaveTextContent('Team Gateway');
    expect(gateway).toHaveTextContent('qwen3-coder');
    expect(gateway).toHaveTextContent('Chat only');
    expect(gateway).toHaveTextContent('Not checked yet');
    expect(within(configured).getByTestId('cloud-provider-custom_desk_vllm')).toHaveTextContent(
      'connection refused'
    );
    // DeepSeek is a bundled declarative provider with a custom_ id — a cloud family, not an endpoint.
    expect(screen.getByRole('region', { name: 'Available to set up' })).toHaveTextContent(
      'custom_deepseek'
    );
    expect(screen.getByText('3 of 4 configured')).toBeInTheDocument();

    await userEvent.click(screen.getByTestId('cloud-provider-open-custom_team_gateway'));
    expect(screen.getByTestId('endpoint-dialog')).toHaveTextContent('custom_team_gateway');
    expect(endpointDialogSpy).toHaveBeenLastCalledWith(
      expect.objectContaining({
        endpoint: expect.objectContaining({
          config: expect.objectContaining({ apiUrl: 'https://gw.corp.example/v1' }),
        }),
      })
    );

    await userEvent.click(screen.getByTestId('cloud-provider-open-custom_deepseek'));
    await userEvent.click(screen.getByTestId('cloud-provider-add-compatible-open'));
    expect(screen.getByTestId('endpoint-dialog')).toHaveTextContent('new endpoint');

    await userEvent.click(screen.getByRole('button', { name: /Recheck connections/ }));
    expect(mockRecheck).toHaveBeenCalledWith(['custom_desk_vllm', 'custom_team_gateway']);
  });

  it('a provider whose connection check failed stays in Configured with the error verbatim', async () => {
    mockList.mockResolvedValue([
      provider('openai', false, {
        connection_checked: true,
        connection_error: 'OpenAI rejected the key: 401 Unauthorized',
      }),
    ]);
    render();
    await waitFor(() => {
      expect(screen.getByRole('alert')).toHaveTextContent('401 Unauthorized');
    });
    expect(screen.getByRole('region', { name: 'Configured' })).toHaveTextContent('openai');
    expect(screen.getByText('Check failed')).toBeInTheDocument();
  });

  it('the action opens the setup dialog for that provider', async () => {
    mockList.mockResolvedValue([provider('openai', false), provider('google', true)]);
    render();
    await waitFor(() => {
      expect(screen.getByTestId('cloud-provider-open-openai')).toBeInTheDocument();
    });
    await userEvent.click(screen.getByTestId('cloud-provider-open-openai'));
    expect(screen.getByTestId('setup-dialog')).toHaveTextContent('openai');
    expect(dialogSpy).toHaveBeenLastCalledWith(
      expect.objectContaining({ provider: expect.objectContaining({ name: 'openai' }) })
    );
  });

  it('a failed provider list renders the failure twin with a working Retry — never a clean empty list', async () => {
    mockList.mockRejectedValueOnce(new Error('agent unreachable'));
    mockList.mockResolvedValueOnce([provider('google', true)]);
    render();
    await waitFor(() => {
      expect(screen.getByText('agent unreachable')).toBeInTheDocument();
    });
    expect(screen.queryByTestId('cloud-provider-google')).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: 'Retry' }));
    await waitFor(() => {
      expect(screen.getByTestId('cloud-provider-google')).toBeInTheDocument();
    });
  });
});
