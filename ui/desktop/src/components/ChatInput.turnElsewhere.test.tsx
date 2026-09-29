import React from 'react';
import { fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import ChatInput from './ChatInput';
import { ChatState } from '../types/chatState';
import { IntlTestWrapper } from '../i18n/test-utils';
import type { ChatServedBy } from './chatServedBy/chatServedBy';
import {
  activeSessions,
  activityOf,
  getSessionActivitySnapshot,
  resetSessionActivityForTests,
  seedSessionActivityForTests,
} from './sessionActivity/sessionActivityStore';

/**
 * Q-500 (3.0.78, 15:48): "Coffee Roasters Double-Charge Incident" (20260929_15) had its turn in the
 * engine — a 200.8K-token prompt, 17m 46s in — sent from the main window, and was opened in a SECOND
 * window. Every window has its own ACP connection and goosed keeps a busy set per connection, so the
 * second window's read listed only the Jira chat (which ran on goosed's process-wide agents): no
 * Stop, no Running pill, "18m ago" in the sidebar, Coffee missing from Active now — an idle composer
 * whose send would have started a second turn on the same chat.
 */

const COFFEE = '20260929_15';
const JIRA = '20260928_19';
const MAIN_WINDOW = 1;
const COFFEE_STARTED = '2026-09-29T12:30:14+00:00';
const JIRA_STARTED = '2026-09-29T12:31:02+00:00';

const turnInFlightSeen: boolean[] = [];
vi.mock('./chatServedBy/useChatServedBy', () => ({
  useChatServedBy: (_provider: unknown, _sessionId: unknown, turnInFlight: boolean) => {
    turnInFlightSeen.push(turnInFlight);
    return {
      served: {
        engine: 'none',
        model: null,
        where: [],
        peerNodeId: null,
        foreign: false,
        contextWindow: null,
        phase: null,
        activity: null,
        work: null,
        busyWithOthers: null,
        busyIn: null,
        turnRequest: null,
        turnWait: null,
        readTps: null,
        readiness: { kind: 'ready' },
      } as unknown as ChatServedBy,
      single: null,
      armed: false,
      turnInFlight,
    };
  },
}));
const { acpCancelPrompt } = vi.hoisted(() => ({
  acpCancelPrompt: vi.fn(async (_sessionId: string) => {}),
}));
vi.mock('../acp/prompt', async (original) => ({
  ...(await original<typeof import('../acp/prompt')>()),
  acpCancelPrompt,
}));
vi.mock('./swarm/swarmContextLimit', () => ({ fetchSwarmPoolContextLimit: async () => null }));
vi.mock('./noNodeNotice/ComposerReadiness', () => ({ ComposerReadinessStrip: () => null }));
vi.mock('./bottom_menu/ContextWindowIndicator', () => ({ ContextWindowIndicator: () => null }));
vi.mock('./settings/models/bottom_bar/ModelsBottomBar', () => ({ default: () => null }));
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
    getCurrentModelAndProvider: async () => ({ model: 'swarm', provider: 'swarm' }),
    currentModel: 'swarm',
    currentProvider: 'swarm',
  }),
}));
vi.mock('./alerts', () => ({
  useAlerts: () => ({ alerts: [], addAlert: vi.fn(), clearAlerts: vi.fn() }),
  AlertType: { Error: 'error', Warning: 'warning', Info: 'info' },
}));
vi.mock('./ui/ReportProblemDialog', async (original) => ({
  ...(await original<typeof import('./ui/ReportProblemDialog')>()),
  ReportProblemDialog: () => null,
}));

class ResizeObserverMock {
  observe() {}
  unobserve() {}
  disconnect() {}
}
beforeAll(() => {
  vi.stubGlobal('ResizeObserver', ResizeObserverMock);
});

const bridge = { stopTurnElsewhere: vi.fn(), showTurnWindow: vi.fn() };
const original = (window as unknown as { electron: object }).electron;

/** The second window's store: its own goosed read, and main's push of the main window's rows. */
function secondWindow(): void {
  seedSessionActivityForTests({
    running: [
      { sessionId: JIRA, sessionName: 'Jira DC to Cloud migration assessment', workingDir: '/w', startedAt: JIRA_STARTED },
    ],
    elsewhere: [
      {
        sessionId: COFFEE,
        sessionName: 'Coffee Roasters Double-Charge Incident',
        workingDir: '/w',
        startedAt: COFFEE_STARTED,
        window: MAIN_WINDOW,
      },
    ],
  });
}

const coffeeComposer = (chatState = ChatState.Idle) => (
  <IntlTestWrapper>
    <ChatInput
      sessionId={COFFEE}
      handleSubmit={vi.fn()}
      chatState={chatState}
      onStop={vi.fn()}
      setView={vi.fn()}
      sessionModel="swarm"
      sessionProvider="swarm"
      sessionLoaded
      workingDir="/w"
    />
  </IntlTestWrapper>
);

beforeEach(() => {
  turnInFlightSeen.length = 0;
  acpCancelPrompt.mockClear();
  bridge.stopTurnElsewhere.mockReset();
  bridge.showTurnWindow.mockReset();
  (window as unknown as { electron: unknown }).electron = { ...original, ...bridge };
});
afterEach(() => {
  resetSessionActivityForTests();
  (window as unknown as { electron: unknown }).electron = original;
});

describe('Q-500: a chat whose turn another window runs reads RUNNING here', () => {
  it('the store: Running since the turn began, in Active now, held by the main window', () => {
    secondWindow();
    const state = getSessionActivitySnapshot();
    expect(activityOf(state, COFFEE).runningSince).toBe(COFFEE_STARTED);
    expect(activityOf(state, COFFEE).turnWindow).toBe(MAIN_WINDOW);
    expect(activeSessions(state).map((s) => s.sessionId)).toEqual([COFFEE, JIRA]);
    // The Jira row is this window's own read: no other window holds it.
    expect(activityOf(state, JIRA).turnWindow).toBeUndefined();
  });

  it('the composer: Stop, not Send, and it says the turn runs in another window', () => {
    secondWindow();
    render(coffeeComposer());
    expect(screen.getByRole('button', { name: 'Stop' })).toBeInTheDocument();
    expect(screen.getByTestId('turn-elsewhere')).toHaveTextContent('Running in another window');
    expect(screen.getByTestId('session-running-pill')).toBeInTheDocument();
    // The served-by derivation reads a turn in flight: the busy bar may not call it someone else's.
    expect(turnInFlightSeen[turnInFlightSeen.length - 1]).toBe(true);
  });

  it('Stop reaches the window whose connection holds the turn; Show brings it forward', () => {
    secondWindow();
    const onStop = vi.fn();
    render(
      <IntlTestWrapper>
        <ChatInput
          sessionId={COFFEE}
          handleSubmit={vi.fn()}
          chatState={ChatState.Idle}
          onStop={onStop}
          setView={vi.fn()}
          sessionModel="swarm"
          sessionProvider="swarm"
          sessionLoaded
          workingDir="/w"
        />
      </IntlTestWrapper>
    );
    fireEvent.click(screen.getByRole('button', { name: 'Stop' }));
    expect(bridge.stopTurnElsewhere).toHaveBeenCalledWith(COFFEE);
    expect(onStop).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'Show that window' }));
    expect(bridge.showTurnWindow).toHaveBeenCalledWith(COFFEE);
  });

  it('the window that holds the turn keeps its own Stop and no bar', () => {
    secondWindow();
    const onStop = vi.fn();
    render(
      <IntlTestWrapper>
        <ChatInput
          sessionId={COFFEE}
          handleSubmit={vi.fn()}
          chatState={ChatState.Streaming}
          onStop={onStop}
          setView={vi.fn()}
          sessionModel="swarm"
          sessionProvider="swarm"
          sessionLoaded
          workingDir="/w"
        />
      </IntlTestWrapper>
    );
    expect(screen.queryByTestId('turn-elsewhere')).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Stop' }));
    expect(onStop).toHaveBeenCalled();
    expect(bridge.stopTurnElsewhere).not.toHaveBeenCalled();
  });

  it('once the other window’s turn ends, the composer is idle again', () => {
    seedSessionActivityForTests({ running: [], elsewhere: [] });
    render(coffeeComposer());
    expect(screen.queryByRole('button', { name: 'Stop' })).toBeNull();
    expect(screen.queryByTestId('turn-elsewhere')).toBeNull();
    expect(turnInFlightSeen[turnInFlightSeen.length - 1]).toBe(false);
  });
});

/**
 * Q-504: a turn goosed runs on its process-wide agents — one NO window sent, like the Jira chat at
 * 15:48 — is in every window's own read, with no window holding its prompt. The composer was idle
 * (sendable, no Stop), and even a relayed Stop had nowhere to go: goosed's cancel reached only a
 * connection's own prompts. Now the composer is busy and Stop cancels through this window's own
 * connection, which reaches the process-wide agents.
 */
describe('Q-504: a chat whose turn no window sent reads RUNNING, and Stop stops it', () => {
  const jiraComposer = (onStop = vi.fn()) => (
    <IntlTestWrapper>
      <ChatInput
        sessionId={JIRA}
        handleSubmit={vi.fn()}
        chatState={ChatState.Idle}
        onStop={onStop}
        setView={vi.fn()}
        sessionModel="swarm"
        sessionProvider="swarm"
        sessionLoaded
        workingDir="/w"
      />
    </IntlTestWrapper>
  );

  it('the composer: Stop, not Send, and it says the turn runs in the background', () => {
    secondWindow();
    render(jiraComposer());
    const stop = screen.getByRole('button', { name: 'Stop' });
    expect(stop).toHaveAttribute('title', 'Stop the turn goose is running in the background');
    expect(screen.getByTestId('turn-elsewhere')).toHaveTextContent('Running in the background');
    // No window holds it, so there is no window to show.
    expect(screen.queryByRole('button', { name: 'Show that window' })).toBeNull();
    expect(turnInFlightSeen[turnInFlightSeen.length - 1]).toBe(true);
  });

  it('Stop cancels through this window’s own connection, never a relay to a window', () => {
    secondWindow();
    const onStop = vi.fn();
    render(jiraComposer(onStop));
    fireEvent.click(screen.getByRole('button', { name: 'Stop' }));
    expect(acpCancelPrompt).toHaveBeenCalledWith(JIRA);
    expect(bridge.stopTurnElsewhere).not.toHaveBeenCalled();
    expect(onStop).not.toHaveBeenCalled();
  });

  it('a row another window lists that no window holds (a linked Mac’s run) stops the same way', () => {
    seedSessionActivityForTests({
      running: [],
      elsewhere: [
        {
          sessionId: JIRA,
          sessionName: 'Jira DC to Cloud migration assessment',
          workingDir: '/w',
          startedAt: JIRA_STARTED,
          window: null,
        },
      ],
    });
    render(jiraComposer());
    expect(screen.getByTestId('turn-elsewhere')).toHaveTextContent('Running in the background');
    fireEvent.click(screen.getByRole('button', { name: 'Stop' }));
    expect(acpCancelPrompt).toHaveBeenCalledWith(JIRA);
    expect(bridge.stopTurnElsewhere).not.toHaveBeenCalled();
  });

  it('once the turn ends, the composer is idle again', () => {
    seedSessionActivityForTests({ running: [], elsewhere: [] });
    render(jiraComposer());
    expect(screen.queryByRole('button', { name: 'Stop' })).toBeNull();
    expect(screen.queryByTestId('turn-elsewhere')).toBeNull();
  });
});
