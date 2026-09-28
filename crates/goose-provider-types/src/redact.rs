//! What a provider URL may say in an error text or a log line.
//!
//! Two kinds of secret ride provider URLs: credentials (`user:pass@`, `?key=…`) and a bearer
//! CAPABILITY in the path — LeanZero Link's inference relay is reached at
//! `http://127.0.0.1:<port>/relay/<capability>/…` (leanzero-link `inference.rs`), where holding the
//! path is the authorization (Q-402). The capability is recognised by the path's SHAPE — the one
//! segment after `/relay/` — never by a list of known secrets, so a relay started after this
//! process formatted nothing, or a secret no registry heard of, is redacted all the same.

/// The path segment whose NEXT segment is a capability.
const RELAY_PREFIX: &str = "/relay/";

/// What a redacted capability reads as.
pub const REDACTED: &str = "…";

/// `text` with every `/relay/<segment>` rendered `/relay/…`. Works on a bare URL and on any text
/// that quotes one (a `reqwest::Error`'s "for url (…)", a `Debug` dump, a JSON body the relay
/// re-rooted). The segment ends where a URL path segment must: `/`, `?`, `#`, whitespace, or a
/// character a serialized path always percent-encodes; everything else in between is the secret —
/// a quote's closing `)` after a bare base URL is over-redacted rather than a secret under-redacted.
pub fn redact_relay_capability(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some((before, after)) = rest.split_once(RELAY_PREFIX) {
        out.push_str(before);
        out.push_str(RELAY_PREFIX);
        let (segment, tail) =
            after.split_at(after.find(ends_a_path_segment).unwrap_or(after.len()));
        if !segment.is_empty() {
            out.push_str(REDACTED);
        }
        rest = tail;
    }
    out.push_str(rest);
    out
}

fn ends_a_path_segment(c: char) -> bool {
    c.is_whitespace()
        || matches!(
            c,
            '/' | '?' | '#' | '"' | '<' | '>' | '`' | '{' | '}' | '\\'
        )
}

/// Strip credentials and sensitive query parameters from a URL for safe
/// inclusion in error messages and logs. Drops userinfo (`user:pass@`),
/// all query parameters (which may contain API keys like `?key=...`) and a
/// relay capability ([`redact_relay_capability`]). A string that doesn't parse
/// as a URL (e.g. a bare path like "v1/models") keeps its text, capability
/// redacted.
pub fn sanitize_url(raw: &str) -> String {
    let Ok(mut url) = reqwest::Url::parse(raw) else {
        return redact_relay_capability(raw);
    };
    if !url.username().is_empty() || url.password().is_some() {
        let _ = url.set_username("");
        let _ = url.set_password(None);
    }
    url.set_query(None);
    redact_relay_capability(url.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    const CAPABILITY: &str = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";

    #[test]
    fn a_relay_url_never_carries_its_capability() {
        let relay = format!("http://127.0.0.1:61001/relay/{CAPABILITY}");
        for (raw, shown) in [
            (
                format!("{relay}/v1/chat/completions"),
                "http://127.0.0.1:61001/relay/…/v1/chat/completions",
            ),
            (
                format!("{relay}/goose/admission"),
                "http://127.0.0.1:61001/relay/…/goose/admission",
            ),
            (relay.clone(), "http://127.0.0.1:61001/relay/…"),
            (
                format!("{relay}/v1/models?key=abc#frag"),
                "http://127.0.0.1:61001/relay/…/v1/models#frag",
            ),
            (
                format!("/relay/{CAPABILITY}/goose/admission"),
                "/relay/…/goose/admission",
            ),
        ] {
            assert_eq!(sanitize_url(&raw), shown, "{raw}");
        }
        // Idempotent: an already-sanitized URL (whose `…` the parser percent-encodes) stays put.
        let once = sanitize_url(&format!("{relay}/v1/models"));
        assert_eq!(sanitize_url(&once), once);
    }

    /// The rule is the path's shape: any segment after `/relay/` is the capability, whatever it
    /// looks like — never only the 64 hex leanzero-link mints today.
    #[test]
    fn the_capability_is_found_by_shape_in_any_text() {
        let text = format!(
            "error sending request for url (http://127.0.0.1:1/relay/{CAPABILITY}/v1/models): \
             and {{\"admission\":\"/relay/not-hex_at-all/goose/admission\"}} and \
             (http://127.0.0.1:2/relay/{CAPABILITY})"
        );
        let redacted = redact_relay_capability(&text);
        assert!(!redacted.contains(CAPABILITY), "{redacted}");
        assert!(!redacted.contains("not-hex_at-all"), "{redacted}");
        assert_eq!(
            redacted,
            "error sending request for url (http://127.0.0.1:1/relay/…/v1/models): and \
             {\"admission\":\"/relay/…/goose/admission\"} and (http://127.0.0.1:2/relay/…"
        );
    }

    /// NEGATIVE CONTROLS: a URL with no `/relay/<segment>` is exactly what sanitizing always gave.
    #[test]
    fn a_url_that_is_not_a_relay_is_unchanged() {
        for (raw, shown) in [
            (
                "http://127.0.0.1:8091/v1/chat/completions",
                "http://127.0.0.1:8091/v1/chat/completions",
            ),
            (
                "https://user:pass@api.example.com/v1/chat?key=sk-123",
                "https://api.example.com/v1/chat",
            ),
            (
                "https://api.example.com/relayed/v1/models",
                "https://api.example.com/relayed/v1/models",
            ),
            (
                "https://api.example.com/v1/relay",
                "https://api.example.com/v1/relay",
            ),
            (
                "https://api.example.com/v1/relay/",
                "https://api.example.com/v1/relay/",
            ),
            ("v1/models", "v1/models"),
        ] {
            assert_eq!(sanitize_url(raw), shown, "{raw}");
        }
        let plain = "Server error (500) at http://127.0.0.1:8091/v1/chat/completions: boom";
        assert_eq!(redact_relay_capability(plain), plain);
    }
}
