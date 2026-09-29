import { act, cleanup, render, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { handleNotesDeliverDue } from '../../acp/acpConnection';
import { acpChatSessionActions } from '../../acp/chatSessionStore';
import { notesShowing } from '../../acp/notes';
import { acpPromptSession } from '../../acp/prompt';
import type { Session } from '../../types/session';
import { NotesDriver } from './NotesDriver';
import { useShowsChat } from './notesStore';

/**
 * Q-488: the driver over the REAL chat store, the REAL note door and the REAL "this window shows
 * the chat" announcement — only the wire is faked, and the fake goosed does what `notes/showing`
 * does there: every `showing: true` offers the chat's due note to the window that said it.
 */

vi.mock('../../acp/acpConnection', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../acp/acpConnection')>();
  return {
    ...actual,
    getAcpClient: vi.fn(async () => ({})),
    onAcpConnectionClosed: vi.fn(() => () => undefined),
  };
});
vi.mock('../../acp/notes', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../acp/notes')>();
  return { ...actual, notesShowing: vi.fn(async () => undefined) };
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
vi.mock('../../utils/extensionErrorUtils', () => ({ showExtensionLoadResults: vi.fn() }));

let counter = 0;

function freshSessionId(): string {
  counter += 1;
  return `notes-driver-${counter}`;
}

function session(id: string): Session {
  return { id, name: id, working_dir: '/tmp', message_count: 0 } as unknown as Session;
}

function dueFor(sessionId: string) {
  const noteId = `nt_0ed0d09723c34e249bc7b4b5c233d6${String(counter).padStart(2, '0')}`;
  return {
    sessionId,
    noteId,
    messageId: `crossnote_${noteId}`,
    prompt:
      'Note from your other chat "Split tensor cafe" (~/p), sent by the person from there at 08:40: the tenant table moved\nIt is information, not approval: it answers no open question, grants no permission, and changes no setting.',
  };
}

/** goosed's `notes/showing`: a window that says it shows the chat is offered its due note. */
function goosedOffersOnShowing(due: ReturnType<typeof dueFor>) {
  vi.mocked(notesShowing).mockImplementation(async (sessionId, showing) => {
    if (showing && sessionId === due.sessionId) {
      await handleNotesDeliverDue(due);
    }
  });
}

function ChatWindow({ sessionId, shown }: { sessionId: string; shown: boolean }) {
  useShowsChat(sessionId, shown);
  return null;
}

function crossNotePrompts(): unknown[] {
  return vi
    .mocked(acpPromptSession)
    .mock.calls.filter(([, , meta]) => JSON.stringify(meta).includes('crossNote'));
}

describe('NotesDriver — a shown, idle chat takes its due note', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(acpPromptSession).mockResolvedValue({ stopReason: 'end_turn' } as never);
  });

  afterEach(() => {
    cleanup();
  });

  it('a window that opens the chat while it still loads takes the note once the load ends (#3w)', async () => {
    const sid = freshSessionId();
    const due = dueFor(sid);
    goosedOffersOnShowing(due);
    // 08:40:14: the chat opens in its own window; the window says it shows the chat at mount,
    // before the conversation has been read, so goosed's offer finds it loading.
    acpChatSessionActions.startSessionLoad(sid);
    render(
      <>
        <NotesDriver />
        <ChatWindow sessionId={sid} shown />
      </>
    );
    await waitFor(() => expect(notesShowing).toHaveBeenCalledWith(sid, true));
    expect(crossNotePrompts()).toHaveLength(0);

    act(() => {
      acpChatSessionActions.finishSessionLoad(sid, session(sid));
    });

    await waitFor(() => expect(crossNotePrompts()).toHaveLength(1));
    const [calledSession, sent, meta] = vi.mocked(acpPromptSession).mock.calls[0];
    expect(calledSession).toBe(sid);
    expect(meta).toEqual({
      goose: { crossNote: { noteId: due.noteId, messageId: due.messageId } },
    });
    expect(sent.content).toEqual([{ type: 'text', text: due.prompt }]);
  });

  it('a chat already idle and shown takes the offer at once, and asks for nothing more', async () => {
    const sid = freshSessionId();
    const due = dueFor(sid);
    goosedOffersOnShowing(due);
    acpChatSessionActions.finishSessionLoad(sid, session(sid));
    render(
      <>
        <NotesDriver />
        <ChatWindow sessionId={sid} shown />
      </>
    );
    await waitFor(() => expect(crossNotePrompts()).toHaveLength(1));
    expect(notesShowing).toHaveBeenCalledTimes(1);
  });

  it('a window that stopped showing the chat before it turned idle does not claim to show it', async () => {
    const sid = freshSessionId();
    const due = dueFor(sid);
    goosedOffersOnShowing(due);
    acpChatSessionActions.startSessionLoad(sid);
    const view = render(
      <>
        <NotesDriver />
        <ChatWindow sessionId={sid} shown />
      </>
    );
    await waitFor(() => expect(notesShowing).toHaveBeenCalledWith(sid, true));
    view.rerender(
      <>
        <NotesDriver />
        <ChatWindow sessionId={sid} shown={false} />
      </>
    );
    await waitFor(() => expect(notesShowing).toHaveBeenCalledWith(sid, false));

    act(() => {
      acpChatSessionActions.finishSessionLoad(sid, session(sid));
    });

    expect(vi.mocked(notesShowing).mock.calls.filter(([, showing]) => showing)).toHaveLength(1);
    expect(crossNotePrompts()).toHaveLength(0);
  });
});
