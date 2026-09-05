import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { cleanup, render as rtlRender, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import ProviderSelector from './ProviderSelector';
import { IntlTestWrapper } from '../../i18n/test-utils';
import type { ProviderDetails } from '../../types/providers';

const render = (ui: React.ReactElement) => rtlRender(ui, { wrapper: IntlTestWrapper });

let mockIsLocal = false;
vi.mock('../../contexts/EditionContext', () => ({
  useEdition: () => ({
    edition: mockIsLocal ? 'local' : 'standard',
    isLocal: mockIsLocal,
    setEdition: vi.fn(),
  }),
}));

let mockLocalInference = true;
vi.mock('../../contexts/FeaturesContext', () => ({
  useFeatures: () => ({
    localInference: mockLocalInference,
    mlxEngine: false,
    leanzeroLink: false,
    isLoading: false,
  }),
}));

const mockList = vi.fn();
vi.mock('../../acp/providers', () => ({
  acpListProviderDetails: (...a: unknown[]) => mockList(...a),
  acpCreateCustomProviderFromRequest: vi.fn(),
}));

vi.mock('./LocalModelPicker', () => ({
  default: () => <div data-testid="local-model-picker" />,
}));
vi.mock('./ProviderConfigForm', () => ({
  default: ({ provider }: { provider: ProviderDetails }) => (
    <div data-testid={`config-form-${provider.name}`} />
  ),
}));
vi.mock('../settings/providers/modal/subcomponents/forms/CustomProviderForm', () => ({
  default: () => <div data-testid="custom-provider-form" />,
}));

function providerOf(name: string, displayName: string): ProviderDetails {
  return {
    name,
    is_configured: false,
    provider_type: 'Builtin',
    metadata: {
      config_keys: [],
      default_model: '',
      description: '',
      display_name: displayName,
      known_models: [],
      model_doc_link: '',
      name,
    },
  };
}

const ALL_PROVIDERS = [
  providerOf('anthropic', 'Anthropic'),
  providerOf('openai', 'OpenAI'),
  providerOf('ollama', 'Ollama'),
  providerOf('omlx', 'oMLX'),
  providerOf('lmstudio', 'LM Studio'),
  providerOf('swarm', 'Goose Swarm'),
  providerOf('google', 'Google Gemini'),
  providerOf('zai', 'Z.ai'),
  providerOf('aws_bedrock', 'Amazon Bedrock'),
  providerOf('custom_deepseek', 'DeepSeek'),
];

beforeEach(() => {
  vi.clearAllMocks();
  mockIsLocal = false;
  mockLocalInference = true;
  mockList.mockResolvedValue(ALL_PROVIDERS);
});
afterEach(() => cleanup());

async function openCloudSelect() {
  await userEvent.click(screen.getByText('Connect to a Provider'));
  await waitFor(() => expect(screen.getByRole('combobox')).toBeInTheDocument());
  await userEvent.click(screen.getByRole('combobox'));
  await waitFor(() => expect(screen.getAllByRole('option').length).toBeGreaterThan(0));
}

describe('ProviderSelector (onboarding) — Goose Swarm (local) edition', () => {
  it('offers Goose Swarm straight through: one click configures swarm/swarm with no credentials', async () => {
    mockIsLocal = true;
    const onConfigured = vi.fn();
    const onFirstSelection = vi.fn();
    render(<ProviderSelector onConfigured={onConfigured} onFirstSelection={onFirstSelection} />);
    const card = screen.getByTestId('onboarding-use-swarm');
    expect(card).toHaveTextContent('Use Goose Swarm');
    expect(card).toHaveTextContent('No API key needed');
    await userEvent.click(card);
    expect(onFirstSelection).toHaveBeenCalled();
    expect(onConfigured).toHaveBeenCalledWith('swarm', 'swarm');
  });

  it('hides the local-model download path even when the capability is on', async () => {
    mockIsLocal = true;
    render(<ProviderSelector onConfigured={vi.fn()} />);
    expect(screen.queryByText('Use a Local Model')).not.toBeInTheDocument();
    expect(screen.getByText('Connect to a Provider')).toBeInTheDocument();
    expect(
      screen.getByText('Connect Amazon Bedrock, Z.ai, Google Gemini, DeepSeek')
    ).toBeInTheDocument();
  });

  it('the cloud select lists ONLY the four swarm families and has no custom-provider affordance', async () => {
    mockIsLocal = true;
    render(<ProviderSelector onConfigured={vi.fn()} />);
    await waitFor(() => expect(mockList).toHaveBeenCalled());
    await openCloudSelect();
    const names = screen.getAllByRole('option').map((o) => o.textContent);
    expect(names).toEqual(['Amazon Bedrock', 'DeepSeek', 'Google Gemini', 'Z.ai']);
    expect(screen.queryByText('Add a custom provider')).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole('option', { name: 'Google Gemini' }));
    expect(await screen.findByTestId('config-form-google')).toBeInTheDocument();
  });
});

describe('ProviderSelector (onboarding) — standard edition is unfiltered', () => {
  it('no swarm card, the local-model card stays, and every provider is listed with the custom button', async () => {
    mockIsLocal = false;
    render(<ProviderSelector onConfigured={vi.fn()} />);
    await waitFor(() => expect(mockList).toHaveBeenCalled());
    expect(screen.queryByTestId('onboarding-use-swarm')).not.toBeInTheDocument();
    expect(screen.getByText('Use a Local Model')).toBeInTheDocument();
    await openCloudSelect();
    const names = screen.getAllByRole('option').map((o) => o.textContent);
    expect(names).toHaveLength(ALL_PROVIDERS.length);
    expect(names).toContain('Anthropic');
    expect(names).toContain('oMLX');
    expect(screen.getByText('Add a custom provider')).toBeInTheDocument();
  });
});
