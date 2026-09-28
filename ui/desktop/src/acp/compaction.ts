import type {
  CompactionPreviewResponse_unstable,
  CompactionSteerDto,
  CompactionSteerResponse_unstable,
} from '@aaif/goose-sdk';
import { getAcpClient } from './acpConnection';

/**
 * Client surface for a chat's compaction (Q-357, `_goose/unstable/session/compaction/*`): what the
 * next compaction would keep word for word (code only — no model call) and the person's note and
 * pins. The meter menu, the compaction card and the Context tab read and write through here and
 * never the `compaction.v0` record another way. Raw `extMethod` like the loops surface, so a field a
 * newer backend adds is never stripped.
 */

export type CompactionPreview = CompactionPreviewResponse_unstable;
export type CompactionSteer = CompactionSteerDto;

async function call<T>(method: string, params: Record<string, unknown>): Promise<T> {
  const client = await getAcpClient();
  return (await client.extMethod(method, params)) as unknown as T;
}

export async function compactionPreview(sessionId: string): Promise<CompactionPreview> {
  return call<CompactionPreview>('_goose/unstable/session/compaction/preview', { sessionId });
}

/** Replaces the chat's note and pins; answers what was saved. */
export async function compactionSteer(
  sessionId: string,
  steer: CompactionSteer
): Promise<CompactionSteer> {
  const saved = await call<CompactionSteerResponse_unstable>(
    '_goose/unstable/session/compaction/steer',
    { sessionId, steer }
  );
  return saved.steer;
}

/** The command that compacts now, with the note when there is one. */
export function compactCommand(note?: string | null): string {
  const trimmed = note?.trim();
  return trimmed ? `/compact ${trimmed}` : '/compact';
}
