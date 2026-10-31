//! Round 5 (2026-10-30): what goose checks in a model's summary AFTER the structural gate passed
//! it — it never refuses here, it FLAGS. A flagged summary is stored with goose's facts before it
//! and a line saying the facts are authoritative (`pillars::stored_summary`).
//!
//! Why: the verifier's GRADES4 (7d16de1ffb, Flash-Next + mock gate, 2026-10-30) stored two kinds
//! of non-summary UNFLAGGED: five bold lines with three words each, and reasoning-only planning
//! notes. Round 3 tried word lists and they broke both ways ("None." and non-English text
//! refused), so the checks here are SIZES and SHAPES: how much the sections say against the
//! conversation they replace, how many sections are under a sentence, and how much of the text is
//! first-person future intent (the one phrase check, four sentence openings).
//!
//! And the numbers: every real summary is a model's recall, and run 2 of GRADES4 wrote "context
//! ~60% of 409,600" — both numbers occur in the conversation, the claim never does. A number (or
//! a share of a quantity) the conversation never showed gets "(unverified)" after it.

use super::{format_message_for_compacting, generic_heading, generic_sections, pillars};
use crate::conversation::message::{Message, MessageContent};
use std::collections::{BTreeSet, HashSet};

/// The share of the replaced conversation's characters (as the transcript renders them: text,
/// tool calls, tool output; no reasoning) a summary's section bodies hold at the least.
/// Measured: the Flash-Next real runs (GRADES4, 223 rows, 340,773 such chars) wrote 10,657 /
/// 11,425 / 11,912 chars of section bodies = 3.1-3.5%; the junk the mock gate stored was 131 and
/// ~230 chars (0.04-0.07%). 1% is a third of the thinnest good one and 15x the largest junk.
pub const THIN_SHARE: f64 = 0.01; // measured: GRADES4 section bodies 3.1-3.5% of replaced
/// The floor never goes under ~300 tokens of text, nor over ~1,700 (a long conversation of big
/// file reads is not asked for a long summary), nor over a tenth of what is replaced (a short
/// conversation's short summary is not thin).
pub const THIN_MIN_CHARS: usize = 1_000; // ratio: ~300 tokens at recall::CHARS_PER_TOKEN
pub const THIN_MAX_CHARS: usize = 6_000; // measured: under the thinnest good summary, 9.3k

/// The four openings a sentence of first-person future intent starts with — the only phrase
/// check in the summary checks, measured by share, never used to refuse.
const INTENT_OPENINGS: [[&str; 2]; 4] = [["i", "will"], ["let", "me"], ["next", "i"], ["now", "i"]];

/// The mark goose writes after a number the conversation never showed.
pub const UNVERIFIED: &str = " (unverified)";

/// The characters of the replaced conversation, as [`THIN_SHARE`] measures them.
pub fn replaced_chars(messages: &[Message]) -> usize {
    messages
        .iter()
        .map(|m| format_message_for_compacting(m).chars().count())
        .sum()
}

/// The fewest characters a summary of `replaced` characters of conversation holds in its sections.
pub fn thin_floor(replaced: usize) -> usize {
    ((replaced as f64 * THIN_SHARE) as usize)
        .clamp(THIN_MIN_CHARS, THIN_MAX_CHARS)
        .min(replaced / 10)
}

/// The section bodies the gate reads: the five named sections when all are there, else the
/// answer's own headings (another language, its own titles).
fn section_bodies(text: &str) -> Vec<String> {
    let named = pillars::written_sections(text);
    let mut seen = [false; super::WRITTEN_PARTS.len()];
    for (part, _) in &named {
        seen[*part] = true;
    }
    if seen.iter().all(|s| *s) {
        named.into_iter().map(|(_, body)| body).collect()
    } else {
        generic_sections(text)
    }
}

/// Whether `c` is a CJK ideograph, kana or hangul syllable: scripts without spaces between words.
fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x3040..=0x30FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xAC00..=0xD7AF | 0xF900..=0xFAFF)
}

/// The words of `body`: whitespace-separated tokens with a letter or digit, and a CJK run counted
/// at two characters a word.
fn words(body: &str) -> usize {
    body.split_whitespace()
        .map(|token| {
            let cjk = token.chars().filter(|c| is_cjk(*c)).count();
            let other = token.chars().any(|c| c.is_alphanumeric() && !is_cjk(c));
            cjk.div_ceil(2) + usize::from(other && cjk == 0)
        })
        .sum()
}

/// A section body under one sentence: no words, or two (a sentence has a subject and a verb and
/// what it is about; "run ls." is not one). One word is the answer that a list is empty ("None.",
/// "Keine.", "无。") and counts as said.
fn under_one_sentence(body: &str) -> bool {
    matches!(words(body), 0 | 2)
}

/// The sentences of `text` outside headings, each trimmed of list and emphasis marks.
fn sentences(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = match generic_heading(line) {
            Some((level, _)) if level > 0 => continue,
            Some((_, inline)) => inline,
            None => line,
        };
        for sentence in line.split(['.', '!', '?', '。', '！', '？', ';']) {
            let sentence = sentence
                .trim_start_matches(|c: char| !c.is_alphanumeric())
                .trim();
            if sentence.chars().any(char::is_alphabetic) {
                out.push(sentence.to_string());
            }
        }
    }
    out
}

/// Whether a sentence opens with first-person future intent ([`INTENT_OPENINGS`]).
fn is_intent(sentence: &str) -> bool {
    let first: Vec<String> = sentence
        .split_whitespace()
        .take(2)
        .map(|w| {
            w.chars()
                .filter(|c| c.is_alphanumeric())
                .flat_map(char::to_lowercase)
                .collect()
        })
        .collect();
    INTENT_OPENINGS
        .iter()
        .any(|[a, b]| first.len() == 2 && first[0] == *a && first[1] == *b)
}

/// Why a summary the gate passed should be stored FLAGGED — `None` when it reads as a summary.
/// `replaced` is [`replaced_chars`] of the conversation it replaces.
pub fn thin_summary(answer: &str, replaced: usize) -> Option<String> {
    let text = pillars::strip_scratch(answer);
    let bodies = section_bodies(&text);
    let mut why = Vec::new();
    let said: usize = if bodies.is_empty() {
        text.chars().count()
    } else {
        bodies.iter().map(|b| b.trim().chars().count()).sum()
    };
    let floor = thin_floor(replaced);
    if said < floor {
        why.push(format!(
            "its sections hold {said} characters, under the {floor} a summary of {replaced} \
             characters of conversation holds at the least"
        ));
    }
    let short = bodies.iter().filter(|b| under_one_sentence(b)).count();
    if !bodies.is_empty() && short * 2 > bodies.len() {
        why.push(format!(
            "{short} of its {} sections are under one sentence",
            bodies.len()
        ));
    }
    let all = sentences(&text);
    let intent = all.iter().filter(|s| is_intent(s)).count();
    if !all.is_empty() && intent * 2 > all.len() {
        why.push(format!(
            "{intent} of its {} sentences say what the model will do next, not what was done",
            all.len()
        ));
    }
    (!why.is_empty()).then(|| why.join("; "))
}

/// Everything the conversation showed the model, for the number check: the text, the tool calls'
/// arguments and the tool output of every row (reasoning left out: a number the model only
/// thought is not one the conversation showed), and `extra` (the kept block, the system prompt).
pub fn known_text(messages: &[Message], extra: &[&str]) -> KnownNumbers {
    let mut text = String::new();
    for message in messages {
        for content in &message.content {
            match content {
                MessageContent::Text(t) => text.push_str(&t.text),
                MessageContent::ToolRequest(request) => {
                    if let Ok(call) = &request.tool_call {
                        if let Some(args) = &call.arguments {
                            for value in args.values() {
                                push_json(&mut text, value);
                            }
                        }
                    }
                }
                MessageContent::ToolResponse(response) => {
                    if let Ok(result) = &response.tool_result {
                        for item in &result.content {
                            if let Some(t) = item.as_text() {
                                text.push_str(&t.text);
                                text.push('\n');
                            }
                        }
                    }
                }
                MessageContent::SystemNotification(n) => text.push_str(&n.msg),
                _ => {}
            }
            text.push('\n');
        }
    }
    for e in extra {
        text.push_str(e);
        text.push('\n');
    }
    KnownNumbers::of(&text)
}

fn push_json(out: &mut String, value: &serde_json::Value) {
    match value {
        serde_json::Value::String(s) => out.push_str(s),
        serde_json::Value::Number(n) => out.push_str(&n.to_string()),
        serde_json::Value::Array(items) => items.iter().for_each(|v| push_json(out, v)),
        serde_json::Value::Object(map) => map.values().for_each(|v| push_json(out, v)),
        _ => {}
    }
    out.push('\n');
}

/// The numbers a conversation showed and its text with thousands separators and runs of space
/// folded, for the exact-match check.
pub struct KnownNumbers {
    numbers: HashSet<String>,
    folded: String,
    /// The text with every run of whitespace one space and its commas kept: "[5, 85]" for a list
    /// the model writes back as "[5,85]".
    spaced: String,
}

/// `text` with `,` dropped and every run of whitespace one space: "409,600" reads "409600".
fn fold(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut space = false;
    for c in text.chars() {
        if c == ',' {
            continue;
        }
        if c.is_whitespace() {
            if !space {
                out.push(' ');
            }
            space = true;
        } else {
            out.push(c);
            space = false;
        }
    }
    out
}

impl KnownNumbers {
    /// Every run of digits (with the `.` and `,` inside it) in `text`, whatever is around it: the
    /// conversation side is read generously so that only a number it truly never showed is marked.
    fn of(text: &str) -> Self {
        let mut numbers = HashSet::new();
        let mut current = String::new();
        for c in text.chars().chain(std::iter::once(' ')) {
            if c.is_ascii_digit() || (!current.is_empty() && (c == '.' || c == ',')) {
                current.push(c);
            } else if !current.is_empty() {
                let number = fold(current.trim_end_matches(['.', ',']));
                // "0.14.3" also shows "0.14" and "14.3"'s parts: each dotted part is known too.
                for part in number.split('.') {
                    numbers.insert(part.to_string());
                }
                numbers.insert(number);
                current.clear();
            }
        }
        Self {
            numbers,
            folded: fold(text),
            spaced: text.split_whitespace().collect::<Vec<_>>().join(" "),
        }
    }
}

impl KnownNumbers {
    /// Whether `token` ("5,85", "15,340") is a list of numbers the conversation showed with a
    /// space after each comma, as code and contracts write them ("[5, 85]", "[15, 340]"); or, when
    /// it cannot be one number (a group after a comma is not three digits: "5,85"), whether each of
    /// its numbers of two digits or more was shown. Real case (MiMo two-Mac Gauntlet, run 51bfaccd,
    /// 2026-10-31): the summary's "clamps pitch [5,85], distance [15,340]" from SB7-CONTRACT.md's
    /// "pitch `[5, 85]`, distance `[15, 340]`" was marked unverified twice.
    fn list_of(&self, token: &str) -> bool {
        let token = token.trim_end_matches('%');
        if !token.contains(',') {
            return false;
        }
        if self.spaced.contains(&token.replace(',', ", ")) {
            return true;
        }
        let mut groups = token.split(',');
        groups.next();
        let one_number = groups.all(|group| group.split('.').next().unwrap_or_default().len() == 3);
        !one_number
            && token.split(',').all(|part| {
                part.chars().filter(char::is_ascii_digit).count() < 2
                    || self.numbers.contains(&fold(part))
            })
    }
}

/// A number the summary states: digits with `.`/`,` between digits, an optional `%`, standing as
/// its own word (not inside a name like `pay_00171` or `R0`).
fn stated_numbers(text: &str) -> Vec<(usize, usize)> {
    let bytes = text.as_bytes();
    let word = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if !bytes[i].is_ascii_digit()
            || (i > 0 && (word(bytes[i - 1]) || bytes[i - 1] == b'.' || bytes[i - 1] == b','))
        {
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len()
            && (bytes[i].is_ascii_digit()
                || ((bytes[i] == b'.' || bytes[i] == b',')
                    && bytes.get(i + 1).is_some_and(u8::is_ascii_digit)))
        {
            i += 1;
        }
        if bytes.get(i) == Some(&b'%') {
            i += 1;
        }
        // Glued to a letter after it ("12k", "2x"): a unit or a name, not checked.
        let glued = bytes
            .get(i)
            .is_some_and(|b| word(*b) || *b >= 0x80 && !is_space_at(text, i));
        if !glued {
            out.push((start, i));
        }
        while i < bytes.len() && (bytes[i].is_ascii_digit() || word(bytes[i])) {
            i += 1;
        }
    }
    out
}

/// Whether the char at byte `i` of `text` is whitespace or punctuation (a non-ASCII byte there
/// may start "×", "→" or "—", which do not glue a unit to a number).
fn is_space_at(text: &str, i: usize) -> bool {
    text.get(i..)
        .unwrap_or_default()
        .chars()
        .next()
        .is_some_and(|c| !c.is_alphanumeric())
}

/// `text[from..to]`; the offsets [`stated_numbers`] gives are ASCII, so always char boundaries.
fn slice(text: &str, from: usize, to: usize) -> &str {
    text.get(from..to).unwrap_or_default()
}

/// The model text with [`UNVERIFIED`] after each number of two digits or more that the
/// conversation never showed, and after each share of a quantity ("60% of 409,600": a percentage,
/// one short word, a number) the conversation never wrote, though each of its numbers it did.
pub fn mark_unverified_numbers(model_text: &str, known: &KnownNumbers) -> String {
    let stated = stated_numbers(model_text);
    let mut marks: BTreeSet<usize> = BTreeSet::new();
    for (k, &(start, end)) in stated.iter().enumerate() {
        let token = slice(model_text, start, end);
        let digits = token.chars().filter(char::is_ascii_digit).count();
        let bare = fold(token.trim_end_matches('%'));
        if digits >= 2 && !known.numbers.contains(&bare) && !known.list_of(token) {
            marks.insert(end);
            continue;
        }
        let Some(&(next_start, next_end)) = stated.get(k + 1) else {
            continue;
        };
        let gap = slice(model_text, end, next_start);
        let joined_by_one_short_word = gap.starts_with(' ')
            && gap.ends_with(' ')
            && gap.trim().chars().count() <= 4
            && !gap.trim().is_empty()
            && gap.trim().chars().all(char::is_alphabetic);
        if token.ends_with('%') && joined_by_one_short_word {
            let phrase = fold(slice(model_text, start, next_end));
            if !known.folded.contains(&phrase) {
                marks.insert(next_end);
            }
        }
    }
    if marks.is_empty() {
        return model_text.to_string();
    }
    let mut out = String::with_capacity(model_text.len() + marks.len() * UNVERIFIED.len());
    let mut from = 0;
    for at in marks {
        out.push_str(slice(model_text, from, at));
        out.push_str(UNVERIFIED);
        from = at;
    }
    out.push_str(slice(model_text, from, model_text.len()));
    out
}

/// How goose's mark after a path the summary names wrong begins.
pub const NOT_ON_DISK_LEAD: &str = " (not on disk";

/// Marks the absolute paths in a model's summary that are not on disk (WR-18, Gabee round 4,
/// 2026-10-31: "Input files … in `/export/`" — the chat's folder held `export/`, the root did not —
/// and the next call ran `grep "ERROR" /export/dockgw-2026-10.log`: "No such file or directory").
/// Only a path whose first folder is not at the root but is in the chat's folder is marked, with
/// the full path that is; any other `/…` word (a real path, an API route, `/v3/docs`, a fenced
/// command) is left alone. Never refused, only
/// marked, like [`mark_unverified_numbers`].
pub fn mark_missing_paths(model_text: &str, chat_folder: Option<&std::path::Path>) -> String {
    let is_path_char =
        |c: char| c.is_alphanumeric() || matches!(c, '/' | '.' | '_' | '-' | '~' | '@' | '+' | '%');
    let chars: Vec<(usize, char)> = model_text.char_indices().collect();
    let mut out = String::with_capacity(model_text.len());
    let mut copied = 0;
    let mut i = 0;
    let mut in_fence = false;
    while i < chars.len() {
        let (at, c) = chars[i];
        // A fenced block is a command or code as written: never marked inside.
        if (i == 0 || chars[i - 1].1 == '\n') && model_text[at..].starts_with("```") {
            in_fence = !in_fence;
        }
        if in_fence {
            i += 1;
            continue;
        }
        let starts_word = i == 0
            || matches!(chars[i - 1].1, ' ' | '\t' | '\n' | '`' | '(' | '"' | '\'' | '[' | '*');
        if c != '/' || !starts_word {
            i += 1;
            continue;
        }
        let mut j = i;
        while j < chars.len() && is_path_char(chars[j].1) {
            j += 1;
        }
        let end = chars.get(j).map_or(model_text.len(), |(n, _)| *n);
        let token = model_text[at..end].trim_end_matches(['.', ',', ':', ';']);
        let token_end = at + token.len();
        i = j.max(i + 1);
        let first = token.trim_start_matches('/').split('/').next().unwrap_or("");
        if first.is_empty() || token.contains("//") || !first.chars().any(char::is_alphabetic) {
            continue;
        }
        let path = std::path::Path::new(token);
        if path.exists() {
            continue;
        }
        // Only a path whose first folder is not at the root but is in the chat's folder: a path
        // under a real root folder may be another machine's, a file still to write, or a route
        // the disk's case-folding matches (`/users/search`), and is left alone.
        if std::path::Path::new("/").join(first).exists() {
            continue;
        }
        let mark = match chat_folder.map(|folder| folder.join(token.trim_start_matches('/'))) {
            Some(there) if there.exists() => format!("{NOT_ON_DISK_LEAD}; {} is)", there.display()),
            _ => continue,
        };
        // After a closing backquote, not inside it.
        let place = if model_text[token_end..].starts_with('`') {
            token_end + 1
        } else {
            token_end
        };
        if model_text[place..].starts_with(NOT_ON_DISK_LEAD) {
            continue;
        }
        out.push_str(&model_text[copied..place]);
        out.push_str(&mark);
        copied = place;
    }
    out.push_str(&model_text[copied..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 340,773: the replaced chars of the Flash-Next stopped run's 223 rows (verifier pre.db).
    const FLASH_NEXT_REPLACED: usize = 340_773;

    fn good_summary_with(decisions: &str) -> String {
        let mut s = String::new();
        for (heading, _) in super::super::WRITTEN_PARTS {
            s.push_str(&format!("## {heading}\n"));
            if heading == "Decisions and reasons" {
                s.push_str(decisions);
                s.push('\n');
                continue;
            }
            for i in 0..12 {
                s.push_str(&format!(
                    "- `app/module_{i}.py` serves the payments API and was checked with curl \
                     against the vendor; its tests pass and the results are in the log.\n"
                ));
            }
        }
        s
    }

    #[test]
    fn the_floor_follows_the_measured_summaries() {
        assert_eq!(thin_floor(FLASH_NEXT_REPLACED), 3_407);
        assert_eq!(thin_floor(5_000), 500);
        assert_eq!(thin_floor(50_000), 1_000);
        assert_eq!(thin_floor(10_000_000), 6_000);
        // The thinnest real summary (10,657 chars of sections) is not thin; the junk is.
        assert!(10_657 > thin_floor(FLASH_NEXT_REPLACED));
    }

    /// GRADES4: the mock gate stored five bold lines that are no summary. Flagged.
    #[test]
    fn five_bold_lines_are_flagged() {
        let junk = "**Thinking:** I should continue.\n\n**Plan:** look at files.\n\n**Step one:** \
                    run ls.\n\n**Step two:** read output.\n\n**Then:** carry on.\n";
        assert!(
            super::super::summary_shortfall(junk).is_none(),
            "the gate passes it"
        );
        let why = thin_summary(junk, FLASH_NEXT_REPLACED).expect("flagged");
        assert!(why.contains("characters"), "{why}");
        assert!(why.contains("under one sentence"), "{why}");
    }

    /// GRADES4: reasoning-only planning notes stored as the summary. Flagged by size; the
    /// "I will now…" kind also by its intent.
    #[test]
    fn planning_notes_are_flagged() {
        let notes = "Constraints:\n- five sections, no tools.\nWhat I know:\n- lots of files were \
                     written.\nApproach:\n- go section by section.\nOutput format:\n- markdown \
                     headings.\nRisks:\n- running long.\n";
        assert!(
            super::super::summary_shortfall(notes).is_none(),
            "the gate passes it"
        );
        assert!(thin_summary(notes, FLASH_NEXT_REPLACED).is_some());

        let mut intent = String::new();
        for (heading, _) in super::super::WRITTEN_PARTS {
            intent.push_str(&format!(
                "## {heading}\nI will now go through the files written in this conversation and \
                 describe each one with its routes, functions and checks in detail. Let me first \
                 read the contract again so nothing is missed in the summary that follows. Next I \
                 will run the self check and the framing check again and write the results.\n"
            ));
        }
        intent = intent.repeat(6);
        let why = thin_summary(&intent, FLASH_NEXT_REPLACED).expect("flagged");
        assert!(why.contains("will do next"), "{why}");
        assert!(
            !why.contains("characters,"),
            "big enough: only the intent flags it: {why}"
        );
    }

    /// The counter-test: "None." sections and short German or Chinese sections are summaries.
    #[test]
    fn none_and_short_german_or_chinese_sections_are_not_flagged() {
        let german = "## Wo wir stehen\nDas Backend läuft, alle Tests sind grün.\n\
                      ## Was jede Datei tut\nKeine.\n\
                      ## Entscheidungen und Gründe\nKeine.\n\
                      ## Noch zu tun\nNone.\n\
                      ## Nächster Schritt\nDie Kamera für die Übersicht einstellen.\n";
        assert!(super::super::summary_shortfall(german).is_none());
        // 0: a conversation too small to set a size floor; the SHAPE checks are what is tested.
        assert_eq!(thin_summary(german, 0), None, "{german}");

        let chinese = "## 当前进度\n后端已完成，同步了全部付款记录。\n\
                       ## 各文件作用\n无。\n\
                       ## 决定与理由\n无。\n\
                       ## 待办\n暂无。\n\
                       ## 下一步\n调整默认相机以满足取景要求。\n";
        assert!(super::super::summary_shortfall(chinese).is_none());
        assert_eq!(thin_summary(chinese, 0), None, "{chinese}");

        let with_none = good_summary_with("None.");
        assert_eq!(thin_summary(&with_none, FLASH_NEXT_REPLACED), None);
        let with_keine = good_summary_with("Keine.");
        assert_eq!(thin_summary(&with_keine, FLASH_NEXT_REPLACED), None);
        // "None." / "Keine." / "无。" as the openings of sentences are never intent.
        for s in [
            "None.",
            "Keine.",
            "无。",
            "Ich werde die Tests laufen lassen.",
        ] {
            assert!(!sentences(s).iter().any(|s| is_intent(s)), "{s}");
        }
    }

    fn known(text: &str) -> KnownNumbers {
        KnownNumbers::of(text)
    }

    /// A list the model writes without the spaces the conversation had is no new number; a
    /// thousands number the conversation never showed still is. Real case: MiMo run 51bfaccd's
    /// summary, "clamps pitch [5,85], distance [15,340]", against SB7-CONTRACT.md's "[5, 85]" and
    /// "[15, 340]", both marked unverified before.
    #[test]
    fn a_list_written_without_its_spaces_is_not_a_new_number() {
        let contract =
            known("Clamps: pitch `[5, 85]`, distance `[15, 340]` and 6.\npayments 12289\n");
        assert_eq!(
            mark_unverified_numbers("clamps pitch [5,85], distance [15,340]", &contract),
            "clamps pitch [5,85], distance [15,340]"
        );
        // Not one number ("85" is not a thousands group) and each part was shown.
        assert_eq!(
            mark_unverified_numbers("pitch 85,15 then", &contract),
            "pitch 85,15 then"
        );
        // A thousands number nobody showed, and a list with a part nobody showed, stay marked.
        assert_eq!(
            mark_unverified_numbers("payments 12,288 and range 5,97", &contract),
            format!("payments 12,288{UNVERIFIED} and range 5,97{UNVERIFIED}")
        );
        // The real thousands number is still known.
        assert_eq!(
            mark_unverified_numbers("payments 12,289", &contract),
            "payments 12,289"
        );
    }

    /// GRADES4 run 2: "context ~60% of 409,600". 409,600 is the window every turn-context line
    /// named and 60% a framing rule the contract states; the share of the window is nowhere.
    #[test]
    fn a_share_the_conversation_never_wrote_is_unverified() {
        let conversation = "context: 239,105 of 409,600 tokens used (58%)\n\
                            the field must span >=60% of the canvas width\n\
                            count 12291 R0 180 days 96 hmax 3.0749925684448027\n";
        let k = known(conversation);
        let text = "Budget: 150 model calls; context ~60% of 409,600. Synced 12,291 payments.";
        let marked = mark_unverified_numbers(text, &k);
        assert_eq!(
            marked,
            "Budget: 150 (unverified) model calls; context ~60% of 409,600 (unverified). Synced \
             12,291 payments."
        );
        // What the conversation did say stays unmarked.
        let said = "context 239,105 of 409,600 tokens; it must span 60% of the canvas.";
        assert_eq!(mark_unverified_numbers(said, &k), said);
    }

    #[test]
    fn names_units_and_single_digits_are_not_numbers_to_check() {
        let k = known("nothing here");
        let text = "pay_00171, R0, D4, v2, 12k rows, 5 sections, sha 7d16de1ffb, x 3 and 4.";
        assert_eq!(mark_unverified_numbers(text, &k), text);
    }

    /// WR-18: the summary's "/export/" was the chat's `export/` folder; a path under an existing
    /// root folder that is gone is marked; routes and real paths are left alone.
    #[test]
    fn a_path_not_on_disk_is_marked_and_a_route_is_not() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("export")).unwrap();
        std::fs::write(dir.path().join("export/dock.log"), "x").unwrap();
        let real = dir.path().join("export/dock.log");
        let text = format!(
            "Input files in `/export/`, log `{}`, gone {}/gone.txt, route `/api/summary` and \
             /v3/docs, url http://127.0.0.1:8850/v3/docs.",
            real.display(),
            dir.path().display()
        );
        let marked = super::mark_missing_paths(&text, Some(dir.path()));
        let there = dir.path().join("export/");
        assert!(
            marked.contains(&format!("`/export/` (not on disk; {} is)", there.display())),
            "{marked}"
        );
        // Under a real root folder: another machine's path or a file still to write, left alone.
        assert!(!marked.contains("gone.txt (not"), "{marked}");
        let fenced = "```\ngrep ERROR /export/dock.log\n```\nthen /export/";
        let fenced_marked = super::mark_missing_paths(fenced, Some(dir.path()));
        assert!(fenced_marked.starts_with("```\ngrep ERROR /export/dock.log\n```"), "{fenced_marked}");
        assert!(fenced_marked.ends_with("is)"), "{fenced_marked}");
        assert!(!marked.contains("dock.log` (not"), "{marked}");
        assert!(!marked.contains("summary` (not"), "{marked}");
        assert!(!marked.contains("docs (not") && !marked.contains("docs. (not"), "{marked}");
        assert_eq!(super::mark_missing_paths(&marked, Some(dir.path())), marked);
    }
}
