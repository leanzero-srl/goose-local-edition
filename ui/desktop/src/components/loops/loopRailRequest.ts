import { useCallback, useSyncExternalStore } from 'react';

/**
 * Two things the composer and the rail share about a chat's loop (DESIGN-SESSION-LOOPS §8.1, §8.3):
 *
 * - the door that opens the rail on its Loop tab — the composer's loop chip is a way into the rail,
 *   not a second loop surface. A request nobody takes answers `false`;
 * - whether the person has seen an ended loop. The rail's pill and the composer's chip both say
 *   "Loop ended" until the Loop tab has been opened once, then both hide — ONE record, so the two
 *   never disagree. It is a per-viewer convenience in localStorage: a storage failure only forgets.
 */

type OpenHandler = (sessionId: string) => boolean;

const openHandlers = new Set<OpenHandler>();

/** The rail registers here; the returned function unregisters it. */
export function onOpenLoopRailRequest(handler: OpenHandler): () => void {
  openHandlers.add(handler);
  return () => {
    openHandlers.delete(handler);
  };
}

/** Open this chat's rail on the Loop tab. Whether a rail took the request. */
export function requestOpenLoopRail(sessionId: string): boolean {
  let taken = false;
  for (const handler of [...openHandlers]) {
    if (handler(sessionId)) taken = true;
  }
  return taken;
}

const endedSeenKey = (loopId: string) => `goose.sessionRail.endedSeen.${loopId}`;
const seenListeners = new Set<() => void>();
// What this window has marked seen, so a storage that refuses writes still hides the chip and the
// pill together for the rest of the session.
const sessionSeen = new Set<string>();

export function readEndedSeen(loopId: string): boolean {
  try {
    return window.localStorage.getItem(endedSeenKey(loopId)) === '1';
  } catch {
    return false;
  }
}

export function markEndedSeen(loopId: string): void {
  try {
    window.localStorage.setItem(endedSeenKey(loopId), '1');
  } catch {
    // Forgotten across remounts only; this session still hides it below.
  }
  sessionSeen.add(loopId);
  for (const listener of [...seenListeners]) listener();
}

function subscribeSeen(listener: () => void): () => void {
  seenListeners.add(listener);
  return () => {
    seenListeners.delete(listener);
  };
}

/** Whether the ended loop `loopId` has been seen (false for no loop). */
export function useEndedSeen(loopId: string | null): boolean {
  const read = useCallback(
    () => (loopId ? sessionSeen.has(loopId) || readEndedSeen(loopId) : false),
    [loopId]
  );
  return useSyncExternalStore(subscribeSeen, read);
}
