import type {
  NeedsYouAction,
  NeedsYouItemDto,
  SessionActivityResponse_unstable,
} from '@aaif/goose-sdk';
import { getAcpClient } from './acpConnection';

/** What every session is doing: the engine's busy set and every open "needs you" item. */
export async function acpSessionActivity(): Promise<SessionActivityResponse_unstable> {
  const client = await getAcpClient();
  return client.goose.sessionActivityGet_unstable({});
}

/** Close an open item: `answer` records the person's text, `dismiss` records nothing. */
export async function acpResolveNeedsYou(
  sessionId: string,
  itemId: string,
  action: NeedsYouAction,
  answer?: string
): Promise<NeedsYouItemDto> {
  const client = await getAcpClient();
  const { item } = await client.goose.needsYouResolve_unstable({
    sessionId,
    itemId,
    action,
    ...(answer != null ? { answer } : {}),
  });
  return item;
}
