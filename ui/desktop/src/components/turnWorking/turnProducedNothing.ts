import { ChatState } from '../../types/chatState';
import type { Message } from '../../types/message';

/**
 * A turn is in flight and nothing of its answer is in the transcript yet: the last message is still
 * the person's. The model's first thought, word or tool call ends it — so does a stop.
 */
export function turnProducedNothing(chatState: ChatState, messages: readonly Message[]): boolean {
  if (chatState !== ChatState.Thinking && chatState !== ChatState.Streaming) return false;
  return messages[messages.length - 1]?.role === 'user';
}
