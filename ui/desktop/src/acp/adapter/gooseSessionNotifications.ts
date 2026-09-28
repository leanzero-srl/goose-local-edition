import type { GooseSessionNotification_unstable } from '@aaif/goose-sdk';
import type { Message } from '../../types/message';
import { COMPACTION_KIND, isCompactionData } from '../../components/compaction/compactionStatus';
import { type AcpChatStateChange, type AdapterState, messagesChange } from './shared';

export function applyGooseSessionNotification(
  state: AdapterState,
  notification: GooseSessionNotification_unstable
): AcpChatStateChange[] {
  const update = notification.update;

  switch (update.sessionUpdate) {
    case 'usage_update':
      return [
        {
          type: 'tokenState',
          tokenState: {
            totalTokens: update.used,
            accumulatedInputTokens: update.accumulatedInputTokens,
            accumulatedOutputTokens: update.accumulatedOutputTokens,
            accumulatedTotalTokens: update.accumulatedInputTokens + update.accumulatedOutputTokens,
            ...(update.accumulatedCost !== undefined
              ? { accumulatedCost: update.accumulatedCost }
              : {}),
          },
        },
      ];
    case 'status_message':
      return applyStatusMessage(state, notification.sessionId, update);
    default:
      return [];
  }
}

function applyStatusMessage(
  state: AdapterState,
  sessionId: string,
  update: Extract<GooseSessionNotification_unstable['update'], { sessionUpdate: 'status_message' }>
): AcpChatStateChange[] {
  const notificationType = update.status.type === 'notice' ? 'inlineMessage' : 'thinkingMessage';
  // A response forming tool calls carries what it has received (Q-151): the chat lists it behind
  // the status line. Every other status carries none, so an earlier line's list never lingers.
  const forming =
    update.status.type === 'progress' ? (update.status.forming ?? undefined) : undefined;
  // Q-169: a stopped turn's notice carries its numbers; the chat renders its line from them.
  const stopped =
    update.status.type === 'notice' && update.status.stopped
      ? {
          kind: 'turnStopped' as const,
          elapsedMs: update.status.stopped.elapsedMs,
          ...(update.status.stopped.outputTokens != null
            ? { outputTokens: update.status.stopped.outputTokens }
            : {}),
        }
      : undefined;

  // Q-357: a compaction's stages ride its status; the card reads them from `data`.
  const compaction = update.status.compaction
    ? { kind: COMPACTION_KIND, ...update.status.compaction }
    : undefined;

  // A progress status replaces the one directly before it: the loading line reads only the last
  // message, and a live counter (a response forming tool calls) would otherwise add a message per tick.
  const last = state.messages[state.messages.length - 1];
  const lastProgress = notificationType === 'thinkingMessage' ? progressContentOf(last) : undefined;
  if (lastProgress) {
    lastProgress.msg = update.status.message;
    lastProgress.data = compaction ?? forming;
    return messagesChange(state);
  }

  // How a compaction ended takes the place of its live card, so the chat shows ONE card at the
  // compaction point that turns from "Compacting" into "Compacted" (or its question, or its failure).
  if (compaction && notificationType === 'inlineMessage') {
    const live = liveCompactionContentOf(state.messages);
    if (live) {
      live.notificationType = 'inlineMessage';
      live.msg = update.status.message;
      live.data = compaction;
      return messagesChange(state);
    }
  }

  state.messages.push({
    id: `acp_status_${sessionId}_${Date.now()}_${Math.random().toString(36).slice(2, 10)}`,
    role: 'assistant',
    created: Math.floor(Date.now() / 1000),
    content: [
      {
        type: 'systemNotification',
        notificationType,
        msg: update.status.message,
        ...(forming ? { data: forming } : {}),
        ...(stopped ? { data: stopped } : {}),
        ...(compaction ? { data: compaction } : {}),
      },
    ],
    metadata: {
      userVisible: true,
      agentVisible: false,
    },
  });

  return messagesChange(state);
}

function progressContentOf(message: Message | undefined) {
  if (!message?.id?.startsWith('acp_status_') || message.content.length !== 1) {
    return undefined;
  }
  const [content] = message.content;
  return content.type === 'systemNotification' && content.notificationType === 'thinkingMessage'
    ? content
    : undefined;
}

/** The newest live compaction card (still reading or writing) this prompt's statuses put up. */
function liveCompactionContentOf(messages: Message[]) {
  for (let i = messages.length - 1; i >= 0; i--) {
    const message = messages[i];
    if (!message.id?.startsWith('acp_status_')) continue;
    for (const content of message.content) {
      if (
        content.type === 'systemNotification' &&
        isCompactionData(content.data) &&
        (content.data.stage === 'reading' || content.data.stage === 'writing')
      ) {
        return content;
      }
    }
  }
  return undefined;
}
