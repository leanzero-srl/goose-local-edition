import { useCallback, useEffect, useSyncExternalStore } from 'react';
import { getAcpClient, onAcpConnectionClosed, onNotesChanged } from '../../acp/acpConnection';
import { notesList, notesShowing, type NotesList } from '../../acp/notes';

/**
 * Each chat's notes as goosed has them (Q-358): the drafts written in it and the notes sent to it.
 * Read when a surface for the chat mounts, and again whenever goosed says the chat's notes changed
 * (`notes/changed`), a `send_note` draft is pinned (its platform event) or the window regains
 * focus — events, never a poll. An unreadable list is kept as its error, never as "no notes".
 */

export interface ChatNotes {
  list: NotesList | null;
  error: string | null;
}

const EMPTY: ChatNotes = { list: null, error: null };

const entries = new Map<string, ChatNotes>();
const generations = new Map<string, number>();
const listeners = new Map<string, Set<() => void>>();
let syncing = false;

function emit(sessionId: string): void {
  for (const listener of [...(listeners.get(sessionId) ?? [])]) listener();
}

export function getChatNotes(sessionId: string): ChatNotes {
  return entries.get(sessionId) ?? EMPTY;
}

export async function refreshChatNotes(sessionId: string): Promise<void> {
  const generation = (generations.get(sessionId) ?? 0) + 1;
  generations.set(sessionId, generation);
  try {
    const list = await notesList(sessionId);
    if (generations.get(sessionId) !== generation) return;
    entries.set(sessionId, { list, error: null });
  } catch (error) {
    if (generations.get(sessionId) !== generation) return;
    const message = error instanceof Error ? error.message : String(error);
    entries.set(sessionId, { list: getChatNotes(sessionId).list, error: message });
  }
  emit(sessionId);
}

/** Put what a call answered in place at once, then re-read goosed's view. */
export function refreshAfter(sessionIds: readonly string[]): void {
  for (const sessionId of sessionIds) void refreshChatNotes(sessionId);
}

function refreshWatched(): void {
  for (const sessionId of listeners.keys()) void refreshChatNotes(sessionId);
}

function onPlatformEvent(event: Event): void {
  const detail = (event as CustomEvent<{ extension?: string; sessionId?: string }>).detail;
  if (detail?.extension === 'notes' && detail.sessionId) void refreshChatNotes(detail.sessionId);
}

function startSync(): void {
  if (syncing) return;
  syncing = true;
  onNotesChanged((changed) => {
    for (const sessionId of changed.sessionIds) {
      if (listeners.has(sessionId)) void refreshChatNotes(sessionId);
    }
  });
  window.addEventListener('platform-event', onPlatformEvent);
  window.addEventListener('focus', refreshWatched);
}

function subscribe(sessionId: string, listener: () => void): () => void {
  startSync();
  const set = listeners.get(sessionId) ?? new Set<() => void>();
  const first = set.size === 0;
  set.add(listener);
  listeners.set(sessionId, set);
  if (first) void refreshChatNotes(sessionId);
  return () => {
    set.delete(listener);
    if (set.size === 0) listeners.delete(sessionId);
  };
}

export function useChatNotes(sessionId: string): ChatNotes {
  // One subscription per chat for the component's life: a new subscribe function each render would
  // unsubscribe and resubscribe every time, and each first subscription re-reads the list — which
  // renders again (measured: a test hung at 100% CPU).
  const subscribeToChat = useCallback(
    (listener: () => void) => subscribe(sessionId, listener),
    [sessionId]
  );
  const read = useCallback(() => getChatNotes(sessionId), [sessionId]);
  return useSyncExternalStore(subscribeToChat, read);
}

export function resetChatNotesForTests(): void {
  entries.clear();
  generations.clear();
  listeners.clear();
  shownHere.clear();
}

export function seedChatNotesForTests(sessionId: string, notes: ChatNotes): void {
  entries.set(sessionId, notes);
  emit(sessionId);
}

// ---------------------------------------------------------------------------------------------
// What this window shows: goosed offers a chat's due note only to the windows that show it.
// ---------------------------------------------------------------------------------------------

const shownHere = new Map<string, number>();
let reannouncing = false;

export function isShownHere(sessionId: string): boolean {
  return (shownHere.get(sessionId) ?? 0) > 0;
}

function announce(sessionId: string, showing: boolean): void {
  notesShowing(sessionId, showing).catch((error) => {
    console.error(
      `Could not tell goose this window ${showing ? 'shows' : 'no longer shows'} ${sessionId}; its notes are offered to the windows goose knows show it:`,
      error
    );
  });
}

/** goosed dropped this window's door with the connection: say again what it shows. */
function watchReconnects(): void {
  if (reannouncing) return;
  reannouncing = true;
  onAcpConnectionClosed(() => {
    getAcpClient().then(
      () => {
        for (const sessionId of shownHere.keys()) announce(sessionId, true);
      },
      (error) => {
        console.error('Could not reconnect to goose to say which chats this window shows:', error);
      }
    );
  });
}

/**
 * The chat this window shows turned idle after it had to leave goosed's offer (Q-488: a window that
 * opens a chat announces it before the chat has loaded, so the offer that announcement brings finds
 * it busy). Saying again that it shows the chat makes goosed offer its due note again. A window that
 * no longer shows the chat says nothing: the window that shows it next is offered the note.
 */
export function askForDueNotes(sessionId: string): void {
  if (isShownHere(sessionId)) announce(sessionId, true);
}

/** While `showing`, this window shows `sessionId`: goosed may offer it the chat's due notes. */
export function useShowsChat(sessionId: string, showing: boolean): void {
  useEffect(() => {
    if (!showing || !sessionId) return undefined;
    watchReconnects();
    const count = shownHere.get(sessionId) ?? 0;
    shownHere.set(sessionId, count + 1);
    if (count === 0) announce(sessionId, true);
    return () => {
      const left = (shownHere.get(sessionId) ?? 1) - 1;
      if (left > 0) {
        shownHere.set(sessionId, left);
        return;
      }
      shownHere.delete(sessionId);
      announce(sessionId, false);
    };
  }, [sessionId, showing]);
}
