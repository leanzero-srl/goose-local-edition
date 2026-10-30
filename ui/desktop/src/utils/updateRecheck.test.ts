import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

/**
 * A running app keeps looking for releases. The owner's laptop ran 3.0.134 from 29 Oct while
 * 3.0.135 and 3.0.136 shipped on 30 Oct and never saw either: the updater checked only once,
 * 5 s after start. It now checks again every UPDATE_RECHECK_INTERVAL_MS until it finds one.
 */

const handlers = new Map<string, (...args: unknown[]) => void>();
const checkForUpdates = vi.fn(() => Promise.resolve(null));

vi.mock('electron-updater', () => ({
  autoUpdater: {
    checkForUpdates,
    setFeedURL: vi.fn(),
    getFeedURL: vi.fn(() => 'https://example.invalid/feed'),
    on: vi.fn((event: string, fn: (...args: unknown[]) => void) => handlers.set(event, fn)),
    currentVersion: { version: '3.0.134' },
  },
}));
vi.mock('electron', () => ({
  app: {
    getVersion: () => '3.0.134',
    isPackaged: false,
    getAppPath: () => '/app',
    getPath: () => '/tmp',
  },
  BrowserWindow: { getAllWindows: () => [] },
  ipcMain: { handle: vi.fn(), on: vi.fn() },
  nativeImage: { createFromPath: vi.fn() },
  Tray: vi.fn(),
  shell: {},
  Menu: { buildFromTemplate: vi.fn() },
  Notification: vi.fn(() => ({ show: vi.fn(), on: vi.fn() })),
}));
vi.mock('./logger', () => ({
  default: { info: vi.fn(), warn: vi.fn(), error: vi.fn(), debug: vi.fn() },
}));
vi.mock('./analytics', () => ({
  trackUpdateCheckStarted: vi.fn(),
  trackUpdateCheckCompleted: vi.fn(),
  trackUpdateDownloadStarted: vi.fn(),
  trackUpdateDownloadProgress: vi.fn(),
  trackUpdateDownloadCompleted: vi.fn(),
  trackUpdateInstallInitiated: vi.fn(),
}));
vi.mock('./githubUpdater', () => ({ githubUpdater: { checkForUpdates: vi.fn() } }));
vi.mock('./recentDirs', () => ({ loadRecentDirs: () => [] }));
vi.mock('./mainBrand', () => ({ getBrandName: () => 'Goose Swarm' }));
vi.mock('../appDialogWindow', () => ({ showAppDialog: vi.fn() }));

describe('a running app keeps checking for updates', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    checkForUpdates.mockClear();
    handlers.clear();
  });
  afterEach(() => vi.useRealTimers());

  it('checks at start, again every interval, and stops once a release is found', async () => {
    const { setupAutoUpdater, UPDATE_RECHECK_INTERVAL_MS } = await import('./autoUpdater');
    setupAutoUpdater();
    expect(checkForUpdates).not.toHaveBeenCalled();

    await vi.advanceTimersByTimeAsync(5000);
    expect(checkForUpdates).toHaveBeenCalledTimes(1);

    await vi.advanceTimersByTimeAsync(UPDATE_RECHECK_INTERVAL_MS);
    expect(checkForUpdates).toHaveBeenCalledTimes(2);

    handlers.get('update-available')?.({ version: '3.0.136' });
    await vi.advanceTimersByTimeAsync(3 * UPDATE_RECHECK_INTERVAL_MS);
    expect(checkForUpdates).toHaveBeenCalledTimes(2);
  });
});
