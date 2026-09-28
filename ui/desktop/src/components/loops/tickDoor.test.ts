import { describe, expect, it, vi } from 'vitest';
import {
  getPendingUserInput,
  setPendingUserInput,
  subscribePendingUserInput,
} from './pendingUserInput';
import { refusalCleared, sendLoopControlNow, tickMarker, tickMeta } from './tickDoor';
import type { AcpChatSessionSnapshot } from '../../acp/chatSessionStore';
import { ChatState } from '../../types/chatState';

const offer = {
  sessionId: 's1',
  loopId: 'lp_0a1b2c3d',
  n: 5,
  messageId: 'looptick_lp_0a1b2c3d_5_0190aaaa-bbbb-7ccc-8ddd-eeeeffff0000',
  prompt: 'Loop tick 5 — "users.csv" · every 10 min',
};

function api() {
  return {
    loopsGet: vi.fn(async () => ({ effectiveStatus: undefined }) as never),
    loopsControl: vi.fn(async () => ({}) as never),
  };
}

describe('sendLoopControlNow — /loop controls typed during a turn go straight through (§7.2, §13 item 4)', () => {
  it.each([
    ['/loop stop', 'stop'],
    ['/loop pause', 'pause'],
    ['/loop resume', 'resume'],
    ['/loop now', 'tickNow'],
    ['  /loop STOP  ', 'stop'],
  ])('%s → loops/control{%s} at once', async (line, action) => {
    const loops = api();

    const sent = await sendLoopControlNow('s1', line, loops);

    expect(sent?.kind).toBe('control');
    expect(loops.loopsControl).toHaveBeenCalledWith('s1', action);
    expect(loops.loopsGet).not.toHaveBeenCalled();
  });

  it('/loop alone reads the status (loops/get), never controls', async () => {
    const loops = api();

    const sent = await sendLoopControlNow('s1', '/loop', loops);

    expect(sent?.kind).toBe('status');
    expect(loops.loopsGet).toHaveBeenCalledWith('s1');
    expect(loops.loopsControl).not.toHaveBeenCalled();
  });

  it.each([
    '/loop fix the tests',
    '/loop every 10m fix the tests',
    '/loop stop now',
    'stop',
    'hello',
    '/loopy',
  ])('%s is not a control form: null, and it takes the ordinary (queued) path', async (line) => {
    const loops = api();

    expect(await sendLoopControlNow('s1', line, loops)).toBeNull();
    expect(loops.loopsControl).not.toHaveBeenCalled();
    expect(loops.loopsGet).not.toHaveBeenCalled();
  });
});

describe('the tick marker', () => {
  it("is the tick's prompt as a user message carrying the runner's id and metadata.loopTick", () => {
    const marker = tickMarker(offer);

    expect(marker.id).toBe(offer.messageId);
    expect(marker.role).toBe('user');
    expect(marker.content).toEqual([{ type: 'text', text: offer.prompt }]);
    expect(marker.metadata).toEqual({
      userVisible: true,
      agentVisible: true,
      loopTick: { loopId: offer.loopId, n: 5, messageId: offer.messageId },
    });
  });

  it('carries the same (loopId, n, messageId) in the prompt _meta that goosed matches', () => {
    expect(tickMeta(offer)).toEqual({
      goose: { loopTick: { loopId: offer.loopId, n: 5, messageId: offer.messageId } },
    });
  });
});

function snapshot(patch: Partial<AcpChatSessionSnapshot>): AcpChatSessionSnapshot {
  return {
    session: { id: 's1' } as never,
    messages: [],
    tokenState: {} as never,
    notifications: [],
    chatState: ChatState.Idle,
    sessionLoadError: undefined,
    submitError: undefined,
    activePromptAttemptId: null,
    activeRunId: null,
    pendingCancelPromptAttemptId: null,
    ...patch,
  };
}

describe('refusalCleared', () => {
  it('a turn refusal clears only when no attempt, no pending cancel, idle, and nothing queued', () => {
    expect(refusalCleared('turn', snapshot({}), 0)).toBe(true);
    expect(refusalCleared('turn', snapshot({}), 1)).toBe(false);
    expect(refusalCleared('turn', snapshot({ activePromptAttemptId: 'a' }), 0)).toBe(false);
    expect(refusalCleared('turn', snapshot({ pendingCancelPromptAttemptId: 'a' }), 0)).toBe(false);
    expect(refusalCleared('turn', snapshot({ chatState: ChatState.Compacting }), 0)).toBe(false);
    expect(refusalCleared('turn', undefined, 0)).toBe(true);
  });

  it('a load refusal clears when the chat is in the store without a load error', () => {
    expect(refusalCleared('session', snapshot({}), 0)).toBe(true);
    expect(refusalCleared('session', snapshot({ session: undefined }), 0)).toBe(false);
    expect(refusalCleared('session', snapshot({ sessionLoadError: 'gone' }), 0)).toBe(false);
    expect(refusalCleared('session', undefined, 0)).toBe(false);
  });
});

describe('pendingUserInput', () => {
  it('holds a per-chat count and tells its subscribers only when it changes', () => {
    const heard = vi.fn();
    const stop = subscribePendingUserInput('queue-1', heard);

    setPendingUserInput('queue-1', 2);
    setPendingUserInput('queue-1', 2);
    setPendingUserInput('queue-2', 1);
    setPendingUserInput('queue-1', 0);

    expect(heard.mock.calls).toEqual([[2], [0]]);
    expect(getPendingUserInput('queue-1')).toBe(0);
    expect(getPendingUserInput('queue-2')).toBe(1);
    stop();
    setPendingUserInput('queue-1', 3);
    expect(heard).toHaveBeenCalledTimes(2);
  });
});
