import { currentLocale } from '../i18n';

/**
 * THE one rule for a message's time, wherever a transcript shows it (UserMessage, GooseMessage,
 * the session history view): a message from today reads as its time ("5:55 AM"); any other day
 * reads as date and time in the same words ("Sep 21, 8:46 PM"), with the year only when it is not
 * this year ("Sep 21, 2025, 8:46 PM"). The date used to be numeric ("09/21/2026 8:46 PM"), so one
 * transcript read in two unrelated formats.
 */
export function formatMessageTimestamp(
  timestamp?: number,
  now: Date = new Date(),
  locale: string = currentLocale
): string {
  const date = timestamp ? new Date(timestamp * 1000) : now;
  const time: Intl.DateTimeFormatOptions = { hour: 'numeric', minute: '2-digit' };

  const sameDay =
    date.getDate() === now.getDate() &&
    date.getMonth() === now.getMonth() &&
    date.getFullYear() === now.getFullYear();
  if (sameDay) {
    return date.toLocaleTimeString(locale, time);
  }

  return date.toLocaleString(locale, {
    month: 'short',
    day: 'numeric',
    ...(date.getFullYear() === now.getFullYear() ? {} : { year: 'numeric' }),
    ...time,
  });
}
