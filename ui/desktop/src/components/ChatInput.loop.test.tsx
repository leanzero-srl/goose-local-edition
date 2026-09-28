import React from 'react';
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi, type Mock } from 'vitest';
import ChatInput from './ChatInput';
import { ChatState } from '../types/chatState';
import type { Message, UserInput } from '../types/message';
import type { ChatServedBy } from './chatServedBy/chatServedBy';
import { IntlTestWrapper } from '../i18n/test-utils';
import { allClasses, assertStudioClean } from './lz/assertStudioClean';
import { missingUtilities } from './lz/compileStudioCss';
import { LoopSessionContext, type LoopSessionValue } from './loops/startLoopRequest';
import type { SessionLoop } from './loops/useSessionLoop';
import { LOOP_MESSAGES, NOW_MS, loopRecord, waitingRecord } from './loops/railFixtures';
import { TEMPLATES } from './loops/startLoopFixtures';
import { getPendingUserInput } from './loops/pendingUserInput';
import { onOpenLoopRailRequest } from './loops/loopRailRequest';

/**
 * Q-228 L4 — the composer's side of a session loop (DESIGN-SESSION-LOOPS §7.1, §7.2, §8.1, §5.3):
 * the Loop button in the slot "Recipes & loops" left, the status chip, what a tick running here
 * changes (placeholder, Stop, the queue's words), `/loop` controls that never wait in the queue,
 * and the queued count the tick door reads.
 */

const loops = vi.hoisted(() => ({
  get: vi.fn(),
  control: vi.fn(),
  templates: vi.fn(),
  start: vi.fn(),
  update: vi.fn(),
}));

vi.mock('../acp/loops', () => ({
  loopsGet: loops.get,
  loopsControl: loops.control,
  loopsTemplates: loops.templates,
  loopsStart: loops.start,
  loopsUpdate: loops.update,
}));
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

let barWidth = 900;
class ResizeObserverMock {
  constructor(
    private readonly callback: (entries: Array<{ contentRect: { width: number } }>) => void
  ) {}
  observe() {
    this.callback([{ contentRect: { width: barWidth } }]);
  }
  unobserve() {}
  disconnect() {}
}

beforeEach(() => {
  barWidth = 900;
  vi.stubGlobal('ResizeObserver', ResizeObserverMock);
  vi.useFakeTimers({ toFake: ['Date'] });
  vi.setSystemTime(NOW_MS);
  vi.spyOn(Date.prototype, 'getTimezoneOffset').mockReturnValue(0);
  for (const fn of Object.values(loops)) fn.mockReset();
  loops.templates.mockResolvedValue({ templates: TEMPLATES });
  window.electron = {
    ...window.electron,
    getWakelockState: vi.fn().mockResolvedValue({ enabled: false, holding: false, error: null }),
    setWakelock: vi.fn(),
  } as typeof window.electron;
});
afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

function mount({
  state = { kind: 'none' } as SessionLoop,
  chatState = ChatState.Idle,
  messages = [] as Message[],
  model = 'swarm',
  handleSubmit = vi.fn<(input: UserInput) => void>(),
  onStop = vi.fn<() => void>(),
}: {
  state?: SessionLoop;
  chatState?: ChatState;
  messages?: Message[];
  model?: string;
  handleSubmit?: Mock<(input: UserInput) => void>;
  onStop?: Mock<() => void>;
} = {}) {
  const reload = vi.fn();
  const value: LoopSessionValue = {
    sessionId: 's1',
    loop: state.kind === 'loop' ? state.loop : null,
    state,
    reload,
  };
  const view = render(
    <IntlTestWrapper>
      <LoopSessionContext.Provider value={value}>
        <ChatInput
          sessionId="s1"
          handleSubmit={handleSubmit}
          chatState={chatState}
          onStop={onStop}
          setView={vi.fn()}
          sessionModel={model}
          sessionProvider="swarm"
          sessionLoaded
          workingDir="/w"
          messages={messages}
        />
      </LoopSessionContext.Provider>
    </IntlTestWrapper>
  );
  return { ...view, reload, handleSubmit, onStop };
}

const running: SessionLoop = { kind: 'loop', loop: loopRecord(), status: 'running' };

const typeAndSend = (text: string) => {
  const input = screen.getByTestId('chat-input');
  fireEvent.change(input, { target: { value: text } });
  fireEvent.keyDown(input, { key: 'Enter' });
};

describe('the composer Loop slot (§7.1, §8.1)', () => {
  it('is the Loop button, between the model chip and the folder, and opens the Start dialog', async () => {
    const { container } = mount();
    const button = screen.getByTestId('composer-loop-button');
    expect(button).toHaveTextContent('Loop');
    expect(button).toHaveAccessibleName(
      "Run this chat's goal again and again — each run is a tick"
    );
    const order = Array.from(
      container.querySelectorAll(
        '[data-testid="models-bottom-bar"], [data-testid="composer-loop-button"], [data-testid="dir-switcher"]'
      )
    ).map((el) => el.getAttribute('data-testid'));
    expect(order).toEqual(['models-bottom-bar', 'composer-loop-button', 'dir-switcher']);
    expect(screen.queryByText('Recipes & loops')).toBeNull();

    fireEvent.click(button);
    const dialog = await screen.findByTestId('start-loop-dialog');
    expect(within(dialog).getByRole('heading', { name: 'Loop this chat' })).toBeInTheDocument();
    await waitFor(() => expect(within(dialog).getByTestId('loop-goal')).toBeInTheDocument());
    // The cost line names what the model chip names — one derivation (servedChipWords).
    expect(within(dialog).getByTestId('loop-cost-line')).toHaveTextContent(
      /^Each tick is one turn on Qwen3\.6-27B.* · This Mac and Work\./
    );
    expect(within(dialog).getByTestId('loop-state-file')).toHaveValue('.goose/loops/loop/NOW.md');
    expect(within(dialog).getByText(/Runs in \/w after every tick/)).toBeInTheDocument();
    assertStudioClean(document.body.querySelector('[data-testid="start-loop-dialog"]')!);
  });

  it('is disabled on a swarm-build chat, with the refusal as its name', () => {
    mount({ model: 'swarm-build' });
    const button = screen.getByTestId('composer-loop-button');
    expect(button).toBeDisabled();
    expect(button).toHaveAccessibleName(
      'Loops run chat turns. This chat builds with the swarm — every tick would start a full build. Use Agent Work for recurring builds.'
    );
  });

  it('is nothing while the loop is still read, and hides when the bar is narrow', () => {
    const { unmount } = mount({ state: { kind: 'loading' } });
    expect(screen.queryByTestId('composer-loop-button')).toBeNull();
    expect(screen.queryByTestId('composer-loop-chip')).toBeNull();
    unmount();
    barWidth = 400;
    mount({ state: running });
    expect(screen.queryByTestId('composer-loop-chip')).toBeNull();
  });

  it('with a loop is its status as a solid chip that opens the rail on Loop', () => {
    const opened: string[] = [];
    const off = onOpenLoopRailRequest((sessionId) => {
      opened.push(sessionId);
      return true;
    });
    const { unmount } = mount({ state: running });
    const chip = screen.getByTestId('composer-loop-chip');
    expect(chip).toHaveTextContent('Tick 5 running · 2m');
    expect(chip).toHaveAttribute('data-tone', 'ok');
    expect(chip.className).toContain('bg-lz-ok-solid');
    fireEvent.click(chip);
    expect(opened).toEqual(['s1']);
    off();
    unmount();

    for (const [state, text, tone] of [
      [{ kind: 'loop', loop: waitingRecord(), status: 'waiting' }, 'Next tick 22:51', 'accent'],
      [{ kind: 'loop', loop: waitingRecord(), status: 'paused' }, 'Loop paused', 'stopped'],
      [{ kind: 'loop', loop: waitingRecord(), status: 'needs_you' }, 'Loop needs you', 'warn'],
      [{ kind: 'loop', loop: waitingRecord(), status: 'ended' }, 'Loop ended', 'stopped'],
      [{ kind: 'unreadable', error: 'bad json' }, 'Loop unreadable', 'err'],
    ] as const) {
      const view = mount({ state: state as SessionLoop });
      const c = screen.getByTestId('composer-loop-chip');
      expect(c, text).toHaveTextContent(text);
      expect(c, text).toHaveAttribute('data-tone', tone);
      view.unmount();
    }
  });

  it('every class the slot and the dialog emit compiles against main.css', async () => {
    const { container, unmount } = mount({ state: running });
    const chipClasses = allClasses(container);
    unmount();
    mount();
    fireEvent.click(screen.getByTestId('composer-loop-button'));
    const dialog = await screen.findByTestId('start-loop-dialog');
    await waitFor(() => expect(within(dialog).getByTestId('loop-goal')).toBeInTheDocument());
    fireEvent.change(within(dialog).getByTestId('loop-goal'), { target: { value: 'Ship it' } });
    const classes = [...new Set([...chipClasses, ...allClasses(document.body)])].filter(
      (c) => !c.startsWith('lucide') && c !== 'page-transition'
    );
    expect(await missingUtilities(classes)).toEqual([]);
  }, 30_000);
});

describe('the composer while a tick runs (§5.3, §8.1)', () => {
  it('names the tick in the placeholder and on Stop, which pauses the loop', () => {
    const { onStop } = mount({
      state: running,
      chatState: ChatState.Streaming,
      messages: LOOP_MESSAGES,
    });
    expect(screen.getByTestId('chat-input')).toHaveAttribute(
      'placeholder',
      'Tick 5 is running — what you send waits for it. Use Send now to steer it.'
    );
    const stop = screen.getByRole('button', { name: 'Stop tick 5 — the loop pauses' });
    fireEvent.click(stop);
    expect(onStop).toHaveBeenCalledTimes(1);
  });

  it('a person’s own turn keeps the ordinary placeholder and Stop', () => {
    mount({
      state: { kind: 'loop', loop: waitingRecord(), status: 'waiting' },
      chatState: ChatState.Streaming,
      messages: [
        ...LOOP_MESSAGES,
        {
          id: 'u1',
          role: 'user',
          created: 0,
          content: [{ type: 'text', text: 'what did tick 5 change?' }],
          metadata: { userVisible: true, agentVisible: true },
        },
      ],
    });
    expect(screen.getByTestId('chat-input')).toHaveAttribute(
      'placeholder',
      'Ask goose to build, fix or explain something'
    );
    expect(screen.getByRole('button', { name: 'Stop' })).toBeInTheDocument();
  });

  it('says a tick finishes in the background when goosed runs one this window never ran', () => {
    mount({ state: running, chatState: ChatState.Idle, messages: [] });
    expect(screen.getByTestId('chat-input')).toHaveAttribute(
      'placeholder',
      'Tick 5 is finishing in the background — you can send when it ends'
    );
  });

  it('queues a message typed during the tick, names the tick Send now steers, and tells the tick door', () => {
    const { handleSubmit, unmount } = mount({
      state: running,
      chatState: ChatState.Streaming,
      messages: LOOP_MESSAGES,
    });
    typeAndSend('also cover svc- accounts');
    expect(handleSubmit).not.toHaveBeenCalled();
    const label = screen.getAllByTestId('queue-steers-tick')[0];
    expect(label).toHaveTextContent('Queued · Send now steers tick 5');
    expect(label.className).toContain('bg-lz-accent');
    expect(screen.getByTitle('Send now — this steers tick 5')).toBeInTheDocument();
    expect(getPendingUserInput('s1')).toBe(1);
    typeAndSend('and keep the seed');
    expect(getPendingUserInput('s1')).toBe(2);
    unmount();
    expect(getPendingUserInput('s1')).toBe(0);
  });

  it('/loop stop during a tick goes straight to goosed — never into the queue — and says what it answered', async () => {
    loops.control.mockResolvedValue({
      loop: waitingRecord({ status: 'ended', statusReason: { kind: 'stopped_by_you', n: 5 } }),
    });
    const { handleSubmit, onStop, reload } = mount({
      state: running,
      chatState: ChatState.Streaming,
      messages: LOOP_MESSAGES,
    });
    typeAndSend('/loop stop');
    await waitFor(() => expect(loops.control).toHaveBeenCalledWith('s1', 'stop'));
    expect(await screen.findByTestId('loop-reply')).toHaveTextContent('Loop stopped after tick 5.');
    expect(screen.queryByTestId('queue-steers-tick')).toBeNull();
    expect(getPendingUserInput('s1')).toBe(0);
    expect(handleSubmit).not.toHaveBeenCalled();
    // "stop" is an interruption word: the control must not ALSO stop the turn as a typed "stop".
    expect(onStop).not.toHaveBeenCalled();
    expect(reload).toHaveBeenCalled();
    expect(screen.getByTestId('chat-input')).toHaveValue('');
    fireEvent.click(screen.getByRole('button', { name: 'Dismiss' }));
    expect(screen.queryByTestId('loop-reply')).toBeNull();
  });

  it('/loop pause and /loop during a tick answer in /loop’s words, a refusal in goosed’s', async () => {
    loops.control.mockResolvedValue({
      refusal: { code: 'runner_absent', reason: 'The loop runner is not in this build' },
    });
    loops.get.mockResolvedValue({ loop: loopRecord() });
    mount({ state: running, chatState: ChatState.Streaming, messages: LOOP_MESSAGES });
    typeAndSend('/loop pause');
    await waitFor(() => expect(loops.control).toHaveBeenCalledWith('s1', 'pause'));
    const reply = await screen.findByTestId('loop-reply');
    expect(reply).toHaveTextContent('The loop runner is not in this build');
    expect(reply).toHaveAttribute('data-refused', 'true');
    typeAndSend('/loop');
    await waitFor(() => expect(loops.get).toHaveBeenCalledWith('s1'));
    await waitFor(() =>
      expect(screen.getByTestId('loop-reply')).toHaveTextContent(
        /^Loop: Make scripts\/generate_users\.js produce every problem class in notes\/kickoff\.md · Tick 5 · started 22:41 · 2m · tick 5$/
      )
    );
  });

  it('/loop with a goal during a tick is queued like any message (starting a loop is not urgent)', () => {
    mount({ state: running, chatState: ChatState.Streaming, messages: LOOP_MESSAGES });
    typeAndSend('/loop fix the tests');
    expect(loops.control).not.toHaveBeenCalled();
    expect(loops.get).not.toHaveBeenCalled();
    expect(screen.getAllByTestId('queue-steers-tick')).not.toHaveLength(0);
    expect(getPendingUserInput('s1')).toBe(1);
  });

  it('/loop pause between turns is an ordinary message: goosed’s /loop answers it', () => {
    const { handleSubmit } = mount({
      state: { kind: 'loop', loop: waitingRecord(), status: 'waiting' },
    });
    typeAndSend('/loop pause');
    expect(handleSubmit).toHaveBeenCalledWith({ msg: '/loop pause', images: [] });
    expect(loops.control).not.toHaveBeenCalled();
  });
});

describe('the composer outside a chat', () => {
  it('has no Loop slot without the chat’s loop context', async () => {
    render(
      <IntlTestWrapper>
        <ChatInput
          sessionId="s1"
          handleSubmit={vi.fn()}
          chatState={ChatState.Idle}
          setView={vi.fn()}
          sessionModel="swarm"
          sessionProvider="swarm"
          sessionLoaded
          workingDir="/w"
        />
      </IntlTestWrapper>
    );
    await act(async () => undefined);
    expect(screen.queryByTestId('composer-loop-button')).toBeNull();
  });
});
