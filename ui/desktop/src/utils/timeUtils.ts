import { currentLocale } from '../i18n';

/** The clock a transcript reads a time in: the locale's own hour and minute ("6:48 AM" in en). */
const MESSAGE_CLOCK: Intl.DateTimeFormatOptions = { hour: 'numeric', minute: '2-digit' };

/**
 * A clock time in the chat's format, for any surface that sits beside a transcript (the loop's tick
 * markers, ticks and pills — Q-316: "06:45" beside the chat's "6:48 AM"). `timeZone` is for a caller
 * that has already shifted the instant to its own offset and reads it back in UTC.
 */
export function formatClockTime(
  ms: number,
  locale: string = currentLocale,
  timeZone?: string
): string {
  return new Date(ms).toLocaleTimeString(locale, { ...MESSAGE_CLOCK, timeZone });
}

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
  const time = MESSAGE_CLOCK;

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
