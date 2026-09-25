/**
 * A turn served by a LeanZero Link peer's engine that the peer dropped. The relay ends the request
 * with a named `linkRelayFailed` (crates/leanzero-link/src/inference.rs `lost_in_flight_text` /
 * `unreachable_peer`), the provider raises it, and the agent loop (agents/agent.rs) appends it as
 * ASSISTANT TEXT to whatever the model had already written — glued, with no separator, because
 * the message's text parts are concatenated:
 *
 *   …listen, andRan into this error: Server error: linkRelayFailed: Link peer '<node id>' lost this
 *   request in flight: the peer answers but no longer holds it (2 looks in a row) — it was dropped
 *   on the peer's side, as when its LeanZero Link restarts.
 *
 *   Please retry if you think this is a transient or recoverable error.
 *
 * (recovery-relaunch-peer 22 s; recovery-kill-link 23 s — Q-49.) The two relay forms are both
 * matched; any other trailing error is left in the text, never guessed at.
 */
export const AGENT_ERROR_WRAP = 'Ran into this error: ';

const LINK_DROP =
  /linkRelayFailed: (?:Link peer '([^']+)' lost this request in flight|cannot reach Link peer '([^']+)')/;

export interface LinkDrop {
  /** What the model wrote before the drop — rendered as the answer, unchanged. */
  answer: string;
  /** The error as goose wrote it, verbatim, for Details. */
  raw: string;
  /** The Link node id the relay named. */
  peerId: string | null;
  /** The relay had sent the request (it was lost mid-way), vs never reached the peer at all. */
  inFlight: boolean;
}

/** The dropped turn's parts when `text` ENDS with a Link relay error, else null. */
export function splitLinkDrop(text: string): LinkDrop | null {
  const at = text.lastIndexOf(AGENT_ERROR_WRAP);
  if (at < 0) return null;
  const tail = text.slice(at);
  const m = LINK_DROP.exec(tail);
  if (!m) return null;
  return {
    answer: text.slice(0, at),
    raw: tail.trim(),
    peerId: m[1] ?? m[2] ?? null,
    inFlight: m[1] != null,
  };
}
