import { useCallback, useEffect, useState } from 'react';

/**
 * Q-340: what a person folded in a chat's needs-you tray — the whole stack, or single cards — kept
 * per chat in this window's localStorage (a viewer's convenience, not the chat's state).
 *
 * A question the person has not yet SEEN in this chat opens the stack, so folding never hides a new
 * one. Of the new ones, the first opens and the rest arrive folded to their one line: two long cards
 * opened at once is exactly the screen that swallowed the chat (3.0.69), while a line per question
 * still shows every one of them.
 */
export interface NeedsYouFold {
  stackFolded: boolean;
  /** The folded cards, by question (or elicitation) id. */
  folded: string[];
  /** Every id the tray has shown in this chat, so a new one is known on sight. */
  seen: string[];
}

const UNFOLDED: NeedsYouFold = { stackFolded: false, folded: [], seen: [] };

export const foldKey = (sessionId: string) => `goose.needsYou.fold.${sessionId}`;

function isFold(value: unknown): value is NeedsYouFold {
  if (typeof value !== 'object' || value === null) return false;
  const fold = value as Record<string, unknown>;
  const ids = (list: unknown) => Array.isArray(list) && list.every((id) => typeof id === 'string');
  return typeof fold.stackFolded === 'boolean' && ids(fold.folded) && ids(fold.seen);
}

/**
 * Nothing stored (or storage unreadable, or a shape this build does not know) reads as nothing
 * folded and nothing seen: the tray opens as for new questions. No question is lost that way.
 */
export function readFold(sessionId: string): NeedsYouFold {
  try {
    const raw = window.localStorage.getItem(foldKey(sessionId));
    if (raw === null) return UNFOLDED;
    const parsed: unknown = JSON.parse(raw);
    return isFold(parsed) ? parsed : UNFOLDED;
  } catch {
    return UNFOLDED;
  }
}

function writeFold(sessionId: string, fold: NeedsYouFold): void {
  try {
    window.localStorage.setItem(foldKey(sessionId), JSON.stringify(fold));
  } catch {
    // A full or blocked storage only costs the fold its memory; the tray still folds this session.
  }
}

/** The fold as it must be with `open` on screen: new ids seen, the stack opened for them. */
export function foldFor(fold: NeedsYouFold, open: readonly string[]): NeedsYouFold {
  const unseen = open.filter((id) => !fold.seen.includes(id));
  if (open.length === 0) return fold;
  return {
    stackFolded: fold.stackFolded && unseen.length === 0,
    folded: [
      ...fold.folded.filter((id) => open.includes(id) && !unseen.includes(id)),
      ...unseen.slice(1),
    ],
    seen: open.filter((id) => fold.seen.includes(id) || unseen.includes(id)),
  };
}

const sameFold = (a: NeedsYouFold, b: NeedsYouFold) =>
  a.stackFolded === b.stackFolded &&
  a.folded.join('\n') === b.folded.join('\n') &&
  a.seen.join('\n') === b.seen.join('\n');

export interface FoldView {
  stackFolded: boolean;
  isFolded: (id: string) => boolean;
  toggleStack: () => void;
  toggle: (id: string) => void;
}

/** `openIds`: the ids the tray shows now, in order. */
export function useNeedsYouFold(sessionId: string, openIds: readonly string[]): FoldView {
  const [stored, setStored] = useState<NeedsYouFold>(() => readFold(sessionId));
  const [storedSession, setStoredSession] = useState(sessionId);
  if (storedSession !== sessionId) {
    setStoredSession(sessionId);
    setStored(readFold(sessionId));
  }

  // Derived during render, so a new question is open on its first frame, never a frame later.
  const fold = foldFor(stored, openIds);
  useEffect(() => {
    if (sameFold(fold, stored)) return;
    setStored(fold);
    writeFold(sessionId, fold);
  }, [fold, stored, sessionId]);

  const change = useCallback(
    (make: (current: NeedsYouFold) => NeedsYouFold) => {
      setStored((current) => {
        const next = make(current);
        writeFold(sessionId, next);
        return next;
      });
    },
    [sessionId]
  );

  return {
    stackFolded: fold.stackFolded,
    isFolded: (id) => fold.folded.includes(id),
    toggleStack: () =>
      change((current) => {
        const now = foldFor(current, openIds);
        return { ...now, stackFolded: !now.stackFolded };
      }),
    toggle: (id) =>
      change((current) => {
        const now = foldFor(current, openIds);
        return {
          ...now,
          folded: now.folded.includes(id)
            ? now.folded.filter((folded) => folded !== id)
            : [...now.folded, id],
        };
      }),
  };
}
