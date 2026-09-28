import type {
  InboxNoteDto,
  NoteChatDto,
  NoteDelivery,
  NoteDraftAction,
  NoteDraftDto,
  NoteInboxAction,
  NotesListResponse_unstable,
} from '@aaif/goose-sdk';
import { getAcpClient } from './acpConnection';

/**
 * Client surface for notes to another chat (Q-358, `_goose/unstable/notes/*`). The draft card, the
 * inbox tray and the notes driver call these and nothing else writes `chat_notes.v0`. Raw
 * `extMethod`, like the loops surface, so a field a newer backend adds is never stripped.
 *
 * `notesSend` is THE PERSON'S CLICK — the only way a note leaves the chat that drafted it. Nothing
 * a model can call reaches it.
 */

export type { InboxNoteDto, NoteChatDto, NoteDelivery, NoteDraftDto, NoteInboxAction };
export type NotesList = NotesListResponse_unstable;

async function call<T>(method: string, params: Record<string, unknown>): Promise<T> {
  const client = await getAcpClient();
  return (await client.extMethod(method, params)) as unknown as T;
}

/** The drafts written in this chat and the notes sent to it. */
export async function notesList(sessionId: string): Promise<NotesList> {
  return call<NotesList>('_goose/unstable/notes/list', { sessionId });
}

/** The chats a note written here could go to, most recently active first. */
export async function notesTargets(sessionId: string): Promise<NoteChatDto[]> {
  const { chats } = await call<{ chats: NoteChatDto[] }>('_goose/unstable/notes/targets', {
    sessionId,
  });
  return chats;
}

/** The person's click on a draft: send it, as they left the text, the way they chose. */
export async function notesSend(
  sessionId: string,
  noteId: string,
  text: string,
  delivery: NoteDelivery
): Promise<NoteDraftDto> {
  const { draft } = await call<{ draft: NoteDraftDto }>('_goose/unstable/notes/send', {
    sessionId,
    noteId,
    text,
    delivery,
  });
  return draft;
}

/** "Not this chat" (another chat picked) or Cancel, on a draft. */
export async function notesDraft(
  sessionId: string,
  noteId: string,
  action: NoteDraftAction
): Promise<NoteDraftDto> {
  const { draft } = await call<{ draft: NoteDraftDto }>('_goose/unstable/notes/draft', {
    sessionId,
    noteId,
    action,
  });
  return draft;
}

/** What the person in the chat a note was sent to does with it. */
export async function notesInbox(
  sessionId: string,
  noteId: string,
  action: NoteInboxAction
): Promise<InboxNoteDto> {
  const { note } = await call<{ note: InboxNoteDto }>('_goose/unstable/notes/inbox', {
    sessionId,
    noteId,
    action,
  });
  return note;
}

/** This window shows (or stopped showing) the chat: goosed offers its due notes only to such windows. */
export async function notesShowing(sessionId: string, showing: boolean): Promise<void> {
  await call<Record<string, never>>('_goose/unstable/notes/showing', { sessionId, showing });
}

/** The prompt `_meta` of a note's own turn (goosed honours it once, for the note's exact words). */
export function crossNoteMeta(noteId: string, messageId: string): Record<string, unknown> {
  return { goose: { crossNote: { noteId, messageId } } };
}
