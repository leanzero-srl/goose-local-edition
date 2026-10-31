//! WR-19: three claims a small local model makes about its own tool results, held against those
//! results. Each one is a checked fact about the transcript, said under the reply by the reply check
//! (`claim_check`), never a judgement of the model:
//!
//! - A number the answer pairs with a name the tool output tallies, where no output pairs that name
//!   with that number. Gabee RU-4 turn 7 (session 20261031_5): the calls returned only per-station
//!   totals (`65 KRK-008`) and global codes (`64 GSM_DROP`); the answer gave "KRK-008 … GSM_DROP
//!   (29)". The truth was 64. The existing count check could not see it: "29" is somewhere in the
//!   session (`KRK-029`), and a bare-number set says nothing about which name a number belongs to.
//! - A claim that the text follows a named date format, after this turn's own edits turned dates
//!   that already had that form into something else. RU-4 turn 25: the report already had
//!   `23.10.2026`; the model ran `sed 's/23\.10\.2026/23.10/g'` and others, then said "All dates in
//!   `october-ops.md` now follow the DD.MM.YYYY format".
//! - "Here are all N lines" over a reply that shows far fewer than N. RU-4 turn 3: "Here are all
//!   **339 ERROR lines**", then a per-station tally.
//!
//! A correct answer stays silent: a sum or difference of the name's own numbers, a share whose two
//! numbers the output holds, a value this turn's calls wrote or ran, a value the reply proposes, code
//! (fenced, indented or inline), a reformat that names the old format, and a number a compaction
//! summary carries (round 2: the reviewer's correct-answer probes, rebuilt as tests).
//!
//! Tunable: `GOOSE_GROUNDING_CHECK` (default off until a review shows 0 false alarms) turns the
//! three findings on; `GOOSE_GROUNDING_REASK` (default off) hands a finding back to the model once per turn so it can check and correct itself.

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use regex::Regex;
use rmcp::model::Role;

use crate::conversation::message::{Message, MessageContent};

pub const ENABLED_KEY: &str = "GOOSE_GROUNDING_CHECK";
pub const REASK_KEY: &str = "GOOSE_GROUNDING_REASK";

/// Off until a correct-answer review shows 0 false alarms (WR-19 round 3).
pub const ENABLED_DEFAULT: bool = false;
pub const REASK_DEFAULT: bool = false;

fn flag(config: &crate::config::Config, key: &str, default: bool) -> bool {
    config.get_param::<bool>(key).unwrap_or(default)
}

pub(crate) fn enabled_in(config: &crate::config::Config) -> bool {
    flag(config, ENABLED_KEY, ENABLED_DEFAULT)
}

pub(crate) fn reask_enabled_in(config: &crate::config::Config) -> bool {
    flag(config, REASK_KEY, REASK_DEFAULT)
}

pub fn enabled() -> bool {
    enabled_in(crate::config::Config::global())
}

pub fn reask_enabled() -> bool {
    reask_enabled_in(crate::config::Config::global())
}

/// The words goose hands back to the model when `GOOSE_GROUNDING_REASK` is on: the finding, and
/// what to do with it. Once per turn (`should_reask`).
pub fn reask_text(findings: &[String]) -> String {
    format!(
        "goose checked your last answer against this conversation's tool output: {} Look at the tool \
         output again (run a command if you need to), then correct the answer. If the answer is right, \
         say which output shows it. Do not change any file just to make this note go away.",
        findings.join(" ")
    )
}

/// Hand a finding back only once per turn: a model that answers the note again with the same
/// claim gets the note on screen, not a loop.
pub fn should_reask(findings: &[String], already_reasked: bool) -> bool {
    !findings.is_empty() && !already_reasked
}

fn text_parts(message: &Message) -> String {
    message
        .content
        .iter()
        .filter_map(|c| match c {
            MessageContent::Text(t) => Some(t.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The text a tool result part carries: plain text, or a text resource the tool embedded (a doc
/// reader returns `Content::embedded_text`; round 3, N2).
fn content_text(c: &rmcp::model::Content) -> Option<&str> {
    if let Some(t) = c.as_text() {
        return Some(t.text.as_str());
    }
    match c.as_resource().map(|r| &r.resource) {
        Some(rmcp::model::ResourceContents::TextResourceContents { text, .. }) => Some(text.as_str()),
        _ => None,
    }
}

fn tool_output_texts(message: &Message) -> Vec<String> {
    message
        .content
        .iter()
        .filter_map(|c| match c {
            MessageContent::ToolResponse(r) => Some(match &r.tool_result {
                Ok(result) => result
                    .content
                    .iter()
                    .filter_map(content_text)
                    .collect::<Vec<_>>()
                    .join("\n"),
                Err(e) => e.message.to_string(),
            }),
            _ => None,
        })
        .collect()
}

/// A call of this turn, with its argument object and what came back.
struct Call {
    arguments: serde_json::Map<String, serde_json::Value>,
    output: String,
    failed: bool,
}

fn calls(messages: &[Message]) -> Vec<Call> {
    let mut responses: HashMap<&str, &rmcp::model::ErrorData> = HashMap::new();
    let mut results: HashMap<&str, &rmcp::model::CallToolResult> = HashMap::new();
    for message in messages {
        for content in &message.content {
            if let MessageContent::ToolResponse(r) = content {
                match &r.tool_result {
                    Ok(result) => {
                        results.insert(r.id.as_str(), result);
                    }
                    Err(e) => {
                        responses.insert(r.id.as_str(), e);
                    }
                }
            }
        }
    }
    let mut out = Vec::new();
    for message in messages {
        for content in &message.content {
            let MessageContent::ToolRequest(request) = content else {
                continue;
            };
            let Ok(call) = &request.tool_call else {
                continue;
            };
            let (output, failed) = if let Some(result) = results.get(request.id.as_str()) {
                (
                    result
                        .content
                        .iter()
                        .filter_map(content_text)
                        .collect::<Vec<_>>()
                        .join("\n"),
                    result.is_error == Some(true),
                )
            } else if let Some(e) = responses.get(request.id.as_str()) {
                (e.message.to_string(), true)
            } else {
                continue;
            };
            out.push(Call {
                arguments: call.arguments.clone().unwrap_or_default(),
                output,
                failed,
            });
        }
    }
    out
}

/// What a call wrote or ran, as lines a number can be read from: the edit's new text, the file's
/// content, the shell command. "I changed MAX_RETRIES = 5" is grounded in the edit that wrote it.
fn argument_text(call: &Call) -> String {
    fn walk(key: &str, value: &serde_json::Value, out: &mut Vec<String>) {
        match value {
            serde_json::Value::String(s) => out.push(s.clone()),
            serde_json::Value::Number(n) => out.push(format!("{key}: {n}")),
            serde_json::Value::Array(items) => items.iter().for_each(|v| walk(key, v, out)),
            serde_json::Value::Object(map) => map.iter().for_each(|(k, v)| walk(k, v, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    for (k, v) in &call.arguments {
        walk(k, v, &mut out);
    }
    out.join("\n")
}

/// Markdown indented code: four spaces or a tab, and not a nested list item or a table row.
fn indented_code(line: &str) -> bool {
    let indented = line.starts_with("    ") || line.starts_with('\t');
    let t = line.trim_start();
    let list_item = t.starts_with(['-', '*', '+', '|'])
        || t.split_once(['.', ')'])
            .is_some_and(|(n, _)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
    indented && !list_item
}

/// What the reply says to the reader: fenced and indented code is quoted output or code, not a
/// claim.
fn prose_lines(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut fenced = false;
    for line in text.lines() {
        let t = line.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            fenced = !fenced;
            continue;
        }
        if !fenced && !t.starts_with('>') && !indented_code(line) {
            out.push(line);
        }
    }
    out
}

// ---- 1. A number paired with a name no output pairs it with -----------------------------------

/// A name a tool output can tally: a code or id with `_` or `-` joining its parts, holding a digit
/// or an underscore, or written in capitals — `GSM_DROP`, `KRK-008`, `account_delete`, `FW-CHECK`.
/// Hyphenated words ("follow-up", "built-in") are not names.
static LABEL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[A-Za-z][A-Za-z0-9]*(?:[_-][A-Za-z0-9]+)+").expect("static regex")
});

fn is_name(token: &str) -> bool {
    token.contains('_')
        || token.chars().any(|c| c.is_ascii_digit())
        || token
            .chars()
            .all(|c| c.is_ascii_uppercase() || c == '-' || c.is_ascii_digit())
}

/// Right after a name: the number it is given — "GSM_DROP (29)", "**FW_CHECKSUM:** 182",
/// "KRK-008 = 65", "LOCK_TIMEOUT — 93". A percent, a decimal, a ratio, a range or a time after the
/// number makes it something other than a count, and is left alone.
static BOUND_NUMBER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(r"^(?:\*\*|__|`)?(?:\s*:\s*|\s*=\s*|\s+[—–]\s+)(?:\*\*|__|`)?\s*{COUNT}(\.\d|\s*%|\s*/\s*\d|\s*[-–]\s*\d|:\d|[A-Za-z])?"))
        .expect("static regex")
});

/// A count as a reply writes it: "182", "12,288", and the Polish/French "12 288" with a space, a
/// no-break space or a narrow no-break space between the thousands (round 3, N6b/N6c: read as 12).
const COUNT: &str = r"(\d{1,3}(?:[ \x{a0}\x{202f}]\d{3})+\b|\d[\d,]*)";

/// Inline code is code: "`max_len=13`", "`MAX_LEN` = 80", "`_utc_time(34927)`" name arguments and
/// constants, not counts.
static CODE_SPAN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"`[^`]*`").expect("static regex"));

/// Right before a name, the number that counts it: "(29 GSM_DROP + 16 LOCK_TIMEOUT", "- 64 GSM_DROP".
static NUMBER_BEFORE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:^|[\s(+,|:]|\*\*)(\d[\d,]*)(?:\*\*)? $").expect("static regex")
});

/// "GSM_DROP (29)", "KRK-008 (65, 19%)": the parenthesis holds the number alone. "`to_minor` (12
/// tests)" counts something else, and "(409 → the existing id)" is a status, not a count.
static PAREN_NUMBER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(r"^(?:\*\*|__|`)?\s*\(\s*(?:\*\*)?{COUNT}(?:\*\*)?\s*[),;]"))
        .expect("static regex")
});

/// "3 of 5", "3 out of 5": a share, held against the two numbers it names.
static OF_TOTAL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:\*\*)?\s+(?:out\s+)?of\s+(?:\*\*)?(\d[\d,]*)\b").expect("static regex")
});
static SHARE_BEFORE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(\d[\d,]*)(?:\*\*)?\s+(?:out\s+)?of\s+(?:\*\*)?$").expect("static regex")
});

/// Words that make the value after them a proposal or a plan, not a report: "move it to API_PORT
/// = 9090", "you could set MAX_LEN = 80", "I'd use DB_PORT = 5433". Read only in the few words
/// right before the name, in the same sentence: round 2 skipped the whole line on any of these
/// words, and "heavily GSM_DROP (29) instead of LOCK_TIMEOUT" (the real F35 split) went silent.
/// "set" and "would" alone are not enough: "the set of KRK-008 errors", "which would be".
static PROPOSAL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:move|moving|switch|switching|bump|bumping|raise|lower|increase|decrease|try|suggest|recommend|propose|consider|let's|let\s+me|i'd|we'd|you'd|could|should|might|(?:to|i'll|we'll|please|can|would|will)\s+(?:set|use|change|make)|setting)\b|^\s*(?:set|use|change|make)\b")
        .expect("static regex")
});

/// "API_PORT = 9090 instead": the number is what the reply would use. "GSM_DROP (29) instead of
/// LOCK_TIMEOUT" contrasts two names and is a report.
static INSTEAD_AFTER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^[^.;!?]{0,16}?\binstead\b(\s+of\b)?").expect("static regex")
});

/// A rounded count: "180 or so", "roughly 180", "around", "approx.", "~", "≈", and the Polish,
/// German and French words for it. Held against the output within a tenth, not exactly.
static HEDGE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\bor\s+so\b|\broughly\b|\baround\b|\bapprox|~|≈|\babout\b|\bnearly\b|\balmost\b|\bcirca\b|\bca\.|-ish\b|\bish\b|\bsome\b|\bokoło\b|\bok\.|\bmniej\s+więcej\b|\bprawie\b|\bblisko\b|\bniemal|\betwa\b|\bungefähr\b|\brund\b|\bknapp\b|\bfast\b|\bcirka\b|\benviron\b|\bà\s+peu\s+près\b|\bpresque\b|\bquasi\b")
        .expect("static regex")
});

/// A unit after a number: "2 min", "120 s", "64 ms", "1.5 GB". The number is a measurement and is
/// compared with the output's measurements of that name after conversion (round 3, N5b: "2 min"
/// over "120 s" fired).
const UNIT: &str = r"(ns|µs|μs|us|ms|msec|milliseconds?|s|sec|secs|seconds?|sek|sekund[a-zęy]*|sekunden|min|mins|minutes?|minut[ay]?|minuten|h|hrs?|hours?|godz[a-z.]*|std|stunden?|d|days?|dni|tage?n?|b|bytes?|kb|kib|mb|mib|gb|gib|tb|tib)\b";
static UNIT_AFTER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"(?i)^\s*{UNIT}")).expect("static regex"));
static MEASURE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(r"(?i)(?:^|[^\d.,])(\d+(?:[.,]\d+)?)\s*{UNIT}")).expect("static regex")
});

/// The dimension of a unit and its size in that dimension's base (seconds, bytes).
fn unit_scale(unit: &str) -> Option<(u8, f64)> {
    let u = unit.to_lowercase();
    let time = |f: f64| Some((0u8, f));
    let size = |f: f64| Some((1u8, f));
    match u.as_str() {
        "ns" => time(1e-9),
        "µs" | "μs" | "us" => time(1e-6),
        "ms" | "msec" | "millisecond" | "milliseconds" => time(1e-3),
        "s" | "sec" | "secs" | "second" | "seconds" | "sek" | "sekunden" => time(1.0),
        "min" | "mins" | "minute" | "minutes" | "minuta" | "minut" | "minuty" | "minuten" => {
            time(60.0)
        }
        "h" | "hr" | "hrs" | "hour" | "hours" | "std" | "stunde" | "stunden" => time(3600.0),
        "d" | "day" | "days" | "dni" | "tag" | "tage" | "tagen" => time(86400.0),
        "b" | "byte" | "bytes" => size(1.0),
        "kb" => size(1e3),
        "kib" => size(1024.0),
        "mb" => size(1e6),
        "mib" => size(1048576.0),
        "gb" => size(1e9),
        "gib" => size(1073741824.0),
        "tb" => size(1e12),
        "tib" => size(1099511627776.0),
        _ if u.starts_with("sekund") => time(1.0),
        _ if u.starts_with("godz") => time(3600.0),
        _ => None,
    }
}

/// "LOCK_TIMEOUT i GSM_DROP: 157", "KRK-008 minus KRK-042 = 6", "A + B = 9": the names joined
/// right before the bound one, whose counts the number is worked out from.
static JOINED_BEFORE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)([A-Za-z][A-Za-z0-9]*(?:[_-][A-Za-z0-9]+)+)(?:\*\*|__|`)?(?:\s+(?:and|i|oraz|und|et|y|e|plus|minus|vs\.?|versus)\s+|\s*[+&−]\s*|\s+[-–]\s+)(?:\*\*|__|`)?$")
        .expect("static regex")
});

fn joined_names(head: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut head = head;
    while let Some(c) = JOINED_BEFORE.captures(head) {
        let n = c.get(1).expect("group 1");
        if !is_name(n.as_str()) {
            break;
        }
        out.push(n.as_str().to_string());
        head = head.get(..n.start()).unwrap_or_default();
    }
    out
}

/// The few words of the sentence right before a name; "… " in front when the sentence is longer,
/// so an imperative ("Set X = 5") is read only at the sentence start.
fn words_before(head: &str) -> String {
    let cut = head
        .rfind([';', '!', '?', '\n'])
        .into_iter()
        .chain(head.match_indices(". ").map(|(k, _)| k).last())
        .max()
        .map_or(0, |k| k + 1);
    let words: Vec<&str> = head.get(cut..).unwrap_or_default().split_whitespace().collect();
    let keep = &words[words.len().saturating_sub(6)..];
    let lead = if keep.len() < words.len() { "… " } else { "" };
    format!("{lead}{}", keep.join(" "))
}

fn whole(number: &str) -> Option<u64> {
    number
        .chars()
        .filter(|c| !matches!(c, ',' | ' ' | '\u{a0}' | '\u{202f}'))
        .collect::<String>()
        .parse()
        .ok()
}

/// A number standing on its own in a line, with where it sits: "  64 GSM_DROP", "GSM_DROP: 64",
/// `"x":64`, "connected_clients:5", "count=64", "64,KRK-008". Pieces of a timestamp, a date, an
/// id, a decimal or a measurement ("02:47:09", "2026-10-01", "KRK-008", "1.5", "12ms") are not.
/// The one reader for both questions the check asks of an output line: does it count this name
/// (`beside`), and which numbers does it hold (`supported`).
fn number_tokens(line: &str) -> Vec<(usize, usize, u64)> {
    let b = line.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if !b[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let start = i;
        let mut end = i;
        while end < b.len()
            && (b[end].is_ascii_digit()
                || (b[end] == b','
                    && end + 1 < b.len()
                    && b[end + 1].is_ascii_digit()
                    && end > start))
        {
            end += 1;
        }
        i = end;
        let before_ok = match start.checked_sub(1).map(|k| b[k]) {
            None => true,
            Some(b':') => start < 2 || !b[start - 2].is_ascii_digit(),
            Some(c) => c.is_ascii_whitespace() || b",;|([{\"'=*`+#".contains(&c),
        };
        let after = b.get(end).copied();
        let next = b.get(end + 1).copied();
        let after_ok = match after {
            None => true,
            Some(b':' | b'.') => !next.is_some_and(|n| n.is_ascii_digit()),
            Some(c) => c.is_ascii_whitespace() || b",;|)]}\"'`*".contains(&c),
        };
        if before_ok && after_ok {
            if let Some(n) = whole(line.get(start..end).unwrap_or_default()) {
                out.push((start, end, n));
            }
        }
    }
    out
}

fn name_matches<'a>(line: &'a str, name: &'a str) -> impl Iterator<Item = regex::Match<'a>> + 'a {
    LABEL.find_iter(line).filter(move |m| m.as_str() == name)
}

fn holds_name(line: &str, name: &str) -> bool {
    line.contains(name) && name_matches(line, name).next().is_some()
}

/// The line counts the name: a number right beside it, as `uniq -c`, a `key: value` dump, compact
/// JSON, a CSV or a table prints it — "  64 GSM_DROP", "65 station=KRK-008", "GSM_DROP: 64",
/// `"GSM_DROP":64`, "connected_clients:5", "| GSM_DROP | 64 |".
fn beside(line: &str, name: &str) -> bool {
    !beside_numbers(line, name).is_empty()
}

/// The numbers right beside the name on the line: the ones that count IT, not the line's others
/// ("64 GSM_DROP, 93 LOCK_TIMEOUT" gives GSM_DROP 64 alone).
fn beside_numbers(line: &str, name: &str) -> Vec<u64> {
    const AFTER: &str = " \t:=,;|\"'`*";
    const BEFORE: &str = " \t:=,;|\"'";
    if !holds_name(line, name) {
        return Vec::new();
    }
    let tokens = number_tokens(line);
    let matches: Vec<regex::Match> = name_matches(line, name).collect();
    tokens
        .iter()
        .filter(|&&(start, end, _)| {
            matches.iter().any(|m| {
                if start >= m.end() {
                    // "64 GSM_DROP, 93 LOCK_TIMEOUT": a number that leads the next name counts that
                    // name, not this one.
                    let gap = line.get(m.end()..start).unwrap_or_default();
                    let next = line
                        .get(end..)
                        .unwrap_or_default()
                        .trim_start_matches(|c: char| BEFORE.contains(c));
                    let leads_a_name = LABEL
                        .find(next)
                        .is_some_and(|n| n.start() == 0 && is_name(n.as_str()));
                    !gap.is_empty() && gap.chars().all(|c| AFTER.contains(c)) && !leads_a_name
                } else if end <= m.start() {
                    let gap = line.get(end..m.start()).unwrap_or_default();
                    // "  40 code=GSM_DROP", "65 station=KRK-008": a `key=` between the count
                    // and the name is the field's key. Round 2 looked for the key's start from
                    // the '=' itself and never found one (P1b).
                    let gap = match gap.strip_suffix('=') {
                        Some(keyed) => {
                            let k = keyed
                                .rfind(|c: char| BEFORE.contains(c))
                                .map_or(0, |k| k + 1);
                            let key = keyed.get(k..).unwrap_or_default();
                            if !key.is_empty()
                                && key.chars().all(|c| c.is_ascii_alphabetic() || c == '_')
                            {
                                keyed.get(..k).unwrap_or_default()
                            } else {
                                gap
                            }
                        }
                        None => gap,
                    };
                    !gap.is_empty() && gap.chars().all(|c| BEFORE.contains(c))
                } else {
                    false
                }
            })
        })
        .map(|&(_, _, n)| n)
        .collect()
}

pub(crate) struct Binding {
    name: String,
    number: u64,
    /// "3 of 5": the total the share names.
    of: Option<u64>,
    co_names: Vec<String>,
    /// "180 or so": held within a tenth.
    approx: bool,
    /// "2 min": a measurement, with its dimension and size in that dimension's base.
    unit: Option<(u8, f64)>,
    /// "LOCK_TIMEOUT i GSM_DROP: 157": the names the number is a sum or difference over.
    joined: Vec<String>,
}

pub(crate) fn bindings(reply: &str) -> Vec<Binding> {
    let mut out = Vec::new();
    for line in prose_lines(reply) {
        let line = CODE_SPAN.replace_all(line, " ");
        let line = line.as_ref();
        let names: Vec<regex::Match> = LABEL
            .find_iter(line)
            .filter(|m| is_name(m.as_str()))
            .collect();
        for m in &names {
            let rest = line.get(m.end()..).unwrap_or_default();
            let head = line.get(..m.start()).unwrap_or_default();
            let mut after = "";
            let (number, of) = match PAREN_NUMBER.captures(rest) {
                Some(c) => {
                    let n = c.get(1).expect("group 1");
                    after = rest.get(n.end()..).unwrap_or_default();
                    (whole(n.as_str()), None)
                }
                None => match BOUND_NUMBER.captures(rest).filter(|c| c.get(2).is_none()) {
                    Some(c) => {
                        let n = c.get(1).expect("group 1");
                        after = rest.get(n.end()..).unwrap_or_default();
                        let of = OF_TOTAL
                            .captures(after)
                            .and_then(|o| whole(&o[1]));
                        (whole(n.as_str()), of)
                    }
                    // "3 of 5 KRK-008 docks": the number before the name is the total.
                    None => match NUMBER_BEFORE.captures(head).and_then(|c| c.get(1)) {
                        Some(n) => match SHARE_BEFORE
                            .captures(head.get(..n.start()).unwrap_or_default())
                            .and_then(|s| whole(&s[1]))
                        {
                            Some(k) => (Some(k), whole(n.as_str())),
                            None => (whole(n.as_str()), None),
                        },
                        None => (None, None),
                    },
                },
            };
            let Some(number) = number else {
                continue;
            };
            let before = words_before(head);
            let proposed = PROPOSAL.is_match(&before)
                || INSTEAD_AFTER
                    .captures(after)
                    .is_some_and(|c| c.get(1).is_none());
            if proposed {
                continue;
            }
            let near_after: String = after.chars().take(24).collect();
            let near_before: String = {
                let w: Vec<&str> = before.split_whitespace().collect();
                w[w.len().saturating_sub(3)..].join(" ")
            };
            let approx = HEDGE.is_match(&near_after) || HEDGE.is_match(&near_before);
            let unit = UNIT_AFTER
                .captures(after)
                .and_then(|c| unit_scale(&c[1]));
            out.push(Binding {
                approx,
                unit,
                joined: joined_names(head),
                name: m.as_str().to_string(),
                number,
                of,
                co_names: names
                    .iter()
                    .map(|o| o.as_str().to_string())
                    .filter(|o| o != m.as_str())
                    .collect(),
            });
        }
    }
    out
}

/// A sum or a difference the reply did over the numbers the outputs give a name: "FW_CHECK: 182"
/// over "50 FW_CHECK" and "132 FW_CHECK", "KRK-042 = 6" over "59" and "53". Two or three terms
/// of distinct values; the pool is only the numbers right beside the name, so a sum over unrelated
/// counts (29 = 38 − 5 − 4 over a station table, 29 = 93 − 64 over "64 GSM_DROP, 93
/// LOCK_TIMEOUT") cannot ground it.
fn arithmetic(target: u64, pool: &[u64]) -> bool {
    // ratio: past this many distinct values a three-term search stops meaning anything (64³/6
    // combinations, and nearly every small number is reachable), so only pairs are tried.
    const TRIPLE_POOL: usize = 64;
    let t = target as i128;
    let p: Vec<i128> = pool.iter().map(|&v| v as i128).collect();
    for i in 0..p.len() {
        for j in i + 1..p.len() {
            if p[i] + p[j] == t || (p[i] - p[j]).abs() == t {
                return true;
            }
            if p.len() > TRIPLE_POOL {
                continue;
            }
            for k in j + 1..p.len() {
                let (a, b, c) = (p[i], p[j], p[k]);
                if [
                    a + b + c,
                    a + b - c,
                    a - b + c,
                    b + c - a,
                    a - b - c,
                    b - a - c,
                    c - a - b,
                ]
                .contains(&t)
                {
                    return true;
                }
            }
        }
    }
    false
}

/// One session line, read once: the names it holds that a binding asks about, and its numbers. A
/// long session is tokenized once per reply, not once per binding.
struct Line<'a> {
    names: Vec<&'a str>,
    numbers: Vec<u64>,
}

fn index<'a>(text: &'a str, wanted: &HashSet<&str>) -> Vec<Line<'a>> {
    text.lines()
        .map(|l| {
            let mut names: Vec<&str> = LABEL
                .find_iter(l)
                .map(|m| m.as_str())
                .filter(|n| wanted.contains(n))
                .collect();
            names.dedup();
            Line {
                names,
                numbers: number_tokens(l).into_iter().map(|(_, _, n)| n).collect(),
            }
        })
        .collect()
}

/// What the session says about one name: the numbers on its lines, how many lines hold it, sums
/// and differences of those numbers, and for a share ("3 of 5") the two numbers anywhere in it.
fn supported(
    binding: &Binding,
    sources: &[Vec<Line>],
    raw: &[String],
    all_numbers: &HashSet<u64>,
    this_turn: &[&str],
) -> bool {
    if let Some((dimension, scale)) = binding.unit {
        // "2 min" over "build_time: 120 s": the output's measurements of the name, converted.
        let value = binding.number as f64 * scale;
        let mut measured = false;
        for line in raw.iter().flat_map(|t| t.lines()).filter(|l| holds_name(l, &binding.name)) {
            for c in MEASURE.captures_iter(line) {
                let Some((d, f)) = unit_scale(&c[2]) else {
                    continue;
                };
                let Ok(v) = c[1].replace(',', ".").parse::<f64>() else {
                    continue;
                };
                if d != dimension {
                    continue;
                }
                measured = true;
                let v = v * f;
                // rounding: "2 min" for 125 s, "64 ms" for 0.0641 s
                if (v - value).abs() <= value.max(v) * 0.05 {
                    return true;
                }
            }
        }
        // A measurement held against counts alone says nothing; this check is about counts.
        return !measured;
    }
    if let Some(total) = binding.of {
        if binding.number <= total
            && all_numbers.contains(&binding.number)
            && all_numbers.contains(&total)
        {
            return true;
        }
    }
    let mut numbers: HashSet<u64> = HashSet::new();
    let mut pool: Vec<u64> = Vec::new();
    for source in sources {
        let lines: Vec<&Line> = source
            .iter()
            .filter(|l| l.names.contains(&binding.name.as_str()))
            .collect();
        numbers.insert(lines.len() as u64);
        for line in &lines {
            numbers.extend(line.numbers.iter().copied());
        }
        // "KRK-008 … GSM_DROP (29)": lines holding both names, counted.
        for co in &binding.co_names {
            numbers.insert(
                lines
                    .iter()
                    .filter(|l| l.names.contains(&co.as_str()))
                    .count() as u64,
            );
        }
    }
    if numbers.contains(&binding.number) {
        return true;
    }
    let near = |v: u64| {
        let tolerance = (binding.number / 10).max(1);
        v.abs_diff(binding.number) <= tolerance
    };
    if binding.approx && numbers.iter().any(|&v| near(v)) {
        return true;
    }
    // A sum is worked out over what the model just read: this turn's outputs, not every count the
    // session ever printed beside the name (RU-4 msg 219's "GSM_DROP (29)" is reachable from
    // enough of those).
    // "LOCK_TIMEOUT i GSM_DROP: 157", "KRK-008 minus KRK-042 = 6": the joined names' counts too.
    // Only names the reply joins to this one: "heavily GSM_DROP (29) instead of LOCK_TIMEOUT"
    // would be 93 - 64 otherwise.
    let mut total: u64 = 0;
    let mut terms = 0usize;
    for name in std::iter::once(&binding.name).chain(binding.joined.iter()) {
        for line in this_turn.iter().flat_map(|o| o.lines()) {
            for n in beside_numbers(line, name) {
                total = total.saturating_add(n);
                terms += 1;
                if !pool.contains(&n) {
                    pool.push(n);
                }
            }
        }
    }
    // "FW_CHECK: 132 in total" over four outputs (12, 30, 40, 50): the sum of every count this
    // turn gives the name, however many.
    if terms >= 2 && (total == binding.number || (binding.approx && near(total))) {
        return true;
    }
    if binding.approx {
        return reachable(&pool).into_iter().any(near);
    }
    arithmetic(binding.number, &pool)
}

/// The sums and differences of two pool values, for a rounded number ("180 or so").
fn reachable(pool: &[u64]) -> Vec<u64> {
    let mut out = Vec::new();
    for i in 0..pool.len() {
        for j in i + 1..pool.len() {
            out.push(pool[i].saturating_add(pool[j]));
            out.push(pool[i].abs_diff(pool[j]));
        }
    }
    out
}

/// What this turn's output says about the name, for the note: the line that counts it, or how
/// many records hold it.
fn tally_line(name: &str, outputs: &[&str]) -> String {
    if let Some(l) = outputs
        .iter()
        .flat_map(|o| o.lines())
        .find(|l| beside(l, name))
    {
        let t = l.trim();
        return if t.chars().count() > 80 {
            format!(
                "the output says “{}…”",
                t.chars().take(80).collect::<String>()
            )
        } else {
            format!("the output says “{t}”")
        };
    }
    let records = outputs
        .iter()
        .flat_map(|o| o.lines())
        .filter(|l| holds_name(l, name))
        .count();
    format!("{records} output lines hold it")
}

/// How this turn's output counts a name: a number right beside it (`beside`), or two or more
/// records holding it as a field value ("code=GSM_DROP", "a,KRK-008,b") for the reply to have
/// counted. A name in code beside other words ("max_len: int = 60", "f(text, max_len)") is
/// neither.
fn tallied_in(outputs: &[&str], name: &str) -> bool {
    let n = regex::escape(name);
    let Ok(field) = Regex::new(&format!(r#"(?:[=,\t]["']?|\|\s*){n}\b"#)) else {
        return false;
    };
    outputs.iter().any(|o| {
        o.lines().any(|l| beside(l, name))
            || o.lines()
                .filter(|l| l.contains(name) && field.is_match(l))
                .nth(1)
                .is_some()
    })
}

/// The texts a number may come from, gathered only when a reply binds a number to a name this
/// turn's output tallies, and cut to the lines that hold one of those names: the check costs
/// nothing on a reply that pairs no names, and a long session is read once per such reply, by a
/// substring test before any regex.
struct Sources<'a> {
    messages: &'a [Message],
    from: usize,
    turn_calls: &'a [Call],
}

impl Sources<'_> {
    fn gather(&self, names: &[&str], shares: bool) -> (Vec<String>, HashSet<u64>) {
        let mut texts: Vec<String> = Vec::new();
        let mut push = |t: String| texts.push(t);
        for (i, m) in self.messages.iter().enumerate() {
            if i < self.from && !m.is_agent_visible() {
                continue;
            }
            for t in tool_output_texts(m) {
                push(t);
            }
            if i >= self.from {
                continue;
            }
            // The user's own words support a number; the model's earlier replies do not — round
            // 4's "GSM_DROP (29)" at turn 7 repeated its own turn-3 invention ("29 GSM_DROP"). A
            // compaction summary (agent-only, not a nudge) stands in for the outputs it replaced;
            // its numbers the conversation never showed are marked "(unverified)" and dropped.
            if m.role == Role::User && m.is_user_visible() {
                push(text_parts(m));
            } else if !m.is_user_visible() && !m.metadata.harness_nudge {
                push(
                    text_parts(m)
                        .lines()
                        .filter(|l| !l.contains("(unverified)"))
                        .collect::<Vec<_>>()
                        .join("\n"),
                );
            }
        }
        for call in self.turn_calls {
            push(argument_text(call));
        }
        let mut kept: Vec<String> = Vec::new();
        let mut all_numbers: HashSet<u64> = HashSet::new();
        for t in texts {
            let lines: Vec<&str> = t
                .lines()
                .filter(|l| names.iter().any(|n| l.contains(n)))
                .collect();
            if shares {
                for l in t.lines() {
                    all_numbers.extend(number_tokens(l).into_iter().map(|(_, _, n)| n));
                }
            }
            kept.push(lines.join("\n"));
        }
        (kept, all_numbers)
    }
}

fn unpaired_numbers(texts: &str, this_turn: &[&str], sources: &Sources) -> Vec<String> {
    let mut tallied: HashMap<String, bool> = HashMap::new();
    let mut checked: HashSet<(String, u64, Option<u64>, Vec<String>)> = HashSet::new();
    let mut candidates: Vec<Binding> = Vec::new();
    for binding in bindings(texts) {
        if !checked.insert((
            binding.name.clone(),
            binding.number,
            binding.of,
            binding.co_names.clone(),
        )) {
            continue;
        }
        if *tallied
            .entry(binding.name.clone())
            .or_insert_with(|| tallied_in(this_turn, &binding.name))
        {
            candidates.push(binding);
        }
    }
    if candidates.is_empty() {
        return Vec::new();
    }
    let mut names: Vec<&str> = Vec::new();
    for b in &candidates {
        names.push(&b.name);
        names.extend(b.co_names.iter().map(String::as_str));
        names.extend(b.joined.iter().map(String::as_str));
    }
    let shares = candidates.iter().any(|b| b.of.is_some());
    let (kept, all_numbers) = sources.gather(&names, shares);
    let wanted: HashSet<&str> = names.iter().copied().collect();
    let session: Vec<Vec<Line>> = kept.iter().map(|t| index(t, &wanted)).collect();
    let mut found: Vec<(String, u64)> = Vec::new();
    for binding in candidates {
        if supported(&binding, &session, &kept, &all_numbers, this_turn)
            || found
                .iter()
                .any(|(n, v)| *n == binding.name && *v == binding.number)
        {
            continue;
        }
        found.push((binding.name, binding.number));
    }
    if found.is_empty() {
        return Vec::new();
    }
    let shown: Vec<String> = found
        .iter()
        .take(3)
        .map(|(name, number)| format!("{name} as {number} ({})", tally_line(name, this_turn)))
        .collect();
    let more = match found.len() {
        n if n > 3 => format!(", and {} more", n - 3),
        _ => String::new(),
    };
    vec![format!(
        "the answer gives {}{more}, but no tool output pairs {} with that number.",
        shown.join(", "),
        if found.len() == 1 {
            "the name"
        } else {
            "those names"
        }
    )]
}

// ---- 2. A date format claimed after edits that took it away -----------------------------------

/// A date format the reply names: "DD.MM.YYYY", "YYYY-MM-DD", "dd/mm/yy".
static DATE_FORMAT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(d{1,2}|m{1,2}|y{2}|y{4})([./-])(d{1,2}|m{1,2}|y{2}|y{4})([./-])(d{1,2}|m{1,2}|y{2}|y{4})\b")
        .expect("static regex")
});

/// The end of a sentence, or a question's: "DD.MM.YYYY" itself has no space after its dots.
static SENTENCE_END: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[.!?](?:\*\*|\*|_)?(?:\s|$)|\n").expect("static regex"));

/// A sentence that says the text IS in the format: "All dates now follow", "used throughout".
static COMPLIANCE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:now|all|every|everywhere|throughout|consistent(?:ly)?|compl(?:y|ies|iant)|fully)\b")
        .expect("static regex")
});

/// A sentence that states the rule or a plan rather than the result: "should always be
/// DD.MM.YYYY", "you told me", "let me fix".
static RULE_OR_PLAN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:should|must|want(?:ed)?|prefer|told|asked|remember|rule|needs?|let\s+me|i'll|will|not)\b|\?\s*$")
        .expect("static regex")
});

static DATE_RUN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\d+(?:[./-]\d+)*").expect("static regex"));

fn part(p: &str) -> &'static str {
    match p.to_ascii_lowercase().as_str() {
        "d" | "m" => r"\d{1,2}",
        "yyyy" => r"\d{4}",
        _ => r"\d{2}",
    }
}

fn format_regex(c: &regex::Captures) -> Option<Regex> {
    let pattern = format!(
        "^{}{}{}{}{}$",
        part(&c[1]),
        regex::escape(&c[2]),
        part(&c[3]),
        regex::escape(&c[4]),
        part(&c[5])
    );
    Regex::new(&pattern).ok()
}

fn dates_in(text: &str, format: &Regex) -> Vec<String> {
    DATE_RUN
        .find_iter(text)
        .map(|m| m.as_str())
        .filter(|d| format.is_match(d))
        .map(str::to_string)
        .collect()
}

static SED_SUBST: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?:^|[\s'"])s(?:/((?:\\.|[^\\/])*)/((?:\\.|[^\\/])*)/|\|((?:\\.|[^\\|])*)\|((?:\\.|[^\\|])*)\||#((?:\\.|[^\\#])*)#((?:\\.|[^\\#])*)#)"#)
        .expect("static regex")
});

/// A sed/perl pattern as the literal text it replaces, when it is one: `23\.10\.2026` →
/// `23.10.2026`. A pattern with a class, a group or a repeat is not a literal and is skipped.
fn literal(pattern: &str) -> Option<String> {
    let mut out = String::new();
    let mut chars = pattern.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                let next = chars.next()?;
                if next.is_ascii_alphanumeric() {
                    return None;
                }
                out.push(next);
            }
            '.' | '[' | ']' | '*' | '+' | '?' | '(' | ')' | '{' | '}' | '^' | '$' | '|' => {
                return None
            }
            _ => out.push(c),
        }
    }
    Some(out)
}

/// A sed replacement as the text it writes: only `&` and `\1` are special there.
fn replacement(text: &str) -> Option<String> {
    let mut out = String::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                let next = chars.next()?;
                if next.is_ascii_alphanumeric() {
                    return None;
                }
                out.push(next);
            }
            '&' => return None,
            _ => out.push(c),
        }
    }
    Some(out)
}

fn edits(calls: &[Call]) -> Vec<(String, String)> {
    const PAIRS: &[(&str, &str)] = &[
        ("before", "after"),
        ("old_str", "new_str"),
        ("old_string", "new_string"),
        ("old_text", "new_text"),
        ("search", "replace"),
    ];
    let mut out = Vec::new();
    for call in calls {
        let output = call.output.trim_start();
        if call.failed || output.starts_with("No match") || output.starts_with("No change") {
            continue;
        }
        for (old, new) in PAIRS {
            if let (Some(a), Some(b)) = (
                call.arguments.get(*old).and_then(|v| v.as_str()),
                call.arguments.get(*new).and_then(|v| v.as_str()),
            ) {
                out.push((a.to_string(), b.to_string()));
            }
        }
        for value in call.arguments.values() {
            let Some(command) = value.as_str() else {
                continue;
            };
            if !(command.contains("sed ") || command.contains("perl ")) {
                continue;
            }
            for c in SED_SUBST.captures_iter(command) {
                let pair = [(1, 2), (3, 4), (5, 6)]
                    .into_iter()
                    .find_map(|(a, b)| Some((c.get(a)?.as_str(), c.get(b)?.as_str())));
                if let Some((a, b)) = pair.and_then(|(a, b)| Some((literal(a)?, replacement(b)?))) {
                    out.push((a, b));
                }
            }
        }
    }
    out
}

/// Before a format the sentence converts AWAY from: "instead of DD.MM.YYYY", "from DD.MM.YYYY".
static SOURCE_BEFORE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:from|instead\s+of|rather\s+than|replac\w*|than|not|old)\s+(?:the\s+)?(?:old\s+)?(?:\*\*|__|`)?$")
        .expect("static regex")
});

/// After a format the sentence converts away from: "every DD.MM.YYYY date to YYYY-MM-DD".
static SOURCE_AFTER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(?:\*\*|__|`)?\s+(?:dates?|values?|entries|ones)\s+(?:to|into)\b")
        .expect("static regex")
});

/// Before the format a sentence converts TO: "to YYYY-MM-DD", "into ISO YYYY-MM-DD".
static TARGET_BEFORE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:to|into)\s+(?:the\s+)?(?:\w+\s+)?(?:\*\*|__|`)?$").expect("static regex")
});

/// The formats a sentence claims the text is in. "now follow YYYY-MM-DD instead of DD.MM.YYYY" and
/// "converted every DD.MM.YYYY date to YYYY-MM-DD" name the old format too; only the one converted
/// TO is a claim about the text.
fn target_formats<'a>(line: &str, named: Vec<regex::Captures<'a>>) -> Vec<regex::Captures<'a>> {
    let span = |c: &regex::Captures| c.get(0).expect("whole match").range();
    let any_target = named
        .iter()
        .any(|c| TARGET_BEFORE.is_match(line.get(..span(c).start).unwrap_or_default()));
    named
        .into_iter()
        .filter(|c| {
            let r = span(c);
            let before = line.get(..r.start).unwrap_or_default();
            let after = line.get(r.end..).unwrap_or_default();
            let source = SOURCE_BEFORE.is_match(before) || SOURCE_AFTER.is_match(after);
            let target = TARGET_BEFORE.is_match(before);
            !source && (target || !any_target)
        })
        .collect()
}

fn format_taken_away(texts: &str, calls: &[Call]) -> Vec<String> {
    let edits = edits(calls);
    if edits.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let prose = prose_lines(texts).join("\n");
    for line in SENTENCE_END.split(&prose) {
        if !COMPLIANCE.is_match(line) || RULE_OR_PLAN.is_match(line) {
            continue;
        }
        let named: Vec<regex::Captures> = DATE_FORMAT.captures_iter(line).collect();
        for c in target_formats(line, named) {
            let name = c[0].to_string();
            if !seen.insert(name.to_uppercase()) {
                continue;
            }
            let Some(format) = format_regex(&c) else {
                continue;
            };
            let mut lost = 0usize;
            let mut example: Option<(String, String)> = None;
            for (before, after) in &edits {
                let had = dates_in(before, &format);
                let kept = dates_in(after, &format);
                if had.len() > kept.len() {
                    lost += had.len() - kept.len();
                    if example.is_none() {
                        let gone = had.iter().find(|d| !kept.contains(d)).cloned();
                        let replaced = gone.as_ref().map(|d| {
                            after
                                .lines()
                                .zip(before.lines())
                                .find(|(_, b)| b.contains(d.as_str()))
                                .map(|(a, _)| a.trim().to_string())
                                .unwrap_or_else(|| after.trim().to_string())
                        });
                        example = gone.zip(replaced);
                    }
                }
            }
            if lost == 0 {
                continue;
            }
            let shown = example
                .map(|(d, a)| {
                    let a: String = a.chars().take(60).collect();
                    format!(" (“{d}” became “{a}”)")
                })
                .unwrap_or_default();
            out.push(format!(
                "the answer says the text follows {name}, but this turn's edits turned {lost} date(s) that already had that form into something else{shown}."
            ));
        }
    }
    out
}

// ---- 3. "Here are all N" over a reply that shows far fewer ------------------------------------

static ALL_SHOWN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:here\s+(?:are|is)|here's|below\s+(?:are|is)|these\s+are|showing|listing|printing|displaying|i've\s+(?:listed|printed)|i\s+(?:listed|printed))\s+(?:all|every|the\s+(?:full|complete|entire))\s+(?:of\s+the\s+)?(?:\*\*|__)?(\d[\d,]*)\b(?:\s+(?:[\w-]+\s+)?(lines)\b)?")
        .expect("static regex")
});

fn shown_items(reply: &str) -> usize {
    reply
        .lines()
        .map(str::trim)
        .filter(|l| {
            !l.is_empty()
                && !l.starts_with('#')
                && !l.starts_with("```")
                && !l
                    .chars()
                    .all(|c| matches!(c, '-' | '|' | ':' | ' ' | '*' | '='))
        })
        .map(|l| 1 + l.matches(',').count())
        .sum()
}

fn bare(line: &str) -> &str {
    line.trim()
        .trim_matches(|c: char| matches!(c, '`' | '*' | '-' | ' '))
}

/// Reply lines that are output lines, as "all N lines" promises: a tally of them is not them. A
/// line is shown when it IS an output line, however short ("a=1", "line 7"), or when a longer one
/// (twelve characters and up, where a chance match inside another line stops being likely) sits
/// inside an output line the reply trimmed.
fn output_lines_shown(reply: &str, outputs: &[&str]) -> usize {
    let exact: HashSet<&str> = outputs
        .iter()
        .flat_map(|o| o.lines())
        .map(bare)
        .filter(|l| !l.is_empty())
        .collect();
    reply
        .lines()
        .map(bare)
        .filter(|l| !l.is_empty())
        .filter(|l| {
            exact.contains(l) || (l.chars().count() >= 12 && outputs.iter().any(|o| o.contains(l)))
        })
        .count()
}

fn all_claimed_part_shown(texts: &str, outputs: &[&str]) -> Vec<String> {
    let Some(c) = ALL_SHOWN.captures(texts) else {
        return Vec::new();
    };
    let Some(n) = whole(&c[1]) else {
        return Vec::new();
    };
    let shown = if c.get(2).is_some() {
        output_lines_shown(texts, outputs)
    } else {
        shown_items(texts)
    };
    // ratio: under half shown is "a tally of them, not them"; under twenty lines a reply that
    // shows a few is a short list, not a claim that hides the rest.
    if n < 20 || (shown as u64) * 2 >= n {
        return Vec::new();
    }
    let line = texts
        .get(c.get(0).map_or(0, |m| m.start())..)
        .unwrap_or_default()
        .lines()
        .next()
        .unwrap_or_default()
        .replace("**", "");
    let line: String = line.trim().chars().take(80).collect();
    vec![format!(
        "the answer says “{line}”, but it shows {shown} of them; the rest are only in the tool output."
    )]
}

// ---- Entry ------------------------------------------------------------------------------------

/// Check this model response (`messages[from..]`) of a turn that began at `start`.
pub fn check(messages: &[Message], start: usize, from: usize) -> Vec<String> {
    if !enabled() {
        return Vec::new();
    }
    check_unconditionally(messages, start, from)
}

pub(crate) fn check_unconditionally(
    messages: &[Message],
    start: usize,
    from: usize,
) -> Vec<String> {
    let from = from.min(messages.len());
    let start = start.min(from);
    let texts = messages[from..]
        .iter()
        .filter(|m| m.role == Role::Assistant && m.is_agent_visible())
        .map(text_parts)
        .filter(|t| !t.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    if texts.is_empty() {
        return Vec::new();
    }
    let turn_calls = calls(&messages[start..]);
    if turn_calls.is_empty() {
        return Vec::new();
    }
    let this_turn: Vec<&str> = turn_calls.iter().map(|c| c.output.as_str()).collect();
    let sources = Sources {
        messages,
        from,
        turn_calls: &turn_calls,
    };
    let mut findings = unpaired_numbers(&texts, &this_turn, &sources);
    findings.extend(format_taken_away(&texts, &turn_calls));
    findings.extend(all_claimed_part_shown(&texts, &this_turn));
    findings
}

#[cfg(test)]
#[path = "grounding_check_tests.rs"]
mod tests;
