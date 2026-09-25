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
