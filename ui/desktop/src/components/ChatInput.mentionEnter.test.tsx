import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import ChatInput from './ChatInput';
import { ChatState } from '../types/chatState';
import { IntlTestWrapper } from '../i18n/test-utils';
import { mentionTriggerStart } from './mentionTrigger';

/**
 * Q-492 (E2E #3x turn 1, 3.0.78): "…mask the email like k***@example.com." was filled into the
 * composer and Enter pressed; the "@" inside the address opened the file picker ("Scanning
 * files…"), Enter went to the picker, and the message was never sent — no error, sessions.db held
 * no user row. The REAL MentionPopover is mounted here (the other ChatInput suites mock it away,
 * which is how this dead end stayed invisible to them).
 */

vi.mock('./settings/models/bottom_bar/ModelsBottomBar', () => ({
  default: () => <div data-testid="models-bottom-bar" />,
}));
vi.mock('./bottom_menu/DirSwitcher', () => ({
  DirSwitcher: () => <div data-testid="dir-switcher" />,
}));
vi.mock('./bottom_menu/BottomMenuExtensionSelection', () => ({
  BottomMenuExtensionSelection: () => <div data-testid="extensions-selector" />,
}));
vi.mock('./bottom_menu/CostTracker', () => ({
  CostTracker: () => <div data-testid="cost-tracker" />,
}));
vi.mock('./bottom_menu/ContextWindowIndicator', () => ({
  ContextWindowIndicator: () => <div data-testid="context-indicator" />,
}));
vi.mock('../acp/autocomplete', () => ({
  listAgentMentionItems: async () => [],
  listSlashCommandItems: async () => [],
}));
vi.mock('../hooks/useAudioRecorder', () => ({
  useAudioRecorder: () => ({
    isEnabled: false,
    dictationProvider: null,
    isRecording: false,
    isTranscribing: false,
    startRecording: vi.fn(),
    stopRecording: vi.fn(),
  }),
}));
vi.mock('./ModelAndProviderContext', () => ({
  useModelAndProvider: () => ({
    getCurrentModelAndProvider: async () => ({ model: 'test-model', provider: 'anthropic' }),
    currentModel: 'test-model',
    currentProvider: 'anthropic',
  }),
}));
vi.mock('./alerts', () => ({
  useAlerts: () => ({ alerts: [], addAlert: vi.fn(), clearAlerts: vi.fn() }),
  AlertType: { Error: 'error', Warning: 'warning', Info: 'info' },
}));
vi.mock('../acp/providers', () => ({ acpListProviderDetails: async () => [] }));
vi.mock('../utils/canonical', () => ({ fetchCanonicalModelInfo: async () => null }));
vi.mock('../acp/mlx-engine', () => ({
  mlxEngineStatus: async () => ({ state: 'stopped', restartRequired: false, availableMemoryGb: 0 }),
}));
vi.mock('./swarm/useFleet', () => ({ fetchSwarmContextLimit: async () => null }));
vi.mock('../acp/diagnostics', () => ({ getDiagnosticsReport: vi.fn() }));

class ResizeObserverMock {
  observe() {}
  unobserve() {}
  disconnect() {}
}

beforeAll(() => {
  vi.stubGlobal('ResizeObserver', ResizeObserverMock);
  Element.prototype.scrollIntoView = vi.fn();
});

type ElectronWithFiles = typeof window.electron & { listFiles: (dir: string) => Promise<string[]> };
const electron = () => window.electron as ElectronWithFiles;
let originalListFiles: ElectronWithFiles['listFiles'] | undefined;

beforeEach(() => {
  originalListFiles = electron().listFiles;
});

afterEach(() => {
  electron().listFiles = originalListFiles as ElectronWithFiles['listFiles'];
});

/** The scan never finishes: the picker sits on "Scanning files…", as it did on 3.0.78. */
const scanForever = () => {
  electron().listFiles = vi.fn(() => new Promise<string[]>(() => {}));
};

/** One file at the working dir's root; every deeper listing fails (so nothing reads as a dir). */
const scanReadme = () => {
  electron().listFiles = vi.fn(async (dir: string) => {
    if (dir === '/tmp') return ['README.md'];
    throw new Error('not a directory');
  });
};

const mount = () => {
  const handleSubmit = vi.fn();
  render(
    <IntlTestWrapper>
      <ChatInput
        sessionId="sess-1"
        handleSubmit={handleSubmit}
        chatState={ChatState.Idle}
        setView={vi.fn()}
        sessionModel="test-model"
        sessionProvider="anthropic"
        sessionLoaded
        workingDir="/tmp"
      />
    </IntlTestWrapper>
  );
  const box = screen.getByTestId('chat-input') as HTMLTextAreaElement;
  return { handleSubmit, box };
};

const fill = (box: HTMLTextAreaElement, value: string) => {
  fireEvent.change(box, { target: { value, selectionStart: value.length } });
};

const RECEIPT = 'Summarise the incident and mask the email like k***@example.com.';

describe('Q-492 — an "@" inside a word never opens the file picker', () => {
  it.each([
    ['the E2E receipt', RECEIPT],
    ['a git remote', 'clone git@github.com:org/repo'],
    ['a user@host', 'ssh into deploy@build-01'],
  ])('%s: no picker, and Enter SENDS the words', async (_label, text) => {
    scanForever();
    const { handleSubmit, box } = mount();
    fill(box, text);
    expect(screen.queryByText('Scanning files...')).toBeNull();
    fireEvent.keyDown(box, { key: 'Enter' });
    await waitFor(() => expect(handleSubmit).toHaveBeenCalledTimes(1));
    expect(handleSubmit.mock.calls[0][0].msg).toBe(text);
  });

  it('the trigger fires only where "@" starts a token', () => {
    expect(mentionTriggerStart('look at @READ')).toBe(8);
    expect(mentionTriggerStart('@src')).toBe(0);
    expect(mentionTriggerStart('see (@src')).toBe(5);
    expect(mentionTriggerStart('say "@src')).toBe(5);
    expect(mentionTriggerStart('line\n@src')).toBe(5);
    expect(mentionTriggerStart('k***@example.com.')).toBe(-1);
    expect(mentionTriggerStart('git@github.com:org/repo')).toBe(-1);
    expect(mentionTriggerStart('deploy@build-01')).toBe(-1);
    expect(mentionTriggerStart('no mention here')).toBe(-1);
  });
});

describe('Q-492 — Enter with the picker open sends unless an item was actively chosen', () => {
  it('an npm scope opens the picker (it starts a token) — Enter while it still scans SENDS', async () => {
    scanForever();
    const { handleSubmit, box } = mount();
    fill(box, 'install @aaif/goose-sdk');
    expect(await screen.findByText('Scanning files...')).toBeInTheDocument();
    fireEvent.keyDown(box, { key: 'Enter' });
    await waitFor(() => expect(handleSubmit).toHaveBeenCalledTimes(1));
    expect(handleSubmit.mock.calls[0][0].msg).toBe('install @aaif/goose-sdk');
    expect(screen.queryByText('Scanning files...')).toBeNull();
  });

  it('items listed but none chosen: nothing is highlighted and Enter SENDS', async () => {
    scanReadme();
    const { handleSubmit, box } = mount();
    fill(box, 'look at @READ');
    expect(await screen.findByText('README.md')).toBeInTheDocument();
    expect(document.querySelector('[data-selected="true"]')).toBeNull();
    fireEvent.keyDown(box, { key: 'Enter' });
    await waitFor(() => expect(handleSubmit).toHaveBeenCalledTimes(1));
    expect(handleSubmit.mock.calls[0][0].msg).toBe('look at @READ');
  });

  it('ArrowDown then Enter still INSERTS the item and sends nothing', async () => {
    scanReadme();
    const { handleSubmit, box } = mount();
    fill(box, 'look at @READ');
    expect(await screen.findByText('README.md')).toBeInTheDocument();
    fireEvent.keyDown(box, { key: 'ArrowDown' });
    await waitFor(() =>
      expect(document.querySelector('[data-selected="true"]')?.textContent).toContain('README.md')
    );
    fireEvent.keyDown(box, { key: 'Enter' });
    await waitFor(() => expect(box.value).toBe('look at /tmp/README.md'));
    expect(handleSubmit).not.toHaveBeenCalled();
    expect(screen.queryByText('README.md')).toBeNull();
  });

  it('Tab takes the top match without arrowing to it', async () => {
    scanReadme();
    const { handleSubmit, box } = mount();
    fill(box, 'look at @READ');
    expect(await screen.findByText('README.md')).toBeInTheDocument();
    fireEvent.keyDown(box, { key: 'Tab' });
    await waitFor(() => expect(box.value).toBe('look at /tmp/README.md'));
    expect(handleSubmit).not.toHaveBeenCalled();
  });
});
