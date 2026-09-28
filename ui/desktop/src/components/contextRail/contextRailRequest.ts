/**
 * The door that opens a chat's rail on its Context tab (Q-357): the meter menu's "See what it
 * keeps" and the compaction card's "What was kept" are ways into the rail, not second surfaces.
 * A request nobody takes answers `false`.
 */

type OpenHandler = (sessionId: string) => boolean;

const openHandlers = new Set<OpenHandler>();

/** The rail registers here; the returned function unregisters it. */
export function onOpenContextRailRequest(handler: OpenHandler): () => void {
  openHandlers.add(handler);
  return () => {
    openHandlers.delete(handler);
  };
}

/** Open this chat's rail on the Context tab. Whether a rail took the request. */
export function requestOpenContextRail(sessionId: string): boolean {
  let taken = false;
  for (const handler of [...openHandlers]) {
    if (handler(sessionId)) taken = true;
  }
  return taken;
}

/**
 * The rail's Context tab re-reads its preview when a compaction ends or the note changes; this is
 * the event those moments raise.
 */
const changedListeners = new Set<(sessionId: string) => void>();

export function onCompactionSteerChanged(listener: (sessionId: string) => void): () => void {
  changedListeners.add(listener);
  return () => {
    changedListeners.delete(listener);
  };
}

export function announceCompactionSteerChanged(sessionId: string): void {
  for (const listener of [...changedListeners]) listener(sessionId);
}
