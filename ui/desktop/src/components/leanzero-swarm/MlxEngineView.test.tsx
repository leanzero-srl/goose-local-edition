import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { cleanup, render as rtlRender, screen, waitFor, within } from '@testing-library/react';
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
  mountFailureBanners,
} from './MlxEngineView';
import { mlxDistributedStart } from '../../acp/mlx-distributed';
import type { PlacementPlan } from '../../acp/mlx-placement';
import { NODES, PLAN_27B, PLAN_FLASH } from './placement.fixtures';
import { NAV_ITEMS } from '../../hooks/useNavigationItems';
import type {
  MlxBrowseHit,
  MlxEngineSettings,
  MlxEngineStatus,
  MlxLocalModel,
} from '../../acp/mlx-engine';
import type { NodeState, NodesResponse } from '../../acp/leanzero-link';
import { GENERATING_STATUS } from './mlxLiveStatus.fixtures';
import { FLASH_READY, STOPPED_WITH_CONFIG } from './mlxDistributed.fixtures';

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

vi.mock('../../acp/mlx-engine', async (importOriginal) => ({
  MlxMountRefusedError: (await importOriginal<typeof import('../../acp/mlx-engine')>())
    .MlxMountRefusedError,
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

const mockReplicaTargets = vi.fn();
const mockReplicate = vi.fn();
const mockReplicaProgress = vi.fn();
const mockReplicaCancel = vi.fn();
vi.mock('../../acp/mlx-replica', () => ({
  mlxEngineReplicaTargets: (...args: unknown[]) => mockReplicaTargets(...args),
  mlxEngineReplicate: (...args: unknown[]) => mockReplicate(...args),
  mlxEngineReplicaProgress: (...args: unknown[]) => mockReplicaProgress(...args),
  mlxEngineReplicaCancel: (...args: unknown[]) => mockReplicaCancel(...args),
}));

// The Macs (the Models columns, the Download-to choice, per-Mac sampling) come from the Link roster
// and are gated on the `leanzeroLink` capability. Default: capability OFF → this Mac alone, every
// mlx op local (nodeId undefined). Multi-Mac tests flip mockFeatures and hand the mesh a connected
// roster with peers.
const mockFeatures = { leanzeroLink: false, mlxDistributed: false };
vi.mock('../../contexts/FeaturesContext', () => ({
  useFeatures: () => ({
    localInference: true,
    mlxEngine: true,
    mlxDistributed: mockFeatures.mlxDistributed,
    leanzeroLink: mockFeatures.leanzeroLink,
    isLoading: false,
  }),
}));

const mockDistributedStatus = vi.fn();
vi.mock('../../acp/mlx-distributed', async (importOriginal) => ({
  foreignOwner: (await importOriginal<typeof import('../../acp/mlx-distributed')>()).foreignOwner,
  mlxDistributedStatus: (...a: unknown[]) => mockDistributedStatus(...a),
  mlxDistributedPreflight: vi.fn(),
  mlxDistributedStart: vi.fn(),
  mlxDistributedStop: vi.fn(),
  mlxDistributedConfigUpdate: vi.fn(),
}));

// The placement planner: by default unreachable (no ACP in jsdom), so the tile keeps the plain
// Mount; the mount-flow tests hand it a plan.
const mockPlacementPlan = vi.fn();
vi.mock('../../acp/mlx-placement', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../../acp/mlx-placement')>()),
  mlxPlacementPlan: (...a: unknown[]) => mockPlacementPlan(...a),
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
// Mesh roster fixtures (leanzeroLink/nodes shape, snake_case).
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

/** The measured TB5 pair: MacBook en3 "Thunderbolt 3" ↔ Studio en3 "Thunderbolt 2", /30. */
const TB_LINK = {
  kind: 'thunderbolt' as const,
  local: {
    device: 'en3',
    hardwarePort: 'Thunderbolt 3',
    kind: 'thunderbolt' as const,
    ipv4: '192.168.0.1',
    prefixLen: 30,
    linkSpeed: '80 Gb/s',
  },
  peer: {
    device: 'en3',
    hardwarePort: 'Thunderbolt 2',
    kind: 'thunderbolt' as const,
    ipv4: '192.168.0.2',
    prefixLen: 30,
    linkSpeed: '80 Gb/s',
  },
};

const TB_TARGET = { nodeId: 'peer-workhorse', hostname: 'workhorse', link: TB_LINK };

/** Turn on the capability + a connected roster with the given peers: every Mac becomes a column. */
function withMesh(peers: NodeState[], self: NodeState = SELF_NODE) {
  mockFeatures.leanzeroLink = true;
  mockLinkStatus.mockResolvedValue(CONNECTED);
  mockLinkNodes.mockResolvedValue({ self, peers } as NodesResponse);
}

/** Run it's "Run on this Mac" — the one start (the planner is unreachable here: every way is offered). */
async function runHere(): Promise<HTMLElement> {
  const run = await screen.findByTestId('placement-run-local');
  await waitFor(() => expect(run).toBeEnabled());
  return run;
}

beforeEach(() => {
  sessionStorage.clear();
  vi.clearAllMocks();
  mockFeatures.leanzeroLink = false;
  mockFeatures.mlxDistributed = false;
  mockLinkStatus.mockResolvedValue({ auth: { state: 'loggedOut' }, nodeCount: 0 });
  mockLinkNodes.mockResolvedValue({ self: SELF_NODE, peers: [] } as NodesResponse);
  mockStatus.mockResolvedValue(statusOf({}));
  mockSettingsRead.mockResolvedValue(SETTINGS);
  mockSettingsUpdate.mockImplementation(async (s: MlxEngineSettings) => s);
  mockModelsList.mockResolvedValue(listOf(MODELS));
  mockMount.mockResolvedValue(undefined);
  mockUnmount.mockResolvedValue(undefined);
  mockPlacementPlan.mockRejectedValue(new Error('no ACP client in this test'));
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
  mockReplicaTargets.mockResolvedValue({ meshConnected: true, targets: [TB_TARGET] });
  mockReplicate.mockResolvedValue({ link: TB_LINK, sourceUrl: 'http://192.168.0.1:54496' });
  mockReplicaProgress.mockResolvedValue(null);
  mockReplicaCancel.mockResolvedValue(undefined);
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

// ---------------------------------------------------------------------------
// The serving-lane overrides ride the same drafts: blank = auto (the model folder decides),
// and only an explicit choice reaches the profile.
// ---------------------------------------------------------------------------

describe('serving-lane profile fields', () => {
  it('a profile without lane fields drafts as auto and writes none back', () => {
    const drafts = draftsFromProfile(SETTINGS.modelProfiles[QWEN]);
    expect(drafts.speculative).toBe('');
    expect(drafts.adapterPath).toBe('');
    expect(drafts.textOnly).toBe('');
    const profile = profileFromDrafts(drafts);
    expect('speculative' in profile).toBe(false);
    expect('adapterPath' in profile).toBe(false);
    expect('textOnly' in profile).toBe(false);
  });

  it('explicit lane choices round-trip and count as values', () => {
    const drafts = draftsFromProfile({
      speculative: 'off',
      adapterPath: '~/lora',
      textOnly: false,
    });
    expect(drafts.speculative).toBe('off');
    expect(drafts.adapterPath).toBe('~/lora');
    expect(drafts.textOnly).toBe('false');
    const profile = profileFromDrafts(drafts);
    expect(profile).toEqual({ speculative: 'off', adapterPath: '~/lora', textOnly: false });
    expect(profileHasValues(profile)).toBe(true);
    // A lane-only profile survives settingsWithProfile (it is not "all blank").
    const payload = settingsWithProfile(SETTINGS, HALF, drafts);
    expect(payload.modelProfiles[HALF]).toEqual(profile);
  });

  it('an unknown speculative draft and a whitespace adapter path are dropped, not sent', () => {
    const drafts = draftsFromProfile(undefined);
    drafts.speculative = 'dflash';
    drafts.adapterPath = '   ';
    drafts.textOnly = 'maybe';
    expect(profileHasValues(profileFromDrafts(drafts))).toBe(false);
  });
});

describe('thinking profile fields', () => {
  it('a profile without thinking choices drafts as auto and writes none back', () => {
    const drafts = draftsFromProfile(SETTINGS.modelProfiles[QWEN]);
    expect(drafts.thinking).toBe('');
    expect(drafts.reasoningEffort).toBe('');
    const profile = profileFromDrafts(drafts);
    expect('thinking' in profile).toBe(false);
    expect('reasoningEffort' in profile).toBe(false);
  });

  it('explicit choices round-trip and a thinking-only profile survives the save', () => {
    const drafts = draftsFromProfile({ thinking: 'on', reasoningEffort: 'low' });
    expect(drafts.thinking).toBe('on');
    expect(drafts.reasoningEffort).toBe('low');
    expect(profileFromDrafts(drafts)).toEqual({ thinking: 'on', reasoningEffort: 'low' });
    const payload = settingsWithProfile(SETTINGS, HALF, draftsFromProfile({ thinking: 'off' }));
    expect(payload.modelProfiles[HALF]).toEqual({ thinking: 'off' });
  });

  it('an unknown thinking draft is dropped, not sent', () => {
    const drafts = draftsFromProfile(undefined);
    drafts.thinking = 'maybe';
    drafts.reasoningEffort = '  ';
    expect(profileHasValues(profileFromDrafts(drafts))).toBe(false);
  });
});

describe('KV cache profile field', () => {
  it('absent drafts as off and writes nothing; explicit modes round-trip; junk is dropped', () => {
    const drafts = draftsFromProfile(SETTINGS.modelProfiles[QWEN]);
    expect(drafts.kvCache).toBe('');
    expect('kvCache' in profileFromDrafts(drafts)).toBe(false);
    expect(profileFromDrafts(draftsFromProfile({ kvCache: 'int4' }))).toEqual({ kvCache: 'int4' });
    const payload = settingsWithProfile(SETTINGS, HALF, draftsFromProfile({ kvCache: 'int8' }));
    expect(payload.modelProfiles[HALF]).toEqual({ kvCache: 'int8' });
    const junk = draftsFromProfile(undefined);
    junk.kvCache = 'int2';
    expect(profileHasValues(profileFromDrafts(junk))).toBe(false);
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
// Pass C: the engine content is the LeanZero MLX TAB of the Goose Swarm view.
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

  it('the nav item is /leanzero-swarm labelled Providers (old /mlx-engine path is gone)', () => {
    expect(NAV_ITEMS.find((i) => i.path === '/mlx-engine')).toBeUndefined();
    const item = NAV_ITEMS.find((i) => i.path === '/leanzero-swarm');
    expect(item?.label).toBe('Providers');
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
      expect(screen.getAllByTestId('mlx-state-badge')[0]).toHaveTextContent('Running');
    });
    expect(screen.getAllByText(QWEN).length).toBeGreaterThan(0);
    expect(screen.getByText('131,072')).toBeInTheDocument();
    expect(screen.getByText('qwen3')).toBeInTheDocument();
    expect(screen.getByText('4242')).toBeInTheDocument();
    expect(screen.getByText('http://127.0.0.1:9600/v1')).toBeInTheDocument();
    unmount();
  });

  it('shows the in-flight count the engine reported (Rapid-MLX num_running + num_waiting)', async () => {
    mockStatus.mockResolvedValue(statusOf({ state: 'running', modelId: QWEN, activeRequests: 3 }));
    const { unmount } = render(<MlxEngineView />);
    await waitFor(() => expect(screen.getByTestId('mlx-inflight-count')).toHaveTextContent('3'));
    expect(screen.getByText('In flight')).toBeInTheDocument();
    unmount();
  });

  it('a running engine with NO count says UNKNOWN and prints activeRequestsError verbatim — never a fabricated 0', async () => {
    mockStatus.mockResolvedValue(
      statusOf({
        state: 'running',
        modelId: QWEN,
        activeRequestsError: 'GET http://127.0.0.1:9600/v1/status returned HTTP 401',
      })
    );
    const { unmount } = render(<MlxEngineView />);
    const unknown = await screen.findByTestId('mlx-inflight-unknown');
    expect(unknown).toHaveTextContent(
      'unknown — GET http://127.0.0.1:9600/v1/status returned HTTP 401'
    );
    expect(screen.queryByTestId('mlx-inflight-count')).toBeNull();
    unmount();
  });

  it('a stopped engine has no in-flight row value — an absent fact, not unknown', async () => {
    mockStatus.mockResolvedValue(statusOf({ state: 'stopped' }));
    const { unmount } = render(<MlxEngineView />);
    await waitFor(() =>
      expect(screen.getAllByTestId('mlx-state-badge')[0]).toHaveTextContent('Stopped')
    );
    expect(screen.queryByTestId('mlx-inflight-unknown')).toBeNull();
    expect(screen.queryByTestId('mlx-inflight-count')).toBeNull();
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
    expect(screen.getAllByTestId('mlx-state-badge')[0]).toHaveTextContent('Failed');
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
    await userEvent.click(await runHere());
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
    await userEvent.click(await runHere());
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

  it('stopped with NO stray listener offers no Unmount — there is nothing to unmount', async () => {
    mockStatus.mockResolvedValue(statusOf({ state: 'stopped' }));
    const { unmount } = render(<MlxEngineView />);
    await runHere();
    expect(screen.queryByRole('button', { name: /Unmount/ })).toBeNull();
    expect(screen.queryByTestId('placement-stop-local')).toBeNull();
    unmount();
  });
});

// ---------------------------------------------------------------------------
// The status hero (UX audit P1): the page leads with the state and the state's own action, and
// the running engine's facts fold away until there is something to show.
// ---------------------------------------------------------------------------

describe('MlxEngineView status hero', () => {
  it('STOPPED leads with a solid stopped tile, the headroom and the picker; Run it starts it; the details fold away', async () => {
    mockStatus.mockResolvedValue(
      statusOf({ state: 'stopped', availableMemoryGb: 96.6, totalMemoryGb: 128 })
    );
    const { unmount } = render(<MlxEngineView />);
    const hero = await screen.findByTestId('mlx-engine-hero');
    await waitFor(() =>
      expect(within(hero).getByTestId('mlx-state-badge')).toHaveAttribute('data-state', 'stopped')
    );
    const tile = within(hero).getByTestId('mlx-state-badge');
    expect(tile.className).toContain('bg-lz-phase-unloaded');
    expect(within(hero).getByText('no model mounted')).toBeInTheDocument();
    expect(within(hero).getByText('96.6 GB available of 128.0 GB')).toBeInTheDocument();
    // The hero picks the model; the ONE way to start it is Run it, right under the hero.
    expect(within(hero).getByRole('combobox', { name: 'Model to mount' })).toBeInTheDocument();
    expect(within(hero).queryByRole('button', { name: /^Mount$/ })).toBeNull();
    expect(within(hero).getByText(/start it in Run it below/)).toBeInTheDocument();
    await runHere();
    // The facts table is collapsed: a disclosure, not twelve rows of "—" leading the page.
    const toggle = screen.getByRole('button', { name: 'Engine details' });
    expect(toggle).toHaveAttribute('aria-expanded', 'false');
    expect(screen.getByLabelText('Engine status')).not.toBeVisible();
    await userEvent.click(toggle);
    expect(screen.getByLabelText('Engine status')).toBeVisible();
    expect(screen.getByText('Spawn command')).toBeVisible();
    // The Engine tab's hero owns the state: no second badge in the tab row.
    expect(screen.getAllByTestId('mlx-state-badge')).toHaveLength(1);
    unmount();
  });

  it('a full file cache reads as available, not pressure: the recorded 2026-09-23 case', async () => {
    // vm_stat then: free 2,272,001 + file-backed 1,610,894 pages of 16 KiB = 59.3 GiB available,
    // 24.6 GiB of it file cache — the page had read "0.0 GB free" in warning orange.
    mockStatus.mockResolvedValue(
      statusOf({
        state: 'stopped',
        availableMemoryGb: 59.3,
        reclaimableCacheGb: 24.6,
        totalMemoryGb: 128,
      })
    );
    const { unmount } = render(<MlxEngineView />);
    const hero = await screen.findByTestId('mlx-engine-hero');
    const line = await within(hero).findByText(
      '59.3 GB available of 128.0 GB (24.6 GB is reclaimable file cache)'
    );
    expect(line.className).not.toContain('text-lz-warn');
    unmount();
  });

  it('a failed memory probe says so instead of drawing 0 GB', async () => {
    mockStatus.mockResolvedValue(
      statusOf({
        state: 'stopped',
        availableMemoryGb: 0,
        totalMemoryGb: 0,
        memoryError: 'host_statistics64(HOST_VM_INFO64) failed with kern_return 5',
      })
    );
    const { unmount } = render(<MlxEngineView />);
    const hero = await screen.findByTestId('mlx-engine-hero');
    expect(
      await within(hero).findByText(/Memory unmeasured: host_statistics64/)
    ).toBeInTheDocument();
    expect(within(hero).queryByText(/GB available of/)).toBeNull();
    unmount();
  });

  it('RUNNING names the served model in the hero, Run it carries Stop, and the details are open', async () => {
    mockStatus.mockResolvedValue(
      statusOf({ state: 'running', modelId: QWEN, pid: 4242, toolCallParser: 'qwen3' })
    );
    const { unmount } = render(<MlxEngineView />);
    const hero = await screen.findByTestId('mlx-engine-hero');
    await waitFor(() =>
      expect(within(hero).getByTestId('mlx-state-badge')).toHaveAttribute('data-state', 'running')
    );
    // No live read in this test (no bridge): what the engine is DOING is unknown, so the fill is the
    // loaded model's idle grey — green is reserved for measured writing.
    expect(within(hero).getByTestId('mlx-state-badge').className).toContain('bg-lz-phase-idle');
    // Named as the served model (display size), and again in the picker as the selection.
    expect(within(hero).getAllByText(QWEN)[0].className).toContain('text-lz-h2');
    // Stopping is Run it's, on the way that runs — the hero keeps no second button for it.
    expect(within(hero).queryByRole('button', { name: /Unmount/ })).toBeNull();
    const local = await screen.findByTestId('placement-way-local');
    expect(within(local).getByTestId('placement-live')).toHaveTextContent('Running');
    await userEvent.click(within(local).getByTestId('placement-stop-local'));
    await waitFor(() => expect(mockUnmount).toHaveBeenCalledTimes(1));
    expect(screen.queryByRole('button', { name: 'Engine details' })).toBeNull();
    expect(screen.getByLabelText('Engine status')).toBeVisible();
    expect(screen.getByText('4242')).toBeVisible();
    unmount();
  });

  it('FAILED shows the error in the hero, Run it names it failed and Run mounts the selection again', async () => {
    mockStatus.mockResolvedValue(
      statusOf({ state: 'failed', modelId: QWEN, lastError: 'port 9600 never opened' })
    );
    const { unmount } = render(<MlxEngineView />);
    const hero = await screen.findByTestId('mlx-engine-hero');
    await waitFor(() =>
      expect(within(hero).getByText('port 9600 never opened')).toBeInTheDocument()
    );
    expect(within(hero).getByTestId('mlx-state-badge').className).toContain('bg-lz-phase-failed');
    const local = await screen.findByTestId('placement-way-local');
    expect(within(local).getByTestId('placement-live')).toHaveAttribute('data-phase', 'failed');
    await userEvent.click(await runHere());
    await waitFor(() => expect(mockMount).toHaveBeenCalledWith(QWEN));
    unmount();
  });

  it('the engine sub-tabs are the subordinate underline register, not a second solid strip', () => {
    const { unmount } = render(<MlxEngineView />);
    const group = screen.getByRole('radiogroup', { name: 'Engine sections' });
    expect(group).toHaveAttribute('data-variant', 'underline');
    const active = screen.getByRole('radio', { name: /^Engine$/ });
    expect(active.className).toContain('border-lz-accent');
    expect(active.className).not.toContain('bg-lz-accent');
    unmount();
  });
});

// ---------------------------------------------------------------------------
// The state tile as a live instrument, end to end through the view shell: the SAME 2-second status
// poll reads Rapid-MLX /v1/status through the main-process bridge (window.electron.mlxLiveStatus)
// while running, watches free memory across a mount, and prices the picked model while stopped.
// ---------------------------------------------------------------------------

type LiveBridge = {
  mlxLiveStatus?: (baseUrl: string) => Promise<unknown>;
  mlxEngineActivity?: () => Promise<unknown>;
};

describe('MlxEngineView state tile instrument', () => {
  const bridge = window.electron as unknown as LiveBridge;
  afterEach(() => {
    delete bridge.mlxLiveStatus;
    delete bridge.mlxEngineActivity;
  });

  it('RUNNING shows WHO the engine serves, as main read it (goose in-flight list)', async () => {
    bridge.mlxLiveStatus = vi.fn(async (baseUrl: string) => ({
      ok: true,
      url: `${baseUrl}/v1/status`,
      body: GENERATING_STATUS,
    }));
    bridge.mlxEngineActivity = vi.fn(async () => ({
      mode: 'running',
      serving: {
        clients: [
          {
            key: 'chat:s1',
            kind: 'chat',
            sessionId: 's1',
            sessionName: 'Memory · verify recall',
            count: 1,
          },
        ],
        unattributed: 2,
        swarmRuns: [],
        error: null,
      },
    }));
    mockStatus.mockResolvedValue(
      statusOf({ state: 'running', modelId: QWEN, baseUrl: 'http://127.0.0.1:8090' })
    );
    const { unmount } = render(<MlxEngineView />);
    const serving = await screen.findByTestId('mlx-serving');
    expect(
      within(serving)
        .getAllByTestId('mlx-serving-row')
        .map((r) => r.textContent)
    ).toEqual(['Chat · Memory · verify recall', "2 requests not from this app's chats or /v1"]);
    unmount();
  });

  it('RUNNING reads /v1/status at the engine base URL and draws the live readout', async () => {
    const live = vi.fn(async (baseUrl: string) => ({
      ok: true,
      url: `${baseUrl}/v1/status`,
      body: GENERATING_STATUS,
    }));
    bridge.mlxLiveStatus = live;
    mockStatus.mockResolvedValue(
      statusOf({ state: 'running', modelId: QWEN, baseUrl: 'http://127.0.0.1:8090' })
    );
    const { unmount } = render(<MlxEngineView />);
    const tps = await screen.findByTestId('mlx-live-tps');
    expect(tps).toHaveTextContent('19.9');
    expect(live).toHaveBeenCalledWith('http://127.0.0.1:8090');
    const tile = screen.getByTestId('mlx-state-badge');
    expect(within(tile).getByTestId('mlx-activity')).toHaveTextContent('Writing');
    expect(tile.className).toContain('bg-lz-phase-writing');
    expect(within(tile).getByText('Reading prompt · 32.3K tokens')).toBeInTheDocument();
    // Nothing to press on the tile: Run it carries the running way's Stop and Measure.
    expect(within(tile).queryByRole('button')).toBeNull();
    expect(await screen.findByTestId('placement-stop-local')).toBeEnabled();
    expect(screen.queryByTestId('placement-run-local')).toBeNull();
    unmount();
  });

  it('RUNNING with a failed read says live stats unavailable and names why', async () => {
    bridge.mlxLiveStatus = vi.fn(async () => ({
      ok: false,
      url: 'http://127.0.0.1:8090/v1/status',
      error: 'unreachable',
      detail: 'connect ECONNREFUSED',
    }));
    mockStatus.mockResolvedValue(
      statusOf({ state: 'running', modelId: QWEN, baseUrl: 'http://127.0.0.1:8090' })
    );
    const { unmount } = render(<MlxEngineView />);
    expect(await screen.findByTestId('mlx-live-unavailable')).toHaveTextContent(
      'unreachable: connect ECONNREFUSED'
    );
    expect(screen.queryByTestId('mlx-live-tps')).toBeNull();
    unmount();
  });

  it('MOUNTING seen from its start fills with the memory the engine claimed toward the model size', async () => {
    mockStatus.mockResolvedValue(statusOf({ state: 'stopped', availableMemoryGb: 60 }));
    const { unmount } = render(<MlxEngineView />);
    await waitFor(() =>
      expect(screen.getByTestId('mlx-state-badge')).toHaveAttribute('data-state', 'stopped')
    );
    // QWEN is 17 GB on disk; 8.5 GB of free memory has gone since the stopped read.
    mockStatus.mockResolvedValue(
      statusOf({ state: 'mounting', modelId: QWEN, availableMemoryGb: 51.5 })
    );
    forceStatusRefresh();
    const fill = await screen.findByTestId('mlx-mount-fill');
    expect(fill).toHaveAttribute('data-measured', 'true');
    expect(fill).toHaveTextContent('8.5of 17.0 GB');
    expect(within(fill).getByRole('progressbar')).toHaveAttribute('aria-valuenow', '50');
    unmount();
  });

  it('MOUNTING opened mid-mount shows the model size and "Loading weights", never a percent', async () => {
    mockStatus.mockResolvedValue(
      statusOf({ state: 'mounting', modelId: QWEN, availableMemoryGb: 51.5 })
    );
    const { unmount } = render(<MlxEngineView />);
    const fill = await screen.findByTestId('mlx-mount-fill');
    expect(fill).toHaveAttribute('data-measured', 'false');
    expect(fill).toHaveTextContent('17.0 GBLoading weights');
    expect(within(fill).getByRole('progressbar')).not.toHaveAttribute('aria-valuenow');
    unmount();
  });

  it('STOPPED draws the sidecar fit verdict for the picked model; Run on this Mac mounts it', async () => {
    const GIB = 1024 * 1024 * 1024;
    // The status asks the sidecar for the picked model's verdict (fitModelId) and the tile draws
    // it as given: 17 GB needed, a 32.2 GB budget of 40.2 GB free — 15.2 to spare.
    mockStatus.mockImplementation(async (_nodeId?: string, fitModelId?: string | null) =>
      statusOf({
        state: 'stopped',
        availableMemoryGb: 40.2,
        totalMemoryGb: 64,
        mountFit:
          fitModelId === QWEN
            ? {
                modelId: QWEN,
                verdict: 'allow',
                needBytes: 17 * GIB,
                weightsBytes: 17 * GIB,
                kvBytes: 0,
                contextTokens: 2304,
                budgetBytes: 32.2 * GIB,
                availableBytes: 40.2 * GIB,
                totalBytes: 64 * GIB,
                ceilingBytes: 48 * GIB,
                marginBytes: 5.952 * GIB,
                marginRatio: 0.093,
                spareBytes: 15.2 * GIB,
                message: 'needs 17.0 GiB with 15.2 GiB to spare',
              }
            : undefined,
      })
    );
    const { unmount } = render(<MlxEngineView />);
    const cost = await screen.findByTestId('mlx-mount-cost');
    expect(mockStatus).toHaveBeenCalledWith(undefined, QWEN);
    expect(cost).toHaveAttribute('data-verdict', 'fits');
    expect(cost).toHaveTextContent('Fits, 15.2 GB to spare');
    const tile = screen.getByTestId('mlx-state-badge');
    expect(within(tile).queryByRole('button', { name: /^Mount$/ })).toBeNull();
    await userEvent.click(await runHere());
    await waitFor(() => expect(mockMount).toHaveBeenCalledWith(QWEN));
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

describe('MlxEngineView — Run it tells the truth about the live engine', () => {
  it('running with the mounted model picked: Run it says Running and offers Stop, never a second start', async () => {
    mockStatus.mockResolvedValue(statusOf({ state: 'running', modelId: QWEN }));
    const { unmount } = render(<MlxEngineView />);
    const local = await screen.findByTestId('placement-way-local');
    await waitFor(() =>
      expect(within(local).getByTestId('placement-live')).toHaveTextContent('Running')
    );
    expect(within(local).getByTestId('placement-stop-local')).toBeEnabled();
    expect(within(local).queryByTestId('placement-run-local')).toBeNull();
    expect(screen.queryByRole('button', { name: /^Mount$/ })).not.toBeInTheDocument();
    unmount();
  });

  it('running with restartRequired: the amber banner owns Remount; Run it offers no restart of its own', async () => {
    mockStatus.mockResolvedValue(
      statusOf({ state: 'running', modelId: QWEN, restartRequired: true })
    );
    const { unmount } = render(<MlxEngineView />);
    await waitFor(() => {
      expect(screen.getByText('Settings changed — remount to apply.')).toBeInTheDocument();
    });
    expect(screen.getByRole('button', { name: /Remount/ })).toBeEnabled();
    await waitFor(() => expect(screen.getByTestId('placement-stop-local')).toBeInTheDocument());
    expect(screen.queryByTestId('placement-run-local')).toBeNull();
    unmount();
  });

  it('running with a DIFFERENT model picked: Run on this Mac switches to it', async () => {
    mockStatus.mockResolvedValue(statusOf({ state: 'running', modelId: QWEN }));
    mockModelsList.mockResolvedValue(listOf(COMPLETE_MODELS));
    const { unmount } = render(<MlxEngineView />);
    await waitFor(() => expect(screen.getByTestId('placement-stop-local')).toBeInTheDocument());
    await userEvent.click(screen.getAllByRole('combobox')[0]);
    await userEvent.click(await screen.findByRole('option', { name: /Other-Model-4bit/ }));
    await userEvent.click(await runHere());
    await waitFor(() => expect(mockMount).toHaveBeenCalledWith(OTHER_MODEL));
    unmount();
  });

  it('mounting: Run it says Mounting in the loading amber, and offers no start', async () => {
    mockStatus.mockResolvedValue(statusOf({ state: 'mounting', modelId: QWEN }));
    const { unmount } = render(<MlxEngineView />);
    const local = await screen.findByTestId('placement-way-local');
    await waitFor(() =>
      expect(within(local).getByTestId('placement-live')).toHaveAttribute('data-phase', 'loading')
    );
    expect(within(local).getByTestId('placement-live')).toHaveTextContent('Mounting');
    expect(within(local).queryByTestId('placement-run-local')).toBeNull();
    unmount();
  });

  it('an explicit pick is never overridden when the engine reports another model', async () => {
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
      expect(screen.getAllByTestId('mlx-state-badge')[0]).toHaveTextContent('Running');
    });
    // The pick survives: Run it is about the picked model, which is not the one running.
    expect(screen.getByText(OTHER_MODEL)).toBeInTheDocument();
    expect(await runHere()).toBeInTheDocument();
    expect(screen.queryByTestId('placement-live')).toBeNull();
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
      expect(screen.getByTestId('model-matrix')).toBeInTheDocument();
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

  it('the serving-lane rows render as auto and Save carries only the explicit choices', async () => {
    const { unmount } = render(<MlxEngineView />);
    await openSamplingTab();
    await waitFor(() => {
      expect(screen.getByLabelText('Temperature')).toHaveValue(0);
    });
    // Auto everywhere: the select shows Auto, the switch is on, no adapter.
    expect(screen.getByRole('combobox', { name: 'Speculative decoding' })).toHaveTextContent(
      /^Auto/
    );
    expect(screen.getByRole('switch', { name: 'Text-only lane' })).toHaveAttribute(
      'aria-checked',
      'true'
    );
    expect(screen.getByLabelText('LoRA adapter folder')).toHaveValue('');
    expect(screen.queryByText('unsaved')).not.toBeInTheDocument();

    await userEvent.click(screen.getByRole('combobox', { name: 'Speculative decoding' }));
    await userEvent.click(await screen.findByRole('option', { name: /^Off/ }));
    await userEvent.click(screen.getByRole('switch', { name: 'Text-only lane' }));
    expect(screen.getByRole('switch', { name: 'Text-only lane' })).toHaveAttribute(
      'aria-checked',
      'false'
    );
    await userEvent.type(screen.getByLabelText('LoRA adapter folder'), '~/lora');
    expect(screen.getByText('unsaved')).toBeInTheDocument();

    await userEvent.click(screen.getByRole('button', { name: 'Save' }));
    await waitFor(() => {
      expect(mockSettingsUpdate).toHaveBeenCalledTimes(1);
    });
    const payload = mockSettingsUpdate.mock.calls[0][0] as MlxEngineSettings;
    expect(payload.modelProfiles[QWEN]).toEqual({
      temperature: 0,
      topK: 40,
      speculative: 'off',
      adapterPath: '~/lora',
      textOnly: false,
    });

    // Switching the lane back on drops the key rather than sending true — auto and true
    // are the same argv, and absent is the honest form.
    await userEvent.click(screen.getByRole('switch', { name: 'Text-only lane' }));
    await userEvent.click(screen.getByRole('button', { name: 'Save' }));
    await waitFor(() => {
      expect(mockSettingsUpdate).toHaveBeenCalledTimes(2);
    });
    const second = mockSettingsUpdate.mock.calls[1][0] as MlxEngineSettings;
    expect('textOnly' in second.modelProfiles[QWEN]).toBe(false);
    expect(second.modelProfiles[QWEN].speculative).toBe('off');
    unmount();
  });

  it('thinking controls follow the template record and Save carries only explicit choices', async () => {
    mockModelsList.mockResolvedValue(
      listOf([
        {
          ...MODELS[0],
          thinking: {
            thinkingSwitch: 'enable_thinking',
            effortLevels: ['xhigh', 'medium', 'low'],
            defaultEffort: 'xhigh',
            preserveThinking: true,
            budgetForcible: true,
          },
        },
        MODELS[1],
      ])
    );
    const { unmount } = render(<MlxEngineView />);
    await openSamplingTab();
    const thinking = await screen.findByRole('radiogroup', { name: 'Thinking' });
    expect(within(thinking).getByRole('radio', { name: 'Auto' })).toHaveAttribute(
      'aria-checked',
      'true'
    );
    expect(
      screen.getByText('Auto = the engine decides (off when tools are used)')
    ).toBeInTheDocument();
    const effort = screen.getByRole('radiogroup', { name: 'Effort' });
    expect(
      within(effort)
        .getAllByRole('radio')
        .map((r) => r.textContent)
    ).toEqual(['Model default (xhigh)', 'xhigh', 'medium', 'low']);
    expect(screen.queryByText('unsaved')).not.toBeInTheDocument();

    await userEvent.click(within(thinking).getByRole('radio', { name: 'On' }));
    await userEvent.click(within(effort).getByRole('radio', { name: 'low' }));
    await userEvent.click(screen.getByRole('button', { name: 'Save' }));
    await waitFor(() => {
      expect(mockSettingsUpdate).toHaveBeenCalledTimes(1);
    });
    const payload = mockSettingsUpdate.mock.calls[0][0] as MlxEngineSettings;
    expect(payload.modelProfiles[QWEN]).toEqual({
      temperature: 0,
      topK: 40,
      thinking: 'on',
      reasoningEffort: 'low',
    });

    // Off makes the effort strip inert and says why; Auto again drops the key.
    await userEvent.click(within(thinking).getByRole('radio', { name: 'Off' }));
    expect(
      screen.getByText('Thinking is off, so the effort level changes nothing.')
    ).toBeInTheDocument();
    await userEvent.click(within(thinking).getByRole('radio', { name: 'Auto' }));
    await userEvent.click(within(effort).getByRole('radio', { name: 'Model default (xhigh)' }));
    await userEvent.click(screen.getByRole('button', { name: 'Save' }));
    await waitFor(() => {
      expect(mockSettingsUpdate).toHaveBeenCalledTimes(2);
    });
    const second = mockSettingsUpdate.mock.calls[1][0] as MlxEngineSettings;
    expect(second.modelProfiles[QWEN]).toEqual({ temperature: 0, topK: 40 });
    unmount();
  });

  it('a template without thinking controls offers none, and a read failure is named', async () => {
    mockModelsList.mockResolvedValue(
      listOf([
        {
          ...MODELS[0],
          thinking: {
            thinkingSwitch: null,
            effortLevels: [],
            preserveThinking: false,
            budgetForcible: false,
          },
        },
        MODELS[1],
      ])
    );
    const { unmount } = render(<MlxEngineView />);
    await openSamplingTab();
    await waitFor(() => {
      expect(screen.getByTestId('mlx-thinking-notice')).toHaveTextContent(
        "This model's chat template declares no thinking controls."
      );
    });
    expect(screen.queryByRole('radiogroup', { name: 'Thinking' })).not.toBeInTheDocument();
    expect(screen.queryByRole('radiogroup', { name: 'Effort' })).not.toBeInTheDocument();
    unmount();

    mockModelsList.mockResolvedValue(
      listOf([{ ...MODELS[0], thinkingError: 'chat template does not parse: unexpected end' }])
    );
    const second = render(<MlxEngineView />);
    await openSamplingTab();
    await waitFor(() => {
      expect(screen.getByTestId('mlx-thinking-notice')).toHaveTextContent(
        'Thinking controls unavailable: chat template does not parse: unexpected end'
      );
    });
    expect(screen.queryByRole('radiogroup', { name: 'Thinking' })).not.toBeInTheDocument();
    second.unmount();
  });

  it('the KV cache row shows what each setting buys on this model and saves only a choice', async () => {
    mockModelsList.mockResolvedValue(
      listOf([
        {
          ...MODELS[0],
          kvCache: {
            attentionLayers: 16,
            stateLayers: 48,
            slidingLayers: 0,
            kvHeads: 4,
            headDim: 256,
            groupSize: 64,
            bf16BytesPerToken: 65536,
            int8BytesPerToken: 34816,
            int4BytesPerToken: 18432,
          },
          kvCacheMeasurement: {
            measuredAt: '2026-09-24',
            engine: 'Rapid-MLX v0.14.3-lz.3',
            prompts: 15,
            noiseFloor: { agreement: 1, identicalAnswers: 15, retrievalFound: true },
            int8: {
              agreement: 0.912,
              identicalAnswers: 9,
              retrievalFound: true,
              decodeTpsRatio: 0.94,
              decodeContextTokens: 32722,
            },
            source: 'evals/mlx-engine-bench/results/2026-09-24-kv-quant',
          },
        },
        MODELS[1],
      ])
    );
    const { unmount } = render(<MlxEngineView />);
    await openSamplingTab();
    const kv = await screen.findByRole('radiogroup', { name: 'KV cache' });
    expect(within(kv).getByRole('radio', { name: 'Off (bf16)' })).toHaveAttribute(
      'aria-checked',
      'true'
    );
    expect(
      screen.getByText("Off: the engine's bf16 cache, 64 KiB per token of context.")
    ).toBeInTheDocument();

    await userEvent.click(within(kv).getByRole('radio', { name: '8-bit' }));
    expect(
      screen.getByText(
        '8-bit: 34 KiB per token instead of 64 KiB — the same memory holds 1.9× the context.'
      )
    ).toBeInTheDocument();
    expect(
      screen.getByText(/91\.2% token agreement with bf16, 9 of 15 answers/)
    ).toBeInTheDocument();
    expect(
      screen.getByText('Decode speed with 32,722 tokens of context: 94% of bf16.')
    ).toBeInTheDocument();
    expect(screen.getByText('91.2% agreement')).toBeInTheDocument();

    // int4 has no record: the row says so instead of inventing a number.
    await userEvent.click(within(kv).getByRole('radio', { name: '4-bit' }));
    expect(screen.getByText(/the same memory holds 3\.6× the context/)).toBeInTheDocument();
    expect(screen.getByText(/Quality not measured on this model/)).toBeInTheDocument();

    await userEvent.click(within(kv).getByRole('radio', { name: '8-bit' }));
    await userEvent.click(screen.getByRole('button', { name: 'Save' }));
    await waitFor(() => {
      expect(mockSettingsUpdate).toHaveBeenCalledTimes(1);
    });
    const payload = mockSettingsUpdate.mock.calls[0][0] as MlxEngineSettings;
    expect(payload.modelProfiles[QWEN]).toEqual({ temperature: 0, topK: 40, kvCache: 'int8' });
    unmount();
  });

  it('a model whose KV cannot be compressed locks the modes and a read failure is named', async () => {
    mockModelsList.mockResolvedValue(
      listOf([
        {
          ...MODELS[0],
          kvCache: {
            attentionLayers: 2,
            stateLayers: 0,
            slidingLayers: 0,
            kvHeads: 4,
            headDim: 80,
            bf16BytesPerToken: 2560,
          },
        },
        MODELS[1],
      ])
    );
    const { unmount } = render(<MlxEngineView />);
    await openSamplingTab();
    const kv = await screen.findByRole('radiogroup', { name: 'KV cache' });
    expect(within(kv).getByRole('radio', { name: '8-bit' })).toBeDisabled();
    expect(within(kv).getByRole('radio', { name: '4-bit' })).toBeDisabled();
    expect(screen.getByText(/head size \(80\) fits none/)).toBeInTheDocument();
    unmount();

    mockModelsList.mockResolvedValue(
      listOf([{ ...MODELS[0], kvCacheError: 'config.json declares no `dtype`/`torch_dtype`' }])
    );
    const second = render(<MlxEngineView />);
    await openSamplingTab();
    await waitFor(() => {
      expect(screen.getByTestId('mlx-kv-notice')).toHaveTextContent(
        'KV cache compression unavailable: config.json declares no `dtype`/`torch_dtype`'
      );
    });
    second.unmount();
  });

  it('the Sampling action in a model cell preselects that model on that Mac', async () => {
    mockModelsList.mockResolvedValue(listOf(COMPLETE_MODELS));
    const { unmount } = render(<MlxEngineView />);
    await openModelsTab();
    const cell = await screen.findByTestId(`model-cell-self-${OTHER_MODEL}`);
    await userEvent.click(within(cell).getByRole('button', { name: 'Sampling on This Mac' }));
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
    expect(screen.getByRole('radio', { name: /^Models/ })).toBeInTheDocument();
  });
  await userEvent.click(screen.getByRole('radio', { name: /^Models/ }));
}

/** The browser is the Models tab's second pane; the first is every Mac's models. */
async function openHfTab() {
  await openModelsTab();
  await userEvent.click(screen.getByRole('radio', { name: 'Hugging Face' }));
}

describe('MlxEngineView models tab', () => {
  it('splits into [On your Macs | Hugging Face]: the table of every Mac first, the browser apart', async () => {
    const { unmount } = render(<MlxEngineView />);
    await openModelsTab();
    // "On your Macs" is the default pane: the models table and each Mac's folder.
    expect(await screen.findByTestId('model-matrix')).toBeInTheDocument();
    await waitFor(() => {
      expect(screen.getByText('/Users/x/mlx-models')).toBeInTheDocument();
    });
    expect(screen.getByTestId('mlx-disk-bar')).toBeInTheDocument();
    expect(screen.getByText(HALF)).toBeInTheDocument();
    expect(screen.queryByLabelText('Search Hugging Face')).not.toBeInTheDocument();

    await userEvent.click(screen.getByRole('radio', { name: 'Hugging Face' }));
    await waitFor(() => {
      expect(screen.getByLabelText('Search Hugging Face')).toBeInTheDocument();
    });
    expect(screen.queryByTestId('model-matrix')).not.toBeInTheDocument();
    // One Mac: no "Download to" choice to make.
    expect(screen.queryByTestId('mlx-download-to')).not.toBeInTheDocument();
    unmount();
  });

  it('lists the models with sizes, flags incomplete downloads, counts what it shows', async () => {
    const { unmount } = render(<MlxEngineView />);
    await openModelsTab();
    const qwen = await screen.findByTestId(`model-cell-self-${QWEN}`);
    expect(qwen).toHaveAttribute('data-cell', 'present');
    expect(qwen).toHaveTextContent('On disk');
    expect(qwen).toHaveTextContent('17 GB');
    expect(screen.getByTestId(`model-cell-self-${HALF}`)).toHaveTextContent(
      'Incomplete · 2 files missing'
    );
    // The tab chips say 2, and the table shows exactly 2 rows.
    expect(screen.getByRole('radio', { name: /^Models/ })).toHaveTextContent('Models2');
    expect(screen.getByRole('radio', { name: /^On your Macs/ })).toHaveTextContent('2');
    expect(screen.getAllByTestId(/^model-row-/)).toHaveLength(2);
    unmount();
  });

  it('an incomplete model offers Resume (works for untracked residue), then shows its real bytes', async () => {
    const { unmount } = render(<MlxEngineView />);
    await openModelsTab();
    const cell = await screen.findByTestId(`model-cell-self-${HALF}`);
    // Incomplete cells trade the Sampling action for Resume; Delete stays.
    expect(within(cell).queryByRole('button', { name: /^Sampling on/ })).toBeNull();
    mockDownloadProgress.mockResolvedValue({
      state: 'downloading',
      totalBytes: 6 * GB,
      downloadedBytes: 3 * GB,
      currentFile: 'model-00002-of-00002.safetensors',
    });
    await userEvent.click(within(cell).getByRole('button', { name: 'Resume' }));
    await waitFor(() => {
      expect(mockDownloadResume).toHaveBeenCalledWith(HALF, undefined);
      expect(screen.getByTestId(`model-cell-self-${HALF}`)).toHaveTextContent('Downloading 50%');
    });
    unmount();
  });

  it('each Mac folder carries the free space of its volume from the modelsList response', async () => {
    const { unmount } = render(<MlxEngineView />);
    await openModelsTab();
    const folder = await screen.findByTestId('models-folder-self');
    await waitFor(() => expect(within(folder).getByTestId('mlx-disk-bar')).toBeInTheDocument());
    expect(within(folder).getByText('250 GB free')).toBeInTheDocument();
    expect(within(folder).getByText('of 500 GB')).toBeInTheDocument();
    unmount();
  });

  it('browses on open (top downloads, no cursor) and a row Download starts a tracked download', async () => {
    mockBrowse.mockResolvedValue({ hits: [HIT_A] });
    const { unmount } = render(<MlxEngineView />);
    await openHfTab();
    await waitFor(() => {
      expect(screen.getByText(HIT_A.id)).toBeInTheDocument();
    });
    expect(mockBrowse).toHaveBeenCalledWith(
      expect.objectContaining({ sort: 'downloads', limit: 20 })
    );
    expect(mockBrowse.mock.calls[0][0].cursor).toBeUndefined();
    // Downloads and likes are plain aligned figures now — no arrow, no heart glyph.
    expect(screen.getByText('12.8K')).toBeInTheDocument();
    expect(screen.getByText('42')).toBeInTheDocument();
    expect(screen.getByText('4-bit')).toBeInTheDocument();
    expect(screen.getByText('qwen3')).toBeInTheDocument();
    // Manifest bytes use the same units as the model card; unknown sizes stay absent.
    expect(screen.getByText('3.20 GB')).toBeInTheDocument();

    await userEvent.click(screen.getByLabelText(`Download ${HIT_A.id}`));
    await waitFor(() => {
      expect(mockDownload).toHaveBeenCalledWith(HIT_A.id, undefined);
      expect(screen.getByTestId(`mlx-download-${HIT_A.id}`)).toBeInTheDocument();
    });
    unmount();
  });

  it('reconnects a server-owned download after navigating away and back', async () => {
    mockBrowse.mockResolvedValue({ hits: [HIT_A] });
    const first = render(<MlxEngineView />);
    await openHfTab();
    await userEvent.click(await screen.findByLabelText(`Download ${HIT_A.id}`));
    await waitFor(() => expect(mockDownload).toHaveBeenCalledOnce());
    first.unmount();
    expect(mockDownloadCancel).not.toHaveBeenCalled();
    expect(mockDownloadPause).not.toHaveBeenCalled();
    mockDownloadProgress.mockResolvedValue({
      state: 'downloading',
      totalBytes: 4 * GB,
      downloadedBytes: 3 * GB,
      currentFile: 'model.safetensors',
    });
    const second = render(<MlxEngineView />);
    await openHfTab();
    await waitFor(() => expect(mockDownloadProgress).toHaveBeenCalledWith(HIT_A.id, undefined));
    expect(await screen.findByTestId(`mlx-download-${HIT_A.id}`)).toBeInTheDocument();
    expect(mockDownload).toHaveBeenCalledOnce();
    second.unmount();
  });

  it('Load more appends the next page via the cursor; a filter change resets pagination', async () => {
    mockBrowse.mockImplementation(async (params: { quant?: string; cursor?: string }) => {
      if (params.quant === '4-bit') return { hits: [HIT_C] };
      if (params.cursor === 'CUR1') return { hits: [HIT_B] };
      return { hits: [HIT_A], nextCursor: 'CUR1' };
    });
    const { unmount } = render(<MlxEngineView />);
    await openHfTab();
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
    await openHfTab();
    await waitFor(() => {
      expect(screen.getByText(HIT_A.id)).toBeInTheDocument();
    });
    await userEvent.click(screen.getByRole('radio', { name: 'Latest' }));
    await waitFor(() => {
      expect(screen.getByText(HIT_B.id)).toBeInTheDocument();
    });
    expect(mockBrowse).toHaveBeenCalledWith(expect.objectContaining({ sort: 'newest' }));
    // createdAt 2026-08-28 renders as a date in the row.
    expect(screen.getByText(/Aug 28, 2026/)).toBeInTheDocument();
    unmount();
  });

  it('a browse failure is loud and an empty result is honest', async () => {
    mockBrowse.mockRejectedValue(new Error('HuggingFace model browse returned HTTP 429'));
    const { unmount } = render(<MlxEngineView />);
    await openHfTab();
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

  it('deleting a model asks through the custom dialog naming the Mac, never window.confirm', async () => {
    mockModelDelete.mockResolvedValue(undefined);
    const confirmSpy = vi.spyOn(window, 'confirm');
    const { unmount } = render(<MlxEngineView />);
    await openModelsTab();
    const cell = await screen.findByTestId(`model-cell-self-${HALF}`);
    await userEvent.click(within(cell).getByRole('button', { name: 'Delete from This Mac' }));
    await waitFor(() => {
      expect(
        screen.getByText(/Delete mlx-community\/Half-Model-8bit \(3\.0 GB\) from This Mac\?/)
      ).toBeInTheDocument();
    });
    await userEvent.click(screen.getByRole('button', { name: 'Delete' }));
    await waitFor(() => {
      expect(mockModelDelete).toHaveBeenCalledWith(HALF, undefined);
    });
    expect(confirmSpy).not.toHaveBeenCalled();
    unmount();
  });

  it('a running download is visible from BOTH panes: inline in the browser, a cell in the table', async () => {
    mockBrowse.mockResolvedValue({ hits: [HIT_A] });
    mockDownloadProgress.mockResolvedValue({
      state: 'downloading',
      totalBytes: 4 * GB,
      downloadedBytes: 1 * GB,
    });
    const { unmount } = render(<MlxEngineView />);
    await openHfTab();
    await waitFor(() => {
      expect(screen.getByText(HIT_A.id)).toBeInTheDocument();
    });
    await userEvent.click(screen.getByLabelText(`Download ${HIT_A.id}`));
    await waitFor(() => {
      expect(screen.getByTestId(`mlx-download-${HIT_A.id}`)).toBeInTheDocument();
    });

    // The table shows the SAME download as the model arriving on this Mac.
    await userEvent.click(screen.getByRole('radio', { name: /^On your Macs/ }));
    await waitFor(() => {
      expect(screen.getByTestId(`model-cell-self-${HIT_A.id}`)).toHaveTextContent(
        'Downloading 25%'
      );
    });

    // And back on Hugging Face it is inline again, exactly once.
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
    await openHfTab();
    await userEvent.type(screen.getByLabelText('Search Hugging Face'), 'qwen{Enter}');
    await waitFor(() => {
      expect(screen.getByText(HIT_C.id)).toBeInTheDocument();
    });
    const browseCalls = mockBrowse.mock.calls.length;

    await userEvent.click(screen.getByRole('radio', { name: /^On your Macs/ }));
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
    await openHfTab();
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
    await openHfTab();
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
    await openHfTab();
    await waitFor(() => {
      expect(screen.getByText('vocabulary may be stale')).toBeInTheDocument();
    });
    first.unmount();
    cleanup();

    mockBrowseFilters.mockRejectedValue(new Error('crawl refused'));
    const second = render(<MlxEngineView />);
    await openHfTab();
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
    await openHfTab();
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
    await openHfTab();
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
    await openHfTab();
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
    await openHfTab();
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

  it('deleting a model clears its finished download so the browser offers Download again', async () => {
    mockBrowse.mockResolvedValue({ hits: [{ ...HIT_A, id: HALF }] });
    mockDownloadProgress.mockResolvedValue({
      state: 'done',
      totalBytes: 3 * GB,
      downloadedBytes: 3 * GB,
    });
    const { unmount } = render(<MlxEngineView />);
    await openModelsTab();
    const cell = await screen.findByTestId(`model-cell-self-${HALF}`);
    await userEvent.click(within(cell).getByRole('button', { name: 'Resume' }));
    await waitFor(() => expect(mockDownloadResume).toHaveBeenCalledWith(HALF, undefined));

    await userEvent.click(
      within(screen.getByTestId(`model-cell-self-${HALF}`)).getByRole('button', {
        name: 'Delete from This Mac',
      })
    );
    await userEvent.click(await screen.findByRole('button', { name: 'Delete' }));
    await waitFor(() => expect(mockModelDelete).toHaveBeenCalledWith(HALF, undefined));

    // Caught live once: a deleted model's row kept saying "done" and Download never came back.
    await userEvent.click(screen.getByRole('radio', { name: 'Hugging Face' }));
    expect(await screen.findByLabelText(`Download ${HALF}`)).toBeInTheDocument();
    expect(screen.queryByTestId(`mlx-download-${HALF}`)).not.toBeInTheDocument();
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
    await openHfTab();
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

    // Back on the Models tab the download is still there with the last REAL bytes.
    await userEvent.click(screen.getByRole('radio', { name: /^Models/ }));
    await waitFor(() => {
      expect(screen.getByTestId(`model-cell-self-${HIT_A.id}`)).toHaveTextContent(
        'Downloading 25%'
      );
    });
    await userEvent.click(screen.getByRole('radio', { name: 'Hugging Face' }));
    await waitFor(() => {
      expect(screen.getByTestId(`mlx-download-${HIT_A.id}`)).toBeInTheDocument();
    });
    expect(screen.getByText('1.00 GB / 4.00 GB')).toBeInTheDocument();
    unmount();
  });
});

// ---------------------------------------------------------------------------

describe('MlxEngineView browser — one accent, neutral columns', () => {
  it('renders hits as aligned columns under a header row; the publisher is neutral text, not a hue', async () => {
    mockBrowse.mockResolvedValue({ hits: [HIT_A] });
    const { unmount } = render(<MlxEngineView />);
    await openHfTab();
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
    await openHfTab();
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
    await openHfTab();
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
    await userEvent.click(screen.getByRole('radio', { name: /^On your Macs/ }));
    await waitFor(() => {
      expect(screen.getByTestId('mlx-disk-bar')).toBeInTheDocument();
    });
    expect(screen.getByText('Incomplete · 2 files missing')).toBeInTheDocument();
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

const PEER = 'peer-workhorse';
const STUDIO = 'Work’s Mac Studio';
const LAPTOP = 'Mihai’s MacBook';
const ME = { ...SELF_NODE, computer_name: LAPTOP };

/** This Mac lists MODELS; the peer lists `peer` (or fails with `peerError`). */
function peerHolds(peer: MlxLocalModel[], peerError?: unknown) {
  mockModelsList.mockImplementation(async (nodeId?: string) => {
    if (nodeId !== PEER) return listOf(MODELS);
    if (peerError) throw peerError;
    return { ...listOf(peer), modelsDir: '/Volumes/Studio/mlx-models' };
  });
}

function studioCleanNow() {
  assertStudioClean(document.body);
}

describe('MlxEngineView — Models: one row per model, one column per Mac', () => {
  const copyButton = () => screen.queryByRole('button', { name: /^Copy from / });

  it('a single Mac is one column, offers no copy and never asks for copy paths', async () => {
    const { unmount } = render(<MlxEngineView />);
    await openModelsTab();
    expect(await screen.findByTestId(`model-cell-self-${QWEN}`)).toHaveAttribute(
      'data-cell',
      'present'
    );
    expect(screen.queryByTestId(`model-cell-${PEER}-${QWEN}`)).toBeNull();
    expect(copyButton()).toBeNull();
    expect(mockReplicaTargets).not.toHaveBeenCalled();
    unmount();
  });

  it('connected with zero peers reads no copy paths either', async () => {
    withMesh([], ME);
    const { unmount } = render(<MlxEngineView />);
    await waitFor(() => expect(mockLinkNodes).toHaveBeenCalled());
    await openModelsTab();
    const matrix = await screen.findByTestId('model-matrix');
    await waitFor(() => expect(within(matrix).getByText(LAPTOP)).toBeInTheDocument());
    expect(copyButton()).toBeNull();
    expect(mockReplicaTargets).not.toHaveBeenCalled();
    unmount();
  });

  it('every Mac is a column under ONE name; "Copy from · Thunderbolt" fills the gap and the cell follows the receiver to done', async () => {
    withMesh([peerNode({ computer_name: STUDIO })], ME);
    let landed = false;
    mockModelsList.mockImplementation(async (nodeId?: string) =>
      nodeId === PEER ? listOf(landed ? [MODELS[0]] : []) : listOf(MODELS)
    );
    mockReplicate.mockResolvedValue(undefined);
    mockReplicaProgress
      .mockResolvedValueOnce({
        state: 'copying',
        sourceUrl: 'http://192.168.0.1:54496',
        link: 'thunderbolt',
        linkDetail: 'Thunderbolt 3 en3 192.168.0.1 → 192.168.0.2 (80 Gb/s)',
        totalBytes: 17 * GB,
        copiedBytes: 4 * GB,
        filesTotal: 4,
        filesDone: 1,
        currentFile: 'model-00002-of-00004.safetensors',
        phase: 'transferring',
        wireBytes: 4 * GB,
        wireMillis: 2000,
        elapsedMillis: 2100,
      })
      .mockImplementation(async () => {
        landed = true;
        return {
          state: 'done',
          sourceUrl: 'http://192.168.0.1:54496',
          link: 'thunderbolt',
          linkDetail: 'Thunderbolt 3 en3 192.168.0.1 → 192.168.0.2 (80 Gb/s)',
          totalBytes: 17 * GB,
          copiedBytes: 17 * GB,
          filesTotal: 4,
          filesDone: 4,
          wireBytes: 17 * GB,
          wireMillis: 8000,
          elapsedMillis: 8400,
        };
      });
    const { unmount } = render(<MlxEngineView />);
    await waitFor(() => expect(mockLinkNodes).toHaveBeenCalled());
    await openModelsTab();
    const matrix = await screen.findByTestId('model-matrix');
    // The names their owners gave them, once each — never a hostname beside it.
    await waitFor(() => expect(within(matrix).getByText(STUDIO)).toBeInTheDocument());
    expect(within(matrix).getByText(LAPTOP)).toBeInTheDocument();
    expect(within(matrix).queryByText('workhorse')).toBeNull();

    const gap = await screen.findByTestId(`model-cell-${PEER}-${QWEN}`);
    expect(gap).toHaveAttribute('data-cell', 'absent');
    expect(gap).toHaveTextContent('Not here');
    await waitFor(() =>
      expect(within(gap).getByRole('button', { name: /^Copy from / })).toHaveTextContent(
        `Copy from ${LAPTOP} · Thunderbolt`
      )
    );
    // Half a model is never copied: the incomplete one offers a download on the other Mac.
    expect(screen.getByTestId(`model-cell-${PEER}-${HALF}`)).toHaveTextContent('Download here');
    expect(screen.getAllByRole('button', { name: /^Copy from / })).toHaveLength(1);

    await userEvent.click(within(gap).getByRole('button', { name: /^Copy from / }));
    expect(mockReplicate).toHaveBeenCalledWith(QWEN, PEER, undefined);
    await waitFor(
      () =>
        expect(screen.getByTestId(`model-cell-${PEER}-${QWEN}`)).toHaveTextContent('Copying 24%'),
      { timeout: 3000 }
    );
    expect(mockReplicaProgress).toHaveBeenCalledWith(QWEN, PEER);
    const detail = screen.getByTestId(`mlx-replica-${QWEN}`);
    expect(detail).toHaveTextContent(`Copying to ${STUDIO} over Thunderbolt`);
    expect(detail).toHaveTextContent('1 of 4 files');
    expect(detail).toHaveTextContent('2.00 GB/s');
    await waitFor(
      () =>
        expect(screen.getByTestId(`model-cell-${PEER}-${QWEN}`)).toHaveAttribute(
          'data-cell',
          'present'
        ),
      { timeout: 4000 }
    );
    expect(screen.getByTestId(`model-cell-${PEER}-${QWEN}`)).toHaveTextContent('On disk');
    studioCleanNow();
    unmount();
  });

  it('a network-only path is labelled network', async () => {
    withMesh([peerNode({ computer_name: STUDIO })], ME);
    peerHolds([]);
    mockReplicaTargets.mockResolvedValue({
      meshConnected: true,
      targets: [
        {
          nodeId: PEER,
          hostname: 'workhorse',
          link: {
            kind: 'network',
            local: {
              device: 'en0',
              hardwarePort: 'Wi-Fi',
              kind: 'wifi',
              ipv4: '192.168.10.127',
              prefixLen: 24,
            },
            peer: {
              device: 'en1',
              hardwarePort: 'Wi-Fi',
              kind: 'wifi',
              ipv4: '192.168.10.161',
              prefixLen: 24,
            },
          },
        },
      ],
    });
    const { unmount } = render(<MlxEngineView />);
    await openModelsTab();
    const gap = await screen.findByTestId(`model-cell-${PEER}-${QWEN}`);
    const copy = await within(gap).findByRole('button', { name: /^Copy from / });
    expect(copy).toHaveTextContent(`Copy from ${LAPTOP} · network`);
    expect(copy).toHaveAttribute(
      'title',
      `Copies ${LAPTOP}’s files straight over the local network; every file is checked against the original.`
    );
    unmount();
  });

  it('no path between the Macs: no copy, goose’s reason, and Download here instead', async () => {
    withMesh([peerNode({ computer_name: STUDIO })], ME);
    peerHolds([]);
    mockReplicaTargets.mockResolvedValue({
      meshConnected: true,
      targets: [
        {
          nodeId: PEER,
          hostname: 'workhorse',
          unavailable:
            'this node and workhorse share no Thunderbolt or LAN subnet; a copy needs a direct path',
        },
      ],
    });
    const { unmount } = render(<MlxEngineView />);
    await openModelsTab();
    const why = await screen.findByTestId(`model-cell-${PEER}-${QWEN}-no-copy`);
    expect(why).toHaveTextContent(
      `No copy from ${LAPTOP}: this node and workhorse share no Thunderbolt or LAN subnet`
    );
    const gap = screen.getByTestId(`model-cell-${PEER}-${QWEN}`);
    expect(within(gap).queryByRole('button', { name: /^Copy from / })).toBeNull();
    expect(within(gap).getByRole('button', { name: 'Download here' })).toBeInTheDocument();
    unmount();
  });

  it('a refused copy says so in goose’s words and polls nothing', async () => {
    withMesh([peerNode({ computer_name: STUDIO })], ME);
    peerHolds([]);
    mockReplicate.mockRejectedValue(
      Object.assign(new Error('Invalid params'), {
        data: "'mlx-community/Qwen3-30B-A3B-4bit' is already complete on this node",
      })
    );
    const { unmount } = render(<MlxEngineView />);
    await openModelsTab();
    const gap = await screen.findByTestId(`model-cell-${PEER}-${QWEN}`);
    await userEvent.click(await within(gap).findByRole('button', { name: /^Copy from / }));
    await waitFor(() =>
      expect(screen.getByTestId(`mlx-replica-${QWEN}`)).toHaveTextContent(
        'is already complete on this node'
      )
    );
    expect(screen.getByTestId(`mlx-replica-${QWEN}`)).toHaveTextContent(
      `Copy to ${STUDIO} failed`
    );
    expect(mockReplicaProgress).not.toHaveBeenCalled();
    unmount();
  });

  it('a Mac whose owner turned model management off is a red Can’t read column naming the switch — never "0"', async () => {
    withMesh(
      [
        peerNode({
          computer_name: STUDIO,
          allows: { manage_models: false, answer_chat: true, run_split: true },
        }),
      ],
      ME
    );
    const { unmount } = render(<MlxEngineView />);
    await openModelsTab();
    const why = await screen.findByTestId(`model-column-gap-${PEER}`);
    expect(why).toHaveTextContent(
      `is off on ${STUDIO} — turn on “Let my other Macs use this Mac” there`
    );
    expect(screen.getByTestId(`model-cell-${PEER}-${QWEN}`)).toHaveAttribute(
      'data-cell',
      'cantRead'
    );
    expect(screen.getByTestId(`model-cell-${PEER}-${QWEN}`)).toHaveTextContent('Can’t read');
    // A Mac that refuses is never asked.
    expect(mockModelsList).not.toHaveBeenCalledWith(PEER);
    expect(document.body).not.toHaveTextContent('403');
    unmount();
  });

  it('an older peer that answers the mesh 403 is described as its switch, not a raw 403', async () => {
    withMesh([peerNode({ computer_name: STUDIO })], ME);
    peerHolds(
      [],
      Object.assign(new Error('Internal error'), {
        data: 'leanzero-link 403 Forbidden: remote model management is disabled on this node',
      })
    );
    const { unmount } = render(<MlxEngineView />);
    await openModelsTab();
    const why = await screen.findByTestId(`model-column-gap-${PEER}`);
    expect(why).toHaveTextContent(
      `is off on ${STUDIO} — turn on “Let my other Macs use this Mac” there`
    );
    expect(why).not.toHaveTextContent('403');
    unmount();
  });

  it('Download to: the browser downloads onto the Mac you pick', async () => {
    withMesh([peerNode({ computer_name: STUDIO })], ME);
    peerHolds([]);
    mockBrowse.mockResolvedValue({ hits: [HIT_A] });
    mockDownloadProgress.mockResolvedValue({
      state: 'downloading',
      totalBytes: 4 * GB,
      downloadedBytes: 1 * GB,
    });
    const { unmount } = render(<MlxEngineView />);
    await openHfTab();
    const to = await screen.findByTestId('mlx-download-to');
    await userEvent.click(within(to).getByRole('radio', { name: STUDIO }));
    await waitFor(() => expect(screen.getByText(HIT_A.id)).toBeInTheDocument());
    await userEvent.click(screen.getByLabelText(`Download ${HIT_A.id}`));
    await waitFor(() => expect(mockDownload).toHaveBeenCalledWith(HIT_A.id, PEER));
    // The table shows it arriving on the Studio, not on this Mac.
    await userEvent.click(screen.getByRole('radio', { name: /^On your Macs/ }));
    await waitFor(() =>
      expect(screen.getByTestId(`model-cell-${PEER}-${HIT_A.id}`)).toHaveTextContent(
        'Downloading 25%'
      )
    );
    expect(screen.getByTestId(`model-cell-self-${HIT_A.id}`)).toHaveAttribute(
      'data-cell',
      'absent'
    );
    unmount();
  });

  it('deleting on another Mac names that Mac and deletes there', async () => {
    withMesh([peerNode({ computer_name: STUDIO })], ME);
    peerHolds([MODELS[0]]);
    mockModelDelete.mockResolvedValue(undefined);
    const { unmount } = render(<MlxEngineView />);
    await openModelsTab();
    const cell = await screen.findByTestId(`model-cell-${PEER}-${QWEN}`);
    await waitFor(() => expect(cell).toHaveAttribute('data-cell', 'present'));
    await userEvent.click(within(cell).getByRole('button', { name: `Delete from ${STUDIO}` }));
    expect(
      await screen.findByText(new RegExp(`Delete ${QWEN.replace('/', '\\/')} \\(.+\\) from ${STUDIO}\\?`))
    ).toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: 'Delete' }));
    await waitFor(() => expect(mockModelDelete).toHaveBeenCalledWith(QWEN, PEER));
    unmount();
  });

  it('each Mac shows its own models folder', async () => {
    withMesh([peerNode({ computer_name: STUDIO })], ME);
    peerHolds([]);
    mockSettingsRead.mockImplementation(async (nodeId?: string) =>
      nodeId === PEER ? { ...SETTINGS, modelsDir: '/Volumes/Studio/mlx-models' } : SETTINGS
    );
    const { unmount } = render(<MlxEngineView />);
    await openModelsTab();
    const folder = await screen.findByTestId(`models-folder-${PEER}`);
    await waitFor(() =>
      expect(folder).toHaveTextContent('/Volumes/Studio/mlx-models')
    );
    expect(folder).toHaveTextContent(STUDIO);
    expect(screen.getByTestId('models-folder-self')).toHaveTextContent('/Users/x/mlx-models');
    unmount();
  });

  it('sampling profiles are per Mac: picking the Studio reads the Studio’s settings', async () => {
    withMesh([peerNode({ computer_name: STUDIO })], ME);
    peerHolds([MODELS[0]]);
    const { unmount } = render(<MlxEngineView />);
    await openSamplingTab();
    const on = await screen.findByTestId('mlx-sampling-mac');
    await userEvent.click(within(on).getByRole('radio', { name: STUDIO }));
    await waitFor(() => expect(mockSettingsRead).toHaveBeenCalledWith(PEER));
    unmount();
  });
});

describe('Engine tab — which engine owns this Mac is always said', () => {
  it('no distributed capability: "Single · this Mac" on the tab row and the tile; Run it offers no split', async () => {
    render(<MlxEngineView />);
    await waitFor(() =>
      expect(screen.getByTestId('mlx-mode-chip')).toHaveTextContent('Single · this Mac')
    );
    expect(screen.getByTestId('mlx-mode')).toHaveTextContent('Single · this Mac');
    await runHere();
    expect(screen.queryByTestId('placement-way-split')).toBeNull();
    expect(mockDistributedStatus).not.toHaveBeenCalled();
  });

  it('the distributed run owns the Mac: the tile is that run, Mount is not offered, the section shows the nodes', async () => {
    mockFeatures.mlxDistributed = true;
    mockDistributedStatus.mockResolvedValue(FLASH_READY);
    render(<MlxEngineView />);
    await waitFor(() =>
      expect(screen.getByTestId('mlx-mode-chip')).toHaveTextContent('Distributed · 2 nodes · JACCL')
    );
    const tile = screen.getByTestId('mlx-state-badge');
    expect(tile).toHaveAttribute('data-mode', 'distributed');
    expect(screen.getByTestId('mlx-mode')).toHaveTextContent('Distributed · 2 nodes · JACCL');
    expect(within(tile).queryByRole('button', { name: 'Mount' })).toBeNull();
    expect(screen.getByTestId('mlx-distributed-owns')).toBeInTheDocument();
    expect(screen.getAllByTestId('mlx-dist-node')).toHaveLength(2);
    expect(screen.getByTestId('mlx-dist-slots')).toHaveTextContent('Slots 0 / 2');
    expect(screen.getByTestId('mlx-dist-tile-load')).toHaveTextContent('slots 0 of 2 · 0 waiting');
    // This Mac's own start waits for the split to stop.
    expect(screen.queryByTestId('placement-run-local')).toBeNull();
  });

  it('a stopped distributed engine leaves the single engine in charge; the split is one way in Run it', async () => {
    mockFeatures.mlxDistributed = true;
    mockDistributedStatus.mockResolvedValue(STOPPED_WITH_CONFIG);
    render(<MlxEngineView />);
    await waitFor(() => expect(mockDistributedStatus).toHaveBeenCalled());
    await waitFor(() =>
      expect(screen.getByTestId('mlx-mode-chip')).toHaveTextContent('Single · this Mac')
    );
    expect(screen.getByTestId('mlx-state-badge')).toHaveAttribute('data-mode', 'single');
    expect(screen.queryByTestId('mlx-distributed-owns')).toBeNull();
    const split = await screen.findByTestId('placement-way-split');
    expect(within(split).getByTestId('placement-split-details')).toBeInTheDocument();
    // No second Start: the split's own section lives folded under that row.
    expect(screen.queryByRole('button', { name: 'Start' })).toBeNull();
  });
});

/**
 * The owner's 3.0.25 test, 2026-09-24: he pressed Mount (single, "Single · this Mac") on
 * rapid-mlx/Qwen3.8-Flash-Next-4bit whose badge said "Needs both Macs"; it failed and the page showed
 * TWO red banners for the one failure. Run it now offers only the ways goose can start, and one
 * failure is one banner.
 */
describe('Run it — the ways follow the placement plan', () => {
  const FLASH = 'rapid-mlx/Qwen3.8-Flash-Next-4bit';
  const SPLIT = 'pipeline:jaccl:local+workhorse';
  // PLAN_FLASH measured with room on both Macs: the pipeline split fits and goose can start it.
  const NEEDS_BOTH: PlacementPlan = {
    ...PLAN_FLASH,
    best: SPLIT,
    bestAvailable: SPLIT,
    badge: { kind: 'needsBothMacs' },
    candidates: (PLAN_FLASH.candidates ?? []).map((c) =>
      c.id === SPLIT ? { ...c, fit: { ...c.fit, status: 'fits' }, outcome: { code: 'best' } } : c
    ),
  };

  function withFlash(plan: PlacementPlan, status: Partial<MlxEngineStatus> = {}) {
    mockSettingsRead.mockResolvedValue({ ...SETTINGS, modelId: FLASH });
    mockModelsList.mockResolvedValue(
      listOf([...MODELS, { id: FLASH, sizeBytes: 45 * GB, complete: true, missingFiles: 0 }])
    );
    mockStatus.mockResolvedValue(statusOf({ state: 'stopped', ...status }));
    mockPlacementPlan.mockResolvedValue({ plans: [plan], nodes: NODES });
  }

  it('"Needs both Macs": only the split has Run, and it starts the split — never a single mount', async () => {
    withFlash(NEEDS_BOTH);
    vi.mocked(mlxDistributedStart).mockResolvedValue({
      started: true,
    } as Awaited<ReturnType<typeof mlxDistributedStart>>);
    const { unmount } = render(<MlxEngineView />);
    const split = await screen.findByTestId('placement-way-split');
    expect(split).toHaveAttribute('data-way', SPLIT);
    const run = await within(split).findByTestId('placement-run-split');
    // Short on this Mac alone: no Run there to be refused.
    expect(screen.queryByTestId('placement-run-local')).toBeNull();
    await userEvent.click(run);
    await waitFor(() => expect(mlxDistributedStart).toHaveBeenCalledWith(null));
    expect(mockMount).not.toHaveBeenCalled();
    unmount();
  });

  it('a refused split start is ONE banner with goose’s own reason', async () => {
    withFlash(NEEDS_BOTH);
    vi.mocked(mlxDistributedStart).mockResolvedValue({
      started: false,
      refusal: { code: 'preflightFailed', message: 'workhorse: 3.1 GiB short of its budget' },
    } as Awaited<ReturnType<typeof mlxDistributedStart>>);
    const { unmount } = render(<MlxEngineView />);
    await userEvent.click(await screen.findByTestId('placement-run-split'));
    await waitFor(() =>
      expect(screen.getByTestId('placement-card')).toHaveTextContent(
        'workhorse: 3.1 GiB short of its budget'
      )
    );
    const alerts = screen.getAllByRole('alert').map((a) => a.textContent ?? '');
    expect(alerts.filter((t) => t.includes('3.1 GiB short'))).toHaveLength(1);
    unmount();
  });

  it('nothing fits: no way offers Run, and the picker badge says the shortfall', async () => {
    withFlash(PLAN_FLASH);
    const { unmount } = render(<MlxEngineView />);
    await screen.findByTestId('placement-way-split');
    expect(screen.queryByTestId('placement-run-local')).toBeNull();
    expect(screen.queryByTestId('placement-run-split')).toBeNull();
    expect(screen.queryByTestId('placement-run-peer')).toBeNull();
    unmount();
  });

  it('a split whose setup names another model: no Run, and its Details open on what to do first', async () => {
    mockFeatures.mlxDistributed = true;
    mockDistributedStatus.mockResolvedValue(STOPPED_WITH_CONFIG);
    withFlash({ ...PLAN_27B, modelId: FLASH });
    const { unmount } = render(<MlxEngineView />);
    const split = await screen.findByTestId('placement-way-split');
    expect(within(split).queryByTestId('placement-run-split')).toBeNull();
    expect(within(split).getByTestId('placement-split-details')).toHaveAttribute(
      'data-state',
      'open'
    );
    unmount();
  });

  it('a model that fits this Mac keeps Run on this Mac', async () => {
    withFlash({
      ...NEEDS_BOTH,
      badge: { kind: 'fitsThisMac' },
      candidates: (NEEDS_BOTH.candidates ?? []).map((c) =>
        c.id === 'single:local' ? { ...c, fit: { ...c.fit, status: 'fits' } } : c
      ),
    });
    const { unmount } = render(<MlxEngineView />);
    await userEvent.click(await runHere());
    await waitFor(() => expect(mockMount).toHaveBeenCalledWith(FLASH));
    unmount();
  });

  it('a gate-blocked mount is ONE banner, not "Mount blocked" + "Mount failed"', async () => {
    const gate =
      'model needs 45.0 GiB + an 8.0 GiB reserve, 31.7 GiB is available — short 21.3 GiB';
    mockStatus.mockResolvedValue(statusOf({ state: 'stopped' }));
    mockMount.mockImplementation(async () => {
      // The sidecar records the gate, then rejects the mount with the same message.
      mockStatus.mockResolvedValue(
        statusOf({ state: 'stopped', gateVerdict: 'block', gateMessage: gate })
      );
      throw Object.assign(new Error('Internal error'), {
        data: `memory gate BLOCK for '${QWEN}': ${gate}`,
      });
    });
    const { unmount } = render(<MlxEngineView />);
    await userEvent.click(await runHere());
    const blocked = await screen.findByTestId('mlx-mount-blocked');
    expect(blocked).toHaveTextContent(gate);
    expect(screen.queryByTestId('mlx-mount-failed')).toBeNull();
    // The one mount failure is one alert (Run it's own "Could not plan" — the planner is
    // unreachable in this test — is a different failure and says so under its own name).
    const alerts = screen.getAllByRole('alert').map((a) => a.textContent ?? '');
    expect(alerts.filter((t) => t.includes(gate))).toHaveLength(1);
    expect(alerts.filter((t) => t.includes('Mount'))).toEqual([`Mount blocked${gate}`]);
    unmount();
  });

  it('no plan read, but goose REFUSES the mount naming a split: one banner, and the split is the other way offered', async () => {
    const { MlxMountRefusedError } =
      await vi.importActual<typeof import('../../acp/mlx-engine')>('../../acp/mlx-engine');
    mockFeatures.mlxDistributed = true;
    mockDistributedStatus.mockResolvedValue(STOPPED_WITH_CONFIG);
    const fitMessage = 'needs 47.1 GiB, the budget is 31.2 GiB — short 15.9 GiB';
    mockSettingsRead.mockResolvedValue({ ...SETTINGS, modelId: FLASH });
    mockModelsList.mockResolvedValue(
      listOf([...MODELS, { id: FLASH, sizeBytes: 45 * GB, complete: true, missingFiles: 0 }])
    );
    mockStatus.mockResolvedValue(statusOf({ state: 'stopped' }));
    const split = NEEDS_BOTH.candidates!.find((c) => c.id === SPLIT)!;
    mockMount.mockImplementation(async () => {
      mockStatus.mockResolvedValue(
        statusOf({ state: 'stopped', gateVerdict: 'block', gateMessage: fitMessage })
      );
      throw new MlxMountRefusedError({
        fit: { modelId: FLASH, verdict: 'block', message: fitMessage },
        alternative: split,
        badge: { kind: 'needsBothMacs' },
      });
    });
    const { unmount } = render(<MlxEngineView />);
    await userEvent.click(await runHere());
    await waitFor(() =>
      expect(screen.getByTestId('mlx-mount-blocked')).toHaveTextContent(fitMessage)
    );
    expect(screen.queryByTestId('mlx-mount-failed')).toBeNull();
    const alerts = screen.getAllByRole('alert').map((a) => a.textContent ?? '');
    expect(alerts.filter((t) => t.includes(fitMessage))).toHaveLength(1);
    expect(screen.getByTestId('placement-way-split')).toBeInTheDocument();
    unmount();
  });

  it('the dedupe is exact: a different failure beside a stale gate block still says so', () => {
    const status = { gateVerdict: 'block', gateMessage: 'short 21.3 GiB' } as const;
    expect(mountFailureBanners(status, "memory gate BLOCK for 'm': short 21.3 GiB")).toEqual({
      gateBlock: 'short 21.3 GiB',
      mountError: null,
    });
    expect(mountFailureBanners(status, 'port 8090 has an unsupervised listener')).toEqual({
      gateBlock: 'short 21.3 GiB',
      mountError: 'port 8090 has an unsupervised listener',
    });
    expect(mountFailureBanners({ gateVerdict: 'warn', gateMessage: 'tight' }, 'x')).toEqual({
      gateBlock: null,
      mountError: 'x',
    });
  });
});
