import { describe, expect, it } from 'vitest';
import type { SessionNotification } from '@agentclientprotocol/sdk';
import { createAcpSessionNotificationAdapter } from '../../acp/sessionNotificationAdapter';
import type { Message } from '../../types/message';

function userMessage(id: string, text: string): Message {
  return {
    id,
    role: 'user',
    created: 1,
    content: [{ type: 'text', text }],
    metadata: { userVisible: true, agentVisible: true },
  };
}

function chunk(messageId: string, text: string, steer = false): SessionNotification {
  return {
    sessionId: 'chat-b',
    update: {
      sessionUpdate: 'user_message_chunk',
      content: { type: 'text', text },
      _meta: { goose: { messageId, created: 2, ...(steer ? { steer: true } : {}) } },
    },
  } as unknown as SessionNotification;
}

describe('a note in the live transcript sits where goosed stored it', () => {
  it('a note added to the next message goes before that message, as the reload shows it', () => {
    const adapter = createAcpSessionNotificationAdapter([
      userMessage('m1', 'earlier'),
      userMessage('m2', 'my next message'),
    ]);
    adapter.apply(chunk('crossnote_nt_1', 'Note from your other chat "A" (~/p) …'));
    expect(adapter.getMessages().map((m) => m.id)).toEqual(['m1', 'crossnote_nt_1', 'm2']);
  });

  it('a note steered into a running turn lands at the end, where it drained', () => {
    const adapter = createAcpSessionNotificationAdapter([userMessage('m2', 'my message')]);
    adapter.apply(chunk('crossnote_nt_2', 'Note from your other chat "A" (~/p) …', true));
    expect(adapter.getMessages().map((m) => m.id)).toEqual(['m2', 'crossnote_nt_2']);
  });

  it('any other user message is appended as before', () => {
    const adapter = createAcpSessionNotificationAdapter([userMessage('m2', 'my message')]);
    adapter.apply(chunk('m3', 'another'));
    expect(adapter.getMessages().map((m) => m.id)).toEqual(['m2', 'm3']);
  });
});
