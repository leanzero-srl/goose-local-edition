/**
 * How much of the person's input waits queued per chat (DESIGN-SESSION-LOOPS §5.3, §9 L4r): the
 * composer's queued messages (L4) and the answers queued on a needs-you card during a turn (Q-341).
 * Each source writes its own count; the tick driver reads the SUM before submitting a tick and waits
 * on it to clear, so what the person queued is always sent before the next tick. Events, never a poll.
 */

import { useCallback, useSyncExternalStore } from 'react';

type Listener = (count: number) => void;
type Source = 'composer' | 'answers';

const counts = new Map<string, Map<Source, number>>();
const listeners = new Map<string, Set<Listener>>();

export function getPendingUserInput(sessionId: string): number {
  let total = 0;
  for (const count of counts.get(sessionId)?.values() ?? []) total += count;
  return total;
}

function setPending(sessionId: string, source: Source, count: number): void {
  const next = Math.max(0, count);
  const bySource = counts.get(sessionId) ?? new Map<Source, number>();
  if ((bySource.get(source) ?? 0) === next) return;
  if (next === 0) {
    bySource.delete(source);
  } else {
    bySource.set(source, next);
  }
  if (bySource.size === 0) {
    counts.delete(sessionId);
  } else {
    counts.set(sessionId, bySource);
  }
  const total = getPendingUserInput(sessionId);
  for (const listener of [...(listeners.get(sessionId) ?? [])]) {
    listener(total);
  }
}

/** The composer's queued messages. */
export function setPendingUserInput(sessionId: string, count: number): void {
  setPending(sessionId, 'composer', count);
}

/** The answers queued on this chat's needs-you cards while its turn runs (Q-341). */
export function setPendingAnswers(sessionId: string, count: number): void {
  setPending(sessionId, 'answers', count);
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
