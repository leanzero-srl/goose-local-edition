import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, type RenderOptions, screen, waitFor } from '@testing-library/react';
import ModelsBottomBar from './ModelsBottomBar';
import { IntlTestWrapper } from '../../../../i18n/test-utils';
import type { MlxEngineStatus } from '../../../../acp/mlx-engine';
import { assertStudioClean } from '../../../lz/assertStudioClean';

const renderWithIntl = (ui: React.ReactElement, options?: RenderOptions) =>
  render(ui, { wrapper: IntlTestWrapper, ...options });

const createDropdownRef = (): React.RefObject<HTMLDivElement> =>
  ({ current: document.createElement('div') }) as React.RefObject<HTMLDivElement>;

let mockCurrentModel: string | null = 'config-model';
let mockCurrentProvider: string | null = 'config-provider';
const mockGetProviders = vi.fn();
const mockOnModelChanged = vi.fn();
const mockChangeModel = vi.fn();

vi.mock('../../../ModelAndProviderContext', () => ({
  useModelAndProvider: () => ({
    currentModel: mockCurrentModel,
    currentProvider: mockCurrentProvider,
    changeModel: mockChangeModel,
  }),
}));

vi.mock('../../../ConfigContext', () => ({
  useConfig: () => ({
    getProviders: mockGetProviders,
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
const mockPollEnabled = vi.fn();
vi.mock('../../../leanzero-swarm/useMlxEngineStatus', () => ({
  useMlxEngineStatusPoll: (enabled: boolean) => {
    mockPollEnabled(enabled);
    return { status: enabled ? mockMlxStatus : null, error: null };
  },
}));

vi.mock('../modelInterface', () => ({
  getProviderMetadata: vi.fn().mockResolvedValue({ display_name: 'Config Provider' }),
}));

vi.mock('../predefinedModelsUtils', () => ({
  getModelDisplayName: (model: string) => `Display ${model}`,
}));

vi.mock('../../../bottom_menu/BottomMenuAlertPopover', () => ({
  default: () => null,
}));

vi.mock('../../../ui/dropdown-menu', () => ({
  DropdownMenu: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
  DropdownMenuTrigger: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
  DropdownMenuContent: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
  DropdownMenuItem: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
}));

vi.mock('../../localInference/ModelSettingsPanel', () => ({
  ModelSettingsPanel: () => null,
}));

vi.mock('../../../ui/scroll-area', () => ({
  ScrollArea: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
}));

function runningStatus(servedModelId?: string): MlxEngineStatus {
  return {
    state: 'running',
    modelId: 'mlx-community/Qwen3-30B-A3B-4bit',
    servedModelId,
    restartRequired: false,
    availableMemoryGb: 40,
    totalMemoryGb: 64,
  };
}

describe('ModelsBottomBar', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockCurrentModel = 'config-model';
    mockCurrentProvider = 'config-provider';
    mockMlxCapability = false;
    mockMlxStatus = null;
    mockGetProviders.mockResolvedValue([]);
    mockChangeModel.mockResolvedValue(true);
  });

  it('shows a loading placeholder while the active session model is still loading', async () => {
    renderWithIntl(
      <ModelsBottomBar
        sessionId="session-123"
        dropdownRef={createDropdownRef()}
        setView={vi.fn()}
        onModelChanged={mockOnModelChanged}
        sessionLoaded={false}
      />
    );

    expect(screen.getByTestId('model-loading-state')).toHaveTextContent('Loading model...');
  });

  it('shows the active session model once the session has loaded', async () => {
    renderWithIntl(
      <ModelsBottomBar
        sessionId="session-123"
        dropdownRef={createDropdownRef()}
        setView={vi.fn()}
        sessionModel="session-model"
        sessionProvider="session-provider"
        onModelChanged={mockOnModelChanged}
        sessionLoaded={true}
      />
    );

    expect(screen.getByText('session-model')).toBeInTheDocument();
    expect(screen.queryByTestId('model-loading-state')).not.toBeInTheDocument();
  });

  it('shows the configured model when there is no active session', async () => {
    renderWithIntl(
      <ModelsBottomBar
        sessionId={null}
        dropdownRef={createDropdownRef()}
        setView={vi.fn()}
        onModelChanged={mockOnModelChanged}
      />
    );

    expect(screen.getByText('config-model')).toBeInTheDocument();
    expect(screen.queryByTestId('model-loading-state')).not.toBeInTheDocument();
  });

  describe('MLX session sync', () => {
    it('syncs the session onto the served id when already on omlx and the engine is running', async () => {
      mockMlxCapability = true;
      mockMlxStatus = runningStatus('qwen3-30b-served');
      renderWithIntl(
        <ModelsBottomBar
          sessionId="session-123"
          dropdownRef={createDropdownRef()}
          setView={vi.fn()}
          sessionModel="stale-served-id"
          sessionProvider="omlx"
          onModelChanged={mockOnModelChanged}
          sessionLoaded={true}
        />
      );

      await waitFor(() => {
        expect(mockChangeModel).toHaveBeenCalledTimes(1);
      });
      expect(mockChangeModel).toHaveBeenCalledWith('session-123', {
        name: 'qwen3-30b-served',
        provider: 'omlx',
        subtext: 'Leanzero MLX',
      });
      await waitFor(() => {
        expect(mockOnModelChanged).toHaveBeenCalledWith({
          model: 'qwen3-30b-served',
          provider: 'omlx',
        });
      });
    });

    it('does not sync when the session already uses the served id', async () => {
      mockMlxCapability = true;
      mockMlxStatus = runningStatus('qwen3-30b-served');
      renderWithIntl(
        <ModelsBottomBar
          sessionId="session-123"
          dropdownRef={createDropdownRef()}
          setView={vi.fn()}
          sessionModel="qwen3-30b-served"
          sessionProvider="omlx"
          onModelChanged={mockOnModelChanged}
          sessionLoaded={true}
        />
      );

      await new Promise((r) => setTimeout(r, 20));
      expect(mockChangeModel).not.toHaveBeenCalled();
    });

    it('never yanks a session off a cloud provider, and does not even poll for one', async () => {
      mockMlxCapability = true;
      mockMlxStatus = runningStatus('qwen3-30b-served');
      renderWithIntl(
        <ModelsBottomBar
          sessionId="session-123"
          dropdownRef={createDropdownRef()}
          setView={vi.fn()}
          sessionModel="claude-sonnet-4"
          sessionProvider="anthropic"
          onModelChanged={mockOnModelChanged}
          sessionLoaded={true}
        />
      );

      await new Promise((r) => setTimeout(r, 20));
      expect(mockChangeModel).not.toHaveBeenCalled();
      expect(mockPollEnabled).toHaveBeenCalledWith(false);
      expect(mockPollEnabled).not.toHaveBeenCalledWith(true);
    });

    it('never syncs while the engine is mounting', async () => {
      mockMlxCapability = true;
      mockMlxStatus = { ...runningStatus('qwen3-30b-served'), state: 'mounting' };
      renderWithIntl(
        <ModelsBottomBar
          sessionId="session-123"
          dropdownRef={createDropdownRef()}
          setView={vi.fn()}
          sessionModel="stale-served-id"
          sessionProvider="omlx"
          onModelChanged={mockOnModelChanged}
          sessionLoaded={true}
        />
      );

      await new Promise((r) => setTimeout(r, 20));
      expect(mockChangeModel).not.toHaveBeenCalled();
    });

    it('never clears the session model when the engine is down', async () => {
      mockMlxCapability = true;
      mockMlxStatus = null;
      renderWithIntl(
        <ModelsBottomBar
          sessionId="session-123"
          dropdownRef={createDropdownRef()}
          setView={vi.fn()}
          sessionModel="qwen3-30b-served"
          sessionProvider="omlx"
          onModelChanged={mockOnModelChanged}
          sessionLoaded={true}
        />
      );

      await new Promise((r) => setTimeout(r, 20));
      expect(mockChangeModel).not.toHaveBeenCalled();
      expect(mockOnModelChanged).not.toHaveBeenCalled();
      expect(screen.getByText('qwen3-30b-served')).toBeInTheDocument();
    });

    it('capability off: no polling, no syncing — the legacy selector as it always was', async () => {
      mockMlxCapability = false;
      mockMlxStatus = runningStatus('qwen3-30b-served');
      renderWithIntl(
        <ModelsBottomBar
          sessionId="session-123"
          dropdownRef={createDropdownRef()}
          setView={vi.fn()}
          sessionModel="stale-served-id"
          sessionProvider="omlx"
          onModelChanged={mockOnModelChanged}
          sessionLoaded={true}
        />
      );

      await new Promise((r) => setTimeout(r, 20));
      expect(mockChangeModel).not.toHaveBeenCalled();
      expect(mockPollEnabled).not.toHaveBeenCalledWith(true);
    });
  });
});

/**
 * Studio remake: the trigger's readout is the meta step (it sits in a quiet Chip in ChatInput);
 * the menu's labels are meta in ink-3, the current model the body step. Never
 * `text-text-primary/70 text-xs`.
 */
describe('ModelsBottomBar (Studio)', () => {
  it('the readout is the meta step; menu labels are meta over a body value', () => {
    const { container } = renderWithIntl(
      <ModelsBottomBar
        sessionId="session-123"
        dropdownRef={createDropdownRef()}
        setView={vi.fn()}
        sessionModel="session-model"
        sessionProvider="session-provider"
        onModelChanged={mockOnModelChanged}
        sessionLoaded={true}
      />
    );
    expect(screen.getByText('session-model').className).toContain('text-lz-meta');
    const label = screen.getByText('Current model');
    expect(label.className).toContain('text-lz-meta');
    expect(label.className).toContain('text-lz-ink-3');
    expect(container.querySelector('.text-xs')).toBeNull();
    expect(container.innerHTML).not.toContain('text-text-primary/70');
    assertStudioClean(container);
  });
});
