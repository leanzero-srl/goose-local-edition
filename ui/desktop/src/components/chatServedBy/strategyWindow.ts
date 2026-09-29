import type { NodeServedTurnDto } from '@aaif/goose-sdk';

/**
 * The window a `strategy:` chat's counter shows (Q-467): the node that served its last turn, as
 * goosed recorded it — the window compaction and the context line read for this chat (Q-463). The
 * pool's engines are not this chat's: a strategy turn a cloud node or the Studio single answered
 * ran on THAT node, and "42k / 262k" (the split's window) was a number about another engine.
 * - `known`: the serving node reported its window when it took the turn.
 * - `unknown`: it did not (a cloud model its catalog lacks), or no node served this chat yet — the
 *   counter says so, never the pool's number.
 * - `unread`: the record is not read yet, or its read failed — nothing new was measured.
 * null = not a strategy chat: the caller's own rule stands.
 */
export type StrategyWindow =
  | { kind: 'known'; window: number }
  | { kind: 'unknown' }
  | { kind: 'unread' };

export function strategyChatWindow(
  model: string | null | undefined,
  record: NodeServedTurnDto | null | undefined
): StrategyWindow | null {
  if (!model?.startsWith('strategy:')) return null;
  if (record === undefined) return { kind: 'unread' };
  const window = record?.contextWindow;
  return typeof window === 'number' && window > 0 ? { kind: 'known', window } : { kind: 'unknown' };
}
