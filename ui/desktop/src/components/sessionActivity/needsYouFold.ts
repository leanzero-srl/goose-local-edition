import { useCallback, useEffect, useMemo, useState } from 'react';

/**
 * Q-340: what a person folded in a chat's needs-you tray — the whole stack, or single cards — kept
 * per chat in this window's localStorage (a viewer's convenience, not the chat's state). A question
 * the person has not yet SEEN in this chat always opens, and opens the stack with it, so folding
 * never hides a new question.
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
 * folded: every card shows. That is the state a person never loses a question in.
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

export interface FoldView {
  stackFolded: boolean;
  isFolded: (id: string) => boolean;
  toggleStack: () => void;
  toggle: (id: string) => void;
}

/** `openIds`: the ids the tray shows now, in order. */
export function useNeedsYouFold(sessionId: string, openIds: readonly string[]): FoldView {
  const [fold, setFold] = useState<NeedsYouFold>(() => readFold(sessionId));
  const [foldSession, setFoldSession] = useState(sessionId);
  if (foldSession !== sessionId) {
    setFoldSession(sessionId);
    setFold(readFold(sessionId));
  }

  const idsKey = openIds.join('\n');
  const unseen = useMemo(
    () => (idsKey ? idsKey.split('\n') : []).filter((id) => !fold.seen.includes(id)),
    [idsKey, fold.seen]
  );

  // A new question opens itself and the stack; closed ids are forgotten so storage stays small.
  useEffect(() => {
    const open = idsKey ? idsKey.split('\n') : [];
    const seen = open.filter((id) => fold.seen.includes(id) || unseen.includes(id));
    const folded = fold.folded.filter((id) => open.includes(id) && !unseen.includes(id));
    const stackFolded = fold.stackFolded && unseen.length === 0 && open.length > 0;
    const same =
      stackFolded === fold.stackFolded &&
      folded.join('\n') === fold.folded.join('\n') &&
      seen.join('\n') === fold.seen.join('\n');
    if (same) return;
    const next = { stackFolded, folded, seen };
    setFold(next);
    // Before the tray has shown anything there is nothing to forget: keep what was stored.
    if (open.length > 0) writeFold(sessionId, next);
  }, [idsKey, unseen, fold, sessionId]);

  const change = useCallback(
    (make: (current: NeedsYouFold) => NeedsYouFold) => {
      setFold((current) => {
        const next = make(current);
        writeFold(sessionId, next);
        return next;
      });
    },
    [sessionId]
  );

  return {
    stackFolded: fold.stackFolded && unseen.length === 0,
    isFolded: (id) => fold.folded.includes(id) && !unseen.includes(id),
    toggleStack: () => change((current) => ({ ...current, stackFolded: !current.stackFolded })),
    toggle: (id) =>
      change((current) => ({
        ...current,
        folded: current.folded.includes(id)
          ? current.folded.filter((folded) => folded !== id)
          : [...current.folded, id],
      })),
  };
}
