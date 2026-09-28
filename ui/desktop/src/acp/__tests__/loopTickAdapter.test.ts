import type { SessionNotification } from '@agentclientprotocol/sdk';
import { describe, expect, it } from 'vitest';
import type { Message } from '../../types/message';
import { createAcpSessionNotificationAdapter } from '../sessionNotificationAdapter';

const TICK = 'looptick_lp_0a1b2c3d_5_aaaabbbbccccddddeeeeffff00001111';

function chunk(
  sessionUpdate: 'user_message_chunk' | 'agent_message_chunk',
  messageId: string
): SessionNotification {
  return {
    sessionId: 's1',
    update: {
      sessionUpdate,
      content: { type: 'text', text: 'Loop tick 5 — …' },
      _meta: { goose: { messageId } },
    } as SessionNotification['update'],
  };
}

function messagesOf(
  changes: ReturnType<ReturnType<typeof createAcpSessionNotificationAdapter>['apply']>
): Message[] {
  const change = changes.find((c) => c.type === 'messages');
  if (!change || change.type !== 'messages') throw new Error('no messages change');
  return change.messages;
}

describe('the adapter marks a replayed loop tick', () => {
  it("maps a user message's looptick_ id to metadata.loopTick", () => {
    const adapter = createAcpSessionNotificationAdapter();
    const [message] = messagesOf(adapter.apply(chunk('user_message_chunk', TICK)));
    expect(message.metadata.loopTick).toEqual({ loopId: 'lp_0a1b2c3d', n: 5, messageId: TICK });
  });

  it('leaves an ordinary id, and an assistant message, unmarked', () => {
    const adapter = createAcpSessionNotificationAdapter();
    const [user] = messagesOf(adapter.apply(chunk('user_message_chunk', 'msg_123')));
    expect(user.metadata.loopTick).toBeUndefined();
    const assistant = messagesOf(adapter.apply(chunk('agent_message_chunk', TICK)))[1];
    expect(assistant.metadata.loopTick).toBeUndefined();
  });

  it('never reads a malformed tick id as a tick', () => {
    const adapter = createAcpSessionNotificationAdapter();
    const [message] = messagesOf(
      adapter.apply(chunk('user_message_chunk', 'looptick_not-a-loop_5_abc'))
    );
    expect(message.metadata.loopTick).toBeUndefined();
  });
});
