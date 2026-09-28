/**
 * How many messages the composer holds queued per chat (DESIGN-SESSION-LOOPS §5.3, §9 L4r). The
 * composer writes it (L4); the tick driver reads it before submitting a tick and waits on it to
 * clear, so a message the user queued is always sent before the next tick. Events, never a poll.
 */

import { useCallback, useSyncExternalStore } from 'react';

type Listener = (count: number) => void;

const counts = new Map<string, number>();
const listeners = new Map<string, Set<Listener>>();

export function getPendingUserInput(sessionId: string): number {
  return counts.get(sessionId) ?? 0;
}

export function setPendingUserInput(sessionId: string, count: number): void {
  const next = Math.max(0, count);
  if (getPendingUserInput(sessionId) === next) return;
  if (next === 0) {
    counts.delete(sessionId);
  } else {
    counts.set(sessionId, next);
  }
  for (const listener of [...(listeners.get(sessionId) ?? [])]) {
    listener(next);
  }
}

export function subscribePendingUserInput(sessionId: string, listener: Listener): () => void {
  const set = listeners.get(sessionId) ?? new Set<Listener>();
  set.add(listener);
  listeners.set(sessionId, set);
  return () => {
    set.delete(listener);
    if (set.size === 0 && listeners.get(sessionId) === set) {
      listeners.delete(sessionId);
    }
  };
}

export function usePendingUserInput(sessionId: string): number {
  const subscribe = useCallback(
    (onChange: () => void) => subscribePendingUserInput(sessionId, onChange),
    [sessionId]
  );
  return useSyncExternalStore(subscribe, () => getPendingUserInput(sessionId));
}
