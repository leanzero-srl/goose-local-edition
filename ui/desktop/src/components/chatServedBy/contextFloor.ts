/**
 * The context counter's measured floor (Q-60). A turn that dropped mid-answer reports no usage,
 * so the counter kept its last value — 0 after a first turn whose 49k-token prompt the Studio had
 * read for 167 s. The prompt the engine read for THIS chat's turn (served-by `turnRequest`) is a
 * measured floor on the context; it holds until goose reports usage again, which then wins.
 */
export interface MeasuredPrompt {
  session: string | null;
  tokens: number;
  /** The usage goose had reported when it was read; a new report retires it. */
  atReported: number;
}

export function nextMeasuredPrompt(
  prev: MeasuredPrompt | null,
  session: string | null,
  promptTokens: number | null,
  reported: number
): MeasuredPrompt | null {
  if (promptTokens == null) return prev;
  if (
    prev &&
    prev.session === session &&
    prev.atReported === reported &&
    prev.tokens >= promptTokens
  ) {
    return prev;
  }
  return { session, tokens: promptTokens, atReported: reported };
}

export function shownContextTokens(
  measured: MeasuredPrompt | null,
  session: string | null,
  reported: number
): number {
  return measured && measured.session === session && measured.atReported === reported
    ? Math.max(reported, measured.tokens)
    : reported;
}

/**
 * The context counter's window while the engine that measured it is down (Q-60). The window is a
 * fact the engine reports only while it answers: a split whose rank died, a route whose Mac left,
 * a pool whose engines all stopped report none, and the counter vanished under the bar (3.0.41,
 * the dead split). The last window measured for THIS chat on THIS model holds until an engine
 * reports one again; another chat or another model never inherits it (0 = no counter).
 */
export interface KnownWindow {
  session: string | null;
  model: string | null;
  limit: number;
}

export function heldContextLimit(
  known: KnownWindow | null,
  session: string | null,
  model: string | null,
  read: number | null
): { limit: number; known: KnownWindow | null } {
  if (read != null && read > 0) return { limit: read, known: { session, model, limit: read } };
  if (known && known.session === session && known.model === model) {
    return { limit: known.limit, known };
  }
  return { limit: 0, known };
}
