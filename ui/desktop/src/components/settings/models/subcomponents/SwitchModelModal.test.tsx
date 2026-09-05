import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { cleanup, render as rtlRender, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { SwitchModelModal } from './SwitchModelModal';
import { IntlTestWrapper } from '../../../../i18n/test-utils';
import type { ProviderDetails } from '../../../../types/providers';

const render = (ui: React.ReactElement) => rtlRender(ui, { wrapper: IntlTestWrapper });

const mockChangeModel = vi.fn();
let mockCurrentProvider = 'anthropic';
let mockCurrentModel = 'claude-sonnet-4';
vi.mock('../../../ModelAndProviderContext', () => ({
  useModelAndProvider: () => ({
    changeModel: mockChangeModel,
    currentModel: mockCurrentModel,
    currentProvider: mockCurrentProvider,
  }),
}));

let mockIsLocal = false;
vi.mock('../../../../contexts/EditionContext', () => ({
  useEdition: () => ({
    edition: mockIsLocal ? 'local' : 'standard',
    isLocal: mockIsLocal,
    setEdition: vi.fn(),
  }),
}));

const mockListProviderDetails = vi.fn();
vi.mock('../../../../acp/providers', () => ({
  acpListProviderDetails: (...args: unknown[]) => mockListProviderDetails(...args),
  acpReadThinkingEffort: vi.fn().mockResolvedValue(null),
  acpSaveThinkingEffort: vi.fn().mockResolvedValue(undefined),
}));

vi.mock('../modelInterface', () => ({
  getProviderMetadata: vi.fn(async (name: string) => ({ display_name: `Display ${name}` })),
  fetchModelsForProviders: vi.fn().mockResolvedValue([]),
  fetchModelReasoning: vi.fn().mockResolvedValue(null),
}));

vi.mock('../predefinedModelsUtils', () => ({
  shouldShowPredefinedModels: () => false,
  getPredefinedModelsFromEnv: () => [],
}));

vi.mock('../../../../utils/analytics', () => ({
  trackModelChanged: vi.fn(),
}));

function providerOf(name: string, displayName: string, configured = true): ProviderDetails {
  return {
    name,
    is_configured: configured,
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

// The registry as this machine sees it: upstream clouds, local backends, the swarm's four cloud
// families (one of them not yet configured) and the Goose Swarm provider itself.
const ALL_PROVIDERS: ProviderDetails[] = [
  providerOf('anthropic', 'Anthropic'),
  providerOf('openai', 'OpenAI'),
  providerOf('ollama', 'Ollama'),
  providerOf('lmstudio', 'LM Studio'),
  providerOf('local', 'Local'),
  providerOf('omlx', 'oMLX'),
  providerOf('google', 'Google Gemini'),
  providerOf('zai', 'Z.ai'),
  providerOf('aws_bedrock', 'Amazon Bedrock'),
  providerOf('custom_deepseek', 'DeepSeek', false),
  providerOf('swarm', 'Goose Swarm'),
];

beforeEach(() => {
  vi.clearAllMocks();
  mockIsLocal = false;
  mockCurrentProvider = 'anthropic';
  mockCurrentModel = 'claude-sonnet-4';
  mockListProviderDetails.mockResolvedValue(ALL_PROVIDERS);
  mockChangeModel.mockResolvedValue(true);
});

afterEach(() => {
  cleanup();
});

async function openProviderMenu() {
  await waitFor(() => {
    expect(screen.getAllByRole('combobox').length).toBeGreaterThan(0);
  });
  await userEvent.click(screen.getAllByRole('combobox')[0]);
  await waitFor(() => {
    expect(screen.getAllByRole('option').length).toBeGreaterThan(0);
  });
}

const rowIds = () =>
  screen
    .getAllByTestId(/^provider-row-/)
    .map((el) => el.getAttribute('data-testid')!.replace('provider-row-', ''));

describe('SwitchModelModal — Goose Swarm (local) edition: only the defined providers', () => {
  it('lists the two Goose Swarm rows FIRST, then the configured swarm cloud families — nothing else', async () => {
    mockIsLocal = true;
    render(
      <SwitchModelModal sessionId={null} onClose={vi.fn()} setView={vi.fn()} initialProvider={null} />
    );
    await openProviderMenu();
    expect(rowIds()).toEqual(['swarm', 'swarm:swarm-build', 'google', 'zai', 'aws_bedrock']);
    expect(screen.getByText('Goose Swarm · Build')).toBeInTheDocument();
    expect(
      screen.getByText('chat — each turn goes to an idle node of your pool, or waits for one')
    ).toBeInTheDocument();
    expect(screen.getByText('plan and fan out a build across the pool')).toBeInTheDocument();
    // the unconfigured family is not offered yet; no upstream cloud, no local backend, no escape
    for (const absent of [
      'DeepSeek',
      'Anthropic',
      'OpenAI',
      'Ollama',
      'LM Studio',
      'Local',
      'oMLX',
      'Leanzero MLX',
      'Use other provider',
    ]) {
      expect(screen.queryByText(absent)).not.toBeInTheDocument();
    }
  });

  it('the Build row submits provider swarm + model swarm-build through changeModel', async () => {
    mockIsLocal = true;
    const onModelSelected = vi.fn();
    render(
      <SwitchModelModal
        sessionId="session-7"
        onClose={vi.fn()}
        setView={vi.fn()}
        initialProvider={null}
        onModelSelected={onModelSelected}
      />
    );
    await openProviderMenu();
    await userEvent.click(screen.getByTestId('provider-row-swarm:swarm-build'));
    await waitFor(() => {
      expect(screen.getByTestId('swarm-row-panel')).toHaveTextContent('swarm-build');
    });
    expect(screen.getByTestId('swarm-row-panel')).toHaveTextContent(
      'plan and fan out a build across the pool'
    );
    await userEvent.click(screen.getByRole('button', { name: 'Select model' }));
    await waitFor(() => expect(mockChangeModel).toHaveBeenCalledTimes(1));
    expect(mockChangeModel).toHaveBeenCalledWith(
      'session-7',
      expect.objectContaining({ name: 'swarm-build', provider: 'swarm' })
    );
    expect(onModelSelected).toHaveBeenCalledWith('swarm-build', 'swarm');
  });

  it('the chat row submits provider swarm + model swarm', async () => {
    mockIsLocal = true;
    render(
      <SwitchModelModal sessionId={null} onClose={vi.fn()} setView={vi.fn()} initialProvider={null} />
    );
    await openProviderMenu();
    await userEvent.click(screen.getByTestId('provider-row-swarm'));
    await waitFor(() => {
      expect(screen.getByTestId('swarm-row-panel')).toHaveTextContent(
        'chat — each turn goes to an idle node of your pool, or waits for one'
      );
    });
    await userEvent.click(screen.getByRole('button', { name: 'Select model' }));
    await waitFor(() => expect(mockChangeModel).toHaveBeenCalledTimes(1));
    expect(mockChangeModel).toHaveBeenCalledWith(
      null,
      expect.objectContaining({ name: 'swarm', provider: 'swarm' })
    );
  });

  it('an omlx session is never preselected: nothing to submit until an allowed row is picked', async () => {
    mockIsLocal = true;
    render(
      <SwitchModelModal
        sessionId="session-omlx"
        onClose={vi.fn()}
        setView={vi.fn()}
        sessionProvider="omlx"
        sessionModel="qwen3-served"
      />
    );
    await waitFor(() => {
      expect(screen.getAllByRole('combobox').length).toBeGreaterThan(0);
    });
    expect(screen.queryByText('oMLX')).not.toBeInTheDocument();
    expect(screen.queryByText('qwen3-served')).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: 'Select model' }));
    expect(mockChangeModel).not.toHaveBeenCalled();
    expect(screen.getByText('Please select a provider')).toBeInTheDocument();
  });

  it('a session already on Goose Swarm opens on its row', async () => {
    mockIsLocal = true;
    mockCurrentProvider = 'swarm';
    mockCurrentModel = 'swarm';
    render(<SwitchModelModal sessionId={null} onClose={vi.fn()} setView={vi.fn()} />);
    await waitFor(() => {
      expect(screen.getByTestId('swarm-row-panel')).toHaveTextContent('swarm');
    });
    expect(screen.getByTestId('provider-row-swarm')).toBeInTheDocument();
  });
});

describe('SwitchModelModal — standard edition is untouched', () => {
  it('lists every configured provider plus "Use other provider"; no Goose Swarm helper rows', async () => {
    mockIsLocal = false;
    render(
      <SwitchModelModal sessionId={null} onClose={vi.fn()} setView={vi.fn()} initialProvider={null} />
    );
    await openProviderMenu();
    expect(screen.getByText('Ollama')).toBeInTheDocument();
    expect(screen.getByText('LM Studio')).toBeInTheDocument();
    expect(screen.getByText('oMLX')).toBeInTheDocument();
    // Anthropic is the current provider: rendered as the control value AND as a menu row
    expect(screen.getAllByText('Anthropic').length).toBeGreaterThan(0);
    expect(screen.getByText('Use other provider')).toBeInTheDocument();
    expect(screen.queryByText('Goose Swarm · Build')).not.toBeInTheDocument();
    expect(rowIds()).not.toContain('swarm:swarm-build');
  });

  it('"Use other provider" still routes to ConfigureProviders', async () => {
    mockIsLocal = false;
    const setView = vi.fn();
    const onClose = vi.fn();
    render(
      <SwitchModelModal sessionId={null} onClose={onClose} setView={setView} initialProvider={null} />
    );
    await openProviderMenu();
    await userEvent.click(screen.getByTestId('provider-row-configure_providers'));
    expect(setView).toHaveBeenCalledWith('ConfigureProviders');
    expect(onClose).toHaveBeenCalled();
  });
});
