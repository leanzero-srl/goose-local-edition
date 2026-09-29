//! Secret values in memory text: found, named, and replaced by `<redacted: NAME>` before an entry is
//! written and again whenever an entry leaves the store (index, search, recall).
//!
//! Why (Q-505, 2026-09-29): `evolve-goose-test-loop.txt` in the global store carried a runbook command
//! line with a live `CONTEXT7_API_KEY=<value>` assignment. Nothing checked it on the way in, and every
//! memory read — the startup index, `search_memories`, `retrieve_memories`, recall's turn context —
//! hands the entry to whatever provider the chat uses, cloud ones included.
//!
//! Three classes, each a general shape rather than a list fitted to one key:
//! 1. an ASSIGNMENT to a secret-named key — `NAME=value`, `NAME: value`, `"name": "value"`, `--name=value`
//!    — where the name's last word is token / secret / password / passwd / credential (or ends in
//!    one: `authToken`, `CLIENT_SECRET`), or `key` qualified as a credential key (`API_KEY`, `apiKey`,
//!    `access_key`, `private-key`). A bare or otherwise-qualified `key` (`"key": "PROJ-12"`, `sortKey`)
//!    counts only when its value is random-shaped (class 3's shape): a Jira issue key is not a secret.
//! 2. a token carrying a vendor's published credential PREFIX (`sk-`, `ghp_`, `github_pat_`, `xoxb-`,
//!    `AKIA`, `AIza`, `glpat-`, `ctx7sk-`, …) with a long enough body holding a digit, anywhere in text;
//! 3. a RANDOM-SHAPED token (long, letters and digits interleaved) a few words after a secret word
//!    ("the API key is …", "Bearer …").
//!
//! Prose that only SAYS the words — "the primary key of the table", "max_tokens: 4096", "a token
//! budget" — carries no value of a secret's shape and is left alone.

/// What `redact_secrets` did: the text with every secret value replaced, and the name each
/// replaced value was filed under, in order of appearance.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Redacted {
    pub text: String,
    pub keys: Vec<String>,
}

/// One secret value found in a text: its 1-based line and the NAME it sits under. Never the value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretHit {
    pub line: usize,
    pub key: String,
}

/// The marker a redacted value becomes.
pub fn redaction_marker(key: &str) -> String {
    format!("<redacted: {key}>")
}

/// `text` with every secret value replaced by [`redaction_marker`]. Idempotent: a marker is a
/// placeholder, never a value, so redacting redacted text changes nothing.
#[allow(clippy::string_slice)] // `find_in_line` ranges start and end at ASCII bytes or the line end.
pub fn redact_secrets(text: &str) -> Redacted {
    let mut out = String::with_capacity(text.len());
    let mut keys = Vec::new();
    for (index, line) in text.split('\n').enumerate() {
        if index > 0 {
            out.push('\n');
        }
        let mut last = 0;
        for found in find_in_line(line) {
            out.push_str(&line[last..found.start]);
            out.push_str(&redaction_marker(&found.key));
            keys.push(found.key);
            last = found.end;
        }
        out.push_str(&line[last..]);
    }
    Redacted { text: out, keys }
}

/// Every secret value in `text` by line and name — what a report may print. The value is never
/// part of a hit.
pub fn secret_hits(text: &str) -> Vec<SecretHit> {
    text.split('\n')
        .enumerate()
        .flat_map(|(index, line)| {
            find_in_line(line).into_iter().map(move |found| SecretHit {
                line: index + 1,
                key: found.key,
            })
        })
        .collect()
}

struct Found {
    start: usize,
    end: usize,
    key: String,
}

/// The last word of a name that makes it a credential name on its own.
const SECRET_WORDS: &[&str] = &["token", "secret", "password", "passwd", "credential"];

/// The qualifiers that make a `key` a credential key: `API_KEY`, `accessKey`, `private-key`.
const CREDENTIAL_KEY_QUALIFIERS: &[&str] = &[
    "api",
    "access",
    "secret",
    "private",
    "auth",
    "client",
    "signing",
    "encryption",
    "license",
    "master",
    "session",
];

/// Words that, a few words before a random-shaped token, say the token is a credential.
const SECRET_CONTEXT_WORDS: &[&str] = &[
    "key",
    "apikey",
    "token",
    "secret",
    "password",
    "passwd",
    "credential",
    "bearer",
];

/// Vendors' published credential prefixes (the shapes secret scanners key on). A match needs a body
/// of at least [`PREFIXED_MIN_BODY_CHARS`] holding a digit, so `sk-learn` is a word, not a key.
const CREDENTIAL_PREFIXES: &[&str] = &[
    "sk-ant-",
    "sk-proj-",
    "sk-",
    "sk_live_",
    "sk_test_",
    "rk_live_",
    "ghp_",
    "gho_",
    "ghu_",
    "ghs_",
    "ghr_",
    "github_pat_",
    "xoxb-",
    "xoxp-",
    "xoxa-",
    "xoxr-",
    "xoxs-",
    "glpat-",
    "ctx7sk-",
    "hf_",
    "npm_",
    "AKIA",
    "ASIA",
    "AIza",
];

// ratio: the shortest body among the prefixed formats above (AWS AKIA + 16); every published format
// is at least this long past its prefix, and an English hyphenated word rarely is and holds a digit.
const PREFIXED_MIN_BODY_CHARS: usize = 16;

// measured: the 32-char base62 values issued by the vendors above interleave letters and digits
// every ~4 chars (expected transitions 2·p·(1−p)·31 ≈ 8 at p = 10/62); a slug with a year
// ("improve-toolcall-reliability-2026") has 1, a Jira key ("PROJ-1234") 1. Four is half the
// expectation, so a real random token passes and a named-thing does not.
const RANDOM_MIN_TRANSITIONS: usize = 4;

// ratio: the shortest random-shaped credential the prefixed class accepts (prefix + 16) — a token
// shorter than that is not flagged by shape alone.
const RANDOM_MIN_CHARS: usize = 20;

// measured: "hunter2"-class passwords are 7+; a colon form ("token: gpt-4o") is prose-prone, so an
// unquoted value after `NAME:` needs letters AND digits AND this length before it reads as a secret.
const COLON_VALUE_MIN_CHARS: usize = 8;

/// How many whitespace words before a random-shaped token a secret word may sit ("the API key is X":
/// key is two words back).
const SECRET_CONTEXT_WINDOW: usize = 3;

fn is_name_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.')
}

fn is_token_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-')
}

/// The words of an identifier, lower-cased: split at `_`, `-`, `.` and camelCase humps.
fn name_words(name: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut previous_lower = false;
    for c in name.chars() {
        if !c.is_ascii_alphanumeric() {
            if !current.is_empty() {
                words.push(std::mem::take(&mut current));
            }
            previous_lower = false;
            continue;
        }
        if c.is_ascii_uppercase() && previous_lower && !current.is_empty() {
            words.push(std::mem::take(&mut current));
        }
        previous_lower = c.is_ascii_lowercase() || c.is_ascii_digit();
        current.push(c.to_ascii_lowercase());
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum NameStrength {
    /// The name says credential: `API_KEY`, `GITHUB_TOKEN`, `db_password`.
    Credential,
    /// The name ends in a bare or otherwise-qualified `key`: `key`, `sortKey`, `issue_key`.
    SomeKey,
}

fn name_strength(name: &str) -> Option<NameStrength> {
    let words = name_words(name);
    let last = words.last()?;
    if SECRET_WORDS.iter().any(|word| last.ends_with(word)) {
        return Some(NameStrength::Credential);
    }
    match last.strip_suffix("key") {
        Some("") => {
            let qualified = words.len() >= 2
                && CREDENTIAL_KEY_QUALIFIERS.contains(&words[words.len() - 2].as_str());
            Some(if qualified {
                NameStrength::Credential
            } else {
                NameStrength::SomeKey
            })
        }
        Some(qualifier) if CREDENTIAL_KEY_QUALIFIERS.contains(&qualifier) => {
            Some(NameStrength::Credential)
        }
        _ => None,
    }
}

/// A value that stands for something else — an env reference, a template slot, an elision, a mask,
/// a marker — is not a secret.
fn is_placeholder(value: &str) -> bool {
    let Some(first) = value.chars().next() else {
        return true;
    };
    if matches!(first, '$' | '%' | '<' | '{' | '[' | '(' | '*' | '…') {
        return true;
    }
    if value.contains("...") || value.contains('…') {
        return true;
    }
    let mut chars = value.chars();
    let head = chars.next();
    value.chars().count() > 1 && chars.all(|c| Some(c) == head)
}

fn is_literal_word(value: &str) -> bool {
    value
        .chars()
        .all(|c| c.is_ascii_digit() || c == '.' || c == '_' || c == ',')
        || matches!(
            value.to_ascii_lowercase().as_str(),
            "true" | "false" | "yes" | "no" | "none" | "null" | "nil" | "on" | "off"
        )
}

fn has_letter_and_digit(value: &str) -> bool {
    value.chars().any(|c| c.is_ascii_alphabetic()) && value.chars().any(|c| c.is_ascii_digit())
}

/// Long, only token characters, and letters interleaved with digits the way generated secrets are.
fn random_shaped(value: &str) -> bool {
    if value.len() < RANDOM_MIN_CHARS
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'+' | b'/' | b'='))
        || (value.contains('/') && !value.contains('+') && value.split('/').count() > 2)
    {
        return false;
    }
    let classes: Vec<bool> = value
        .bytes()
        .filter(u8::is_ascii_alphanumeric)
        .map(|b| b.is_ascii_digit())
        .collect();
    let transitions = classes.windows(2).filter(|w| w[0] != w[1]).count();
    transitions >= RANDOM_MIN_TRANSITIONS
}

fn prefixed_credential(token: &str) -> Option<&'static str> {
    CREDENTIAL_PREFIXES.iter().copied().find(|prefix| {
        token.strip_prefix(prefix).is_some_and(|body| {
            body.len() >= PREFIXED_MIN_BODY_CHARS && body.bytes().any(|b| b.is_ascii_digit())
        })
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Separator {
    Equals,
    Colon,
}

/// After a name ending at `pos`: an optional closing quote, `=` or `:`, then the value's byte range
/// and whether it was quoted.
#[allow(clippy::string_slice)] // every offset is at an ASCII byte (a quote, a separator) or the end.
fn assigned_value(line: &str, pos: usize) -> Option<(usize, usize, Separator, bool)> {
    let bytes = line.as_bytes();
    let mut j = pos;
    if j < bytes.len() && matches!(bytes[j], b'"' | b'\'') {
        j += 1;
    }
    while j < bytes.len() && matches!(bytes[j], b' ' | b'\t') {
        j += 1;
    }
    let separator = match bytes.get(j)? {
        b'=' if bytes.get(j + 1) != Some(&b'=') && bytes.get(j + 1) != Some(&b'>') => {
            Separator::Equals
        }
        b':' if !matches!(bytes.get(j + 1), Some(b':' | b'/')) => Separator::Colon,
        _ => return None,
    };
    j += 1;
    while j < bytes.len() && matches!(bytes[j], b' ' | b'\t') {
        j += 1;
    }
    let quote = *bytes.get(j)?;
    if matches!(quote, b'"' | b'\'' | b'`') {
        let start = j + 1;
        let end = start + line[start..].find(quote as char)?;
        return Some((start, end, separator, true));
    }
    let start = j;
    let mut end = start;
    while end < bytes.len()
        && !bytes[end].is_ascii_whitespace()
        && !matches!(
            bytes[end],
            b'"' | b'\'' | b'`' | b',' | b';' | b')' | b']' | b'}' | b'>' | b'|' | b'&'
        )
    {
        end += 1;
    }
    while end > start && matches!(bytes[end - 1], b'.' | b':') {
        end -= 1;
    }
    (end > start).then_some((start, end, separator, false))
}

fn assignment_is_secret(
    strength: NameStrength,
    value: &str,
    separator: Separator,
    quoted: bool,
) -> bool {
    if value.is_empty()
        || is_placeholder(value)
        || is_literal_word(value)
        || value.chars().any(char::is_whitespace)
    {
        return false;
    }
    if prefixed_credential(value).is_some() || random_shaped(value) {
        return true;
    }
    match strength {
        NameStrength::SomeKey => false,
        NameStrength::Credential if quoted || separator == Separator::Equals => true,
        NameStrength::Credential => {
            value.len() >= COLON_VALUE_MIN_CHARS && has_letter_and_digit(value)
        }
    }
}

/// Every secret value in one line, as non-overlapping byte ranges in order.
/// Every offset is where a scan over ASCII bytes started or stopped — at an ASCII byte or the
/// line's end — so every slice lands on a char boundary.
#[allow(clippy::string_slice)]
fn find_in_line(line: &str) -> Vec<Found> {
    let bytes = line.as_bytes();
    let mut found: Vec<Found> = Vec::new();

    let mut i = 0;
    while i < bytes.len() {
        let starts_name = (bytes[i].is_ascii_alphabetic() || bytes[i] == b'_')
            && (i == 0 || !(bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_'));
        if !starts_name {
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() && is_name_char(bytes[i]) {
            i += 1;
        }
        let name = line[start..i].trim_end_matches(['.', '-']);
        let Some(strength) = name_strength(name) else {
            continue;
        };
        if let Some((value_start, value_end, separator, quoted)) = assigned_value(line, i) {
            if assignment_is_secret(strength, &line[value_start..value_end], separator, quoted) {
                found.push(Found {
                    start: value_start,
                    end: value_end,
                    key: name.to_string(),
                });
                i = value_end;
            }
        }
    }

    let overlaps = |found: &[Found], start: usize, end: usize| {
        found.iter().any(|f| start < f.end && f.start < end)
    };

    let mut j = 0;
    while j < bytes.len() {
        if !is_token_char(bytes[j]) || (j > 0 && bytes[j - 1].is_ascii_alphanumeric()) {
            j += 1;
            continue;
        }
        let start = j;
        while j < bytes.len() && is_token_char(bytes[j]) {
            j += 1;
        }
        let token = &line[start..j];
        if let Some(prefix) = prefixed_credential(token) {
            if !overlaps(&found, start, j) {
                found.push(Found {
                    start,
                    end: j,
                    key: format!("{prefix} key"),
                });
            }
        }
    }

    let words: Vec<(usize, &str)> = line
        .split(|c: char| c.is_ascii_whitespace())
        .scan(0usize, |offset, word| {
            let at = *offset;
            *offset += word.len() + 1;
            Some((at, word))
        })
        .filter(|(_, word)| !word.is_empty())
        .collect();
    let punctuation: &[char] = &[
        '"', '\'', '`', ',', ';', ':', '.', '(', ')', '[', ']', '<', '>',
    ];
    for (index, &(at, word)) in words.iter().enumerate() {
        let trimmed = word.trim_matches(punctuation);
        if trimmed.is_empty() || !random_shaped(trimmed) {
            continue;
        }
        let start = at + word.find(trimmed).unwrap_or(0);
        let end = start + trimmed.len();
        if overlaps(&found, start, end) {
            continue;
        }
        let context = words[index.saturating_sub(SECRET_CONTEXT_WINDOW)..index]
            .iter()
            .rev()
            .find_map(|(_, before)| {
                let before = before.trim_matches(punctuation);
                let last = name_words(before).pop()?;
                SECRET_CONTEXT_WORDS
                    .contains(&last.as_str())
                    .then(|| before.to_string())
            });
        if let Some(key) = context {
            found.push(Found { start, end, key });
        }
    }

    found.sort_by_key(|f| f.start);
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    const FAKE_OPENAI: &str = "sk-test-0000000000000000000000";
    const FAKE_CONTEXT7: &str = "ctx7sk-00000000-aaaa-1111-bbbb-222222222222";
    const FAKE_RANDOM: &str = "a1b2c3d4e5f6a7b8c9d0e1f2";

    fn redacted(text: &str) -> String {
        redact_secrets(text).text
    }

    #[test]
    fn an_env_assignment_on_a_runbook_line_is_redacted_under_its_name() {
        let line = format!(
            "`LMSTUDIO_HOST=http://localhost:1234 CONTEXT7_API_KEY={FAKE_CONTEXT7} goose swarm run`"
        );
        let out = redact_secrets(&line);
        assert!(!out.text.contains(FAKE_CONTEXT7), "{}", out.text);
        assert!(out
            .text
            .contains("CONTEXT7_API_KEY=<redacted: CONTEXT7_API_KEY> goose swarm run"));
        assert!(out.text.contains("LMSTUDIO_HOST=http://localhost:1234"));
        assert_eq!(out.keys, vec!["CONTEXT7_API_KEY"]);
    }

    #[test]
    fn every_assignment_syntax_is_redacted() {
        for (text, key) in [
            (format!("OPENAI_API_KEY={FAKE_OPENAI}"), "OPENAI_API_KEY"),
            ("DB_PASSWORD=hunter2".to_string(), "DB_PASSWORD"),
            ("db_password = 'hunter2'".to_string(), "db_password"),
            (format!("\"apiKey\": \"{FAKE_RANDOM}\""), "apiKey"),
            ("github_token: abc123def456".to_string(), "github_token"),
            ("--auth-token=zzz999yyy".to_string(), "auth-token"),
            (format!("client_secret: {FAKE_RANDOM}"), "client_secret"),
        ] {
            let out = redact_secrets(&text);
            assert_eq!(out.keys, vec![key.to_string()], "{text} -> {}", out.text);
            assert!(out.text.contains(&redaction_marker(key)), "{}", out.text);
        }
    }

    #[test]
    fn a_vendor_prefixed_key_is_redacted_wherever_it_stands() {
        for token in [
            FAKE_OPENAI,
            "sk-ant-api03-0000000000000000000000",
            "ghp_000000000000000000000000000000000000",
            "github_pat_00000000000000000000aaaa",
            "xoxb-0000000000-000000000000",
            "AKIA0000000000000000",
            "glpat-0000000000000000aaaa",
        ] {
            let text = format!("use {token} for the calls");
            let out = redacted(&text);
            assert!(!out.contains(token), "{out}");
            assert!(out.starts_with("use <redacted: "), "{out}");
        }
    }

    #[test]
    fn a_random_token_after_a_secret_word_is_redacted() {
        let out = redact_secrets(&format!("The API key is {FAKE_RANDOM}."));
        assert_eq!(out.text, "The API key is <redacted: key>.");
        let out = redact_secrets(&format!("Authorization: Bearer {FAKE_RANDOM}"));
        assert!(!out.text.contains(FAKE_RANDOM), "{}", out.text);
    }

    #[test]
    fn prose_about_keys_and_tokens_is_left_alone() {
        for text in [
            "the primary key of the table is the issue id",
            "Key: always ask the client before a production change.",
            "max_tokens: 4096 and \"max_tokens\": 32768",
            "a token budget of 128k tokens per turn",
            "\"key\": \"PROJ-1234\" is the Jira issue key",
            "sortKey: createdAt",
            "The session token idles ~30 days.",
            "API_KEY=$CONTEXT7_API_KEY, TOKEN={API_KEY}, password: <ask Irvin>",
            "KEY_PATH=~/.ssh/id_ed25519 and GOOSE_MAX_TOKENS=8000",
            "improve-toolcall-reliability-2026 is the key note",
            "commit 4901b724c is the key change",
            "sk-learn is a library",
            "https://example.com/token/abc",
            "enable_token_refresh=true",
        ] {
            let out = redact_secrets(text);
            assert_eq!(out.text, text, "changed: {text}");
            assert!(out.keys.is_empty());
        }
    }

    #[test]
    fn redaction_is_idempotent() {
        let once = redacted(&format!(
            "CONTEXT7_API_KEY={FAKE_CONTEXT7} and {FAKE_OPENAI}"
        ));
        assert_eq!(redacted(&once), once);
        assert!(redact_secrets(&once).keys.is_empty());
    }

    #[test]
    fn hits_name_the_line_and_key_and_never_the_value() {
        let text = format!("headline\nrun with DB_PASSWORD=hunter2\n\nuse {FAKE_OPENAI}");
        let hits = secret_hits(&text);
        assert_eq!(
            hits,
            vec![
                SecretHit {
                    line: 2,
                    key: "DB_PASSWORD".to_string()
                },
                SecretHit {
                    line: 4,
                    key: "sk- key".to_string()
                },
            ]
        );
        let shown = format!("{hits:?}");
        assert!(!shown.contains("hunter2") && !shown.contains(FAKE_OPENAI));
    }

    #[test]
    fn non_ascii_text_around_a_secret_is_kept_intact() {
        let text = format!("Schlüssel — API_KEY={FAKE_OPENAI} — “fertig”");
        let out = redacted(&text);
        assert_eq!(out, "Schlüssel — API_KEY=<redacted: API_KEY> — “fertig”");
    }
}
