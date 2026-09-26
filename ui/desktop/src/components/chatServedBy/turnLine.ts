import type { FormingStatus } from '@aaif/goose-sdk';
import type { IntlShape } from 'react-intl';
import { defineMessages } from '../../i18n';
import { ChatState } from '../../types/chatState';
import { getThinkingMessage, type Message } from '../../types/message';
import { turnCueText } from './turnCueText';
import type { TurnCue } from './turnStatus';

const i18n = defineMessages({
  swarmHeld: {
    id: 'turnLine.swarmHeld',
    defaultMessage: 'swarm paused — nothing is running until you resume',
  },
  withForming: {
    id: 'turnLine.withForming',
    defaultMessage: '{cue} — {forming}',
  },
  formingCalls: {
    id: 'turnLine.formingCalls',
    defaultMessage:
      '{count, plural, one {a tool call to {tool}} other {# tool calls, the latest to {tool}}}',
  },
});

/** What the status line under the composer says for this turn, and what its disclosure lists. */
export interface TurnLine {
  /** undefined = the chat state's own default words. */
  message: string | undefined;
  forming: FormingStatus | null;
}

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

/**
 * THE STATUS LINE (Q-151). A HELD swarm run outranks everything: the provider call is still open but
 * nothing is computed. Then the turn's cue (turnStatus.ts) — while the engine writes, its own time,
 * tokens and rate lead, and the calls forming follow by the names the chat gives tools; the counts
 * of what the chat has received stay behind the disclosure, where they no longer read as the size of
 * the answer (18.5k chars beside the engine's 24k tokens). Without a cue, goose's own words.
 */
export function turnLine(
  intl: IntlShape,
  chatState: ChatState,
  held: boolean,
  cue: TurnCue | null,
  last: Message | undefined
): TurnLine {
  if (chatState === ChatState.LoadingConversation) return { message: undefined, forming: null };
  if (held) return { message: intl.formatMessage(i18n.swarmHeld), forming: null };
  const forming = formingOf(last);
  if (!cue) return { message: getThinkingMessage(last), forming };
  const cueText = turnCueText(intl, cue);
  if (cue.kind !== 'writing' || !forming) return { message: cueText, forming };
  const latest = forming.calls[forming.calls.length - 1];
  return {
    message: intl.formatMessage(i18n.withForming, {
      cue: cueText,
      forming: intl.formatMessage(i18n.formingCalls, {
        count: forming.calls.length,
        tool: latest.title,
      }),
    }),
    forming,
  };
}
