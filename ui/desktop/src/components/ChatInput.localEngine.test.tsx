import React from 'react';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { describe, it, expect, vi, beforeAll, beforeEach } from 'vitest';
import ChatInput from './ChatInput';
import { ChatState } from '../types/chatState';
import { IntlTestWrapper } from '../i18n/test-utils';
import type { ChatServedBy } from './chatServedBy/chatServedBy';

/**
 * The composer on a local engine (omlx / swarm).
 *
 * Q-60, regressed on 3.0.41 (ROUND-2026-09-25-4 §3): "the counter is absent on the dead split".
 * 4333464ef kept the window only while a ROUTE reconnects; a split whose rank died reports no
 * window (`contextWindow: null`, readiness `split-stopped`), so the limit fell to 0 and the
 * counter vanished under the bar. The window measured for this chat on this model now holds.
 */

let served: ChatServedBy;
vi.mock('./chatServedBy/useChatServedBy', () => ({
  useChatServedBy: () => ({ served, single: null, armed: true, turnInFlight: false }),
}));
vi.mock('./noNodeNotice/ComposerReadiness', () => ({ ComposerReadinessStrip: () => null }));
vi.mock('./bottom_menu/ContextWindowIndicator', () => ({
  ContextWindowIndicator: ({ tokenLimit }: { tokenLimit: number }) =>
    tokenLimit ? <div data-testid="context-indicator" data-limit={tokenLimit} /> : null,
}));
vi.mock('./settings/models/bottom_bar/ModelsBottomBar', () => ({
  default: () => <div data-testid="models-bottom-bar" />,
}));
vi.mock('./bottom_menu/DirSwitcher', () => ({ DirSwitcher: () => null }));
vi.mock('./bottom_menu/CostTracker', () => ({ CostTracker: () => null }));
vi.mock('./MentionPopover', () => ({
  default: React.forwardRef(function MentionPopoverMock() {
    return null;
  }),
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
    getCurrentModelAndProvider: async () => ({ model: 'qwen-27b', provider: 'omlx' }),
    currentModel: 'qwen-27b',
    currentProvider: 'omlx',
  }),
}));
vi.mock('./alerts', () => ({
  useAlerts: () => ({ alerts: [], addAlert: vi.fn(), clearAlerts: vi.fn() }),
  AlertType: { Error: 'error', Warning: 'warning', Info: 'info' },
}));
vi.mock('./ui/Diagnostics', async (original) => ({
  ...(await original<typeof import('./ui/Diagnostics')>()),
  DiagnosticsModal: () => null,
}));
vi.mock('./swarm/AgentSetupWizard', () => ({
  default: ({ isOpen }: { isOpen: boolean }) =>
    isOpen ? <div data-testid="recipes-and-loops-open" /> : null,
}));

class ResizeObserverMock {
  observe() {}
  unobserve() {}
  disconnect() {}
}
beforeAll(() => {
  vi.stubGlobal('ResizeObserver', ResizeObserverMock);
});

const splitServing = (contextWindow: number | null, kind: 'ready' | 'unknown'): ChatServedBy =>
  ({
    engine: contextWindow ? 'split' : 'none',
    model: 'mlx-community/Qwen3.6-27B-8bit',
    where: ['This Mac', 'Work'],
    peerNodeId: null,
    foreign: false,
    contextWindow,
    phase: contextWindow ? 'idle' : 'failed',
    activity: null,
    work: null,
    busyWithOthers: null,
    busyIn: null,
    turnRequest: null,
    readTps: null,
    readiness: { kind },
  }) as unknown as ChatServedBy;

const input = (sessionId: string, model = 'qwen-27b', provider = 'omlx') => (
  <IntlTestWrapper>
    <ChatInput
      sessionId={sessionId}
      handleSubmit={vi.fn()}
      chatState={ChatState.Idle}
      setView={vi.fn()}
      sessionModel={model}
      sessionProvider={provider}
      sessionLoaded
      workingDir="/tmp"
    />
  </IntlTestWrapper>
);

describe('the context counter while the split that measured it is down (Q-60)', () => {
  beforeEach(() => {
    served = splitServing(262144, 'ready');
  });

  it('keeps "/ 262k" when the split dies — the window does not vanish under the bar', async () => {
    const view = render(input('s1'));
    await waitFor(() =>
      expect(screen.getByTestId('context-indicator').getAttribute('data-limit')).toBe('262144')
    );
    served = splitServing(null, 'unknown');
    view.rerender(input('s1'));
    // Give the re-read (servedAt changed) its turn, then the counter must still be there.
    await new Promise((r) => setTimeout(r, 20));
    expect(screen.getByTestId('context-indicator').getAttribute('data-limit')).toBe('262144');
  });

  it('another model never inherits the window: no counter until its engine reports one', async () => {
    const view = render(input('s1'));
    await waitFor(() => expect(screen.getByTestId('context-indicator')).toBeInTheDocument());
    served = splitServing(null, 'unknown');
    view.rerender(input('s1', 'another-model'));
    await waitFor(() => expect(screen.queryByTestId('context-indicator')).toBeNull());
  });

  it('a chat whose engine never reported a window shows no counter (never a default)', async () => {
    served = splitServing(null, 'unknown');
    render(input('s2'));
    await new Promise((r) => setTimeout(r, 20));
    expect(screen.queryByTestId('context-indicator')).toBeNull();
  });
});

/**
 * Q-6, round live-1: a "Coding · Agent" toggle sat in the swarm composer. LEANZERO_PERSONA was read
 * nowhere — a send did the same thing either way — and "Agent" promised "Autonomous — runs a loop".
 * No mode that changes nothing: the bar carries no toggle, only the launcher, one click away.
 */
describe('the swarm composer has no mode toggle, only a launcher (Q-6)', () => {
  beforeEach(() => {
    served = splitServing(null, 'unknown');
  });

  it('no pressed-button pair and no "Persona" group', async () => {
    render(input('s1', 'swarm-model', 'swarm'));
    await screen.findByTestId('recipes-and-loops');
    expect(screen.queryByRole('group', { name: 'Persona' })).toBeNull();
    expect(screen.queryByRole('button', { name: /^(Coding|Agent)$/ })).toBeNull();
    expect(document.querySelector('[aria-pressed]')).toBeNull();
  });

  it('the launcher is there from the start and opens the hub in one click', async () => {
    render(input('s1', 'swarm-model', 'swarm'));
    fireEvent.click(await screen.findByTestId('recipes-and-loops'));
    expect(screen.getByTestId('recipes-and-loops-open')).toBeInTheDocument();
  });

  // Q-7: "Set up agent" opened a recipes/loops hub, not agent settings. It is named for what it opens.
  it('the launcher is named for what it opens — never "agent"', async () => {
    render(input('s1', 'swarm-model', 'swarm'));
    const launcher = await screen.findByTestId('recipes-and-loops');
    expect(launcher).toHaveTextContent('Recipes & loops');
    expect(launcher.getAttribute('title')).toBe(
      'Build a recipe, then run it in a loop on a schedule'
    );
    expect(launcher.textContent + (launcher.getAttribute('title') ?? '')).not.toMatch(/agent/i);
  });
});
