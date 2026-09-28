/**
 * What a text the desktop shows or logs may say about a LeanZero Link relay (Q-409).
 *
 * goosed hands the desktop a linked Mac's engine as `http://127.0.0.1:<port>/relay/<capability>`,
 * and holding that path IS the authorization to use the relay. This is goose's one rule for hiding
 * it — `crates/goose-provider-types/src/redact.rs` `redact_relay_capability`, ported: the capability
 * is found by the path's SHAPE (the one segment after `/relay/`), never by a list of known secrets.
 * Both ports run the cases in `crates/goose-provider-types/src/redact.fixture.json`, so the two
 * cannot drift.
 */

const RELAY_PREFIX = '/relay/';

/** What a redacted capability reads as. */
export const REDACTED = '…';

/**
 * Where a URL path segment must end: Unicode White_Space (Rust's `char::is_whitespace` — a JS `\s`
 * differs on U+0085 and U+FEFF), or a character a serialized path always percent-encodes.
 */
const SEGMENT_END = /[\p{White_Space}/?#"<>`{}\\]/u;

/** `text` with every `/relay/<segment>` rendered `/relay/…`, in a bare URL or any text quoting one. */
export function redactRelayCapability(text: string): string {
  let out = '';
  let rest = text;
  for (let at = rest.indexOf(RELAY_PREFIX); at >= 0; at = rest.indexOf(RELAY_PREFIX)) {
    out += rest.slice(0, at) + RELAY_PREFIX;
    const after = rest.slice(at + RELAY_PREFIX.length);
    const found = after.search(SEGMENT_END);
    const end = found < 0 ? after.length : found;
    if (end > 0) out += REDACTED;
    rest = after.slice(end);
  }
  return out + rest;
}
