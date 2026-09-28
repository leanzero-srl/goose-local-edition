import { describe, expect, it, vi } from 'vitest';
import type { Message } from '../../types/message';
import { getAcpClient } from '../acpConnection';
import { acpPromptSession, messageToAcpPromptContent } from '../prompt';

vi.mock('../acpConnection', () => ({ getAcpClient: vi.fn() }));

function textMessage(text: string): Message {
  return {
    id: 'message-1',
    role: 'user',
    created: 123,
    content: [{ type: 'text', text }],
    metadata: { userVisible: true, agentVisible: true },
  };
}

// Q-228 (L4r): the tick is marked by `_meta.goose.loopTick`; before this the request carried only
// {sessionId, prompt}, so goosed could never tell a tick from a typed message.
describe('acpPromptSession', () => {
  it('puts the meta on the PromptRequest as _meta', async () => {
    const prompt = vi.fn().mockResolvedValue({ stopReason: 'end_turn' });
    vi.mocked(getAcpClient).mockResolvedValue({ prompt } as never);
    const meta = { goose: { loopTick: { loopId: 'lp_0000abcd', n: 3, messageId: 'mid' } } };

    await acpPromptSession('s1', textMessage('Loop tick 3'), meta);

    expect(prompt).toHaveBeenCalledWith({
      sessionId: 's1',
      prompt: [{ type: 'text', text: 'Loop tick 3' }],
      _meta: meta,
    });
  });

  it('sends no _meta key at all for a typed message', async () => {
    const prompt = vi.fn().mockResolvedValue({ stopReason: 'end_turn' });
    vi.mocked(getAcpClient).mockResolvedValue({ prompt } as never);

    await acpPromptSession('s1', textMessage('hi'));

    expect(prompt.mock.calls[0][0]).toEqual({
      sessionId: 's1',
      prompt: [{ type: 'text', text: 'hi' }],
    });
    expect('_meta' in prompt.mock.calls[0][0]).toBe(false);
  });
});

describe('messageToAcpPromptContent', () => {
  it('converts text and image content into ACP prompt blocks', () => {
    const message: Message = {
      id: 'message-1',
      role: 'user',
      created: 123,
      content: [
        { type: 'text', text: 'Describe this' },
        { type: 'image', data: 'abc123', mimeType: 'image/png' },
      ],
      metadata: { userVisible: true, agentVisible: true },
    };

    expect(messageToAcpPromptContent(message)).toEqual([
      { type: 'text', text: 'Describe this' },
      { type: 'image', data: 'abc123', mimeType: 'image/png' },
    ]);
  });

  it('omits empty text content and unsupported content blocks', () => {
    const message: Message = {
      id: 'message-1',
      role: 'user',
      created: 123,
      content: [
        { type: 'text', text: '   ' },
        {
          type: 'toolResponse',
          id: 'tool-1',
          toolResult: { status: 'success', value: [] },
        },
      ],
      metadata: { userVisible: true, agentVisible: true },
    } as Message;

    expect(messageToAcpPromptContent(message)).toEqual([]);
  });
});
