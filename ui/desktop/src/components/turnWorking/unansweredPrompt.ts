import { ChatState } from '../../types/chatState';
import type { ImageData, Message } from '../../types/message';

/** What decides whether a turn could still be answering the person — the chat's live facts. */
export interface TurnLiveness {
  chatState: ChatState;
  /** This window's prompt call in flight for the chat. */
  activePromptAttemptId: string | null;
  /** The engine's run for the chat, as its session updates last said. */
  activeRunId: string | null;
  pendingCancelPromptAttemptId: string | null;
  /** The last prompt failed to submit: the failed-prompt banner already says so. */
  submitError: string | undefined;
  /** The engine's activity has been read at least once (before that, "not running" is unknown). */
  engineRead: boolean;
  /** The engine lists a turn running for the chat (a schedule, another window, a linked Mac). */
  engineRunning: boolean;
}

/**
 * The person's prompt that no turn is answering, or null. Q-493: a chat whose turn died with the
 * app showed the prompt and nothing after it — it read as goose still thinking, or never asked.
 * Every live fact must say no turn runs; any one of them saying it might — a turn starting,
 * streaming, cancelling, the engine's busy set not read yet — hides the line.
 */
export function unansweredPromptOf(
  messages: readonly Message[],
  live: TurnLiveness
): Message | null {
  if (live.chatState !== ChatState.Idle) return null;
  if (live.activePromptAttemptId || live.activeRunId || live.pendingCancelPromptAttemptId) {
    return null;
  }
  if (live.submitError !== undefined) return null;
  if (!live.engineRead || live.engineRunning) return null;
  const last = messages[messages.length - 1];
  if (!last || last.role !== 'user' || !last.metadata.userVisible) return null;
  // A tool result rides a user message too; only words the person wrote are a prompt.
  const asked = last.content.some((c) => c.type === 'text' && c.text.trim().length > 0);
  return asked ? last : null;
}

/** The prompt's own words and images, to send again through the composer's door. */
export function resendInputOf(message: Message): { msg: string; images: ImageData[] } {
  let msg = '';
  const images: ImageData[] = [];
  for (const content of message.content) {
    if (content.type === 'text') msg += content.text;
    else if (content.type === 'image')
      images.push({ data: content.data, mimeType: content.mimeType });
  }
  return { msg, images };
}
