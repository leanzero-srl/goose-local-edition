import '@testing-library/jest-dom';
import { vi, afterEach, expect } from 'vitest';
import { cleanup } from '@testing-library/react';
import { TONE_TEXT, TONES } from '../components/lz/tokens';

// Mock Electron modules before any imports
vi.mock('electron', () => ({
  app: {
    getPath: vi.fn((name: string) => {
      if (name === 'userData') return '/tmp/test-user-data';
      if (name === 'temp') return '/tmp';
      if (name === 'home') return '/tmp/home';
      return '/tmp';
    }),
  },
  ipcRenderer: {
    invoke: vi.fn(),
    send: vi.fn(),
    on: vi.fn(),
    off: vi.fn(),
  },
}));

// This is the standard set up to ensure that React Testing Library's
// automatic cleanup runs after each test.
afterEach(() => {
  cleanup();
});

// Q-247: every element any test rendered that NAMES a tone as its text colour must PAINT it in the
// compiled CSS — an error line carrying a TYPE step's ink painted grey across the app for weeks
// while every class-name assertion passed. Registered AFTER cleanup: vitest runs afterEach hooks
// as a stack, so this reads the DOM before cleanup empties it.
const TONE_TEXT_SELECTOR = TONES.map((tone) => `.${TONE_TEXT[tone]}`).join(', ');
afterEach(async () => {
  if (typeof document === 'undefined' || !document.body.querySelector(TONE_TEXT_SELECTOR)) return;
  const { tonesThatDoNotPaint } = await import('../components/lz/tonePaint');
  expect(
    await tonesThatDoNotPaint(document.body),
    'a tone that does not paint (Q-247) — another colour utility wins in the compiled CSS'
  ).toEqual([]);
});

// Mock console methods to avoid noise in tests
// eslint-disable-next-line no-undef
global.console = {
  ...console,
  log: vi.fn(),
  warn: vi.fn(),
  error: vi.fn(),
};

// Mock window.navigator.clipboard for copy functionality tests
Object.assign(navigator, {
  clipboard: {
    writeText: vi.fn(() => Promise.resolve()),
  },
});

// Mock settings store for tests
const mockSettings: Record<string, unknown> = {
  showMenuBarIcon: true,
  showDockIcon: true,
  enableWakelock: false,
  spellcheckEnabled: true,
  keyboardShortcuts: {
    focusWindow: 'CommandOrControl+Alt+G',
    quickLauncher: 'CommandOrControl+Alt+Shift+G',
    newChat: 'CommandOrControl+T',
    newChatWindow: 'CommandOrControl+N',
    openDirectory: 'CommandOrControl+O',
    settings: 'CommandOrControl+,',
    find: 'CommandOrControl+F',
    findNext: 'CommandOrControl+G',
    findPrevious: 'CommandOrControl+Shift+G',
    alwaysOnTop: 'CommandOrControl+Shift+T',
  },
  externalGoosed: {
    enabled: false,
    url: '',
    secret: '',
  },
  theme: 'light',
  useSystemTheme: true,
  language: 'system',
  responseStyle: 'concise',
  showPricing: true,
  showLmStudioFleet: false,
  seenAnnouncementIds: [],
};

// Mock window.electron for renderer process
Object.defineProperty(window, 'electron', {
  writable: true,
  value: {
    platform: 'darwin',
    getSetting: vi.fn((key: string) => Promise.resolve(mockSettings[key])),
    setSetting: vi.fn((key: string, value: unknown) => {
      mockSettings[key] = value;
      return Promise.resolve();
    }),
    reloadApp: vi.fn(),
    showMessageBox: vi.fn(() => Promise.resolve({ response: 0 })),
    getIsFullScreen: vi.fn(() => Promise.resolve(false)),
    on: vi.fn(),
    off: vi.fn(),
    // The theme bridge: main owns nativeTheme; a fixed choice answers itself, 'system' answers light.
    setThemeSource: vi.fn(async (preference: string) => ({ dark: preference === 'dark' })),
    onNativeThemeUpdated: vi.fn(() => () => {}),
    onSwarmDelta: vi.fn(() => () => {}),
    onSystemResumed: vi.fn(() => () => {}),
    // The fleet probes run in main (utils/fleetProbe.ts); with no LM Studio in a test the honest answer
    // is a NAMED unreachable — the same offline state a real install without a fleet shows.
    fleetProbe: vi.fn(async (endpoint: string) => ({
      ok: false,
      url: endpoint,
      error: 'unreachable',
      detail: 'no fleet in tests',
    })),
    // macOS local network privacy (localNetwork.ts): the alert trigger and the Settings door.
    touchLocalNetwork: vi.fn(async () => []),
    openLocalNetworkSettings: vi.fn(async () => true),
  },
});
