import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { cleanup, render as rtlRender, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { IntlTestWrapper } from '../../i18n/test-utils';
import MlxEngineView, {
  draftsFromSettings,
  settingsWithDrafts,
  formatGb,
} from './MlxEngineView';
import type {
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
const mockHfSearch = vi.fn();
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
  mlxEngineHfSearch: (...args: unknown[]) => mockHfSearch(...args),
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

const SETTINGS: MlxEngineSettings = {
  modelId: 'mlx-community/Qwen3-30B-A3B-4bit',
  modelsDir: '/Users/x/mlx-models',
  port: 9600,
  temperature: 0,
  topK: 40,
  spawnCommand: ['uvx', 'rapid-mlx', 'serve'],
};

const MODELS: MlxLocalModel[] = [
  { id: 'mlx-community/Qwen3-30B-A3B-4bit', sizeBytes: 17 * GB, complete: true },
  { id: 'mlx-community/Half-Model-8bit', sizeBytes: 3 * GB, complete: false },
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
  mockHfSearch.mockResolvedValue([]);
  mockDownload.mockResolvedValue(undefined);
  mockDownloadProgress.mockResolvedValue(null);
});

afterEach(() => {
  cleanup();
});

// ---------------------------------------------------------------------------
// The honest-payload contract: a cleared field is ABSENT, an explicit 0 is 0.
// This is the exact spot where "0 vs unset" silently corrupts sampling settings.
// ---------------------------------------------------------------------------

describe('sampling drafts keep 0 and unset apart', () => {
  it('a persisted 0 round-trips as the text "0", never as blank', () => {
    const drafts = draftsFromSettings(SETTINGS);
    expect(drafts.temperature).toBe('0');
    expect(drafts.topK).toBe('40');
    expect(drafts.topP).toBe('');
  });

  it('a blank draft leaves the key absent; "0" sends the number 0', () => {
    const drafts = draftsFromSettings(SETTINGS);
    drafts.temperature = '0';
    drafts.topP = '';
    drafts.minP = '0.05';
    const payload = settingsWithDrafts(SETTINGS, drafts);
    expect(payload.temperature).toBe(0);
    expect('topP' in payload).toBe(false);
    expect(payload.minP).toBe(0.05);
  });

  it('clearing a previously-set field drops it from the payload entirely', () => {
    const drafts = draftsFromSettings(SETTINGS);
    drafts.temperature = '';
    const payload = settingsWithDrafts(SETTINGS, drafts);
    expect('temperature' in payload).toBe(false);
  });

  it('non-numeric settings pass through untouched', () => {
    const payload = settingsWithDrafts(SETTINGS, draftsFromSettings(SETTINGS));
    expect(payload.modelsDir).toBe('/Users/x/mlx-models');
    expect(payload.port).toBe(9600);
    expect(payload.spawnCommand).toEqual(['uvx', 'rapid-mlx', 'serve']);
    expect(payload.modelId).toBe('mlx-community/Qwen3-30B-A3B-4bit');
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
// The engine tab renders backend truth: state, gate text VERBATIM, failure twins.
// ---------------------------------------------------------------------------

describe('MlxEngineView engine tab', () => {
  it('shows a running engine with model id, context window, parser, and pid', async () => {
    mockStatus.mockResolvedValue(
      statusOf({
        state: 'running',
        modelId: 'mlx-community/Qwen3-30B-A3B-4bit',
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
    expect(screen.getAllByText('mlx-community/Qwen3-30B-A3B-4bit').length).toBeGreaterThan(0);
    expect(screen.getByText('131,072')).toBeInTheDocument();
    expect(screen.getByText('qwen3')).toBeInTheDocument();
    expect(screen.getByText('4242')).toBeInTheDocument();
    expect(screen.getByText('http://127.0.0.1:9600/v1')).toBeInTheDocument();
    unmount();
  });

  it('renders the gate message verbatim as a solid red banner', async () => {
    const gate =
      'BLOCK: model needs 24.0 GB but only 9.1 GB of unified memory is free — close something or pick a smaller quant';
    mockStatus.mockResolvedValue(statusOf({ state: 'stopped', gateMessage: gate }));
    const { unmount } = render(<MlxEngineView />);
    await waitFor(() => {
      expect(screen.getByText(gate)).toBeInTheDocument();
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
      statusOf({
        state: 'running',
        modelId: 'mlx-community/Qwen3-30B-A3B-4bit',
        restartRequired: true,
      })
    );
    const { unmount } = render(<MlxEngineView />);
    await waitFor(() => {
      expect(screen.getByText('Settings changed — remount to apply.')).toBeInTheDocument();
    });
    await userEvent.click(screen.getByRole('button', { name: /Remount/ }));
    await waitFor(() => {
      expect(mockUnmount).toHaveBeenCalledTimes(1);
      expect(mockMount).toHaveBeenCalledWith('mlx-community/Qwen3-30B-A3B-4bit');
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
});

// ---------------------------------------------------------------------------
// The models tab: folder truth, list truth (header counts what the body shows),
// partial downloads flagged, download start renders a real progress row.
// ---------------------------------------------------------------------------

describe('MlxEngineView models tab', () => {
  it('lists local models with sizes, flags partial downloads, counts what it shows', async () => {
    const { unmount } = render(<MlxEngineView />);
    await waitFor(() => {
      expect(screen.getByText('Models')).toBeInTheDocument();
    });
    await userEvent.click(screen.getByRole('button', { name: /Models/ }));
    await waitFor(() => {
      expect(screen.getByText('/Users/x/mlx-models')).toBeInTheDocument();
    });
    expect(screen.getByText('mlx-community/Half-Model-8bit')).toBeInTheDocument();
    expect(screen.getByText('partial download')).toBeInTheDocument();
    expect(screen.getByText('17 GB')).toBeInTheDocument();
    // The tab chip and the section chip both say 2, and the body shows exactly 2 rows.
    expect(screen.getAllByText('2').length).toBeGreaterThanOrEqual(2);
    expect(screen.getAllByLabelText(/^Delete /).length).toBe(2);
    unmount();
  });

  it('HF search renders hits and Download starts a real tracked download', async () => {
    mockHfSearch.mockResolvedValue([
      { id: 'mlx-community/New-Model-4bit', downloads: 12800, likes: 42, updatedAt: '2026-08-20T10:00:00Z' },
    ]);
    const { unmount } = render(<MlxEngineView />);
    await waitFor(() => {
      expect(screen.getByText('Models')).toBeInTheDocument();
    });
    await userEvent.click(screen.getByRole('button', { name: /Models/ }));
    await userEvent.type(screen.getByLabelText('Search Hugging Face'), 'new-model');
    await userEvent.click(screen.getByRole('button', { name: /Search/ }));
    await waitFor(() => {
      expect(mockHfSearch).toHaveBeenCalledWith('new-model', 25);
      expect(screen.getByText('mlx-community/New-Model-4bit')).toBeInTheDocument();
    });
    expect(screen.getByText('↓ 12.8K')).toBeInTheDocument();

    await userEvent.click(screen.getByRole('button', { name: /Download/ }));
    await waitFor(() => {
      expect(mockDownload).toHaveBeenCalledWith('mlx-community/New-Model-4bit');
      expect(screen.getByTestId('mlx-download-mlx-community/New-Model-4bit')).toBeInTheDocument();
    });
    unmount();
  });

  it('deleting a model asks through the custom dialog, never window.confirm', async () => {
    mockModelDelete.mockResolvedValue(undefined);
    const confirmSpy = vi.spyOn(window, 'confirm');
    const { unmount } = render(<MlxEngineView />);
    await waitFor(() => {
      expect(screen.getByText('Models')).toBeInTheDocument();
    });
    await userEvent.click(screen.getByRole('button', { name: /Models/ }));
    await waitFor(() => {
      expect(screen.getByLabelText('Delete mlx-community/Half-Model-8bit')).toBeInTheDocument();
    });
    await userEvent.click(screen.getByLabelText('Delete mlx-community/Half-Model-8bit'));
    // The custom confirmation dialog appears with the model named.
    await waitFor(() => {
      expect(screen.getByText(/Delete mlx-community\/Half-Model-8bit/)).toBeInTheDocument();
    });
    await userEvent.click(screen.getByRole('button', { name: 'Delete' }));
    await waitFor(() => {
      expect(mockModelDelete).toHaveBeenCalledWith('mlx-community/Half-Model-8bit');
    });
    expect(confirmSpy).not.toHaveBeenCalled();
    unmount();
  });
});
