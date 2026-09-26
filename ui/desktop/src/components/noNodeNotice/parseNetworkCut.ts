/**
 * A turn whose stream was CUT: the provider raised `ProviderError::NetworkError` and the agent loop
 * (agents/agent.rs) ended the turn with it as assistant text, the closer last —
 *
 *   Network error: Stream decode error: stream ended before completion: no finish_reason and no
 *   [DONE] after 9771 data frames — the answer is incomplete
 *
 *   Please resend your message to try again.
 *
 * (E2E #3, 10:07: what the split's hang stop looked like in the chat — Q-122.) Glued after
 * whatever the model had written. Only this exact shape is matched; any other text is left alone.
 */
export const NETWORK_ERROR_PREFIX = 'Network error: ';
export const NETWORK_ERROR_CLOSER = 'Please resend your message to try again.';

export interface NetworkCut {
  /** What the model wrote before the cut — rendered as written. */
  answer: string;
  /** The error as goose wrote it, verbatim, closer included — for Details. */
  raw: string;
}

export function splitNetworkCut(text: string): NetworkCut | null {
  if (!text.trimEnd().endsWith(NETWORK_ERROR_CLOSER)) return null;
  const at = text.lastIndexOf(NETWORK_ERROR_PREFIX);
  if (at < 0) return null;
  return { answer: text.slice(0, at), raw: text.slice(at).trim() };
}
