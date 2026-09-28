import { act, renderHook } from '@testing-library/react';
import type { ReactNode } from 'react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { acpChatSessionActions } from '../acp/chatSessionStore';
import { acpPromptSession } from '../acp/prompt';
import { IntlTestWrapper } from '../i18n/test-utils';
import type { Session } from '../types/session';
import { useChatSession } from './useChatSession';

/**
 * Q-344: the prompt goosed receives for a needs-you card's answer carries
 * `_meta.goose.needsYouAnswers` — the mark that keeps goosed from reading it as typed and closing
 * the chat's other open questions as superseded (Q-298). A typed message carries no mark. Over the
 * REAL chat store and submitMessage; only the prompt call is faked.
 */

vi.mock('../acp/prompt', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../acp/prompt')>();
  return { ...actual, acpPromptSession: vi.fn(), acpCancelPrompt: vi.fn() };
});
vi.mock('../utils/extensionErrorUtils', () => ({ showExtensionLoadResults: vi.fn() }));

let counter = 0;

function loadedChat(): string {
  counter += 1;
  const sessionId = `needs-you-mark-${counter}`;
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

describe('Q-344: the card answer mark on the prompt', () => {
  beforeEach(() => {
    vi.mocked(acpPromptSession)
      .mockReset()
      .mockResolvedValue({ stopReason: 'end_turn' } as never);
  });

  it('a needs-you answer goes with _meta.goose.needsYouAnswers naming every item it answers', async () => {
    const sessionId = loadedChat();
    const text =
      'Answer to your question "Which database?": PostgreSQL\n\nAnswer to your question "Which delimiter?": A tab';
    await submit(sessionId, { msg: text, images: [], needsYouAnswers: ['ny_db', 'ny_csv'] });

    expect(acpPromptSession).toHaveBeenCalledTimes(1);
    const [calledSession, sent, meta] = vi.mocked(acpPromptSession).mock.calls[0];
    expect(calledSession).toBe(sessionId);
    expect(sent.content).toEqual([{ type: 'text', text }]);
    expect(meta).toEqual({ goose: { needsYouAnswers: ['ny_db', 'ny_csv'] } });
  });

  it('a typed message goes with no mark, so goosed supersedes the open questions', async () => {
    const sessionId = loadedChat();
    await submit(sessionId, { msg: 'Use SQLite', images: [] });

    expect(acpPromptSession).toHaveBeenCalledTimes(1);
    expect(vi.mocked(acpPromptSession).mock.calls[0][2]).toBeUndefined();
  });
});
