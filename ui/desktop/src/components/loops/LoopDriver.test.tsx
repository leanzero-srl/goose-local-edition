import { act, cleanup, render, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { getAcpClient, handleLoopsTickDue } from '../../acp/acpConnection';
import { acpChatSessionActions, acpChatSessionStore } from '../../acp/chatSessionStore';
import { loopsReady, loopsTickRefused } from '../../acp/loops';
import { acpPromptSession } from '../../acp/prompt';
import { acpLoadSession, sessionInfoToSession } from '../../acp/sessions';
import { ChatState } from '../../types/chatState';
import type { Message } from '../../types/message';
import type { Session } from '../../types/session';
import { LoopDriver } from './LoopDriver';
import { setPendingUserInput } from './pendingUserInput';

/**
 * Q-228 (L4r): the driver over the REAL chat store and the REAL submitMessage — only the wire
 * (the prompt call, the session load, the loops methods) is faked. What a tick looks like to the
 * window is what these assert: the prompt goosed receives, the marker in the transcript, and the
 * refusal/ready pair the runner re-offers on.
 */

const closeListeners = new Set<() => void>();

vi.mock('../../acp/acpConnection', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../acp/acpConnection')>();
  return {
    ...actual,
    getAcpClient: vi.fn(async () => ({})),
    onAcpConnectionClosed: vi.fn((listener: () => void) => {
      closeListeners.add(listener);
      return () => closeListeners.delete(listener);
    }),
  };
});
vi.mock('../../acp/prompt', () => ({ acpPromptSession: vi.fn(), acpCancelPrompt: vi.fn() }));
vi.mock('../../acp/sessions', () => ({
  acpLoadSession: vi.fn(),
  isAcpSessionLoadInFlight: vi.fn(() => false),
  sessionInfoToSession: vi.fn(),
  acpForkSession: vi.fn(),
  acpNewSession: vi.fn(),
  acpTruncateSessionConversation: vi.fn(),
}));
vi.mock('../../acp/loops', () => ({
  loopsTickRefused: vi.fn(async () => ({})),
  loopsReady: vi.fn(async () => ({ reoffered: true })),
}));
vi.mock('../../utils/extensionErrorUtils', () => ({ showExtensionLoadResults: vi.fn() }));

let counter = 0;

function freshSessionId(): string {
  counter += 1;
  return `loop-driver-${counter}`;
}

function session(id: string): Session {
  return { id, name: id, working_dir: '/tmp', message_count: 0 } as unknown as Session;
}

function offerFor(sessionId: string, n = 4) {
  const loopId = 'lp_0a1b2c3d';
  return {
    sessionId,
    loopId,
    n,
    messageId: `looptick_${loopId}_${n}_0190aaaa-bbbb-7ccc-8ddd-${String(counter).padStart(12, '0')}`,
    prompt: `Loop tick ${n} — "Make the generator produce every class" · every 10 min`,
  };
}

function deferred() {
  let resolve!: () => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<never>((res, rej) => {
    resolve = () => res({ stopReason: 'end_turn' } as never);
    reject = rej;
  });
  return { promise, resolve, reject };
}

function loaded(sessionId: string): void {
  acpChatSessionActions.finishSessionLoad(sessionId, session(sessionId));
}

async function offer(o: ReturnType<typeof offerFor>): Promise<void> {
  await act(async () => {
    await handleLoopsTickDue(o);
  });
}

function messages(sessionId: string): Message[] {
  return acpChatSessionStore.getSnapshot(sessionId)?.messages ?? [];
}

describe('LoopDriver', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    cleanup();
  });

  it('submits a tick for an idle chat through the prompt door, with _meta.goose.loopTick, and its marker shows', async () => {
    const sid = freshSessionId();
    loaded(sid);
    const turn = deferred();
    vi.mocked(acpPromptSession).mockReturnValue(turn.promise);
    render(<LoopDriver />);
    const o = offerFor(sid);

    await offer(o);

    await waitFor(() => expect(acpPromptSession).toHaveBeenCalledTimes(1));
    const [calledSession, sent, meta] = vi.mocked(acpPromptSession).mock.calls[0];
    expect(calledSession).toBe(sid);
    expect(meta).toEqual({
      goose: { loopTick: { loopId: o.loopId, n: o.n, messageId: o.messageId } },
    });
    expect(sent.content).toEqual([{ type: 'text', text: o.prompt }]);

    const marker = messages(sid).find((m) => m.id === o.messageId);
    expect(marker).toBeDefined();
    expect(marker?.role).toBe('user');
    expect(marker?.metadata).toMatchObject({
      userVisible: true,
      loopTick: { loopId: o.loopId, n: o.n, messageId: o.messageId },
    });
    expect(messages(sid).filter((m) => m.id === o.messageId)).toHaveLength(1);
    expect(acpChatSessionStore.getSnapshot(sid)?.activePromptAttemptId).not.toBeNull();
    expect(loopsTickRefused).not.toHaveBeenCalled();

    await act(async () => {
      turn.resolve();
    });
    await waitFor(() =>
      expect(acpChatSessionStore.getSnapshot(sid)?.activePromptAttemptId).toBeNull()
    );
    expect(messages(sid).filter((m) => m.id === o.messageId)).toHaveLength(1);
  });

  it('a repeated tickDue for the same (loopId, n, messageId) submits once', async () => {
    const sid = freshSessionId();
    loaded(sid);
    const turn = deferred();
    vi.mocked(acpPromptSession).mockReturnValue(turn.promise);
    render(<LoopDriver />);
    const o = offerFor(sid);

    await offer(o);
    await offer(o);
    await act(async () => {
      turn.resolve();
    });
    await offer(o);

    expect(acpPromptSession).toHaveBeenCalledTimes(1);
    expect(loopsTickRefused).not.toHaveBeenCalled();
    expect(messages(sid).filter((m) => m.id === o.messageId)).toHaveLength(1);
  });

  it('a busy store answers tickRefused{turn_running}; the attempt clearing sends exactly one loops/ready; the re-offer submits', async () => {
    const sid = freshSessionId();
    loaded(sid);
    acpChatSessionActions.startPromptAttempt(sid, 'users-turn');
    vi.mocked(acpPromptSession).mockResolvedValue({ stopReason: 'end_turn' } as never);
    render(<LoopDriver />);
    const o = offerFor(sid);

    await offer(o);

    expect(loopsTickRefused).toHaveBeenCalledWith(sid, o.loopId, o.n, { kind: 'turn_running' });
    expect(acpPromptSession).not.toHaveBeenCalled();
    expect(messages(sid).some((m) => m.id === o.messageId)).toBe(false);
    expect(loopsReady).not.toHaveBeenCalled();

    act(() => {
      acpChatSessionActions.finishPromptAttemptIfCurrent(sid, 'users-turn');
    });
    await waitFor(() => expect(loopsReady).toHaveBeenCalledTimes(1));
    expect(loopsReady).toHaveBeenCalledWith(sid);

    act(() => {
      acpChatSessionActions.setMessages(sid, [...messages(sid)]);
    });
    expect(loopsReady).toHaveBeenCalledTimes(1);

    await offer(o);
    await waitFor(() => expect(acpPromptSession).toHaveBeenCalledTimes(1));
    expect(messages(sid).some((m) => m.id === o.messageId)).toBe(true);
  });

  it('a chat that is not idle without an attempt (an in-place edit truncating) refuses the tick, so the edit is never swallowed', async () => {
    const sid = freshSessionId();
    loaded(sid);
    acpChatSessionActions.setChatState(sid, ChatState.Thinking);
    render(<LoopDriver />);
    const o = offerFor(sid);

    await offer(o);

    expect(loopsTickRefused).toHaveBeenCalledWith(sid, o.loopId, o.n, { kind: 'turn_running' });
    expect(acpPromptSession).not.toHaveBeenCalled();
    act(() => {
      acpChatSessionActions.setChatState(sid, ChatState.Idle);
    });
    await waitFor(() => expect(loopsReady).toHaveBeenCalledTimes(1));
  });

  it('a message queued in the composer refuses the tick (queued_message); the queue emptying sends ready once', async () => {
    const sid = freshSessionId();
    loaded(sid);
    setPendingUserInput(sid, 1);
    render(<LoopDriver />);
    const o = offerFor(sid);

    await offer(o);

    expect(loopsTickRefused).toHaveBeenCalledWith(sid, o.loopId, o.n, { kind: 'queued_message' });
    expect(acpPromptSession).not.toHaveBeenCalled();
    expect(loopsReady).not.toHaveBeenCalled();

    act(() => setPendingUserInput(sid, 0));
    await waitFor(() => expect(loopsReady).toHaveBeenCalledTimes(1));
  });

  it('a pending cancel refuses the tick (pending_cancel) instead of throwing', async () => {
    const sid = freshSessionId();
    loaded(sid);
    acpChatSessionActions.startPromptAttempt(sid, 'stopping');
    acpChatSessionActions.startPromptCancellation(sid, 'stopping');
    render(<LoopDriver />);
    const o = offerFor(sid);

    await offer(o);

    expect(loopsTickRefused).toHaveBeenCalledWith(sid, o.loopId, o.n, { kind: 'pending_cancel' });
    act(() => {
      acpChatSessionActions.clearPromptCancellation(sid, 'stopping');
    });
    await waitFor(() => expect(loopsReady).toHaveBeenCalledTimes(1));
  });

  it('loads a chat the store does not hold, then submits the tick to it', async () => {
    const sid = freshSessionId();
    vi.mocked(acpLoadSession).mockResolvedValue({ sessionInfo: {}, meta: {} } as never);
    vi.mocked(sessionInfoToSession).mockReturnValue(session(sid));
    vi.mocked(acpPromptSession).mockResolvedValue({ stopReason: 'end_turn' } as never);
    render(<LoopDriver />);
    const o = offerFor(sid);

    await offer(o);

    expect(acpLoadSession).toHaveBeenCalledWith(sid);
    await waitFor(() => expect(acpPromptSession).toHaveBeenCalledTimes(1));
    expect(loopsTickRefused).not.toHaveBeenCalled();
    expect(messages(sid).some((m) => m.id === o.messageId)).toBe(true);
  });

  it('a chat that cannot be loaded answers load_failed with the error', async () => {
    const sid = freshSessionId();
    vi.spyOn(console, 'error').mockImplementation(() => {});
    vi.mocked(acpLoadSession).mockRejectedValue(new Error('session not found: ' + sid));
    render(<LoopDriver />);
    const o = offerFor(sid);

    await offer(o);

    expect(loopsTickRefused).toHaveBeenCalledWith(sid, o.loopId, o.n, {
      kind: 'load_failed',
      error: 'session not found: ' + sid,
    });
    expect(acpPromptSession).not.toHaveBeenCalled();
  });

  it('goosed refusing the prompt withdraws the marker and answers submit_failed with its words', async () => {
    const sid = freshSessionId();
    loaded(sid);
    vi.mocked(acpPromptSession).mockRejectedValue(new Error('session is busy in another run'));
    render(<LoopDriver />);
    const o = offerFor(sid);

    await offer(o);

    await waitFor(() =>
      expect(loopsTickRefused).toHaveBeenCalledWith(sid, o.loopId, o.n, {
        kind: 'submit_failed',
        error: 'Submit error: session is busy in another run',
      })
    );
    expect(messages(sid).some((m) => m.id === o.messageId)).toBe(false);
    expect(loopsReady).not.toHaveBeenCalled();
  });

  it('a tick whose turn ran and then failed keeps its marker and refuses nothing', async () => {
    const sid = freshSessionId();
    loaded(sid);
    const o = offerFor(sid);
    vi.mocked(acpPromptSession).mockImplementation(async () => {
      acpChatSessionActions.setMessages(sid, [
        ...messages(sid),
        {
          id: 'reply-1',
          role: 'assistant',
          created: 1,
          content: [{ type: 'text', text: 'Reading NOW.md' }],
          metadata: { userVisible: true, agentVisible: true },
        },
      ]);
      throw new Error('stream ended early');
    });
    render(<LoopDriver />);

    await offer(o);

    await waitFor(() => expect(acpChatSessionStore.getSnapshot(sid)?.submitError).toBeDefined());
    expect(messages(sid).map((m) => m.id)).toEqual([o.messageId, 'reply-1']);
    expect(loopsTickRefused).not.toHaveBeenCalled();
  });

  it('an offer that arrived before the driver mounted is submitted on mount', async () => {
    const sid = freshSessionId();
    loaded(sid);
    vi.mocked(acpPromptSession).mockResolvedValue({ stopReason: 'end_turn' } as never);
    const o = offerFor(sid);

    await handleLoopsTickDue(o);
    expect(acpPromptSession).not.toHaveBeenCalled();
    render(<LoopDriver />);

    await waitFor(() => expect(acpPromptSession).toHaveBeenCalledTimes(1));
  });

  it('detects a dead connection and connects again at once, so goosed sees a live tick door', async () => {
    render(<LoopDriver />);
    expect(getAcpClient).toHaveBeenCalledTimes(1);
    expect(closeListeners.size).toBe(1);

    act(() => {
      for (const listener of [...closeListeners]) listener();
    });

    expect(getAcpClient).toHaveBeenCalledTimes(2);
    cleanup();
    expect(closeListeners.size).toBe(0);
  });
});
