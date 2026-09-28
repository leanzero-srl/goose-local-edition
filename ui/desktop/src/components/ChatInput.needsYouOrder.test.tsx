import React, { useRef, useState } from 'react';
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import ChatInput from './ChatInput';
import NeedsYouTray from './sessionActivity/NeedsYouCard';
import { ChatState } from '../types/chatState';
import type { ChatServedBy } from './chatServedBy/chatServedBy';
import { IntlTestWrapper } from '../i18n/test-utils';
import {
  resetSessionActivityForTests,
  seedSessionActivityForTests,
  type NeedsYouItemDto,
} from './sessionActivity/sessionActivityStore';
import {
  answersWaiting,
  resetAnswerQueuesForTests,
  useAnswerQueue,
} from './sessionActivity/needsYouAnswerQueue';
import { getPendingUserInput } from './loops/pendingUserInput';

/**
 * Q-341 — THE ORDER when a turn ends with both a needs-you answer and a composer message queued:
 * the answer (resolved on the engine, then sent) goes first; the composer's message waits for the
 * answer's turn to end. The reverse lets the message supersede the question the person answered
 * (Q-298 closes every open question when a message arrives) and the answer is refused as closed.
 *
 * The chat here refuses a message while a turn runs, exactly as useChatSession.handleSubmit does
 * (it returns without sending), so a message sent into a busy chat shows up in `refused`.
 */

const acp = vi.hoisted(() => ({
  acpSessionActivity: vi.fn(),
  acpResolveNeedsYou: vi.fn(),
}));
vi.mock('../acp/needsYou', () => acp);
vi.mock('./engineGlance/glanceStore', () => ({ useGlanceNodes: () => ({ kind: 'unread' }) }));
const served = vi.hoisted(
  () =>
    ({
      engine: 'split',
      model: 'mlx-community/Qwen3.6-27B-8bit',
      where: ['This Mac', 'Work'],
      peerNodeId: null,
      foreign: false,
      contextWindow: 262144,
      phase: 'idle',
      activity: null,
      work: null,
      busyWithOthers: null,
      busyIn: null,
      turnRequest: null,
      readTps: null,
      readiness: { kind: 'ready' },
    }) as unknown as ChatServedBy
);
vi.mock('./chatServedBy/useChatServedBy', () => ({
  useChatServedBy: () => ({ served, single: null, armed: true, turnInFlight: false }),
}));
vi.mock('./noNodeNotice/ComposerReadiness', () => ({ ComposerReadinessStrip: () => null }));
vi.mock('./settings/models/bottom_bar/ModelsBottomBar', () => ({
  default: () => <div data-testid="models-bottom-bar" />,
}));
vi.mock('./bottom_menu/DirSwitcher', () => ({
  DirSwitcher: () => <div data-testid="dir-switcher" />,
}));
vi.mock('./bottom_menu/CostTracker', () => ({ CostTracker: () => null }));
vi.mock('./bottom_menu/ContextWindowIndicator', () => ({ ContextWindowIndicator: () => null }));
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
vi.mock('../acp/providers', () => ({ acpListProviderDetails: async () => [] }));
vi.mock('../utils/canonical', () => ({ fetchCanonicalModelInfo: async () => null }));
vi.mock('../acp/mlx-engine', () => ({
  mlxEngineStatus: async () => ({ state: 'stopped', restartRequired: false, availableMemoryGb: 0 }),
}));
vi.mock('./swarm/useFleet', () => ({ fetchSwarmContextLimit: async () => null }));
vi.mock('./swarm/swarmContextLimit', () => ({ fetchSwarmPoolContextLimit: async () => null }));
vi.mock('../acp/diagnostics', () => ({ getDiagnosticsReport: vi.fn() }));

class ResizeObserverMock {
  constructor(
    private readonly callback: (entries: Array<{ contentRect: { width: number } }>) => void
  ) {}
  observe() {
    this.callback([{ contentRect: { width: 900 } }]);
  }
  unobserve() {}
  disconnect() {}
}

const LEAD: NeedsYouItemDto = {
  id: 'ny_lead',
  sessionId: 'jira',
  sessionName: 'Jira Migration Assessment',
  workingDir: '/w',
  question: 'An inactive project lead — which decision does the script give them?',
  why: 'The stated rules conflict on exactly that case.',
  recommendedAnswer: 'migrate — apply the lead override in every case',
  options: ['skip unless they have an email'],
  createdAt: '2026-09-28T09:08:00Z',
  status: 'open',
};
const ANSWER = `Answer to your question "${LEAD.question}": ${LEAD.recommendedAnswer}`;

const chat = {
  sent: [] as string[],
  refused: [] as string[],
  setState: (_: ChatState) => {},
};

function Chat({ holdComposer }: { holdComposer: boolean }) {
  const [chatState, setChatState] = useState(ChatState.Streaming);
  const stateRef = useRef(chatState);
  chat.setState = (next) => {
    stateRef.current = next;
    setChatState(next);
  };
  const queue = useAnswerQueue('jira');
  const handleSubmit = (input: { msg: string }) => {
    if (stateRef.current !== ChatState.Idle) {
      chat.refused.push(input.msg);
      return;
    }
    chat.sent.push(input.msg);
    chat.setState(ChatState.Streaming);
  };
  return (
    <IntlTestWrapper>
      <NeedsYouTray
        sessionId="jira"
        chatState={chatState}
        sendAnswer={(text) => handleSubmit({ msg: text })}
      />
      <ChatInput
        sessionId="jira"
        handleSubmit={handleSubmit}
        chatState={chatState}
        queueHeld={holdComposer && answersWaiting(queue)}
        setView={vi.fn()}
        sessionModel="swarm"
        sessionProvider="swarm"
        sessionLoaded
        workingDir="/w"
        messages={[]}
      />
    </IntlTestWrapper>
  );
}

function queueBoth() {
  const input = screen.getByTestId('chat-input');
  fireEvent.change(input, { target: { value: 'also cover svc- accounts' } });
  fireEvent.keyDown(input, { key: 'Enter' });
  fireEvent.click(
    within(screen.getByTestId('needs-you-card')).getByTestId('needs-you-recommended')
  );
}

beforeEach(() => {
  vi.stubGlobal('ResizeObserver', ResizeObserverMock);
  window.localStorage.clear();
  chat.sent = [];
  chat.refused = [];
  acp.acpResolveNeedsYou.mockReset().mockResolvedValue(LEAD);
  acp.acpSessionActivity.mockReset().mockResolvedValue({ running: [], needsYou: [], failed: [] });
  seedSessionActivityForTests({ needsYou: [LEAD] });
});
afterEach(() => {
  resetSessionActivityForTests();
  resetAnswerQueuesForTests();
  vi.unstubAllGlobals();
});

describe('Q-341: a queued needs-you answer and a queued composer message', () => {
  it('the answer goes first (resolved, then sent); the message waits for the answer’s turn to end; nothing is refused', async () => {
    render(<Chat holdComposer />);
    queueBoth();
    expect(chat.sent).toEqual([]);
    // The tick door counts both: the composer's message and the card's answer.
    expect(getPendingUserInput('jira')).toBe(2);

    act(() => chat.setState(ChatState.Idle));
    await waitFor(() => expect(chat.sent).toEqual([ANSWER]));
    expect(acp.acpResolveNeedsYou).toHaveBeenCalledWith(
      'jira',
      LEAD.id,
      'answer',
      LEAD.recommendedAnswer
    );

    // The answer's turn runs; the composer message is still queued, not lost.
    await act(async () => {});
    expect(chat.sent).toEqual([ANSWER]);
    expect(getPendingUserInput('jira')).toBe(1);

    act(() => chat.setState(ChatState.Idle));
    await waitFor(() => expect(chat.sent).toEqual([ANSWER, 'also cover svc- accounts']));
    expect(chat.refused).toEqual([]);
  });

  it('without the hold the composer would go first and the answer be refused — why the hold exists', async () => {
    render(<Chat holdComposer={false} />);
    queueBoth();
    act(() => chat.setState(ChatState.Idle));
    await waitFor(() => expect(chat.refused).toEqual([ANSWER]));
    expect(chat.sent).toEqual(['also cover svc- accounts']);
  });
});
