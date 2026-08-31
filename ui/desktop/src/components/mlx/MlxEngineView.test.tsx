import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { cleanup, render as rtlRender, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { IntlTestWrapper } from '../../i18n/test-utils';
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

const mockStatus = vi.fn();
const mockMount = vi.fn();
const mockUnmount = vi.fn();
const mockSettingsRead = vi.fn();
const mockSettingsUpdate = vi.fn();
const mockModelsList = vi.fn();
const mockModelDelete = vi.fn();
const mockBrowse = vi.fn();
const mockDownload = vi.fn();
const mockDownloadProgress = vi.fn();
const mockDownloadCancel = vi.fn();

vi.mock('../../acp/mlx-engine', () => ({
  mlxEngineStatus: (...args: unknown[]) => mockStatus(...args),
  mlxEngineMount: (...args: unknown[]) => mockMount(...args),
  mlxEngineUnmount: (...args: unknown[]) => mockUnmount(...args),
  mlxEngineSettingsRead: (...args: unknown[]) => mockSettingsRead(...args),
  mlxEngineSettingsUpdate: (...args: unknown[]) => mockSettingsUpdate(...args),
  mlxEngineModelsList: (...args: unknown[]) => mockModelsList(...args),
  mlxEngineModelDelete: (...args: unknown[]) => mockModelDelete(...args),
  mlxEngineBrowse: (...args: unknown[]) => mockBrowse(...args),
  mlxEngineDownload: (...args: unknown[]) => mockDownload(...args),
  mlxEngineDownloadProgress: (...args: unknown[]) => mockDownloadProgress(...args),
  mlxEngineDownloadCancel: (...args: unknown[]) => mockDownloadCancel(...args),
}));

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
  { id: QWEN, sizeBytes: 17 * GB, complete: true },
  { id: HALF, sizeBytes: 3 * GB, complete: false },
];

function statusOf(overrides: Partial<MlxEngineStatus>): MlxEngineStatus {
  return {
    state: 'stopped',
    restartRequired: false,
    availableMemoryGb: 40.2,
    totalMemoryGb: 64,
    ...overrides,
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  mockStatus.mockResolvedValue(statusOf({}));
  mockSettingsRead.mockResolvedValue(SETTINGS);
  mockSettingsUpdate.mockImplementation(async (s: MlxEngineSettings) => s);
  mockModelsList.mockResolvedValue(MODELS);
  mockMount.mockResolvedValue(undefined);
  mockUnmount.mockResolvedValue(undefined);
  mockBrowse.mockResolvedValue({ hits: [] });
  mockDownload.mockResolvedValue(undefined);
  mockDownloadProgress.mockResolvedValue(null);
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
// The rename: the window says Leanzero MLX, powered by Rapid-MLX, and the nav
// item carries the same name.
// ---------------------------------------------------------------------------

describe('Leanzero MLX naming', () => {
  it('the header is titled Leanzero MLX with a visible powered-by line', async () => {
    const { unmount } = render(<MlxEngineView />);
    expect(screen.getByRole('heading', { name: 'Leanzero MLX' })).toBeInTheDocument();
    expect(screen.getByText('Powered by Rapid-MLX')).toBeInTheDocument();
    await waitFor(() => {
      expect(screen.getAllByTestId('mlx-state-badge').length).toBeGreaterThan(0);
    });
    unmount();
  });

  it('the nav item for /mlx-engine is labelled Leanzero MLX', () => {
    const item = NAV_ITEMS.find((i) => i.path === '/mlx-engine');
    expect(item?.label).toBe('Leanzero MLX');
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
      expect(mockMount).toHaveBeenCalledWith(QWEN);
    });
    unmount();
  });

  it('a rejected mount renders the backend error text', async () => {
    mockStatus.mockResolvedValue(statusOf({ state: 'stopped' }));
    mockSettingsRead.mockResolvedValue({ ...SETTINGS });
    mockMount.mockRejectedValue(new Error('model directory is incomplete: missing weights.safetensors'));
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
  { id: QWEN, sizeBytes: 17 * GB, complete: true },
  { id: OTHER_MODEL, sizeBytes: 4 * GB, complete: true },
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
    expect(screen.getByRole('button', { name: /Mounted/ })).toBeDisabled();
    expect(screen.getByRole('button', { name: /Remount/ })).toBeEnabled();
    unmount();
  });

  it('running with a DIFFERENT selection offers an enabled "Switch model" that mounts the selection', async () => {
    mockStatus.mockResolvedValue(statusOf({ state: 'running', modelId: QWEN }));
    mockModelsList.mockResolvedValue(COMPLETE_MODELS);
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
      expect(mockMount).toHaveBeenCalledWith(OTHER_MODEL);
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
    mockModelsList.mockResolvedValue(COMPLETE_MODELS);
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
    expect(screen.getByRole('button', { name: 'Sampling' })).toBeInTheDocument();
  });
  await userEvent.click(screen.getByRole('button', { name: 'Sampling' }));
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
    expect(
      screen.getByText(/per-request values sent by goose override them/)
    ).toBeInTheDocument();
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
      expect(mockMount).toHaveBeenCalledWith(QWEN);
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

    await userEvent.click(screen.getByRole('button', { name: /Models/ }));
    await waitFor(() => {
      expect(screen.getByText('/Users/x/mlx-models')).toBeInTheDocument();
    });
    await userEvent.click(screen.getByRole('button', { name: 'Sampling' }));
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

  it('the per-model Sampling affordance on the Models tab preselects that row model', async () => {
    const { unmount } = render(<MlxEngineView />);
    await waitFor(() => {
      expect(screen.getByText('Models')).toBeInTheDocument();
    });
    await userEvent.click(screen.getByRole('button', { name: /Models/ }));
    await waitFor(() => {
      expect(screen.getByLabelText(`Sampling for ${HALF}`)).toBeInTheDocument();
    });
    await userEvent.click(screen.getByLabelText(`Sampling for ${HALF}`));
    await waitFor(() => {
      expect(
        screen.getByText(/per-request values sent by goose override them/)
      ).toBeInTheDocument();
    });
    // The picker holds the row's model, not the default.
    expect(screen.getByText(HALF)).toBeInTheDocument();
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
    expect(screen.getByRole('button', { name: /Models/ })).toBeInTheDocument();
  });
  await userEvent.click(screen.getByRole('button', { name: /Models/ }));
}

describe('MlxEngineView models tab', () => {
  it('lists local models with sizes, flags partial downloads, counts what it shows', async () => {
    const { unmount } = render(<MlxEngineView />);
    await openModelsTab();
    await waitFor(() => {
      expect(screen.getByText('/Users/x/mlx-models')).toBeInTheDocument();
    });
    expect(screen.getByText(HALF)).toBeInTheDocument();
    expect(screen.getByText('partial download')).toBeInTheDocument();
    expect(screen.getByText('17 GB')).toBeInTheDocument();
    // The tab chip and the section chip both say 2, and the body shows exactly 2 rows.
    expect(screen.getAllByText('2').length).toBeGreaterThanOrEqual(2);
    expect(screen.getAllByLabelText(/^Delete /).length).toBe(2);
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
      expect.objectContaining({ sort: 'downloads', limit: 20 })
    );
    expect(mockBrowse.mock.calls[0][0].cursor).toBeUndefined();
    expect(screen.getByText('↓ 12.8K')).toBeInTheDocument();
    expect(screen.getByText('4-bit')).toBeInTheDocument();
    expect(screen.getByText('qwen3')).toBeInTheDocument();

    await userEvent.click(screen.getByLabelText(`Download ${HIT_A.id}`));
    await waitFor(() => {
      expect(mockDownload).toHaveBeenCalledWith(HIT_A.id);
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
    expect(screen.getByText('2 loaded')).toBeInTheDocument();
    const loadMoreCall = mockBrowse.mock.calls.find((c) => c[0].cursor === 'CUR1');
    expect(loadMoreCall).toBeTruthy();

    // Changing the quant filter refetches page 1 (no cursor) and REPLACES the list.
    await userEvent.click(screen.getByRole('combobox', { name: 'Quant filter' }));
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
    await userEvent.click(screen.getByRole('button', { name: 'Latest' }));
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
    await openModelsTab();
    await waitFor(() => {
      expect(
        screen.getByText('HuggingFace model browse returned HTTP 429')
      ).toBeInTheDocument();
    });
    expect(screen.getByText('Browse failed')).toBeInTheDocument();

    mockBrowse.mockResolvedValue({ hits: [] });
    // Committing a search refetches and lands on the honest empty state.
    await userEvent.type(screen.getByLabelText('Search Hugging Face'), 'nothing-matches{Enter}');
    await waitFor(() => {
      expect(screen.getByText('No MLX models match these filters.')).toBeInTheDocument();
    });
    expect(
      mockBrowse.mock.calls.some((c) => c[0].query === 'nothing-matches')
    ).toBe(true);
    unmount();
  });

  it('deleting a model asks through the custom dialog, never window.confirm', async () => {
    mockModelDelete.mockResolvedValue(undefined);
    const confirmSpy = vi.spyOn(window, 'confirm');
    const { unmount } = render(<MlxEngineView />);
    await openModelsTab();
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
      expect(mockModelDelete).toHaveBeenCalledWith(HALF);
    });
    expect(confirmSpy).not.toHaveBeenCalled();
    unmount();
  });
});
