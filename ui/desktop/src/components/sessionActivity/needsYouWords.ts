import type { IntlShape } from 'react-intl';
import { defineMessages } from '../../i18n';

/**
 * Q-486: the top bar said "2 need you" (open questions), the engine glance "1 needs you" (chats)
 * and the Active now row "Needs you · 2" — one screen, three counts, the same words. Every surface
 * that counts what waits on the person says it through HERE: when each waiting chat asks one
 * question the two counts agree and "N need you" says both; when they differ, the words name
 * what is counted — "2 questions in 1 chat".
 */
export const needsYouWords = defineMessages({
  chats: {
    id: 'needsYou.count.chats',
    defaultMessage: '{count, plural, one {# needs you} other {# need you}}',
  },
  questionsInChats: {
    id: 'needsYou.count.questionsInChats',
    defaultMessage:
      '{questions, plural, one {# question} other {# questions}} in {chats, plural, one {# chat} other {# chats}}',
  },
});

export interface NeedsYouCount {
  /** Open questions (and live elicitations) across the waiting chats. */
  questions: number;
  /** Chats with at least one of them. */
  chats: number;
}

export function needsYouCountLabel(intl: IntlShape, { questions, chats }: NeedsYouCount): string {
  return questions === chats
    ? intl.formatMessage(needsYouWords.chats, { count: chats })
    : intl.formatMessage(needsYouWords.questionsInChats, { questions, chats });
}
