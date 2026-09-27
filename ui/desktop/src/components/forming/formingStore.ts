import { useEffect, useSyncExternalStore } from 'react';
import type { FormingStatus } from '@aaif/goose-sdk';
import type { Message } from '../../types/message';

/**
 * WHAT A CHAT'S TURN IS FORMING, by session — the tool calls a response is still streaming, and the
 * text written beside them (Q-151). The chat that holds the stream publishes it (BaseChat); the
 * engine card at the foot of the sidebar reads it for the chat it names and lists it behind "What
 * it's writing" (Q-215: the status line under the composer that carried this disclosure is gone).
 */

/**
 * The forming response the last status carried (the adapter keeps it on the progress line's
 * `data`, gooseSessionNotifications.ts); null for every other status.
 */
export function formingOf(message: Message | undefined): FormingStatus | null {
  if (!message || message.role !== 'assistant') return null;
  for (const content of message.content) {
    if (content.type !== 'systemNotification' || content.notificationType !== 'thinkingMessage') {
      continue;
    }
    const data = content.data as Partial<FormingStatus> | undefined;
    return data && Array.isArray(data.calls) && data.calls.length > 0
      ? (data as FormingStatus)
      : null;
  }
  return null;
}

const bySession = new Map<string, FormingStatus>();
const listeners = new Set<() => void>();

function publish(sessionId: string, forming: FormingStatus | null): void {
  if (forming) bySession.set(sessionId, forming);
  else if (!bySession.delete(sessionId)) return;
  listeners.forEach((l) => l());
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** The chat that holds `sessionId`'s stream says what its turn is forming; gone when it unmounts. */
export function usePublishForming(sessionId: string | null, forming: FormingStatus | null): void {
  useEffect(() => {
    if (!sessionId) return undefined;
    publish(sessionId, forming);
    return () => publish(sessionId, null);
  }, [sessionId, forming]);
}

/** What `sessionId`'s turn is forming now; null when nothing forms or no window holds that chat. */
export function useFormingOf(sessionId: string | null): FormingStatus | null {
  return useSyncExternalStore(subscribe, () =>
    sessionId ? (bySession.get(sessionId) ?? null) : null
  );
}

export function resetFormingForTests(): void {
  bySession.clear();
  listeners.clear();
}
