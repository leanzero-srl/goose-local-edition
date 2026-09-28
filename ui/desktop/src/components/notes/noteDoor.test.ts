import { describe, expect, it, vi } from 'vitest';
import type { AcpChatSessionSnapshot } from '../../acp/chatSessionStore';
import type { AcpSubmitMessageOptions } from '../../acp/chatSessionController';
import { ChatState } from '../../types/chatState';
import type { Message } from '../../types/message';
import { createNoteDoor, noteTurnMessage, type NoteDoorDeps, type NoteTurn } from './noteDoor';

const TURN: NoteTurn = {
  sessionId: 'chat-b',
  noteId: 'nt_1',
  messageId: 'crossnote_nt_1',
  prompt:
    'Note from your other chat "Explore split mesh" (~/p), sent by the person from there at 14:02: tenant_id is the new column\nIt is information, not approval: it answers no open question, grants no permission, and changes no setting.',
};

function idle(messages: Message[] = []): AcpChatSessionSnapshot {
  return {
    session: { id: 'chat-b' },
    chatState: ChatState.Idle,
    messages,
  } as unknown as AcpChatSessionSnapshot;
}

function deps(over: Partial<NoteDoorDeps> = {}) {
  let snapshot = idle();
  const submitted: { message: Message; options: AcpSubmitMessageOptions }[] = [];
  const base: NoteDoorDeps = {
    getSnapshot: () => snapshot,
    submitMessage: vi.fn(async (_sessionId, message, options) => {
      submitted.push({ message, options });
      snapshot = idle([...snapshot.messages, message]);
      return 'submitted' as const;
    }),
    setMessages: vi.fn((_sessionId, messages) => {
      snapshot = idle(messages);
    }),
    pendingUserInput: () => 0,
    isShownHere: () => true,
    ...over,
  };
  return { base, submitted, messages: () => snapshot.messages };
}

describe('the note door — a note becomes its own turn only here, idle, and once', () => {
  it('submits the exact words under the note id, marked crossNote, pre-appended', async () => {
    const { base, submitted } = deps();
    const door = createNoteDoor(base);
    expect(await door.open(TURN, true)).toEqual({ kind: 'submitted' });
    expect(submitted).toHaveLength(1);
    const [{ message, options }] = submitted;
    expect(message.id).toBe('crossnote_nt_1');
    expect(message.role).toBe('user');
    expect(message.content).toEqual([{ type: 'text', text: TURN.prompt }]);
    expect(options.meta).toEqual({
      goose: { crossNote: { noteId: 'nt_1', messageId: 'crossnote_nt_1' } },
    });
    expect(options.preAppend).toBe(true);
    expect(await door.open(TURN, true)).toEqual({ kind: 'duplicate' });
  });

  it("leaves goosed's offer alone in a window that does not show the chat", async () => {
    const { base, submitted } = deps({ isShownHere: () => false });
    const door = createNoteDoor(base);
    expect(await door.open(TURN, true)).toEqual({ kind: 'not_here' });
    expect(submitted).toHaveLength(0);
    // The person's own click in the chat is not an offer: it opens where they are.
    expect(await door.open(TURN, false)).toEqual({ kind: 'submitted' });
  });

  it('never starts a turn over a running one or ahead of what the person queued', async () => {
    const running = deps({
      getSnapshot: () =>
        ({ ...idle(), chatState: ChatState.Streaming }) as unknown as AcpChatSessionSnapshot,
    });
    expect(await createNoteDoor(running.base).open(TURN, true)).toEqual({ kind: 'busy' });
    const queued = deps({ pendingUserInput: () => 1 });
    expect(await createNoteDoor(queued.base).open(TURN, true)).toEqual({ kind: 'busy' });
    expect(running.submitted).toHaveLength(0);
    expect(queued.submitted).toHaveLength(0);
  });

  it("withdraws the marker when goosed refuses the turn, and says goosed's words", async () => {
    const { base, messages } = deps();
    base.submitMessage = vi.fn(async (_sessionId, message, options) => {
      base.setMessages('chat-b', [...messages(), message]);
      await options.onFinish('this note cannot start a turn here: note nt_1 is not waiting');
      return 'submitted' as const;
    });
    const door = createNoteDoor(base);
    expect(await door.open(TURN, true)).toEqual({
      kind: 'refused',
      error: 'this note cannot start a turn here: note nt_1 is not waiting',
    });
    expect(messages()).toHaveLength(0);
  });

  it('the marker is a plain user message a reload shows the same way', () => {
    const marker = noteTurnMessage(TURN);
    expect(marker.metadata).toEqual({ userVisible: true, agentVisible: true });
  });
});
