import { act, renderHook } from '@testing-library/react';
import type { ReactNode } from 'react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { acpChatSessionActions } from '../acp/chatSessionStore';
import { acpPromptSession } from '../acp/prompt';
import { IntlTestWrapper } from '../i18n/test-utils';
import type { Session } from '../types/session';
import { useChatSession } from './useChatSession';

/**
 * Q-381: "Answer on {next} for now" sends the refused turn's text again with
 * `_meta.goose.answerOn = {node}` — goosed runs THAT prompt's reply on the node and the next prompt,
 * carrying no mark, goes back to the chat's lead. Over the REAL chat store and submitMessage; only
 * the prompt call is faked.
 */

vi.mock('../acp/prompt', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../acp/prompt')>();
  return { ...actual, acpPromptSession: vi.fn(), acpCancelPrompt: vi.fn() };
});
vi.mock('../utils/extensionErrorUtils', () => ({ showExtensionLoadResults: vi.fn() }));

let counter = 0;

function loadedChat(): string {
  counter += 1;
  const sessionId = `answer-on-mark-${counter}`;
  acpChatSessionActions.finishSessionLoad(sessionId, {
    id: sessionId,
    name: sessionId,
    working_dir: '/tmp',
    message_count: 0,
  } as unknown as Session);
  return sessionId;
}

function wrapper({ children }: { children: ReactNode }) {
  return <IntlTestWrapper>{children}</IntlTestWrapper>;
}

async function submit(
  sessionId: string,
  input: Parameters<ReturnType<typeof useChatSession>['handleSubmit']>[0]
) {
  const { result } = renderHook(() => useChatSession({ sessionId, onStreamFinish: vi.fn() }), {
    wrapper,
  });
  await act(async () => {
    await result.current.handleSubmit(input);
  });
}

describe('Q-381: the answer-on mark on the prompt', () => {
  beforeEach(() => {
    vi.mocked(acpPromptSession)
      .mockReset()
      .mockResolvedValue({ stopReason: 'end_turn' } as never);
  });

  it('goes with _meta.goose.answerOn naming the node, on that one prompt only', async () => {
    const sessionId = loadedChat();
    await submit(sessionId, { msg: 'Summarise the notes', images: [], answerOn: 'flash-here' });
    await submit(sessionId, { msg: 'And the changelog?', images: [] });

    expect(acpPromptSession).toHaveBeenCalledTimes(2);
    const [calledSession, sent, meta] = vi.mocked(acpPromptSession).mock.calls[0];
    expect(calledSession).toBe(sessionId);
    expect(sent.content).toEqual([{ type: 'text', text: 'Summarise the notes' }]);
    expect(meta).toEqual({ goose: { answerOn: { node: 'flash-here' } } });
    expect(vi.mocked(acpPromptSession).mock.calls[1][2]).toBeUndefined();
  });

  it('rides beside a needs-you mark without losing either', async () => {
    const sessionId = loadedChat();
    await submit(sessionId, {
      msg: 'Answer to your question "Which database?": PostgreSQL',
      images: [],
      needsYouAnswers: ['ny_db'],
      answerOn: 'flash-here',
    });
    expect(vi.mocked(acpPromptSession).mock.calls[0][2]).toEqual({
      goose: { needsYouAnswers: ['ny_db'], answerOn: { node: 'flash-here' } },
    });
  });
});
