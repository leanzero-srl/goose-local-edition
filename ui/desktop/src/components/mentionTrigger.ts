/** What may stand right before an "@" that opens the picker: nothing, whitespace, an opening bracket or quote. */
const TOKEN_START_BEFORE = /[\s([{<"'`“‘]/;

/**
 * Where the "@" the composer's file/agent picker is for starts in the text before the cursor, or
 * -1. Only an "@" that STARTS a token counts: inside a word — "k***@example.com",
 * "git@github.com:org/repo", "deploy@build-01" — it is part of what the person is saying, and
 * opening the picker there took their Enter and the message never went (Q-492).
 */
export function mentionTriggerStart(beforeCursor: string): number {
  const at = beforeCursor.lastIndexOf('@');
  if (at <= 0) return at;
  return TOKEN_START_BEFORE.test(beforeCursor[at - 1]) ? at : -1;
}
