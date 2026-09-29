import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useEffect } from 'react';
import { act, fireEvent, render, screen } from '@testing-library/react';
import { IntlProvider } from 'react-intl';

const acp = vi.hoisted(() => ({
  acpSessionActivity: vi.fn(),
  acpResolveNeedsYou: vi.fn(),
}));
vi.mock('../../acp/needsYou', () => acp);

import { UnansweredPromptLine } from './UnansweredPromptLine';
import { stoppedTurnOf, type TurnLiveness } from './unansweredPrompt';
import { acpChatSessionActions } from '../../acp/chatSessionStore';
import {
  refreshSessionActivity,
  resetSessionActivityForTests,
  seedSessionActivityForTests,
} from '../sessionActivity/sessionActivityStore';
import { ChatState } from '../../types/chatState';
import type { Message } from '../../types/message';
import type { Session } from '../../types/session';
import { TONE_FILL } from '../lz';
import { allClasses, assertStudioClean } from '../lz/assertStudioClean';
import { missingUtilities } from '../lz/compileStudioCss';

/**
 * Q-493, receipt: session 20260928_18 ("Lighthouse keeper story request"). The app was killed
 * mid-turn (Q-490); the last row in sessions.db is the person's prompt, with no assistant row and
 * no live turn. Reopened, the chat showed the prompt and nothing after it.
 */
const SESSION = '20260928_18';
const PROMPT =
  'Now write a 500-word story about a lighthouse keeper who finds a message in a bottle.';

function userText(id: string, text: string): Message {
  return {
    id,
    role: 'user',
    created: 1_790_000_000,
    content: [{ type: 'text', text }],
    metadata: { userVisible: true, agentVisible: true },
  };
}

function assistantText(id: string, text: string): Message {
  return {
    id,
    role: 'assistant',
    created: 1_790_000_010,
    content: [{ type: 'text', text }],
    metadata: { userVisible: true, agentVisible: true },
  };
}

function session(): Session {
  return {
    id: SESSION,
    name: 'Lighthouse keeper story request',
    created_at: '2026-09-28T18:00:00Z',
    updated_at: '2026-09-28T18:05:00Z',
    working_dir: '/tmp',
    message_count: 3,
    extension_data: {},
    source: 'test',
    conversation: [],
  } as unknown as Session;
}

/** The chat as the engine replays it on load: its rows, then the load finishing idle. */
function loadChat(messages: Message[]) {
  acpChatSessionActions.startSessionLoad(SESSION);
  acpChatSessionActions.setMessages(SESSION, messages);
  acpChatSessionActions.finishSessionLoad(SESSION, session());
}

const RECEIPT_ROWS = [
  userText('m1', 'Tell me about lighthouses.'),
  assistantText('m2', 'Lighthouses guide ships…'),
  userText('m3', PROMPT),
];

function renderLine(onResend = vi.fn(), sendBlocked = false) {
  return render(
    <IntlProvider locale="en" messages={{}}>
      <UnansweredPromptLine sessionId={SESSION} sendBlocked={sendBlocked} onResend={onResend} />
    </IntlProvider>
  );
}

beforeEach(() => {
  acpChatSessionActions.deleteSnapshot(SESSION);
});

afterEach(() => {
  resetSessionActivityForTests();
  acpChatSessionActions.deleteSnapshot(SESSION);
});

describe('Q-493: a prompt no turn answers is said, with a Resend', () => {
  it('the receipt: one honest line under the prompt, and Resend sends the same words', async () => {
    seedSessionActivityForTests({});
    loadChat(RECEIPT_ROWS);
    const onResend = vi.fn();
    const { container } = renderLine(onResend);

    const line = await screen.findByTestId('unanswered-prompt');
    expect(line).toHaveTextContent('goose stopped before answering this');
    expect(line).toHaveTextContent(
      'The turn ended without a reply — the app may have closed while goose was working.'
    );
    fireEvent.click(screen.getByRole('button', { name: 'Resend' }));
    expect(onResend).toHaveBeenCalledWith({ msg: PROMPT, images: [] });
    assertStudioClean(container);
  });

  it('a turn starting hides it at once, and it stays gone while the turn streams', async () => {
    seedSessionActivityForTests({});
    loadChat(RECEIPT_ROWS);
    renderLine();
    await screen.findByTestId('unanswered-prompt');

    act(() => {
      acpChatSessionActions.startPromptAttempt(SESSION, 'attempt-1');
    });
    expect(screen.queryByTestId('unanswered-prompt')).toBeNull();
    expect(screen.queryByRole('button', { name: 'Resend' })).toBeNull();
  });

  it('a turn auto-started on load (a resumed chat) never flashes the line', async () => {
    seedSessionActivityForTests({});
    loadChat(RECEIPT_ROWS);
    // useAutoSubmit starts the turn from BaseChat's effect in the commit that found the chat idle.
    function AutoStart() {
      useEffect(() => {
        acpChatSessionActions.startPromptAttempt(SESSION, 'auto-start');
      }, []);
      return null;
    }
    const host = document.createElement('div');
    document.body.appendChild(host);
    // Every node ever committed under the host, so a line painted for one commit is still caught.
    let painted = 0;
    const count = (records: ReturnType<MutationObserver['takeRecords']>) =>
      records.forEach((record) =>
        record.addedNodes.forEach((node) => {
          if (!(node instanceof HTMLElement)) return;
          if (node.matches('[data-testid="unanswered-prompt"]')) painted += 1;
          painted += node.querySelectorAll('[data-testid="unanswered-prompt"]').length;
        })
      );
    const observer = new MutationObserver(count);
    observer.observe(host, { childList: true, subtree: true });
    render(
      <IntlProvider locale="en" messages={{}}>
        <UnansweredPromptLine sessionId={SESSION} sendBlocked={false} onResend={vi.fn()} />
        <AutoStart />
      </IntlProvider>,
      { container: host }
    );
    await act(async () => {});
    count(observer.takeRecords());
    observer.disconnect();
    expect(painted).toBe(0);
    expect(screen.queryByTestId('unanswered-prompt')).toBeNull();
  });

  it('never while the engine lists a turn running for the chat (another window, a schedule)', async () => {
    seedSessionActivityForTests({
      running: [{ sessionId: SESSION, startedAt: '2026-09-28T18:04:00Z' }] as never,
    });
    loadChat(RECEIPT_ROWS);
    renderLine();
    await act(async () => {});
    expect(screen.queryByTestId('unanswered-prompt')).toBeNull();

    // The run ends with no reply written: the engine's busy set drops it, and the line appears.
    act(() => {
      seedSessionActivityForTests({});
    });
    expect(await screen.findByTestId('unanswered-prompt')).toBeInTheDocument();
  });

  it('never before the engine was read — "running: []" is not asked yet, not "nothing runs"', async () => {
    let answer: (value: unknown) => void = () => {};
    acp.acpSessionActivity.mockReturnValue(new Promise((resolve) => (answer = resolve)));
    resetSessionActivityForTests();
    loadChat(RECEIPT_ROWS);
    renderLine();
    await act(async () => {});
    expect(screen.queryByTestId('unanswered-prompt')).toBeNull();

    await act(async () => {
      answer({ running: [], needsYou: [], failed: [] });
      await refreshSessionActivity();
    });
    expect(await screen.findByTestId('unanswered-prompt')).toBeInTheDocument();
  });

  it('a failed turn says it failed and why — never "the app may have closed"', async () => {
    seedSessionActivityForTests({
      failed: [
        { sessionId: SESSION, failedAt: '2026-09-28T18:05:00Z', reason: 'model unloaded' },
      ] as never,
    });
    loadChat(RECEIPT_ROWS);
    renderLine();
    const line = await screen.findByTestId('unanswered-prompt');
    expect(line).toHaveTextContent('The turn failed before a reply: model unloaded');
    expect(line).not.toHaveTextContent('the app may have closed');
  });

  it('a chat whose prompt was answered shows nothing', async () => {
    seedSessionActivityForTests({});
    loadChat([...RECEIPT_ROWS, assistantText('m4', 'The keeper, Ada, …')]);
    renderLine();
    await act(async () => {});
    expect(screen.queryByTestId('unanswered-prompt')).toBeNull();
  });

  it('Resend is disabled while a stop is still being cancelled', async () => {
    seedSessionActivityForTests({});
    loadChat(RECEIPT_ROWS);
    renderLine(vi.fn(), true);
    await screen.findByTestId('unanswered-prompt');
    expect(screen.getByRole('button', { name: 'Resend' })).toBeDisabled();
  });

  it('paints a SOLID stopped fill, and every class compiles', async () => {
    seedSessionActivityForTests({});
    loadChat(RECEIPT_ROWS);
    const { container } = renderLine();
    const line = await screen.findByTestId('unanswered-prompt');
    const pill = line.firstElementChild as HTMLElement;
    for (const cls of TONE_FILL.stopped.split(' ')) expect(pill.className).toContain(cls);
    expect(line.className).not.toMatch(/border-l|\/\d0\b|opacity-/);
    expect(
      await missingUtilities(allClasses(container).filter((c) => !c.startsWith('lucide')))
    ).toEqual([]);
  }, 30_000);
});

/**
 * Q-495, receipt: the #3w note turn Q-490 killed at req-1130 was between tool calls — the last rows
 * are the model's tool call and its result, with no reply after them. Reopened, the chat showed a
 * Completed tool card and nothing else, as if goose were about to continue.
 */
function toolCall(id: string, callId: string, words = ''): Message {
  return {
    id,
    role: 'assistant',
    created: 1_790_000_020,
    content: [
      ...(words ? [{ type: 'text', text: words }] : []),
      {
        type: 'toolRequest',
        id: callId,
        toolCall: { status: 'success', value: { name: 'developer__shell', arguments: {} } },
      },
    ],
    metadata: { userVisible: true, agentVisible: true },
  } as unknown as Message;
}

function toolResult(id: string, callId: string): Message {
  return {
    id,
    role: 'user',
    created: 1_790_000_030,
    content: [{ type: 'toolResponse', id: callId, toolResult: { status: 'success', value: [] } }],
    metadata: { userVisible: true, agentVisible: true },
  } as unknown as Message;
}

function reasoning(id: string): Message {
  return {
    id,
    role: 'assistant',
    created: 1_790_000_040,
    content: [{ type: 'thinking', thinking: 'Now let me read the notes file…', signature: '' }],
    metadata: { userVisible: true, agentVisible: true },
  } as unknown as Message;
}

function stoppedNotice(id: string): Message {
  return {
    id,
    role: 'assistant',
    created: 1_790_000_050,
    content: [
      {
        type: 'systemNotification',
        notificationType: 'inlineMessage',
        msg: 'You stopped this answer after 6 min',
        data: { kind: 'turnStopped', elapsedMs: 372_000 },
      },
    ],
    metadata: { userVisible: true, agentVisible: false },
  } as unknown as Message;
}

const MIDWAY_ROWS = [
  userText('m1', 'Deliver the note to the other chat.'),
  toolCall('m2', 'call-1', 'Let me find that chat first.'),
  toolResult('m3', 'call-1'),
];

describe('Q-495: a turn that died between tool calls is said, with a Continue', () => {
  it('the receipt: "goose stopped mid-way", and Continue sends a short continue through the door', async () => {
    seedSessionActivityForTests({});
    loadChat(MIDWAY_ROWS);
    const onResend = vi.fn();
    const { container } = renderLine(onResend);

    const line = await screen.findByTestId('unanswered-prompt');
    expect(line).toHaveTextContent('goose stopped mid-way — the app closed while it was working');
    expect(line).not.toHaveTextContent('goose stopped before answering this');
    expect(screen.queryByRole('button', { name: 'Resend' })).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Continue' }));
    expect(onResend).toHaveBeenCalledWith({ msg: 'Continue where you left off.', images: [] });
    assertStudioClean(container);
  });

  it('a chat that ends on the tool call itself (no result yet) is mid-way too', async () => {
    seedSessionActivityForTests({});
    loadChat(MIDWAY_ROWS.slice(0, 2));
    renderLine();
    const line = await screen.findByTestId('unanswered-prompt');
    expect(line).toHaveTextContent('goose stopped mid-way');
    expect(line).toHaveAttribute('data-kind', 'midway');
  });

  it('a turn starting hides it at once', async () => {
    seedSessionActivityForTests({});
    loadChat(MIDWAY_ROWS);
    renderLine();
    await screen.findByTestId('unanswered-prompt');
    act(() => {
      acpChatSessionActions.startPromptAttempt(SESSION, 'continue-1');
    });
    expect(screen.queryByTestId('unanswered-prompt')).toBeNull();
  });

  it('never while the engine lists the turn running (it is between tool calls, not dead)', async () => {
    seedSessionActivityForTests({
      running: [{ sessionId: SESSION, startedAt: '2026-09-28T18:04:00Z' }] as never,
    });
    loadChat(MIDWAY_ROWS);
    renderLine();
    await act(async () => {});
    expect(screen.queryByTestId('unanswered-prompt')).toBeNull();
  });

  it('a failed turn says it failed and why — never "the app closed"', async () => {
    seedSessionActivityForTests({
      failed: [
        { sessionId: SESSION, failedAt: '2026-09-28T18:05:00Z', reason: 'model unloaded' },
      ] as never,
    });
    loadChat(MIDWAY_ROWS);
    renderLine();
    const line = await screen.findByTestId('unanswered-prompt');
    expect(line).toHaveTextContent('goose stopped mid-way');
    expect(line).toHaveTextContent('The turn failed before a reply: model unloaded');
    expect(line).not.toHaveTextContent('the app closed');
    expect(screen.getByRole('button', { name: 'Continue' })).toBeInTheDocument();
  });

  it('Continue is disabled while a stop is still being cancelled', async () => {
    seedSessionActivityForTests({});
    loadChat(MIDWAY_ROWS);
    renderLine(vi.fn(), true);
    await screen.findByTestId('unanswered-prompt');
    expect(screen.getByRole('button', { name: 'Continue' })).toBeDisabled();
  });

  it('a turn the person stopped ends on its notice, and one that replied on words: nothing', async () => {
    seedSessionActivityForTests({});
    loadChat([...MIDWAY_ROWS, stoppedNotice('m4')]);
    renderLine();
    await act(async () => {});
    expect(screen.queryByTestId('unanswered-prompt')).toBeNull();

    act(() => {
      acpChatSessionActions.setMessages(SESSION, [
        ...MIDWAY_ROWS,
        assistantText('m4', 'Delivered.'),
      ]);
    });
    await act(async () => {});
    expect(screen.queryByTestId('unanswered-prompt')).toBeNull();
  });
});

describe('stoppedTurnOf — every live fact must say no turn runs', () => {
  const idle: TurnLiveness = {
    chatState: ChatState.Idle,
    activePromptAttemptId: null,
    activeRunId: null,
    pendingCancelPromptAttemptId: null,
    submitError: undefined,
    engineRead: true,
    engineRunning: false,
  };
  const rows = RECEIPT_ROWS;

  it('the receipt is an unanswered prompt', () => {
    expect(stoppedTurnOf(rows, idle)).toEqual({ kind: 'prompt', message: rows[2] });
  });

  it('the Q-495 receipt is mid-way, on the tool result', () => {
    expect(stoppedTurnOf(MIDWAY_ROWS, idle)).toEqual({ kind: 'midway', message: MIDWAY_ROWS[2] });
  });

  it.each<[string, Partial<TurnLiveness>]>([
    ['loading', { chatState: ChatState.LoadingConversation }],
    ['thinking', { chatState: ChatState.Thinking }],
    ['streaming', { chatState: ChatState.Streaming }],
    ['waiting on the person', { chatState: ChatState.WaitingForUserInput }],
    ['compacting', { chatState: ChatState.Compacting }],
    ['a prompt call in flight', { activePromptAttemptId: 'a' }],
    ['the engine run live', { activeRunId: 'run-1' }],
    ['a stop being cancelled', { pendingCancelPromptAttemptId: 'a' }],
    ['a failed submit (the banner says it)', { submitError: 'Submit error: closed' }],
    ['the engine not read yet', { engineRead: false }],
    ['the engine running it', { engineRunning: true }],
  ])('%s: neither unanswered nor mid-way', (_label, over) => {
    expect(stoppedTurnOf(rows, { ...idle, ...over })).toBeNull();
    expect(stoppedTurnOf(MIDWAY_ROWS, { ...idle, ...over })).toBeNull();
  });

  it('a tool result riding a user message is no prompt — it is a turn stopped mid-way', () => {
    const result = toolResult('t1', 'x');
    expect(stoppedTurnOf([...rows.slice(0, 2), result], idle)).toEqual({
      kind: 'midway',
      message: result,
    });
  });

  it('a tool call as the last row is mid-way', () => {
    const call = toolCall('t1', 'x', 'Checking the file.');
    expect(stoppedTurnOf([rows[0], call], idle)).toEqual({ kind: 'midway', message: call });
  });

  it('reasoning after the tool result is no reply: still mid-way', () => {
    expect(stoppedTurnOf([...MIDWAY_ROWS, reasoning('m4')], idle)).toEqual({
      kind: 'midway',
      message: MIDWAY_ROWS[2],
    });
  });

  it('words after the tool result, or the stopped notice, end the turn: nothing', () => {
    expect(stoppedTurnOf([...MIDWAY_ROWS, assistantText('m4', 'Done.')], idle)).toBeNull();
    expect(stoppedTurnOf([...MIDWAY_ROWS, stoppedNotice('m4')], idle)).toBeNull();
  });

  it('a hidden last row says nothing', () => {
    const hidden = { ...toolResult('t1', 'x'), metadata: { userVisible: false, agentVisible: true } };
    expect(stoppedTurnOf([...rows.slice(0, 2), hidden], idle)).toBeNull();
  });

  it('an empty chat has nothing unanswered', () => {
    expect(stoppedTurnOf([], idle)).toBeNull();
  });
});
