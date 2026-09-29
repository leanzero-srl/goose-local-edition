import { ChatState } from '../../types/chatState';
import type { ImageData, Message, MessageContent } from '../../types/message';

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
 * How a turn no one is running was left:
 * - `prompt` (Q-493): the transcript ends on the person's prompt, with no reply;
 * - `midway` (Q-495): it ends on a tool call or a tool result with no reply after it — the turn
 *   died between tool calls. `message` is the row holding that call or result.
 */
export type StoppedTurn = { kind: 'prompt' | 'midway'; message: Message };

/**
 * The turn no one is answering, or null. Q-493: a chat whose turn died with the app showed the
 * prompt and nothing after it — it read as goose still thinking, or never asked. Q-495: one that
 * died between tool calls showed a finished tool card and nothing after it — it read as goose
 * about to continue. Every live fact must say no turn runs; any one of them saying it might — a
 * turn starting, streaming, cancelling, the engine's busy set not read yet — hides the line.
 * A turn the person stopped ends on its stored "You stopped this answer" notice and a finished one
 * on the model's words, so neither ends on a tool row.
 */
export function stoppedTurnOf(
  messages: readonly Message[],
  live: TurnLiveness
): StoppedTurn | null {
  if (live.chatState !== ChatState.Idle) return null;
  if (live.activePromptAttemptId || live.activeRunId || live.pendingCancelPromptAttemptId) {
    return null;
  }
  if (live.submitError !== undefined) return null;
  if (!live.engineRead || live.engineRunning) return null;
  const last = messages[messages.length - 1];
  if (!last || !last.metadata.userVisible) return null;
  // A tool result rides a user message too; only words the person wrote are a prompt.
  if (last.role === 'user' && last.content.some(isWords)) return { kind: 'prompt', message: last };
  const toolRow = lastToolRowOf(messages);
  return toolRow ? { kind: 'midway', message: toolRow } : null;
}

function isWords(content: MessageContent): boolean {
  return content.type === 'text' && content.text.trim().length > 0;
}

/**
 * The row whose tool call or tool result is the last thing the turn produced, or null. Reasoning
 * streamed after it is no reply; anything else after it — words, a notice — is the turn's end.
 */
function lastToolRowOf(messages: readonly Message[]): Message | null {
  for (let i = messages.length - 1; i >= 0; i--) {
    const message = messages[i];
    for (let j = message.content.length - 1; j >= 0; j--) {
      const content = message.content[j];
      if (content.type === 'thinking' || content.type === 'redactedThinking') continue;
      if (content.type === 'text' && !isWords(content)) continue;
      const tool = content.type === 'toolRequest' || content.type === 'toolResponse';
      return tool && message.metadata.userVisible ? message : null;
    }
  }
  return null;
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
