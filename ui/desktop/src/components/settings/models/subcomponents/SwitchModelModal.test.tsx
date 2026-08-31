import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { cleanup, render as rtlRender, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { SwitchModelModal } from './SwitchModelModal';
import { IntlTestWrapper } from '../../../../i18n/test-utils';
import type { ProviderDetails } from '../../../../types/providers';
import type { MlxEngineStatus } from '../../../../acp/mlx-engine';

const render = (ui: React.ReactElement) => rtlRender(ui, { wrapper: IntlTestWrapper });

const mockChangeModel = vi.fn();
vi.mock('../../../ModelAndProviderContext', () => ({
  useModelAndProvider: () => ({
    changeModel: mockChangeModel,
    currentModel: 'claude-sonnet-4',
    currentProvider: 'anthropic',
  }),
}));

let mockMlxCapability = false;
vi.mock('../../../../contexts/FeaturesContext', () => ({
  useFeatures: () => ({
    localInference: false,
    mlxEngine: mockMlxCapability,
    isLoading: false,
  }),
}));

let mockMlxStatus: MlxEngineStatus | null = null;
let mockMlxStatusError: string | null = null;
vi.mock('../../../mlx/useMlxEngineStatus', () => ({
  useMlxEngineStatusPoll: () => ({ status: mockMlxStatus, error: mockMlxStatusError }),
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

function providerOf(name: string, displayName: string): ProviderDetails {
  return {
    name,
    is_configured: true,
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

const ALL_PROVIDERS: ProviderDetails[] = [
  providerOf('anthropic', 'Anthropic'),
  providerOf('openai', 'OpenAI'),
  providerOf('ollama', 'Ollama'),
  providerOf('lmstudio', 'LM Studio'),
  providerOf('local', 'Local'),
  providerOf('omlx', 'oMLX'),
];

function mlxStatusOf(overrides: Partial<MlxEngineStatus>): MlxEngineStatus {
  return {
    state: 'stopped',
    restartRequired: false,
    availableMemoryGb: 40,
    totalMemoryGb: 64,
    ...overrides,
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  mockMlxCapability = false;
  mockMlxStatus = null;
  mockMlxStatusError = null;
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
}

describe('SwitchModelModal provider list — Leanzero edition policy', () => {
  it('capability ON: lists cloud providers plus one Leanzero MLX entry, local providers hidden', async () => {
    mockMlxCapability = true;
    render(
      <SwitchModelModal sessionId={null} onClose={vi.fn()} setView={vi.fn()} initialProvider={null} />
    );
    await openProviderMenu();
    expect(await screen.findByRole('option', { name: 'Leanzero MLX' })).toBeInTheDocument();
    expect(screen.getByRole('option', { name: 'Anthropic' })).toBeInTheDocument();
    expect(screen.getByRole('option', { name: 'OpenAI' })).toBeInTheDocument();
    // Hidden in the UI only — the providers stay registered and configured in code.
    expect(screen.queryByRole('option', { name: 'Ollama' })).not.toBeInTheDocument();
    expect(screen.queryByRole('option', { name: 'LM Studio' })).not.toBeInTheDocument();
    expect(screen.queryByRole('option', { name: 'Local' })).not.toBeInTheDocument();
    expect(screen.queryByRole('option', { name: 'oMLX' })).not.toBeInTheDocument();
  });

  it('capability OFF: the selector lists every configured provider exactly as before', async () => {
    mockMlxCapability = false;
    render(
      <SwitchModelModal sessionId={null} onClose={vi.fn()} setView={vi.fn()} initialProvider={null} />
    );
    await openProviderMenu();
    expect(await screen.findByRole('option', { name: 'Ollama' })).toBeInTheDocument();
    expect(screen.getByRole('option', { name: 'LM Studio' })).toBeInTheDocument();
    expect(screen.getByRole('option', { name: 'oMLX' })).toBeInTheDocument();
    expect(screen.queryByRole('option', { name: 'Leanzero MLX' })).not.toBeInTheDocument();
  });
});

describe('SwitchModelModal — the Leanzero MLX entry', () => {
  it('running engine: shows the served id and submits (omlx, servedModelId) via changeModel', async () => {
    mockMlxCapability = true;
    mockMlxStatus = mlxStatusOf({
      state: 'running',
      modelId: 'mlx-community/Qwen3-30B-A3B-4bit',
      servedModelId: 'qwen3-30b-served',
    });
    const onModelSelected = vi.fn();
    render(
      <SwitchModelModal
        sessionId="session-42"
        onClose={vi.fn()}
        setView={vi.fn()}
        initialProvider="omlx"
        onModelSelected={onModelSelected}
      />
    );
    await waitFor(() => {
      expect(screen.getByTestId('mlx-entry-panel')).toBeInTheDocument();
    });
    expect(screen.getByText('running')).toBeInTheDocument();
    expect(screen.getByText('qwen3-30b-served')).toBeInTheDocument();

    await userEvent.click(screen.getByRole('button', { name: 'Select model' }));
    await waitFor(() => {
      expect(mockChangeModel).toHaveBeenCalledTimes(1);
    });
    expect(mockChangeModel).toHaveBeenCalledWith(
      'session-42',
      expect.objectContaining({ name: 'qwen3-30b-served', provider: 'omlx' })
    );
    expect(onModelSelected).toHaveBeenCalledWith('qwen3-30b-served', 'omlx');
  });

  it('engine down: says "no model mounted", blocks submit, and the button opens the engine window', async () => {
    mockMlxCapability = true;
    mockMlxStatus = mlxStatusOf({ state: 'stopped' });
    const setView = vi.fn();
    const onClose = vi.fn();
    render(
      <SwitchModelModal
        sessionId="session-42"
        onClose={onClose}
        setView={setView}
        initialProvider="omlx"
      />
    );
    await waitFor(() => {
      expect(screen.getByTestId('mlx-entry-panel')).toBeInTheDocument();
    });
    expect(screen.getByText('no model mounted')).toBeInTheDocument();

    // Submit stays blocked: there is no served model to select.
    await userEvent.click(screen.getByRole('button', { name: 'Select model' }));
    expect(mockChangeModel).not.toHaveBeenCalled();

    await userEvent.click(screen.getByRole('button', { name: /Open Leanzero MLX/ }));
    expect(setView).toHaveBeenCalledWith('mlxEngine');
    expect(onClose).toHaveBeenCalled();
  });

  it('mounting engine: pulsing mounting state, no open-window action, submit blocked', async () => {
    mockMlxCapability = true;
    mockMlxStatus = mlxStatusOf({
      state: 'mounting',
      modelId: 'mlx-community/Qwen3-30B-A3B-4bit',
    });
    render(
      <SwitchModelModal
        sessionId="session-42"
        onClose={vi.fn()}
        setView={vi.fn()}
        initialProvider="omlx"
      />
    );
    await waitFor(() => {
      expect(screen.getByTestId('mlx-entry-panel')).toBeInTheDocument();
    });
    expect(screen.getByText('mounting')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /Open Leanzero MLX/ })).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: 'Select model' }));
    expect(mockChangeModel).not.toHaveBeenCalled();
  });

  it('unreachable status renders the failure twin verbatim, never a stale claim', async () => {
    mockMlxCapability = true;
    mockMlxStatus = null;
    mockMlxStatusError = 'agent connection lost';
    render(
      <SwitchModelModal
        sessionId="session-42"
        onClose={vi.fn()}
        setView={vi.fn()}
        initialProvider="omlx"
      />
    );
    await waitFor(() => {
      expect(screen.getByTestId('mlx-entry-panel')).toBeInTheDocument();
    });
    expect(screen.getByText('unreachable')).toBeInTheDocument();
    expect(screen.getByText('agent connection lost')).toBeInTheDocument();
  });
});
