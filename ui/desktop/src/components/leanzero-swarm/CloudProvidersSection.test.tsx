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

const mockList = vi.fn();
const mockRecheck = vi.fn();
vi.mock('../../acp/providers', () => ({
  acpListProviderDetails: (...a: unknown[]) => mockList(...a),
  acpRecheckProviderConnections: (...a: unknown[]) => mockRecheck(...a),
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

beforeEach(() => {
  vi.clearAllMocks();
});
afterEach(() => cleanup());

describe('CloudProvidersSection', () => {
  it('lists every registry cloud provider as a row, hides local engines, shows the default model', async () => {
    mockList.mockResolvedValue([
      provider('anthropic', true, { default_model: 'claude-sonnet-5' }),
      provider('openai', false),
      provider('lmstudio', true),
      provider('ollama', false),
      provider('omlx', true),
      provider('swarm', true),
      provider('aws_bedrock', true, { default_model: null }),
      provider('zai', false),
      provider('google', true, { default_model: 'gemini-3-pro' }),
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
