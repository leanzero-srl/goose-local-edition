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
//! Tunable: `GOOSE_GROUNDING_CHECK` (default on) turns the three findings off; `GOOSE_GROUNDING_REASK`
//! (default off) hands a finding back to the model once per turn so it can check and correct itself.

use std::collections::HashSet;
use std::sync::LazyLock;

use regex::Regex;
use rmcp::model::Role;

use crate::conversation::message::{Message, MessageContent};

pub const ENABLED_KEY: &str = "GOOSE_GROUNDING_CHECK";
pub const REASK_KEY: &str = "GOOSE_GROUNDING_REASK";

fn flag(key: &str, default: bool) -> bool {
    crate::config::Config::global()
        .get_param::<bool>(key)
        .unwrap_or(default)
}

pub fn enabled() -> bool {
    flag(ENABLED_KEY, true)
}

pub fn reask_enabled() -> bool {
    flag(REASK_KEY, false)
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

fn tool_output_texts(message: &Message) -> Vec<String> {
    message
        .content
        .iter()
        .filter_map(|c| match c {
            MessageContent::ToolResponse(r) => Some(match &r.tool_result {
                Ok(result) => result
                    .content
                    .iter()
                    .filter_map(|c| c.as_text().map(|t| t.text.as_str()))
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
    let mut out = Vec::new();
    for message in messages {
        for content in &message.content {
            let MessageContent::ToolRequest(request) = content else {
                continue;
            };
            let Ok(call) = &request.tool_call else {
                continue;
            };
            let response = messages.iter().find_map(|m| {
                m.content.iter().find_map(|c| match c {
                    MessageContent::ToolResponse(r) if r.id == request.id => Some(r),
                    _ => None,
                })
            });
            let Some(response) = response else { continue };
            let (output, failed) = match &response.tool_result {
                Ok(result) => (
                    result
                        .content
                        .iter()
                        .filter_map(|c| c.as_text().map(|t| t.text.as_str()))
                        .collect::<Vec<_>>()
                        .join("\n"),
                    result.is_error == Some(true),
                ),
                Err(e) => (e.message.to_string(), true),
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

/// What the reply says to the reader: fenced code is quoted output or code, not a claim.
fn prose_lines(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut fenced = false;
    for line in text.lines() {
        let t = line.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            fenced = !fenced;
            continue;
        }
        if !fenced && !t.starts_with('>') {
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
    Regex::new(r"^(?:\*\*|__|`)?(?:\s*:\s*|\s*=\s*|\s+[—–]\s+)(?:\*\*|__|`)?\s*(\d[\d,]*)(\.\d|\s*%|\s*/\s*\d|\s*[-–]\s*\d|:\d|[A-Za-z])?")
        .expect("static regex")
});

static CODE_WITH_DIGIT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"`[^`]*\d[^`]*`").expect("static regex"));

/// Right before a name, the number that counts it: "(29 GSM_DROP + 16 LOCK_TIMEOUT", "- 64 GSM_DROP".
static NUMBER_BEFORE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:^|[\s(+,|:]|\*\*)(\d[\d,]*)(?:\*\*)? $").expect("static regex")
});

/// "GSM_DROP (29)", "KRK-008 (65, 19%)": the parenthesis holds the number alone. "`to_minor` (12
/// tests)" counts something else, and "(409 → the existing id)" is a status, not a count.
static PAREN_NUMBER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:\*\*|__|`)?\s*\(\s*(?:\*\*)?(\d[\d,]*)(?:\*\*)?\s*[),;]")
        .expect("static regex")
});

/// A number standing on its own in an output line: "  64 GSM_DROP", "GSM_DROP: 64", `"x": 64`,
/// "count=64", "64,KRK-008". Pieces of a timestamp or a date ("02:47:09", "2026-10-01") are not.
static STANDALONE_NUMBER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?:^|[\s,;|(\[{"'=]|:\s)(\d[\d,]*(?:\.\d+)?)(?:$|[\s,;|)\]}"'])"#)
        .expect("static regex")
});

fn whole(number: &str) -> Option<u64> {
    number.replace(',', "").parse().ok()
}

fn holds_name(line: &str, name: &str) -> bool {
    LABEL.find_iter(line).any(|m| m.as_str() == name)
}

fn standalone_numbers(line: &str) -> Vec<u64> {
    // Doubled spaces: two numbers that share one separator ("64 93") are both read.
    let mut out = Vec::new();
    let spaced = line.replace(' ', "  ");
    for c in STANDALONE_NUMBER.captures_iter(&spaced) {
        if let Some(n) = c.get(1).and_then(|m| whole(m.as_str())) {
            out.push(n);
        }
    }
    out
}

pub(crate) struct Binding {
    name: String,
    number: u64,
    co_names: Vec<String>,
}

pub(crate) fn bindings(reply: &str) -> Vec<Binding> {
    let mut out = Vec::new();
    for line in prose_lines(reply) {
        // "`max_len=13`", "`_utc_time(34927)`": code with its arguments, not a count.
        let line = CODE_WITH_DIGIT.replace_all(line, " ");
        let line = line.as_ref();
        let names: Vec<regex::Match> = LABEL
            .find_iter(line)
            .filter(|m| is_name(m.as_str()))
            .collect();
        for m in &names {
            let rest = line.get(m.end()..).unwrap_or_default();
            let number = match PAREN_NUMBER.captures(rest) {
                Some(c) => c.get(1),
                None => BOUND_NUMBER
                    .captures(rest)
                    .filter(|c| c.get(2).is_none())
                    .and_then(|c| c.get(1))
                    .or_else(|| {
                        NUMBER_BEFORE
                            .captures(line.get(..m.start()).unwrap_or_default())
                            .and_then(|c| c.get(1))
                    }),
            };
            let Some(number) = number.and_then(|n| whole(n.as_str())) else {
                continue;
            };
            out.push(Binding {
                name: m.as_str().to_string(),
                number,
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

/// What the session says about one name: the numbers on its lines, and how many lines hold it.
fn supported(binding: &Binding, sources: &[&str]) -> bool {
    let mut numbers: HashSet<u64> = HashSet::new();
    for source in sources {
        let lines: Vec<&str> = source
            .lines()
            .filter(|l| holds_name(l, &binding.name))
            .collect();
        numbers.insert(lines.len() as u64);
        for line in &lines {
            numbers.extend(standalone_numbers(line));
        }
        // "KRK-008 … GSM_DROP (29)": lines holding both names, counted, and their numbers.
        for co in &binding.co_names {
            let both: Vec<&&str> = lines.iter().filter(|l| holds_name(l, co)).collect();
            numbers.insert(both.len() as u64);
        }
    }
    numbers.contains(&binding.number)
}

/// What this turn's output says about the name, for the note: the line that counts it, or how
/// many records hold it.
fn tally_line(name: &str, outputs: &[&str]) -> Option<String> {
    let beside = beside_patterns(name)?;
    if let Some(l) = outputs
        .iter()
        .flat_map(|o| o.lines())
        .find(|l| beside.is_match(l))
    {
        let t = l.trim();
        return Some(if t.chars().count() > 80 {
            format!(
                "the output says “{}…”",
                t.chars().take(80).collect::<String>()
            )
        } else {
            format!("the output says “{t}”")
        });
    }
    let records = outputs
        .iter()
        .flat_map(|o| o.lines())
        .filter(|l| holds_name(l, name))
        .count();
    Some(format!("{records} output lines hold it"))
}

fn beside_patterns(name: &str) -> Option<regex::RegexSet> {
    let n = regex::escape(name);
    regex::RegexSet::new([
        format!(r#"(?:^|[\s,;|(\[{{"'])\d[\d,]*[\s:=,;|"']+(?:[A-Za-z_]+=)?{n}\b"#),
        format!(r#"\b{n}["'`*]*[\s:=,;|"'`*]+\d[\d,]*(?:$|[\s,;|)\]}}"'])"#),
    ])
    .ok()
}

/// How this turn's output counts a name: a number right beside it, as `uniq -c`, a `key: value`
/// dump, a CSV or a table prints it — "  64 GSM_DROP", "65 station=KRK-008", "GSM_DROP: 64",
/// `"GSM_DROP": 64`, "| GSM_DROP | 64 |" — or two or more records holding it as a field value
/// ("code=GSM_DROP", "a,KRK-008,b") for the reply to have counted. A name in code beside other
/// words ("max_len: int = 60", "f(text, max_len)") is neither.
fn tallied_in(outputs: &[&str], name: &str) -> bool {
    let n = regex::escape(name);
    let Some(beside) = beside_patterns(name) else {
        return false;
    };
    let Ok(field) = Regex::new(&format!(r#"(?:[=,\t]["']?|\|\s*){n}\b"#)) else {
        return false;
    };
    outputs.iter().any(|o| {
        o.lines().any(|l| beside.is_match(l))
            || o.lines().filter(|l| field.is_match(l)).nth(1).is_some()
    })
}

fn unpaired_numbers(texts: &str, this_turn: &[&str], session: &[&str]) -> Vec<String> {
    let mut found: Vec<(String, u64, Option<String>)> = Vec::new();
    let mut tallied: std::collections::HashMap<String, bool> = std::collections::HashMap::new();
    let mut checked: HashSet<(String, u64, Vec<String>)> = HashSet::new();
    for binding in bindings(texts) {
        if !checked.insert((
            binding.name.clone(),
            binding.number,
            binding.co_names.clone(),
        )) {
            continue;
        }
        let tallied_now = *tallied
            .entry(binding.name.clone())
            .or_insert_with(|| tallied_in(this_turn, &binding.name));
        if !tallied_now || supported(&binding, session) {
            continue;
        }
        if found
            .iter()
            .any(|(n, v, _)| *n == binding.name && *v == binding.number)
        {
            continue;
        }
        let line = tally_line(&binding.name, this_turn);
        found.push((binding.name, binding.number, line));
    }
    if found.is_empty() {
        return Vec::new();
    }
    let shown: Vec<String> = found
        .iter()
        .take(3)
        .map(|(name, number, line)| match line {
            Some(line) => format!("{name} as {number} ({line})"),
            None => format!("{name} as {number}"),
        })
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
        for c in DATE_FORMAT.captures_iter(line) {
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

/// Reply lines that are output lines, as "all N lines" promises: a tally of them is not them.
fn output_lines_shown(reply: &str, outputs: &[&str]) -> usize {
    reply
        .lines()
        .map(|l| {
            l.trim()
                .trim_matches(|c: char| matches!(c, '`' | '*' | '-' | ' '))
        })
        .filter(|l| l.chars().count() >= 12)
        .filter(|l| outputs.iter().any(|o| o.contains(l)))
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
    let mut session_texts: Vec<String> = Vec::new();
    for m in messages[..from].iter().filter(|m| m.is_agent_visible()) {
        session_texts.extend(tool_output_texts(m));
        // The user's own words support a number; the model's earlier replies do not — round 4's
        // "GSM_DROP (29)" at turn 7 repeated its own turn-3 invention ("29 GSM_DROP"), and a
        // compaction summary is the model's words too.
        if m.role == Role::User && m.is_user_visible() {
            let t = text_parts(m);
            if !t.is_empty() {
                session_texts.push(t);
            }
        }
    }
    for m in &messages[from..] {
        session_texts.extend(tool_output_texts(m));
    }
    let mut session: Vec<&str> = session_texts.iter().map(String::as_str).collect();
    session.extend(this_turn.iter().copied());

    let mut findings = unpaired_numbers(&texts, &this_turn, &session);
    findings.extend(format_taken_away(&texts, &turn_calls));
    findings.extend(all_claimed_part_shown(&texts, &this_turn));
    findings
}

#[cfg(test)]
#[path = "grounding_check_tests.rs"]
mod tests;
