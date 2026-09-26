import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, type RenderOptions, screen, waitFor } from '@testing-library/react';
import ModelsBottomBar from './ModelsBottomBar';
import { IntlTestWrapper } from '../../../../i18n/test-utils';
import type { MlxEngineStatus } from '../../../../acp/mlx-engine';
import { assertStudioClean } from '../../../lz/assertStudioClean';
import userEvent from '@testing-library/user-event';
import { deriveChatServedBy, type ChatServedBy } from '../../../chatServedBy/chatServedBy';
import { SPLIT_STOPPED_E2E2 } from '../../../chatServedBy/splitStop.fixtures';

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
  DropdownMenuItem: ({
    children,
    onClick,
    'data-testid': testId,
  }: {
    children: React.ReactNode;
    onClick?: () => void;
    'data-testid'?: string;
  }) => (
    <div role="menuitem" data-testid={testId} onClick={onClick}>
      {children}
    </div>
  ),
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

/**
 * Q-5 / Q-12: the chip named the provider id ("swarm") and its menu dead-ended in a provider picker
 * that never named the 27B or the Mac it runs on. It now reads the one derivation of where chat goes.
 */
describe('ModelsBottomBar — the chip names what serves chat', () => {
  const HF = 'Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx';
  const STUDIO: ChatServedBy = {
    engine: 'remote',
    model: HF,
    where: ["Work's Mac Studio"],
    peerNodeId: 'worksmacstudio-lan-9c1e2a',
    foreign: false,
    contextWindow: 262144,
    phase: 'idle',
    activity: 'idle',
    busyWithOthers: null,
    turnRequest: null,
    readTps: null,
    readiness: { kind: 'ready' },
  };
  const renderChip = (served: ChatServedBy | null, setView = vi.fn()) =>
    renderWithIntl(
      <ModelsBottomBar
        sessionId="session-123"
        dropdownRef={createDropdownRef()}
        setView={setView}
        sessionModel="swarm"
        sessionProvider="swarm"
        onModelChanged={mockOnModelChanged}
        sessionLoaded={true}
        served={served}
      />
    );

  beforeEach(() => {
    vi.clearAllMocks();
    mockCurrentModel = 'config-model';
    mockCurrentProvider = 'config-provider';
  });

  it('Q-56: the dot’s word is ON SCREEN beside it, not only its aria-label', async () => {
    renderChip({ ...STUDIO, phase: 'writing', activity: 'generating' });
    expect((await screen.findByTestId('model-chip-phase')).textContent).toBe('Writing');
  });

  it('a route to the Studio: "<model> · Work\'s Mac Studio" with the phase dot — never "swarm"', () => {
    const { container } = renderChip(STUDIO);
    const chip = screen.getByTestId('model-chip-served');
    expect(chip).toHaveTextContent("Qwen3.8-27B-Atlassian-Q8-mlx · Work's Mac Studio");
    expect(chip).toHaveAttribute('title', HF);
    expect(chip).toHaveAttribute('data-engine', 'remote');
    expect(screen.queryByText('swarm')).toBeNull();
    const dots = screen.getAllByTestId('lz-status-dot');
    expect(dots[0]).toHaveAttribute('data-phase', 'idle');
    expect(dots[0]).toHaveAttribute('aria-label', 'Idle');
    assertStudioClean(container);
  });

  it('the menu names the model, where and what it is doing, its window — and Open Engine changes model or Mac', async () => {
    const setView = vi.fn();
    renderChip({ ...STUDIO, phase: 'writing', activity: 'generating' }, setView);
    const head = screen.getByTestId('model-menu-served');
    expect(head).toHaveTextContent('Qwen3.8-27B-Atlassian-Q8-mlx');
    expect(head).toHaveTextContent("Writing on Work's Mac Studio");
    expect(head).toHaveTextContent('262,144-token context');
    const open = screen.getByTestId('model-menu-open-engine');
    expect(open).toHaveTextContent('Open Engine');
    expect(open).toHaveTextContent('Change the model or the Mac it runs on');
    await userEvent.setup().click(open);
    expect(setView).toHaveBeenCalledWith('mlxEngine');
    // Q-41: the provider switch stays for anyone leaving for a cloud provider — and says that,
    // never "Change Provider" into a picker whose swarm row names no model.
    const leave = screen.getByTestId('model-menu-switch');
    expect(leave).toHaveTextContent('Use a cloud provider instead');
    expect(leave).toHaveTextContent('Leaves Qwen3.8-27B-Atlassian-Q8-mlx for this chat');
    expect(screen.queryByText('Change Provider')).toBeNull();
  });

  it('the split names both Macs; another window’s run says so', () => {
    renderChip({
      ...STUDIO,
      engine: 'split',
      where: ['Mihai Macbook', 'Work’s Mac Studio'],
      foreign: true,
      peerNodeId: null,
    });
    expect(screen.getByTestId('model-chip-served')).toHaveTextContent(
      'Qwen3.8-27B-Atlassian-Q8-mlx · Mihai Macbook and Work’s Mac Studio'
    );
    expect(screen.getByTestId('model-menu-served')).toHaveTextContent('run by another window');
  });

  it('Q-71: a split whose window was sized from free memory says so, and how to grow it', () => {
    renderChip({
      ...STUDIO,
      engine: 'split',
      where: ['Mihai Macbook', 'Work’s Mac Studio'],
      peerNodeId: null,
      contextWindow: 141568,
      contextFromFreeMemory: true,
    });
    expect(screen.getByTestId('model-menu-context')).toHaveTextContent(
      '142k context on this split — sized from the memory free when it started; restart it to grow'
    );
  });

  it('a route’s window is the model’s: the plain token count', () => {
    renderChip(STUDIO);
    expect(screen.getByTestId('model-menu-context')).toHaveTextContent('262,144-token context');
  });

  it('nothing runs: the model a Mount would bring, "not running", the unloaded dot — and Open Engine', () => {
    renderChip({
      ...STUDIO,
      engine: 'none',
      where: ['This Mac'],
      peerNodeId: null,
      contextWindow: null,
      phase: 'unloaded',
      activity: null,
    });
    expect(screen.getByTestId('model-chip-served')).toHaveTextContent(
      'Qwen3.8-27B-Atlassian-Q8-mlx · not running'
    );
    expect(screen.getByTestId('model-menu-served')).toHaveTextContent(
      'Not running — start it from the Engine'
    );
    expect(screen.getByTestId('model-menu-open-engine')).toBeInTheDocument();
  });

  it('Q-81: the split chat was on stopped — "Split stopped", both Macs, red; the menu says why', () => {
    const served = deriveChatServedBy({
      provider: 'omlx',
      lookup: {
        state: 'ready',
        devices: [],
        settings: {
          modelId: HF,
          servedModelName: 'mihai-qwen3.8-27b-atlassian-q8-mlx',
          modelsDir: '/models',
          port: 8090,
          spawnCommand: [],
          modelProfiles: {},
        },
      },
      single: {
        state: 'stopped',
        restartRequired: false,
        availableMemoryGb: 61.2,
        totalMemoryGb: 128,
      },
      distributed: SPLIT_STOPPED_E2E2,
      remote: null,
      remoteReadError: null,
      main: null,
      sessionId: 'session-123',
      turnInFlight: false,
      thisMac: 'This Mac',
      engineLabel: 'LeanZero MLX',
    });
    renderChip(served);
    expect(screen.getByTestId('model-chip-phase').textContent).toBe('Split stopped');
    const chip = screen.getByTestId('model-chip-served');
    expect(chip).toHaveTextContent(
      'Qwen3.8-27B-Atlassian-Q8-mlx · Mihai Macbook and Work’s Mac Studio'
    );
    expect(chip).not.toHaveTextContent('not running');
    expect(chip).toHaveAttribute('title', 'Work’s Mac Studio ran out of memory');
    expect(screen.getAllByTestId('lz-status-dot')[0]).toHaveAttribute('data-phase', 'failed');
    expect(screen.getByTestId('model-menu-served')).toHaveTextContent(
      'Stopped on Mihai Macbook and Work’s Mac Studio — Work’s Mac Studio ran out of memory'
    );
  });

  it('Q-111: the Studio’s goose is gone — the chip says the composer bar’s words, held, and names only the model', () => {
    const ROUTE = {
      state: 'reconnecting',
      peer: 'worksmacstudio-lan-9c1e2a',
      peerHostname: 'WorksMacStudio.lan',
      peerComputerName: "Work's Mac Studio",
    };
    const gone: ChatServedBy = {
      ...STUDIO,
      phase: 'held',
      activity: null,
      readiness: {
        kind: 'reconnecting',
        status: ROUTE,
        why: 'unreachable: connect ECONNREFUSED',
        cause: null,
        gone: { because: 'unreachable', lostForMs: 3_600_000, longestComebackMs: 25_000 },
        instead: { kind: 'none' },
      },
    };
    const { container } = renderChip(gone);
    const words = "Work's Mac Studio’s goose isn’t running";
    const phase = screen.getByTestId('model-chip-phase');
    expect(phase.textContent).toBe(words);
    expect(phase).toHaveAttribute('title', words);
    expect(screen.getByTestId('model-chip-served').textContent).toBe(
      'Qwen3.8-27B-Atlassian-Q8-mlx'
    );
    expect(screen.getAllByTestId('lz-status-dot')[0]).toHaveAttribute('data-phase', 'held');
    expect(screen.getByTestId('model-menu-served')).toHaveTextContent(words);
    expect(screen.queryByText(/Reconnecting|Queued/)).toBeNull();
    assertStudioClean(container);

    // A blip keeps its own word.
    renderChip({
      ...gone,
      phase: 'loading',
      readiness: { ...gone.readiness, gone: null } as ChatServedBy['readiness'],
    });
    expect(screen.getAllByTestId('model-chip-phase')[1].textContent).toBe('Reconnecting');
  });

  it('a cloud provider (nothing served): the chip is the provider’s own label, no Open Engine', () => {
    renderChip(null);
    expect(screen.queryByTestId('model-chip-served')).toBeNull();
    expect(screen.queryByTestId('model-menu-open-engine')).toBeNull();
    expect(screen.getByText('swarm')).toBeInTheDocument();
    expect(screen.getByTestId('model-menu-switch')).toHaveTextContent('Change Provider');
  });
});
