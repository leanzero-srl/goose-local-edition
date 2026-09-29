import { useEffect, useSyncExternalStore } from 'react';

/**
 * A CHAT'S TURN HELD BEFORE THE MODEL, by session (Q-461). While the node loader holds the turn —
 * its wait, or a load for it — or the engine queues it with a reason, the composer's bar says so
 * in its own words; the working row under the user's message then says nothing, so one status
 * speaks at a time (live 3.0.74: "Waiting for the model's first words · 1m" beside "Waiting while
 * Work's Mac Studio serves …"). The composer holds the one served-by derivation and publishes here.
 */
const held = new Set<string>();
const listeners = new Set<() => void>();

function publish(sessionId: string, isHeld: boolean): void {
  if (isHeld === held.has(sessionId)) return;
  if (isHeld) held.add(sessionId);
  else held.delete(sessionId);
  listeners.forEach((l) => l());
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** The composer of `sessionId` says whether its bar speaks for the turn; gone when it unmounts. */
export function usePublishTurnHeld(sessionId: string | null, isHeld: boolean): void {
  useEffect(() => {
    if (!sessionId) return undefined;
    publish(sessionId, isHeld);
    return () => publish(sessionId, false);
  }, [sessionId, isHeld]);
}

/** Whether `sessionId`'s turn is held where the composer's bar says why. */
export function useTurnHeldOf(sessionId: string | null): boolean {
  return useSyncExternalStore(subscribe, () => (sessionId ? held.has(sessionId) : false));
}

export function resetTurnHeldForTests(): void {
  held.clear();
  listeners.clear();
}
