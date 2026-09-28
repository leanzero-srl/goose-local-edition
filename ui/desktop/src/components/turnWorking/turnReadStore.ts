import { useEffect, useSyncExternalStore } from 'react';
import type { PromptRead } from '../leanzero-swarm/engineFigures';

/**
 * A CHAT'S PROMPT BEING READ, by session (Q-301). The composer holds the one served-by derivation
 * (`useChatServedBy`) and publishes its turn's read — `promptRead` of the request the card leads
 * with — here; the chat's working row under the user's message reads it. The row never derives a
 * figure of its own.
 */
const bySession = new Map<string, PromptRead>();
const listeners = new Set<() => void>();

function publish(sessionId: string, read: PromptRead | null): void {
  if (read) bySession.set(sessionId, read);
  else if (!bySession.delete(sessionId)) return;
  listeners.forEach((l) => l());
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** The composer of `sessionId` says how far its turn's prompt is read; gone when it unmounts. */
export function usePublishTurnRead(sessionId: string | null, read: PromptRead | null): void {
  useEffect(() => {
    if (!sessionId) return undefined;
    publish(sessionId, read);
    return () => publish(sessionId, null);
  }, [sessionId, read]);
}

/** How far `sessionId`'s turn's prompt is read; null when no prompt of its is being read. */
export function useTurnReadOf(sessionId: string | null): PromptRead | null {
  return useSyncExternalStore(subscribe, () =>
    sessionId ? (bySession.get(sessionId) ?? null) : null
  );
}

export function resetTurnReadForTests(): void {
  bySession.clear();
  listeners.clear();
}
