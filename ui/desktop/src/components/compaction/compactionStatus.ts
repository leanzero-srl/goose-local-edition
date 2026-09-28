import type { CompactionStatus } from '@aaif/goose-sdk';
import type { Message } from '../../types/message';

/**
 * A chat's compaction as its messages carry it (Q-357). The engine sends every stage as a status
 * (`StatusMessage::Progress{compaction}` while it reads and writes, `Notice{compaction}` when it
 * ends); the ACP adapter keeps the status on the message's `data` with `kind: 'compaction'`, and
 * the stored notice replays the same way. One reader for the card, the composer and the queue.
 */

export const COMPACTION_KIND = 'compaction';

const STAGES = new Set(['reading', 'writing', 'done', 'question', 'failed']);

export function isCompactionData(data: unknown): data is CompactionStatus & { kind: 'compaction' } {
  if (!data || typeof data !== 'object') return false;
  const record = data as Record<string, unknown>;
  return (
    record.kind === COMPACTION_KIND && typeof record.stage === 'string' && STAGES.has(record.stage)
  );
}

/** The compaction status a message carries, when it is a compaction's card. */
export function compactionOf(message: Message | undefined): CompactionStatus | undefined {
  if (!message || message.role !== 'assistant') return undefined;
  for (const content of message.content) {
    if (content.type === 'systemNotification' && isCompactionData(content.data)) {
      return content.data;
    }
  }
  return undefined;
}

export function isRunning(status: CompactionStatus | undefined): boolean {
  return status?.stage === 'reading' || status?.stage === 'writing';
}

/**
 * The chat is compacting right now: its latest compaction card is still reading or writing. A
 * message typed now waits in the queue and is sent against the compacted conversation.
 */
export function compactingNow(messages: readonly Message[]): boolean {
  for (let i = messages.length - 1; i >= 0; i--) {
    const status = compactionOf(messages[i]);
    if (status) return isRunning(status);
  }
  return false;
}
