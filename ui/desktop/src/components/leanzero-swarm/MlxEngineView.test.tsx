import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { cleanup, render as rtlRender, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { IntlTestWrapper } from '../../i18n/test-utils';
import { allClasses, assertStudioClean } from '../lz/assertStudioClean';
import { missingUtilities } from '../lz/compileStudioCss';
import MlxEngineView, {
  draftsFromProfile,
  profileFromDrafts,
  profileHasValues,
  sanitizeSettingsForWrite,
  settingsWithProfile,
  formatGb,
} from './MlxEngineView';
import { NAV_ITEMS } from '../../hooks/useNavigationItems';
import type {
  MlxBrowseHit,
  MlxEngineSettings,
  MlxEngineStatus,
  MlxLocalModel,
} from '../../acp/mlx-engine';
import type { NodeState, NodesResponse } from '../../acp/leanzero-link';

const mockStatus = vi.fn();
const mockMount = vi.fn();
const mockUnmount = vi.fn();
const mockSettingsRead = vi.fn();
const mockSettingsUpdate = vi.fn();
const mockModelsList = vi.fn();
const mockModelDelete = vi.fn();
const mockBrowse = vi.fn();
const mockBrowseFilters = vi.fn();
const mockModelCard = vi.fn();
const mockDownload = vi.fn();
const mockDownloadProgress = vi.fn();
const mockDownloadCancel = vi.fn();
const mockDownloadPause = vi.fn();
const mockDownloadResume = vi.fn();

vi.mock('../../acp/mlx-engine', () => ({
  mlxEngineStatus: (...args: unknown[]) => mockStatus(...args),
  mlxEngineMount: (...args: unknown[]) => mockMount(...args),
  mlxEngineUnmount: (...args: unknown[]) => mockUnmount(...args),
  mlxEngineSettingsRead: (...args: unknown[]) => mockSettingsRead(...args),
  mlxEngineSettingsUpdate: (...args: unknown[]) => mockSettingsUpdate(...args),
  mlxEngineModelsList: (...args: unknown[]) => mockModelsList(...args),
  mlxEngineModelDelete: (...args: unknown[]) => mockModelDelete(...args),
  mlxEngineBrowse: (...args: unknown[]) => mockBrowse(...args),
  mlxEngineBrowseFilters: (...args: unknown[]) => mockBrowseFilters(...args),
  mlxEngineModelCard: (...args: unknown[]) => mockModelCard(...args),
  mlxEngineDownload: (...args: unknown[]) => mockDownload(...args),
  mlxEngineDownloadProgress: (...args: unknown[]) => mockDownloadProgress(...args),
  mlxEngineDownloadCancel: (...args: unknown[]) => mockDownloadCancel(...args),
  mlxEngineDownloadPause: (...args: unknown[]) => mockDownloadPause(...args),
  mlxEngineDownloadResume: (...args: unknown[]) => mockDownloadResume(...args),
}));

// The device picker sources the mesh roster from leanzeroLink and is gated on the
// `leanzeroLink` capability. Default: capability OFF → the view is exactly as before, every
// mlx op local (nodeId undefined). Tests that exercise the remote path flip mockFeatures and
// hand the mesh a connected roster with peers.
const mockFeatures = { leanzeroLink: false };
vi.mock('../../contexts/FeaturesContext', () => ({
  useFeatures: () => ({
    localInference: true,
    mlxEngine: true,
    leanzeroLink: mockFeatures.leanzeroLink,
    isLoading: false,
  }),
}));

const mockLinkStatus = vi.fn();
const mockLinkNodes = vi.fn();
vi.mock('../../acp/leanzero-link', async (importActual) => {
  const actual = await importActual<typeof import('../../acp/leanzero-link')>();
  return {
    ...actual,
    leanzeroLinkStatus: (...a: unknown[]) => mockLinkStatus(...a),
    leanzeroLinkNodes: (...a: unknown[]) => mockLinkNodes(...a),
  };
});

const render = (ui: React.ReactElement) => rtlRender(ui, { wrapper: IntlTestWrapper });

// jsdom has no ResizeObserver; radix ScrollArea needs one (same stub as SwarmWorkspace.test.tsx).
class ResizeObserverMock {
  observe() {}
  unobserve() {}
  disconnect() {}
}
vi.stubGlobal('ResizeObserver', ResizeObserverMock);

const GB = 1024 * 1024 * 1024;

const QWEN = 'mlx-community/Qwen3-30B-A3B-4bit';
const HALF = 'mlx-community/Half-Model-8bit';

// The backend still echoes LEGACY flat sampling fields (temperature/topK here) until its
// one-time migration runs — the write path must strip them, never send them back.
const SETTINGS: MlxEngineSettings = {
  modelId: QWEN,
  modelsDir: '/Users/x/mlx-models',
  port: 9600,
  temperature: 0,
  topK: 40,
  servedModelName: 'leanzero-mlx',
  spawnCommand: ['uvx', 'rapid-mlx', 'serve'],
  modelProfiles: {
    [QWEN]: { temperature: 0, topK: 40 },
  },
};

const MODELS: MlxLocalModel[] = [
  { id: QWEN, sizeBytes: 17 * GB, complete: true, missingFiles: 0 },
  { id: HALF, sizeBytes: 3 * GB, complete: false, missingFiles: 2 },
];

/** The modelsList wire shape: models plus the models volume's disk numbers. */
function listOf(models: MlxLocalModel[]) {
  return { models, diskAvailableBytes: 250 * GB, diskTotalBytes: 500 * GB };
}

const FILTERS = {
  quants: ['4-bit', '8-bit', '6-bit', 'bf16', '3-bit'],
  archs: ['qwen3_5', 'llama', 'qwen3', 'qwen3_moe', 'gemma3'],
  authors: ['mlx-community', 'lmstudio-community', 'Qwen'],
  sampledRepos: 708,
  computedAt: 1756640000,
};

function statusOf(overrides: Partial<MlxEngineStatus>): MlxEngineStatus {
  return {
    state: 'stopped',
    restartRequired: false,
    availableMemoryGb: 40.2,
    totalMemoryGb: 64,
    ...overrides,
  };
}

// ---------------------------------------------------------------------------
// Mesh roster fixtures for the device picker (leanzeroLink/nodes shape, snake_case).
// ---------------------------------------------------------------------------

const SELF_NODE: NodeState = {
  node_id: 'self-node',
  hostname: 'this-mac',
  mesh_ip: '100.64.0.1',
  status: { type: 'Idle' },
  sessions_active: 0,
  updated_at: '2026-09-01T12:00:00Z',
};

function peerNode(overrides: Partial<NodeState> = {}): NodeState {
  return {
    node_id: 'peer-workhorse',
    hostname: 'workhorse',
    mesh_ip: '100.64.0.2',
    status: { type: 'Idle' },
    sessions_active: 0,
    updated_at: '2026-09-01T12:00:00Z',
    ...overrides,
  };
}

const CONNECTED = {
  auth: { state: 'connected', email: 'm@x.co', meshIp: '100.64.0.1' },
  nodeCount: 2,
};

/** Turn on the capability + a connected roster with the given peers, so the picker renders. */
function withMesh(peers: NodeState[]) {
  mockFeatures.leanzeroLink = true;
  mockLinkStatus.mockResolvedValue(CONNECTED);
  mockLinkNodes.mockResolvedValue({ self: SELF_NODE, peers } as NodesResponse);
}

beforeEach(() => {
  vi.clearAllMocks();
  mockFeatures.leanzeroLink = false;
  mockLinkStatus.mockResolvedValue({ auth: { state: 'loggedOut' }, nodeCount: 0 });
  mockLinkNodes.mockResolvedValue({ self: SELF_NODE, peers: [] } as NodesResponse);
  mockStatus.mockResolvedValue(statusOf({}));
  mockSettingsRead.mockResolvedValue(SETTINGS);
  mockSettingsUpdate.mockImplementation(async (s: MlxEngineSettings) => s);
  mockModelsList.mockResolvedValue(listOf(MODELS));
  mockMount.mockResolvedValue(undefined);
  mockUnmount.mockResolvedValue(undefined);
  mockBrowse.mockResolvedValue({ hits: [] });
  mockBrowseFilters.mockResolvedValue(FILTERS);
  mockModelCard.mockResolvedValue({
    readmeTruncated: false,
    files: [],
    totalBytes: 0,
    tags: [],
    downloads: 0,
    likes: 0,
  });
  mockDownload.mockResolvedValue(undefined);
  mockDownloadProgress.mockResolvedValue(null);
  mockDownloadCancel.mockResolvedValue(undefined);
  mockDownloadPause.mockResolvedValue(undefined);
  mockDownloadResume.mockResolvedValue(undefined);
});

afterEach(() => {
  cleanup();
});

// ---------------------------------------------------------------------------
// The honest-payload contract, now per model: a cleared field is ABSENT from the
// profile, an explicit 0 is 0 — the exact spot where "0 vs unset" silently
// corrupts sampling. And the write path NEVER carries the legacy flat fields.
// ---------------------------------------------------------------------------

describe('per-model profile drafts keep 0 and unset apart', () => {
  it('a persisted 0 round-trips as the text "0", never as blank', () => {
    const drafts = draftsFromProfile(SETTINGS.modelProfiles[QWEN]);
    expect(drafts.temperature).toBe('0');
    expect(drafts.topK).toBe('40');
    expect(drafts.topP).toBe('');
  });

  it('a blank draft leaves the key absent; "0" sends the number 0', () => {
    const drafts = draftsFromProfile(SETTINGS.modelProfiles[QWEN]);
    drafts.temperature = '0';
    drafts.topP = '';
    drafts.minP = '0.05';
    const profile = profileFromDrafts(drafts);
    expect(profile.temperature).toBe(0);
    expect('topP' in profile).toBe(false);
    expect(profile.minP).toBe(0.05);
  });

  it('a profile with no set fields reads as empty', () => {
    expect(profileHasValues(profileFromDrafts(draftsFromProfile(undefined)))).toBe(false);
    expect(profileHasValues({ temperature: 0 })).toBe(true);
  });
});

describe('settings write payloads', () => {
  it('sanitize strips the legacy flat sampling fields and keeps servedModelName', () => {
    const payload = sanitizeSettingsForWrite(SETTINGS);
    expect('temperature' in payload).toBe(false);
    expect('topK' in payload).toBe(false);
    expect(payload.servedModelName).toBe('leanzero-mlx');
    expect(payload.modelId).toBe(QWEN);
    expect(payload.modelsDir).toBe('/Users/x/mlx-models');
    expect(payload.port).toBe(9600);
    expect(payload.spawnCommand).toEqual(['uvx', 'rapid-mlx', 'serve']);
    expect(payload.modelProfiles).toEqual(SETTINGS.modelProfiles);
  });

  it('settingsWithProfile rewrites ONE model profile and leaves the others untouched', () => {
    const settings: MlxEngineSettings = {
      ...SETTINGS,
      modelProfiles: { ...SETTINGS.modelProfiles, [HALF]: { topP: 0.9 } },
    };
    const drafts = draftsFromProfile(settings.modelProfiles[QWEN]);
    drafts.temperature = '0.7';
    const payload = settingsWithProfile(settings, QWEN, drafts);
    expect(payload.modelProfiles[QWEN]).toEqual({ temperature: 0.7, topK: 40 });
    expect(payload.modelProfiles[HALF]).toEqual({ topP: 0.9 });
    expect('temperature' in payload).toBe(false);
  });

  it('an all-blank draft set removes the model entry entirely', () => {
    const drafts = draftsFromProfile(SETTINGS.modelProfiles[QWEN]);
    drafts.temperature = '';
    drafts.topK = '';
    const payload = settingsWithProfile(SETTINGS, QWEN, drafts);
    expect(QWEN in payload.modelProfiles).toBe(false);
  });
});

describe('formatGb', () => {
  it('shows sizes in GB with an honest unknown for zero', () => {
    expect(formatGb(17 * GB)).toBe('17 GB');
    expect(formatGb(1.5 * GB)).toBe('1.5 GB');
    expect(formatGb(0)).toBe('unknown size');
  });
});

// ---------------------------------------------------------------------------
// Pass C: the engine content is the LeanZero MLX TAB of the Goose Flock view.
// The page header moved to LeanZeroSwarmView; this panel keeps the sub-tab bar,
// the live state badge and the powered-by line. The nav carries the VIEW's name.
// ---------------------------------------------------------------------------

describe('LeanZero MLX panel naming', () => {
  it('the panel shows the engine sub-tabs, the state badge and the powered-by line', async () => {
    const { unmount } = render(<MlxEngineView />);
    expect(screen.getByText('Powered by Rapid-MLX')).toBeInTheDocument();
    expect(screen.getByRole('radio', { name: /^Engine$/ })).toBeInTheDocument();
    expect(screen.getByRole('radio', { name: /Sampling/ })).toBeInTheDocument();
    await waitFor(() => {
      expect(screen.getAllByTestId('mlx-state-badge').length).toBeGreaterThan(0);
    });
    unmount();
  });

  it('the nav item is /leanzero-swarm labelled Goose Flock (old /mlx-engine path is gone)', () => {
    expect(NAV_ITEMS.find((i) => i.path === '/mlx-engine')).toBeUndefined();
    const item = NAV_ITEMS.find((i) => i.path === '/leanzero-swarm');
    expect(item?.label).toBe('Goose Flock');
  });
});

// ---------------------------------------------------------------------------
// The engine tab renders backend truth: state, gate text VERBATIM, failure twins.
// ---------------------------------------------------------------------------

describe('MlxEngineView engine tab', () => {
  it('shows a running engine with model id, context window, parser, and pid', async () => {
    mockStatus.mockResolvedValue(
      statusOf({
        state: 'running',
        modelId: QWEN,
        baseUrl: 'http://127.0.0.1:9600/v1',
        pid: 4242,
        contextWindow: 131072,
        toolCallParser: 'qwen3',
      })
    );
    const { unmount } = render(<MlxEngineView />);
    await waitFor(() => {
      expect(screen.getAllByTestId('mlx-state-badge')[0]).toHaveTextContent('running');
    });
    expect(screen.getAllByText(QWEN).length).toBeGreaterThan(0);
    expect(screen.getByText('131,072')).toBeInTheDocument();
    expect(screen.getByText('qwen3')).toBeInTheDocument();
    expect(screen.getByText('4242')).toBeInTheDocument();
    expect(screen.getByText('http://127.0.0.1:9600/v1')).toBeInTheDocument();
    unmount();
  });

  it('renders a BLOCK gate message verbatim as a solid red banner', async () => {
    const gate =
      'BLOCK: model needs 24.0 GB but only 9.1 GB of unified memory is free — close something or pick a smaller quant';
    mockStatus.mockResolvedValue(
      statusOf({ state: 'stopped', gateMessage: gate, gateVerdict: 'block' })
    );
    const { unmount } = render(<MlxEngineView />);
    await waitFor(() => {
      expect(screen.getByText(gate)).toBeInTheDocument();
      expect(screen.getByText('Mount blocked')).toBeInTheDocument();
    });
    unmount();
  });

  it('an ALLOW gate verdict renders no red banner (live-caught defect: allow shown as blocked)', async () => {
    const gate = 'model 5.6 GiB fits with 15.0 GiB above the 9.6 GiB floor';
    mockStatus.mockResolvedValue(
      statusOf({ state: 'stopped', gateMessage: gate, gateVerdict: 'allow' })
    );
    const { unmount } = render(<MlxEngineView />);
    await waitFor(() => {
      expect(screen.queryByText('Mount blocked')).not.toBeInTheDocument();
      expect(screen.queryByText('Memory pressure')).not.toBeInTheDocument();
    });
    unmount();
  });

  it('a WARN gate verdict renders the amber pressure banner', async () => {
    const gate = 'fits, but only 2.1 GiB above the floor — expect pressure under load';
    mockStatus.mockResolvedValue(
      statusOf({ state: 'stopped', gateMessage: gate, gateVerdict: 'warn' })
    );
    const { unmount } = render(<MlxEngineView />);
    await waitFor(() => {
      expect(screen.getByText(gate)).toBeInTheDocument();
      expect(screen.getByText('Memory pressure')).toBeInTheDocument();
      expect(screen.queryByText('Mount blocked')).not.toBeInTheDocument();
    });
    unmount();
  });

  it('a failed engine surfaces lastError instead of looking merely stopped', async () => {
    mockStatus.mockResolvedValue(
      statusOf({ state: 'failed', lastError: 'sidecar exited with code 1 before the port opened' })
    );
    const { unmount } = render(<MlxEngineView />);
    await waitFor(() => {
      expect(
        screen.getByText('sidecar exited with code 1 before the port opened')
      ).toBeInTheDocument();
    });
    expect(screen.getAllByTestId('mlx-state-badge')[0]).toHaveTextContent('failed');
    unmount();
  });

  it('a failed status probe renders its error rather than fabricating a context window', async () => {
    mockStatus.mockResolvedValue(
      statusOf({ state: 'running', modelId: 'm', probeError: 'probe timed out after 3s' })
    );
    const { unmount } = render(<MlxEngineView />);
    await waitFor(() => {
      expect(screen.getByText(/probe timed out after 3s/)).toBeInTheDocument();
    });
    unmount();
  });

  it('restartRequired shows the amber banner and Remount does unmount then mount', async () => {
    mockStatus.mockResolvedValue(
      statusOf({ state: 'running', modelId: QWEN, restartRequired: true })
    );
    const { unmount } = render(<MlxEngineView />);
    await waitFor(() => {
      expect(screen.getByText('Settings changed — remount to apply.')).toBeInTheDocument();
    });
    await userEvent.click(screen.getByRole('button', { name: /Remount/ }));
    await waitFor(() => {
      expect(mockUnmount).toHaveBeenCalledTimes(1);
      expect(mockMount).toHaveBeenCalledWith(QWEN, undefined);
    });
    unmount();
  });

  it('a rejected mount renders the backend error text', async () => {
    mockStatus.mockResolvedValue(statusOf({ state: 'stopped' }));
    mockSettingsRead.mockResolvedValue({ ...SETTINGS });
    mockMount.mockRejectedValue(
      new Error('model directory is incomplete: missing weights.safetensors')
    );
    const { unmount } = render(<MlxEngineView />);
    await waitFor(() => {
      expect(screen.getByRole('button', { name: /^Mount$/ })).toBeEnabled();
    });
    await userEvent.click(screen.getByRole('button', { name: /^Mount$/ }));
    await waitFor(() => {
      expect(
        screen.getByText('model directory is incomplete: missing weights.safetensors')
      ).toBeInTheDocument();
    });
    unmount();
  });

  it('a rejected mount with an ACP RequestError shows the sidecar reason (data), not the class (message)', async () => {
    // Measured 2026-09-02: {"code":-32602,"message":"Invalid params","data":"port 8090 has an
    // unsupervised listener — unmount/reclaim it first"} rendered as "Mount failed  Invalid params".
    mockStatus.mockResolvedValue(statusOf({ state: 'stopped' }));
    mockSettingsRead.mockResolvedValue({ ...SETTINGS });
    mockMount.mockRejectedValue(
      Object.assign(new Error('Invalid params'), {
        code: -32602,
        data: 'port 8090 has an unsupervised listener — unmount/reclaim it first',
      })
    );
    const { unmount } = render(<MlxEngineView />);
    await waitFor(() => {
      expect(screen.getByRole('button', { name: /^Mount$/ })).toBeEnabled();
    });
    await userEvent.click(screen.getByRole('button', { name: /^Mount$/ }));
    await waitFor(() => {
      expect(
        screen.getByText('port 8090 has an unsupervised listener — unmount/reclaim it first')
      ).toBeInTheDocument();
    });
    expect(screen.queryByText('Invalid params')).not.toBeInTheDocument();
    unmount();
  });

  it('a stray listener while stopped shows the amber banner and an enabled Unmount that reclaims it', async () => {
    mockStatus.mockResolvedValue(statusOf({ state: 'stopped', strayListenerPort: 9600 }));
    const { unmount } = render(<MlxEngineView />);
    await waitFor(() => {
      expect(
        screen.getByText('unsupervised engine on port 9600 — Unmount reclaims it')
      ).toBeInTheDocument();
    });
    const unmountBtn = screen.getByRole('button', { name: /Unmount/ });
    expect(unmountBtn).toBeEnabled();
    await userEvent.click(unmountBtn);
    await waitFor(() => {
      expect(mockUnmount).toHaveBeenCalledTimes(1);
    });
    unmount();
  });

  it('stopped with NO stray listener keeps Unmount disabled', async () => {
    mockStatus.mockResolvedValue(statusOf({ state: 'stopped' }));
    const { unmount } = render(<MlxEngineView />);
    await waitFor(() => {
      expect(screen.getByRole('button', { name: /^Mount$/ })).toBeEnabled();
    });
    expect(screen.getByRole('button', { name: /Unmount/ })).toBeDisabled();
    unmount();
  });
});

// ---------------------------------------------------------------------------
// Mount card truth: the primary button and the picker report the LIVE engine,
// not just mount intent. A window opened onto a running engine says "Mounted";
// a different selection while running offers "Switch model"; an explicit user
// pick is never overridden by the status poll.
// ---------------------------------------------------------------------------

const OTHER_MODEL = 'mlx-community/Other-Model-4bit';
const COMPLETE_MODELS: MlxLocalModel[] = [
  { id: QWEN, sizeBytes: 17 * GB, complete: true, missingFiles: 0 },
  { id: OTHER_MODEL, sizeBytes: 4 * GB, complete: true, missingFiles: 0 },
];

/** Flip visibility off/on so the 2s status poll refreshes immediately. */
function forceStatusRefresh() {
  Object.defineProperty(document, 'visibilityState', { value: 'hidden', configurable: true });
  document.dispatchEvent(new Event('visibilitychange'));
  Object.defineProperty(document, 'visibilityState', { value: 'visible', configurable: true });
  document.dispatchEvent(new Event('visibilitychange'));
}

describe('MlxEngineView mount card truth', () => {
  it('running with the mounted model selected shows a DISABLED "Mounted" status button', async () => {
    mockStatus.mockResolvedValue(statusOf({ state: 'running', modelId: QWEN }));
    const { unmount } = render(<MlxEngineView />);
    await waitFor(() => {
      expect(screen.getByRole('button', { name: /Mounted/ })).toBeInTheDocument();
    });
    expect(screen.getByRole('button', { name: /Mounted/ })).toBeDisabled();
    expect(screen.queryByRole('button', { name: /^Mount$/ })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /Switch model/ })).not.toBeInTheDocument();
    unmount();
  });

  it('running with restartRequired keeps "Mounted" disabled — the amber banner owns the action', async () => {
    mockStatus.mockResolvedValue(
      statusOf({ state: 'running', modelId: QWEN, restartRequired: true })
    );
    const { unmount } = render(<MlxEngineView />);
    await waitFor(() => {
      expect(screen.getByText('Settings changed — remount to apply.')).toBeInTheDocument();
    });
    // The banner commits on the FIRST status render; the picker-follows-truth effect that turns
    // the primary button into "Mounted" lands one commit later — wait for it, then assert.
    await waitFor(() => {
      expect(screen.getByRole('button', { name: /Mounted/ })).toBeInTheDocument();
    });
    expect(screen.getByRole('button', { name: /Mounted/ })).toBeDisabled();
    expect(screen.getByRole('button', { name: /Remount/ })).toBeEnabled();
    unmount();
  });

  it('running with a DIFFERENT selection offers an enabled "Switch model" that mounts the selection', async () => {
    mockStatus.mockResolvedValue(statusOf({ state: 'running', modelId: QWEN }));
    mockModelsList.mockResolvedValue(listOf(COMPLETE_MODELS));
    const { unmount } = render(<MlxEngineView />);
    await waitFor(() => {
      expect(screen.getByRole('button', { name: /Mounted/ })).toBeInTheDocument();
    });
    await userEvent.click(screen.getAllByRole('combobox')[0]);
    await userEvent.click(await screen.findByRole('option', { name: /Other-Model-4bit/ }));
    const switchButton = await screen.findByRole('button', { name: /Switch model/ });
    expect(switchButton).toBeEnabled();
    await userEvent.click(switchButton);
    await waitFor(() => {
      expect(mockMount).toHaveBeenCalledWith(OTHER_MODEL, undefined);
    });
    unmount();
  });

  it('mounting shows a disabled spinner button, never an actionable Mount', async () => {
    mockStatus.mockResolvedValue(statusOf({ state: 'mounting', modelId: QWEN }));
    const { unmount } = render(<MlxEngineView />);
    await waitFor(() => {
      expect(screen.getByRole('button', { name: /Mounting/ })).toBeInTheDocument();
    });
    expect(screen.getByRole('button', { name: /Mounting/ })).toBeDisabled();
    expect(screen.queryByRole('button', { name: /^Mount$/ })).not.toBeInTheDocument();
    unmount();
  });

  it('stopped still offers the plain Mount action', async () => {
    mockStatus.mockResolvedValue(statusOf({ state: 'stopped' }));
    const { unmount } = render(<MlxEngineView />);
    await waitFor(() => {
      expect(screen.getByRole('button', { name: /^Mount$/ })).toBeEnabled();
    });
    unmount();
  });

  it('an explicit user selection is never overridden when the engine reports a mounted model', async () => {
    mockStatus.mockResolvedValue(statusOf({ state: 'stopped' }));
    mockModelsList.mockResolvedValue(listOf(COMPLETE_MODELS));
    const { unmount } = render(<MlxEngineView />);
    await waitFor(() => {
      expect(screen.getAllByRole('combobox').length).toBeGreaterThan(0);
    });
    await userEvent.click(screen.getAllByRole('combobox')[0]);
    await userEvent.click(await screen.findByRole('option', { name: /Other-Model-4bit/ }));
    expect(screen.getByText(OTHER_MODEL)).toBeInTheDocument();

    mockStatus.mockResolvedValue(statusOf({ state: 'running', modelId: QWEN }));
    forceStatusRefresh();
    await waitFor(() => {
      expect(screen.getAllByTestId('mlx-state-badge')[0]).toHaveTextContent('running');
    });
    // The user's pick survives: the button offers Switch model, not the Mounted status.
    expect(screen.getByText(OTHER_MODEL)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /Switch model/ })).toBeEnabled();
    expect(screen.queryByRole('button', { name: /Mounted/ })).not.toBeInTheDocument();
    unmount();
  });
});

// ---------------------------------------------------------------------------
// The sampling tab — PER MODEL now: a picker selects the profile being edited,
// drafts live in the shell keyed by model id (so two models keep separate
// unsaved edits), and Save writes modelProfiles — never the legacy flat fields.
// ---------------------------------------------------------------------------

const ALL_SAMPLING_LABELS = [
  'Temperature',
  'Top P',
  'Top K',
  'Min P',
  'Repetition penalty',
  'Presence penalty',
  'Frequency penalty',
  'Context limit (tokens)',
];

async function openSamplingTab() {
  await waitFor(() => {
    expect(screen.getByRole('radio', { name: 'Sampling' })).toBeInTheDocument();
  });
  await userEvent.click(screen.getByRole('radio', { name: 'Sampling' }));
}

describe('MlxEngineView sampling tab', () => {
  it('renders all fields for the default-selected model and names the mounted model', async () => {
    mockStatus.mockResolvedValue(statusOf({ state: 'running', modelId: QWEN }));
    const { unmount } = render(<MlxEngineView />);
    await openSamplingTab();
    await waitFor(() => {
      expect(screen.getByLabelText('Temperature')).toBeInTheDocument();
    });
    for (const label of ALL_SAMPLING_LABELS) {
      expect(screen.getByLabelText(label)).toBeInTheDocument();
    }
    expect(screen.getByRole('button', { name: 'Save' })).toBeInTheDocument();
    // The honest caption: per-model profiles, per-request values win.
    expect(screen.getByText(/per-request values sent by goose override them/)).toBeInTheDocument();
    expect(screen.getByText(/Profiles apply at\s+mount, per model/)).toBeInTheDocument();
    expect(screen.getByText('Currently mounted:')).toBeInTheDocument();
    // The saved profile for the mounted model prefills: temperature 0 is the text "0".
    expect(screen.getByLabelText('Temperature')).toHaveValue(0);
    unmount();
  });

  it('with nothing mounted the tab says so and defaults to the last-mounted model', async () => {
    const { unmount } = render(<MlxEngineView />);
    await openSamplingTab();
    await waitFor(() => {
      expect(screen.getByText('no model mounted')).toBeInTheDocument();
    });
    expect(screen.queryByText('Currently mounted:')).not.toBeInTheDocument();
    // settings.modelId is the fallback selection — its fields render.
    await waitFor(() => {
      expect(screen.getByLabelText('Temperature')).toHaveValue(0);
    });
    unmount();
  });

  it('the restart-required banner renders on the sampling tab and Remount works from there', async () => {
    mockStatus.mockResolvedValue(
      statusOf({ state: 'running', modelId: QWEN, restartRequired: true })
    );
    const { unmount } = render(<MlxEngineView />);
    await openSamplingTab();
    await waitFor(() => {
      expect(screen.getByText('Settings changed — remount to apply.')).toBeInTheDocument();
    });
    await userEvent.click(screen.getByRole('button', { name: /Remount/ }));
    await waitFor(() => {
      expect(mockUnmount).toHaveBeenCalledTimes(1);
      expect(mockMount).toHaveBeenCalledWith(QWEN, undefined);
    });
    unmount();
  });

  it('unsaved sampling edits survive switching tabs', async () => {
    const { unmount } = render(<MlxEngineView />);
    await openSamplingTab();
    await waitFor(() => {
      expect(screen.getByLabelText('Presence penalty')).toBeInTheDocument();
    });
    await userEvent.type(screen.getByLabelText('Presence penalty'), '0.5');
    expect(screen.getByLabelText('Presence penalty')).toHaveValue(0.5);
    expect(screen.getByText('unsaved')).toBeInTheDocument();

    await userEvent.click(screen.getByRole('radio', { name: /Models/ }));
    await waitFor(() => {
      expect(screen.getByLabelText('Search Hugging Face')).toBeInTheDocument();
    });
    await userEvent.click(screen.getByRole('radio', { name: 'Sampling' }));
    await waitFor(() => {
      expect(screen.getByLabelText('Presence penalty')).toHaveValue(0.5);
    });
    expect(screen.getByText('unsaved')).toBeInTheDocument();
    unmount();
  });

  it('two models keep separate drafts, and Save writes ONLY the selected profile — never legacy flat fields', async () => {
    const { unmount } = render(<MlxEngineView />);
    await openSamplingTab();
    await waitFor(() => {
      expect(screen.getByLabelText('Temperature')).toHaveValue(0);
    });

    // Edit the default-selected model (QWEN: saved temperature 0 -> 0.7).
    await userEvent.clear(screen.getByLabelText('Temperature'));
    await userEvent.type(screen.getByLabelText('Temperature'), '0.7');
    expect(screen.getByText('unsaved')).toBeInTheDocument();

    // Switch to the second model — a clean slate, NOT the first model's draft.
    await userEvent.click(screen.getByRole('combobox', { name: 'Sampling model' }));
    await userEvent.click(await screen.findByRole('option', { name: /Half-Model-8bit/ }));
    await waitFor(() => {
      expect(screen.getByLabelText('Temperature')).toHaveValue(null);
    });
    expect(screen.queryByText('unsaved')).not.toBeInTheDocument();
    await userEvent.type(screen.getByLabelText('Temperature'), '0.2');
    expect(screen.getByText('unsaved')).toBeInTheDocument();

    // Back to the first model: its own draft (0.7) is intact.
    await userEvent.click(screen.getByRole('combobox', { name: 'Sampling model' }));
    await userEvent.click(await screen.findByRole('option', { name: /Qwen3-30B/ }));
    await waitFor(() => {
      expect(screen.getByLabelText('Temperature')).toHaveValue(0.7);
    });

    // Save writes the whole settings object with ONLY this model's profile rebuilt.
    await userEvent.click(screen.getByRole('button', { name: 'Save' }));
    await waitFor(() => {
      expect(mockSettingsUpdate).toHaveBeenCalledTimes(1);
    });
    const payload = mockSettingsUpdate.mock.calls[0][0] as MlxEngineSettings;
    expect(payload.modelProfiles[QWEN]).toEqual({ temperature: 0.7, topK: 40 });
    expect('temperature' in payload).toBe(false);
    expect('topK' in payload).toBe(false);
    expect(payload.servedModelName).toBe('leanzero-mlx');

    // The OTHER model's unsaved draft survived the save.
    await userEvent.click(screen.getByRole('combobox', { name: 'Sampling model' }));
    await userEvent.click(await screen.findByRole('option', { name: /Half-Model-8bit/ }));
    await waitFor(() => {
      expect(screen.getByLabelText('Temperature')).toHaveValue(0.2);
    });
    expect(screen.getByText('unsaved')).toBeInTheDocument();
    unmount();
  });

  it('the per-model Sampling affordance on the Downloaded rows preselects that row model', async () => {
    mockModelsList.mockResolvedValue(listOf(COMPLETE_MODELS));
    const { unmount } = render(<MlxEngineView />);
    await waitFor(() => {
      expect(screen.getByText('Models')).toBeInTheDocument();
    });
    await userEvent.click(screen.getByRole('radio', { name: /Models/ }));
    await userEvent.click(screen.getByRole('radio', { name: /^Downloaded/ }));
    await waitFor(() => {
      expect(screen.getByLabelText(`Sampling for ${OTHER_MODEL}`)).toBeInTheDocument();
    });
    await userEvent.click(screen.getByLabelText(`Sampling for ${OTHER_MODEL}`));
    await waitFor(() => {
      expect(
        screen.getByText(/per-request values sent by goose override them/)
      ).toBeInTheDocument();
    });
    // The picker holds the row's model, not the default.
    expect(screen.getByText(OTHER_MODEL)).toBeInTheDocument();
    expect(screen.getByLabelText('Presence penalty')).toBeInTheDocument();
    unmount();
  });
});

// ---------------------------------------------------------------------------
// The models tab: folder truth, list truth (header counts what the body shows),
// partial downloads flagged, and the paginated Hugging Face BROWSER.
// ---------------------------------------------------------------------------

const HIT_A: MlxBrowseHit = {
  id: 'mlx-community/New-Model-4bit',
  author: 'mlx-community',
  downloads: 12800,
  likes: 42,
  createdAt: '2026-08-20T10:00:00Z',
  tags: ['mlx', '4-bit'],
  quant: '4-bit',
  arch: 'qwen3',
  sizeBytesEstimate: 3.2 * GB,
};
const HIT_B: MlxBrowseHit = {
  id: 'lmstudio-community/Second-Model-8bit',
  author: 'lmstudio-community',
  downloads: 900,
  likes: 7,
  createdAt: '2026-08-28T10:00:00Z',
  tags: ['mlx', '8-bit'],
  quant: '8-bit',
  arch: 'llama',
};
const HIT_C: MlxBrowseHit = {
  id: 'mlx-community/Filtered-Model-4bit',
  author: 'mlx-community',
  downloads: 300,
  likes: 3,
  createdAt: '2026-08-29T10:00:00Z',
  tags: ['mlx', '4-bit'],
  quant: '4-bit',
  arch: 'qwen3',
};

async function openModelsTab() {
  await waitFor(() => {
    expect(screen.getByRole('radio', { name: /Models/ })).toBeInTheDocument();
  });
  await userEvent.click(screen.getByRole('radio', { name: /Models/ }));
}

/** The owner's split: local content lives on the second-level Downloaded tab. */
async function openDownloadedTab() {
  await openModelsTab();
  await userEvent.click(screen.getByRole('radio', { name: /^Downloaded/ }));
}

describe('MlxEngineView models tab', () => {
  it('splits into [Hugging Face | Downloaded]: the browser on one, the local library on the other', async () => {
    const { unmount } = render(<MlxEngineView />);
    await openModelsTab();
    // Hugging Face is the default pane: the browser is here, the local library is not.
    await waitFor(() => {
      expect(screen.getByLabelText('Search Hugging Face')).toBeInTheDocument();
    });
    expect(screen.queryByText('/Users/x/mlx-models')).not.toBeInTheDocument();
    expect(screen.queryByText(HALF)).not.toBeInTheDocument();

    await userEvent.click(screen.getByRole('radio', { name: /^Downloaded/ }));
    await waitFor(() => {
      expect(screen.getByText('/Users/x/mlx-models')).toBeInTheDocument();
    });
    expect(screen.getByTestId('mlx-disk-bar')).toBeInTheDocument();
    expect(screen.getByText(HALF)).toBeInTheDocument();
    expect(screen.queryByLabelText('Search Hugging Face')).not.toBeInTheDocument();
    unmount();
  });

  it('lists local models with sizes, flags incomplete downloads, counts what it shows', async () => {
    const { unmount } = render(<MlxEngineView />);
    await openDownloadedTab();
    await waitFor(() => {
      expect(screen.getByText('/Users/x/mlx-models')).toBeInTheDocument();
    });
    expect(screen.getByText(HALF)).toBeInTheDocument();
    expect(screen.getByText('incomplete — missing 2 file(s)')).toBeInTheDocument();
    expect(screen.getByText('17 GB')).toBeInTheDocument();
    // The tab chip and the section chip both say 2, and the body shows exactly 2 rows.
    expect(screen.getAllByText('2').length).toBeGreaterThanOrEqual(2);
    expect(screen.getAllByLabelText(/^Delete /).length).toBe(2);
    unmount();
  });

  it('an incomplete model offers Resume (works for untracked residue) and its progress row', async () => {
    const { unmount } = render(<MlxEngineView />);
    await openDownloadedTab();
    await waitFor(() => {
      expect(screen.getByText('incomplete — missing 2 file(s)')).toBeInTheDocument();
    });
    // Incomplete rows trade the Sampling affordance for Resume; Delete stays.
    expect(screen.queryByLabelText(`Sampling for ${HALF}`)).not.toBeInTheDocument();
    mockDownloadProgress.mockResolvedValue({
      state: 'downloading',
      totalBytes: 6 * GB,
      downloadedBytes: 3 * GB,
      currentFile: 'model-00002-of-00002.safetensors',
    });
    await userEvent.click(screen.getByLabelText(`Resume ${HALF}`));
    await waitFor(() => {
      expect(mockDownloadResume).toHaveBeenCalledWith(HALF, undefined);
      expect(screen.getByTestId(`mlx-download-${HALF}`)).toBeInTheDocument();
    });
    expect(screen.getByText('3.00 GB / 6.00 GB')).toBeInTheDocument();
    unmount();
  });

  it('the disk bar shows the models volume free space from the modelsList response', async () => {
    const { unmount } = render(<MlxEngineView />);
    await openDownloadedTab();
    await waitFor(() => {
      expect(screen.getByTestId('mlx-disk-bar')).toBeInTheDocument();
    });
    expect(screen.getByText('250 GB free')).toBeInTheDocument();
    expect(screen.getByText('of 500 GB')).toBeInTheDocument();
    unmount();
  });

  it('browses on open (top downloads, no cursor) and a row Download starts a tracked download', async () => {
    mockBrowse.mockResolvedValue({ hits: [HIT_A] });
    const { unmount } = render(<MlxEngineView />);
    await openModelsTab();
    await waitFor(() => {
      expect(screen.getByText(HIT_A.id)).toBeInTheDocument();
    });
    expect(mockBrowse).toHaveBeenCalledWith(
      expect.objectContaining({ sort: 'downloads', limit: 20 }),
      undefined
    );
    expect(mockBrowse.mock.calls[0][0].cursor).toBeUndefined();
    // Downloads and likes are plain aligned figures now — no arrow, no heart glyph.
    expect(screen.getByText('12.8K')).toBeInTheDocument();
    expect(screen.getByText('42')).toBeInTheDocument();
    expect(screen.getByText('4-bit')).toBeInTheDocument();
    expect(screen.getByText('qwen3')).toBeInTheDocument();
    // The size ESTIMATE renders with its ~ marker; a hit without one shows no size at all.
    expect(screen.getByText('~3.2 GB')).toBeInTheDocument();

    await userEvent.click(screen.getByLabelText(`Download ${HIT_A.id}`));
    await waitFor(() => {
      expect(mockDownload).toHaveBeenCalledWith(HIT_A.id, undefined);
      expect(screen.getByTestId(`mlx-download-${HIT_A.id}`)).toBeInTheDocument();
    });
    unmount();
  });

  it('Load more appends the next page via the cursor; a filter change resets pagination', async () => {
    mockBrowse.mockImplementation(async (params: { quant?: string; cursor?: string }) => {
      if (params.quant === '4-bit') return { hits: [HIT_C] };
      if (params.cursor === 'CUR1') return { hits: [HIT_B] };
      return { hits: [HIT_A], nextCursor: 'CUR1' };
    });
    const { unmount } = render(<MlxEngineView />);
    await openModelsTab();
    await waitFor(() => {
      expect(screen.getByText(HIT_A.id)).toBeInTheDocument();
    });

    // Page 2 appends — page 1 rows stay, header count follows the body.
    await userEvent.click(screen.getByRole('button', { name: 'Load more' }));
    await waitFor(() => {
      expect(screen.getByText(HIT_B.id)).toBeInTheDocument();
    });
    expect(screen.getByText(HIT_A.id)).toBeInTheDocument();
    // The panel header counts what the table shows: two rows loaded.
    expect(screen.getByTestId('lz-section-count')).toHaveTextContent('2');
    const loadMoreCall = mockBrowse.mock.calls.find((c) => c[0].cursor === 'CUR1');
    expect(loadMoreCall).toBeTruthy();

    // Changing the quant filter refetches page 1 (no cursor) and REPLACES the list.
    await userEvent.click(screen.getByLabelText('Quant filter'));
    await userEvent.click(await screen.findByRole('option', { name: '4-bit' }));
    await waitFor(() => {
      expect(screen.getByText(HIT_C.id)).toBeInTheDocument();
    });
    expect(screen.queryByText(HIT_A.id)).not.toBeInTheDocument();
    expect(screen.queryByText(HIT_B.id)).not.toBeInTheDocument();
    const quantCall = mockBrowse.mock.calls.find((c) => c[0].quant === '4-bit');
    expect(quantCall).toBeTruthy();
    expect(quantCall?.[0].cursor).toBeUndefined();
    unmount();
  });

  it('Latest mode passes sort newest and shows the created date prominently', async () => {
    mockBrowse.mockImplementation(async (params: { sort: string }) => {
      if (params.sort === 'newest') return { hits: [HIT_B] };
      return { hits: [HIT_A] };
    });
    const { unmount } = render(<MlxEngineView />);
    await openModelsTab();
    await waitFor(() => {
      expect(screen.getByText(HIT_A.id)).toBeInTheDocument();
    });
    await userEvent.click(screen.getByRole('radio', { name: 'Latest' }));
    await waitFor(() => {
      expect(screen.getByText(HIT_B.id)).toBeInTheDocument();
    });
    expect(mockBrowse).toHaveBeenCalledWith(expect.objectContaining({ sort: 'newest' }), undefined);
    // createdAt 2026-08-28 renders as a date in the row.
    expect(screen.getByText(/Aug 28, 2026/)).toBeInTheDocument();
    unmount();
  });

  it('a browse failure is loud and an empty result is honest', async () => {
    mockBrowse.mockRejectedValue(new Error('HuggingFace model browse returned HTTP 429'));
    const { unmount } = render(<MlxEngineView />);
    await openModelsTab();
    await waitFor(() => {
      expect(screen.getByText('HuggingFace model browse returned HTTP 429')).toBeInTheDocument();
    });
    expect(screen.getByText('Browse failed')).toBeInTheDocument();

    mockBrowse.mockResolvedValue({ hits: [] });
    // Committing a search refetches and lands on the honest empty state.
    await userEvent.type(screen.getByLabelText('Search Hugging Face'), 'nothing-matches{Enter}');
    await waitFor(() => {
      expect(screen.getByText('No MLX models match these filters.')).toBeInTheDocument();
    });
    expect(mockBrowse.mock.calls.some((c) => c[0].query === 'nothing-matches')).toBe(true);
    unmount();
  });

  it('deleting a model asks through the custom dialog, never window.confirm', async () => {
    mockModelDelete.mockResolvedValue(undefined);
    const confirmSpy = vi.spyOn(window, 'confirm');
    const { unmount } = render(<MlxEngineView />);
    await openDownloadedTab();
    await waitFor(() => {
      expect(screen.getByLabelText(`Delete ${HALF}`)).toBeInTheDocument();
    });
    await userEvent.click(screen.getByLabelText(`Delete ${HALF}`));
    // The custom confirmation dialog appears with the model named.
    await waitFor(() => {
      expect(screen.getByText(/Delete mlx-community\/Half-Model-8bit/)).toBeInTheDocument();
    });
    await userEvent.click(screen.getByRole('button', { name: 'Delete' }));
    await waitFor(() => {
      expect(mockModelDelete).toHaveBeenCalledWith(HALF, undefined);
    });
    expect(confirmSpy).not.toHaveBeenCalled();
    unmount();
  });

  it('a running download is visible from BOTH sub-tabs (Active downloads carries the orphans)', async () => {
    mockBrowse.mockResolvedValue({ hits: [HIT_A] });
    mockDownloadProgress.mockResolvedValue({
      state: 'downloading',
      totalBytes: 4 * GB,
      downloadedBytes: 1 * GB,
    });
    const { unmount } = render(<MlxEngineView />);
    await openModelsTab();
    await waitFor(() => {
      expect(screen.getByText(HIT_A.id)).toBeInTheDocument();
    });
    // Started from the browser row: inline on the Hugging Face pane.
    await userEvent.click(screen.getByLabelText(`Download ${HIT_A.id}`));
    await waitFor(() => {
      expect(screen.getByTestId(`mlx-download-${HIT_A.id}`)).toBeInTheDocument();
    });

    // The Downloaded pane shows the SAME download — HIT_A is not local, so the
    // Active downloads card carries it. One row per repo per pane, never two.
    await userEvent.click(screen.getByRole('radio', { name: /^Downloaded/ }));
    await waitFor(() => {
      expect(screen.getByText('Active downloads')).toBeInTheDocument();
    });
    expect(screen.getAllByTestId(`mlx-download-${HIT_A.id}`)).toHaveLength(1);

    // And back on Hugging Face it is inline again, still exactly once.
    await userEvent.click(screen.getByRole('radio', { name: 'Hugging Face' }));
    await waitFor(() => {
      expect(screen.getAllByTestId(`mlx-download-${HIT_A.id}`)).toHaveLength(1);
    });
    expect(screen.queryByText('Active downloads')).not.toBeInTheDocument();
    unmount();
  });

  it('browser state (query, hits) survives a sub-tab round trip without refetching', async () => {
    mockBrowse.mockImplementation(async (params: { query?: string }) => {
      if (params.query === 'qwen') return { hits: [HIT_C] };
      return { hits: [HIT_A] };
    });
    const { unmount } = render(<MlxEngineView />);
    await openModelsTab();
    await userEvent.type(screen.getByLabelText('Search Hugging Face'), 'qwen{Enter}');
    await waitFor(() => {
      expect(screen.getByText(HIT_C.id)).toBeInTheDocument();
    });
    const browseCalls = mockBrowse.mock.calls.length;

    await userEvent.click(screen.getByRole('radio', { name: /^Downloaded/ }));
    await waitFor(() => {
      expect(screen.getByText('/Users/x/mlx-models')).toBeInTheDocument();
    });
    await userEvent.click(screen.getByRole('radio', { name: 'Hugging Face' }));

    // The applied query, its results and the input text are all still there — no new fetch.
    expect(await screen.findByText(HIT_C.id)).toBeInTheDocument();
    expect(screen.getByLabelText('Search Hugging Face')).toHaveValue('qwen');
    expect(mockBrowse.mock.calls.length).toBe(browseCalls);
    unmount();
  });
});

// ---------------------------------------------------------------------------
// Type-ahead filter comboboxes fed by the backend's LIVE vocabularies: typing
// filters client-side with frequency order preserved, selection applies the
// server-side browse filter, free text passes through as-is, and a stale/failed
// vocabulary says so instead of pretending.
// ---------------------------------------------------------------------------

describe('MlxEngineView browse filter comboboxes', () => {
  it('typing in the Arch combobox filters the vocabulary, frequency order preserved, and selecting applies server-side', async () => {
    mockBrowse.mockResolvedValue({ hits: [HIT_A] });
    const { unmount } = render(<MlxEngineView />);
    await openModelsTab();
    await waitFor(() => {
      expect(screen.getByText(HIT_A.id)).toBeInTheDocument();
    });
    expect(mockBrowseFilters).toHaveBeenCalledTimes(1);

    await userEvent.click(screen.getByLabelText('Arch filter'));
    const input = await screen.findByLabelText('Search Arch');
    // Escape closes without applying anything…
    await userEvent.keyboard('{Escape}');
    expect(screen.queryByLabelText('Search Arch')).not.toBeInTheDocument();

    // …reopen and type: the vocabulary narrows to the qwen3 family, backend order kept.
    await userEvent.click(screen.getByLabelText('Arch filter'));
    await userEvent.type(await screen.findByLabelText('Search Arch'), 'qwen3');
    const options = screen.getAllByRole('option');
    expect(options.map((o) => o.textContent)).toEqual(['qwen3_5', 'qwen3', 'qwen3_moe']);

    await userEvent.click(screen.getByRole('option', { name: 'qwen3' }));
    await waitFor(() => {
      expect(mockBrowse.mock.calls.some((c) => c[0].arch === 'qwen3')).toBe(true);
    });
    // The applied filter renders as a solid chip carrying its value.
    expect(screen.getByLabelText('Arch filter')).toHaveTextContent('Arch: qwen3');
    void input;
    unmount();
  });

  it('free text applies as-is, a malformed value surfaces the backend error, and the chip ✕ clears', async () => {
    mockBrowse.mockImplementation(async (params: { quant?: string }) => {
      if (params.quant === 'q4_k_m')
        throw new Error("quant 'q4_k_m' is not a HuggingFace MLX quant tag");
      return { hits: [HIT_A] };
    });
    const { unmount } = render(<MlxEngineView />);
    await openModelsTab();
    await waitFor(() => {
      expect(screen.getByText(HIT_A.id)).toBeInTheDocument();
    });

    await userEvent.click(screen.getByLabelText('Quant filter'));
    await userEvent.type(await screen.findByLabelText('Search Quant'), 'q4_k_m');
    // No vocabulary match — the free-text row is offered; Enter applies it as-is.
    expect(screen.getByRole('option', { name: /q4_k_m/ })).toBeInTheDocument();
    await userEvent.keyboard('{Enter}');
    await waitFor(() => {
      expect(
        screen.getByText("quant 'q4_k_m' is not a HuggingFace MLX quant tag")
      ).toBeInTheDocument();
    });
    expect(screen.getByLabelText('Quant filter')).toHaveTextContent('Quant: q4_k_m');

    // ✕ clears the filter and the browse recovers.
    await userEvent.click(screen.getByLabelText('Clear Quant filter'));
    await waitFor(() => {
      expect(screen.getByText(HIT_A.id)).toBeInTheDocument();
    });
    expect(
      screen.queryByText("quant 'q4_k_m' is not a HuggingFace MLX quant tag")
    ).not.toBeInTheDocument();
    unmount();
  });

  it('a stale vocabulary (refreshError) and a failed vocabulary load both say so', async () => {
    mockBrowseFilters.mockResolvedValue({ ...FILTERS, refreshError: 'HTTP 500 from HF' });
    const first = render(<MlxEngineView />);
    await openModelsTab();
    await waitFor(() => {
      expect(screen.getByText('vocabulary may be stale')).toBeInTheDocument();
    });
    first.unmount();
    cleanup();

    mockBrowseFilters.mockRejectedValue(new Error('crawl refused'));
    const second = render(<MlxEngineView />);
    await openModelsTab();
    await waitFor(() => {
      expect(
        screen.getByText('filter vocabulary unavailable — free text still works')
      ).toBeInTheDocument();
    });
    second.unmount();
  });
});

// ---------------------------------------------------------------------------
// The fullscreen model card modal: real repo facts, EXACT size from the file
// tree, README through the chat markdown renderer, truncation twin, Esc/✕.
// ---------------------------------------------------------------------------

const CARD = {
  readmeMarkdown: '# New Model readme heading\n\nBody text of the model card.',
  readmeTruncated: true,
  files: [
    { path: 'config.json', sizeBytes: 1200 },
    { path: 'model-00001-of-00002.safetensors', sizeBytes: 5 * GB },
  ],
  totalBytes: 5 * GB + 1200,
  tags: ['mlx', '4-bit'],
  downloads: 12800,
  likes: 42,
  license: 'apache-2.0',
  createdAt: '2026-08-20T10:00:00Z',
  lastModified: '2026-08-25T10:00:00Z',
};

describe('MlxEngineView model card modal', () => {
  it('clicking a browse row opens the fullscreen card with facts, files, markdown, and the truncation notice', async () => {
    mockBrowse.mockResolvedValue({ hits: [HIT_A] });
    mockModelCard.mockResolvedValue(CARD);
    const { unmount } = render(<MlxEngineView />);
    await openModelsTab();
    await waitFor(() => {
      expect(screen.getByText(HIT_A.id)).toBeInTheDocument();
    });
    await userEvent.click(screen.getByLabelText(`Open model card for ${HIT_A.id}`));
    await waitFor(() => {
      expect(screen.getByTestId('mlx-model-card-modal')).toBeInTheDocument();
    });
    expect(mockModelCard).toHaveBeenCalledWith(HIT_A.id, undefined);
    await waitFor(() => {
      expect(screen.getByText('apache-2.0')).toBeInTheDocument();
    });
    // File listing in mono with sizes, plus the EXACT total (not the row's ~estimate).
    expect(screen.getByText('model-00001-of-00002.safetensors')).toBeInTheDocument();
    expect(screen.getByText('5.00 GB total')).toBeInTheDocument();
    // README rendered through the app's markdown renderer, not dumped as text.
    expect(screen.getByRole('heading', { name: 'New Model readme heading' })).toBeInTheDocument();
    // Truncation twin with the outbound link.
    expect(screen.getByText(/read the full page on huggingface\.co/)).toBeInTheDocument();
    // ✕ closes.
    await userEvent.click(screen.getByLabelText('Close model card'));
    expect(screen.queryByTestId('mlx-model-card-modal')).not.toBeInTheDocument();
    unmount();
  });

  it('row action buttons do NOT open the card; Esc closes it; an absent README is honest', async () => {
    mockBrowse.mockResolvedValue({ hits: [HIT_A] });
    const { unmount } = render(<MlxEngineView />);
    await openModelsTab();
    await waitFor(() => {
      expect(screen.getByText(HIT_A.id)).toBeInTheDocument();
    });
    // Download is a row ACTION — it must not open the modal.
    await userEvent.click(screen.getByLabelText(`Download ${HIT_A.id}`));
    await waitFor(() => {
      expect(screen.getByTestId(`mlx-download-${HIT_A.id}`)).toBeInTheDocument();
    });
    expect(screen.queryByTestId('mlx-model-card-modal')).not.toBeInTheDocument();

    await userEvent.click(screen.getByLabelText(`Open model card for ${HIT_A.id}`));
    await waitFor(() => {
      expect(screen.getByTestId('mlx-model-card-modal')).toBeInTheDocument();
    });
    // Default mock card has no readmeMarkdown — absence renders as absence.
    await waitFor(() => {
      expect(screen.getByText('This repo has no README.')).toBeInTheDocument();
    });
    await userEvent.keyboard('{Escape}');
    await waitFor(() => {
      expect(screen.queryByTestId('mlx-model-card-modal')).not.toBeInTheDocument();
    });
    unmount();
  });
});

// ---------------------------------------------------------------------------
// Download lifecycle: pause → paused chip + Resume; resume continues (and its
// restarted-from-zero twin renders); cancel DELETES on disk so the row
// disappears and the local list refreshes; tracking lives in the view shell so
// tab switches keep the rows live and the poll running.
// ---------------------------------------------------------------------------

describe('MlxEngineView download lifecycle', () => {
  it('pause flips to a paused chip with Resume; resume continues and reports restarted files', async () => {
    mockBrowse.mockResolvedValue({ hits: [HIT_A] });
    let state: 'queued' | 'paused' | 'downloading' = 'queued';
    mockDownloadPause.mockImplementation(async () => {
      state = 'paused';
    });
    mockDownloadResume.mockImplementation(async () => {
      state = 'downloading';
    });
    mockDownloadProgress.mockImplementation(async () => ({
      state,
      totalBytes: 4 * GB,
      downloadedBytes: 1 * GB,
      ...(state === 'downloading' ? { restartedFiles: ['model-00001-of-00002.safetensors'] } : {}),
    }));

    const { unmount } = render(<MlxEngineView />);
    await openModelsTab();
    await waitFor(() => {
      expect(screen.getByText(HIT_A.id)).toBeInTheDocument();
    });
    await userEvent.click(screen.getByLabelText(`Download ${HIT_A.id}`));
    await waitFor(() => {
      expect(screen.getByLabelText(`Pause ${HIT_A.id}`)).toBeInTheDocument();
    });

    await userEvent.click(screen.getByLabelText(`Pause ${HIT_A.id}`));
    await waitFor(() => {
      expect(mockDownloadPause).toHaveBeenCalledWith(HIT_A.id, undefined);
      expect(screen.getByText('paused')).toBeInTheDocument();
    });
    expect(screen.queryByLabelText(`Pause ${HIT_A.id}`)).not.toBeInTheDocument();

    await userEvent.click(screen.getByLabelText(`Resume ${HIT_A.id}`));
    await waitFor(() => {
      expect(mockDownloadResume).toHaveBeenCalledWith(HIT_A.id, undefined);
      expect(screen.getByText('downloading')).toBeInTheDocument();
    });
    // The restarted-from-zero twin is visible, names in the tooltip.
    const restarted = screen.getByText('restarted from zero: 1 file(s)');
    expect(restarted).toHaveAttribute('title', 'model-00001-of-00002.safetensors');
    unmount();
  });

  it('a cancelled download disappears and the local models list refreshes (the dir is gone)', async () => {
    mockBrowse.mockResolvedValue({ hits: [HIT_A] });
    let cancelled = false;
    mockDownloadCancel.mockImplementation(async () => {
      cancelled = true;
    });
    mockDownloadProgress.mockImplementation(async () =>
      cancelled
        ? { state: 'cancelled', totalBytes: 0, downloadedBytes: 0 }
        : { state: 'queued', totalBytes: 0, downloadedBytes: 0 }
    );

    const { unmount } = render(<MlxEngineView />);
    await openModelsTab();
    await waitFor(() => {
      expect(screen.getByText(HIT_A.id)).toBeInTheDocument();
    });
    await userEvent.click(screen.getByLabelText(`Download ${HIT_A.id}`));
    await waitFor(() => {
      expect(screen.getByLabelText(`Cancel ${HIT_A.id}`)).toBeInTheDocument();
    });
    const listCallsBefore = mockModelsList.mock.calls.length;

    await userEvent.click(screen.getByLabelText(`Cancel ${HIT_A.id}`));
    await waitFor(() => {
      expect(mockDownloadCancel).toHaveBeenCalledWith(HIT_A.id, undefined);
      expect(screen.queryByTestId(`mlx-download-${HIT_A.id}`)).not.toBeInTheDocument();
    });
    // The local list refreshed — the backend deleted the partial repo dir.
    expect(mockModelsList.mock.calls.length).toBeGreaterThan(listCallsBefore);
    // The plain Download action returns for the row.
    expect(screen.getByLabelText(`Download ${HIT_A.id}`)).toBeInTheDocument();
    unmount();
  });

  it('deleting a model clears its finished download row so Download comes back honest', async () => {
    mockDownloadProgress.mockResolvedValue({
      state: 'done',
      totalBytes: 3 * GB,
      downloadedBytes: 3 * GB,
    });
    const { unmount } = render(<MlxEngineView />);
    await openDownloadedTab();
    await waitFor(() => {
      expect(screen.getByLabelText(`Resume ${HALF}`)).toBeInTheDocument();
    });
    await userEvent.click(screen.getByLabelText(`Resume ${HALF}`));
    await waitFor(() => {
      expect(screen.getByText('done')).toBeInTheDocument();
    });

    await userEvent.click(screen.getByLabelText(`Delete ${HALF}`));
    await waitFor(() => {
      expect(screen.getByText(/Delete mlx-community\/Half-Model-8bit/)).toBeInTheDocument();
    });
    await userEvent.click(screen.getByRole('button', { name: 'Delete' }));
    await waitFor(() => {
      expect(mockModelDelete).toHaveBeenCalledWith(HALF, undefined);
      // Caught live: without this, the deleted model's row kept saying "done" and the
      // Download action never returned.
      expect(screen.queryByTestId(`mlx-download-${HALF}`)).not.toBeInTheDocument();
    });
    unmount();
  });

  it('switching tabs mid-download keeps the row live and the poll running', async () => {
    mockBrowse.mockResolvedValue({ hits: [HIT_A] });
    mockDownloadProgress.mockResolvedValue({
      state: 'downloading',
      totalBytes: 4 * GB,
      downloadedBytes: 1 * GB,
    });
    const { unmount } = render(<MlxEngineView />);
    await openModelsTab();
    await waitFor(() => {
      expect(screen.getByText(HIT_A.id)).toBeInTheDocument();
    });
    await userEvent.click(screen.getByLabelText(`Download ${HIT_A.id}`));
    await waitFor(() => {
      expect(screen.getByTestId(`mlx-download-${HIT_A.id}`)).toBeInTheDocument();
    });

    // Leave for the Engine tab: the rows unmount but the SHELL keeps polling.
    await userEvent.click(screen.getByRole('radio', { name: 'Engine' }));
    await waitFor(() => {
      expect(screen.queryByTestId(`mlx-download-${HIT_A.id}`)).not.toBeInTheDocument();
    });
    mockDownloadProgress.mockClear();
    await waitFor(() => expect(mockDownloadProgress).toHaveBeenCalledWith(HIT_A.id, undefined), {
      timeout: 3000,
    });

    // Back on the Models tab the row is still there with the last REAL bytes.
    await userEvent.click(screen.getByRole('radio', { name: /Models/ }));
    await waitFor(() => {
      expect(screen.getByTestId(`mlx-download-${HIT_A.id}`)).toBeInTheDocument();
    });
    expect(screen.getByText('1.00 GB / 4.00 GB')).toBeInTheDocument();
    unmount();
  });
});

// ---------------------------------------------------------------------------
// Device picker — manage models on ANY linked device. The picker is sourced from
// leanzeroLink/nodes and gated on the capability + a connected mesh; every op threads
// the selected node's id. The common case now (no worker deployed) is capability-present-
// but-not-connected OR capability-absent: no peers, no picker, byte-identical local view.
// ---------------------------------------------------------------------------

describe('MlxEngineView device picker (remote model management)', () => {
  async function selectPeer(nodeId = 'peer-workhorse') {
    await waitFor(() =>
      expect(screen.getByRole('combobox', { name: 'Manage models on device' })).toBeInTheDocument()
    );
    await userEvent.click(screen.getByRole('combobox', { name: 'Manage models on device' }));
    await userEvent.click(screen.getByTestId(`mlx-device-target-option-${nodeId}`));
  }

  it('capability absent → no picker, every op targets THIS device (nodeId undefined)', async () => {
    const { unmount } = render(<MlxEngineView />);
    await waitFor(() => expect(mockStatus).toHaveBeenCalled());
    expect(screen.queryByTestId('mlx-device-target')).not.toBeInTheDocument();
    expect(mockStatus).toHaveBeenCalledWith(undefined);
    expect(mockModelsList).toHaveBeenCalledWith(undefined);
    expect(mockLinkNodes).not.toHaveBeenCalled();
    unmount();
  });

  it('capability present but NOT connected → no picker, behaves exactly as today', async () => {
    mockFeatures.leanzeroLink = true;
    mockLinkStatus.mockResolvedValue({
      auth: { state: 'loggedIn', email: 'm@x.co' },
      nodeCount: 0,
    });
    const { unmount } = render(<MlxEngineView />);
    await waitFor(() => expect(mockLinkStatus).toHaveBeenCalled());
    await waitFor(() => expect(mockStatus).toHaveBeenCalledWith(undefined));
    expect(screen.queryByTestId('mlx-device-target')).not.toBeInTheDocument();
    unmount();
  });

  it('connected but zero peers → no picker (no clutter)', async () => {
    withMesh([]);
    const { unmount } = render(<MlxEngineView />);
    await waitFor(() => expect(mockLinkNodes).toHaveBeenCalled());
    await waitFor(() => expect(mockStatus).toHaveBeenCalledWith(undefined));
    expect(screen.queryByTestId('mlx-device-target')).not.toBeInTheDocument();
    unmount();
  });

  it('lists This device + each connected peer with an idle/busy chip', async () => {
    withMesh([
      peerNode({ hostname: 'workhorse', node_id: 'peer-workhorse' }),
      peerNode({
        hostname: 'studio',
        node_id: 'peer-studio',
        status: { type: 'Busy', session_id: 's1' },
      }),
    ]);
    const { unmount } = render(<MlxEngineView />);
    await waitFor(() =>
      expect(screen.getByRole('combobox', { name: 'Manage models on device' })).toBeInTheDocument()
    );
    await userEvent.click(screen.getByRole('combobox', { name: 'Manage models on device' }));
    expect(screen.getByTestId('mlx-device-target-option-self')).toHaveTextContent('This device');
    const wh = screen.getByTestId('mlx-device-target-option-peer-workhorse');
    expect(wh).toHaveTextContent('workhorse');
    expect(wh).toHaveTextContent('idle');
    const st = screen.getByTestId('mlx-device-target-option-peer-studio');
    expect(st).toHaveTextContent('studio');
    expect(st).toHaveTextContent('busy');
    unmount();
  });

  it('selecting a peer threads its nodeId into status, models and settings, and banners the device', async () => {
    withMesh([peerNode()]);
    const { unmount } = render(<MlxEngineView />);
    await selectPeer();
    await waitFor(() => {
      expect(mockModelsList).toHaveBeenCalledWith('peer-workhorse');
      expect(mockStatus).toHaveBeenCalledWith('peer-workhorse');
      expect(mockSettingsRead).toHaveBeenCalledWith('peer-workhorse');
    });
    expect(screen.getByText('Managing models on workhorse (remote)')).toBeInTheDocument();
    unmount();
  });

  it("a remote node's mount-gate BLOCK renders verbatim in the existing banner", async () => {
    const BLOCK = 'Not enough memory: model needs 22.0 GB, only 5.1 GB free';
    mockStatus.mockImplementation(async (nodeId?: string) =>
      nodeId === 'peer-workhorse'
        ? statusOf({ gateMessage: BLOCK, gateVerdict: 'block' })
        : statusOf({})
    );
    withMesh([peerNode()]);
    const { unmount } = render(<MlxEngineView />);
    await selectPeer();
    await waitFor(() => expect(screen.getByText(BLOCK)).toBeInTheDocument());
    expect(screen.getByText('Mount blocked')).toBeInTheDocument();
    unmount();
  });

  it('an unreachable peer surfaces its error verbatim in the existing banner', async () => {
    const ERR = 'not connected to the mesh';
    mockStatus.mockImplementation(async (nodeId?: string) => {
      if (nodeId === 'peer-workhorse') throw new Error(ERR);
      return statusOf({});
    });
    withMesh([peerNode()]);
    const { unmount } = render(<MlxEngineView />);
    await selectPeer();
    await waitFor(() => expect(screen.getByText(ERR)).toBeInTheDocument());
    expect(screen.getByText('Engine unreachable')).toBeInTheDocument();
    unmount();
  });

  it('deleting on a remote device names the device in the confirm dialog and targets it', async () => {
    mockModelDelete.mockResolvedValue(undefined);
    withMesh([peerNode()]);
    const { unmount } = render(<MlxEngineView />);
    await selectPeer();
    await waitFor(() =>
      expect(screen.getByText('Managing models on workhorse (remote)')).toBeInTheDocument()
    );
    await openDownloadedTab();
    await waitFor(() => expect(screen.getByLabelText(`Delete ${HALF}`)).toBeInTheDocument());
    await userEvent.click(screen.getByLabelText(`Delete ${HALF}`));
    // Scope to the dialog message — "on workhorse" also appears in the remote banner above.
    await waitFor(() =>
      expect(
        screen.getByText(/Delete mlx-community\/Half-Model-8bit.*on workhorse/)
      ).toBeInTheDocument()
    );
    await userEvent.click(screen.getByRole('button', { name: 'Delete' }));
    await waitFor(() => expect(mockModelDelete).toHaveBeenCalledWith(HALF, 'peer-workhorse'));
    unmount();
  });

  it('cancelling a remote download names the device in the confirm dialog and targets it', async () => {
    mockBrowse.mockResolvedValue({ hits: [HIT_A] });
    withMesh([peerNode()]);
    const { unmount } = render(<MlxEngineView />);
    await selectPeer();
    await openModelsTab();
    await waitFor(() => expect(screen.getByText(HIT_A.id)).toBeInTheDocument());
    await userEvent.click(screen.getByLabelText(`Download ${HIT_A.id}`));
    await waitFor(() => expect(screen.getByLabelText(`Cancel ${HIT_A.id}`)).toBeInTheDocument());
    await userEvent.click(screen.getByLabelText(`Cancel ${HIT_A.id}`));
    // A LOCAL cancel goes straight through; a remote one asks first, naming the device.
    // Scope to the dialog message — the remote banner also contains "on workhorse".
    await waitFor(() =>
      expect(screen.getByText(/Cancel the download of.*on workhorse/)).toBeInTheDocument()
    );
    await userEvent.click(screen.getByRole('button', { name: 'Cancel download' }));
    await waitFor(() =>
      expect(mockDownloadCancel).toHaveBeenCalledWith(HIT_A.id, 'peer-workhorse')
    );
    unmount();
  });
});

// ---------------------------------------------------------------------------
// The token doctrine on the Hugging Face browser (main.css `.local-edition`): ONE accent, the
// node ramp for node identity ONLY, metadata as aligned neutral columns. The hot-pink tab and
// the rainbow publisher chips were this view breaking that doctrine.
// ---------------------------------------------------------------------------

describe('MlxEngineView browser — one accent, neutral columns', () => {
  it('renders hits as aligned columns under a header row; the publisher is neutral text, not a hue', async () => {
    mockBrowse.mockResolvedValue({ hits: [HIT_A] });
    const { unmount } = render(<MlxEngineView />);
    await openModelsTab();
    await waitFor(() => {
      expect(screen.getByText(HIT_A.id)).toBeInTheDocument();
    });
    const table = screen.getByRole('table', { name: 'Hugging Face MLX models' });
    const header = table.querySelector('thead')!;
    for (const col of [
      'Model',
      'Publisher',
      'Quant',
      'Arch',
      'Size',
      'Downloads',
      'Likes',
      'Created',
    ]) {
      expect(header).toHaveTextContent(col);
    }
    const publisher = screen.getByText('mlx-community');
    expect(publisher.style.backgroundColor).toBe('');
    expect(publisher.style.color).toBe('');
    expect(publisher.className).toContain('text-lz-ink-3');
    expect(publisher.className).toContain('tnum');
    // Likes is a number — no heart, no arrow, no glyph.
    expect(screen.queryByText(/[♥↓]/)).not.toBeInTheDocument();
    // The node ramp never reaches this view: no class on the page names a node token.
    expect(document.querySelectorAll('[class*="lz-node-"]')).toHaveLength(0);
    expect(document.querySelectorAll('[style*="--color-node-"]')).toHaveLength(0);
    unmount();
  });

  it('the active sort segment and the row action are the single accent — never node-5 pink', async () => {
    mockBrowse.mockResolvedValue({ hits: [HIT_A] });
    const { unmount } = render(<MlxEngineView />);
    await openModelsTab();
    await waitFor(() => {
      expect(screen.getByText(HIT_A.id)).toBeInTheDocument();
    });
    const active = screen.getByRole('radio', { name: 'Top downloads' });
    expect(active).toHaveAttribute('aria-checked', 'true');
    expect(active.className).toContain('bg-lz-accent');
    const inactive = screen.getByRole('radio', { name: 'Latest' });
    expect(inactive.className).not.toContain('bg-lz-accent');
    expect(inactive.style.backgroundColor).toBe('');
    const download = screen.getByLabelText(`Download ${HIT_A.id}`);
    expect(download.className).toContain('bg-lz-accent');
    // Exactly one filled element per row: the action.
    const row = screen.getByLabelText(`Open model card for ${HIT_A.id}`).closest('tr')!;
    const filled = Array.from(row.querySelectorAll<HTMLElement>('[class*="bg-lz-"]')).filter((el) =>
      /(^|\s)bg-lz-(accent|ok|warn|err|stopped|node)/.test(el.className)
    );
    expect(filled).toHaveLength(1);
    expect(filled[0]).toBe(download);
    // Nothing on the page is hand-coloured.
    expect(
      Array.from(document.querySelectorAll<HTMLElement>('[style]')).filter(
        (el) => el.style.backgroundColor !== '' || el.style.color !== ''
      )
    ).toHaveLength(0);
    unmount();
  });
});

// ---------------------------------------------------------------------------
// LeanZero Studio: every tab rendered, every emitted class compiled through the
// real Tailwind pipeline, and the design bans (no rail, no faded tint, no native
// select) refused on the rendered tree — in the states a person actually sees.
// ---------------------------------------------------------------------------

describe('MlxEngineView — Studio clean on every tab', () => {
  /** lucide stamps its icon name on each <svg>; that is an identity, not a utility. */
  const utilities = () => allClasses(document.body).filter((c) => !c.startsWith('lucide'));
  const studioClean = () => assertStudioClean(document.body);

  it('engine tab: banners, the status KeyValue, the mount controls', async () => {
    mockStatus.mockResolvedValue(
      statusOf({
        state: 'running',
        modelId: QWEN,
        contextWindow: 131072,
        toolCallParser: 'qwen3',
        pid: 4242,
        baseUrl: 'http://127.0.0.1:9600/v1',
        gateVerdict: 'warn',
        gateMessage: 'memory headroom is thin',
        restartRequired: true,
        probeError: 'probe timed out after 3s',
      })
    );
    const { unmount } = render(<MlxEngineView />);
    await waitFor(() => {
      expect(screen.getByText('Memory pressure')).toBeInTheDocument();
    });
    await waitFor(() => {
      expect(screen.getByText('131,072')).toBeInTheDocument();
    });
    studioClean();
    expect(await missingUtilities(utilities())).toEqual([]);
    unmount();
  });

  it('models tab: the browser table with a live download, a filter menu open, then the library', async () => {
    mockBrowse.mockResolvedValue({ hits: [HIT_A, HIT_B], nextCursor: 'c2' });
    mockDownloadProgress.mockResolvedValue({
      state: 'downloading',
      totalBytes: 4 * GB,
      downloadedBytes: GB,
      currentFile: 'model.safetensors',
    });
    const { unmount } = render(<MlxEngineView />);
    await openModelsTab();
    await waitFor(() => {
      expect(screen.getByText(HIT_A.id)).toBeInTheDocument();
    });
    await userEvent.click(screen.getByLabelText(`Download ${HIT_A.id}`));
    await waitFor(() => {
      expect(screen.getByTestId(`mlx-download-${HIT_A.id}`)).toBeInTheDocument();
    });
    await userEvent.click(screen.getByLabelText('Arch filter'));
    await screen.findByLabelText('Search Arch');
    expect(screen.getByRole('button', { name: 'Load more' })).toBeInTheDocument();
    studioClean();
    expect(await missingUtilities(utilities())).toEqual([]);

    await userEvent.keyboard('{Escape}');
    await openDownloadedTab();
    await waitFor(() => {
      expect(screen.getByTestId('mlx-disk-bar')).toBeInTheDocument();
    });
    expect(screen.getByText('incomplete — missing 2 file(s)')).toBeInTheDocument();
    studioClean();
    expect(await missingUtilities(utilities())).toEqual([]);
    unmount();
  });

  it('sampling tab: the two-column form with a set and an unset field', async () => {
    const { unmount } = render(<MlxEngineView />);
    await openSamplingTab();
    await waitFor(() => {
      expect(screen.getByLabelText('Temperature')).toBeInTheDocument();
    });
    await userEvent.type(screen.getByLabelText('Presence penalty'), '0.5');
    expect(screen.getByText('unsaved')).toBeInTheDocument();
    // An unset field says so in quiet text beside the control; a set one offers Clear.
    expect(screen.getAllByText('engine default').length).toBeGreaterThan(0);
    expect(screen.getAllByRole('button', { name: /Clear/ }).length).toBeGreaterThan(0);
    studioClean();
    expect(await missingUtilities(utilities())).toEqual([]);
    unmount();
  });
});
