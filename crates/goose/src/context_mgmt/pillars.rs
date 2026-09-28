//! Q-357: what a chat's compaction keeps WORD FOR WORD. Built by code from the whole stored
//! session — messages an earlier compaction hid stay in the session store, so nothing drifts from
//! one compaction to the next — and appended to the model's summary as a `<kept-by-goose>` block.
//! The model writes only what code cannot know: where the work stands, the next step, and the
//! decisions and reasons the ledger does not hold.
//!
//! Why (E2E #3p, 2026-09-28, sessions.db message 771038): the summary the model wrote was 8,821
//! chars, 6,460 of them (73%) the `<analysis>` scratch the prompt asked for and nothing stripped,
//! re-read on every later turn; and it re-wrote the person's words, the files and the errors from
//! memory — its summary contradicts its own analysis on the six project leads.

use super::{arguments_excerpt, excerpt, outcome_and_output, RECORD_EXCERPT_CHARS};
use crate::agents::execute_commands::parse_slash_command;
use crate::agents::platform_extensions::developer::file_diff::{written_files, WrittenFile};
use crate::conversation::message::{audience_includes, Message, MessageContent};
use rmcp::model::Role;

pub const KEPT_OPEN: &str = "<kept-by-goose>";
pub const KEPT_CLOSE: &str = "</kept-by-goose>";

/// The first line of a summary written under a note: how the model read the note.
pub const NOTE_MARKER: &str = "NOTE";

/// One message the person wrote, as the kept block carries it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asked {
    pub text: String,
    /// Cut to an excerpt so the block fits its share of the window; `chars` is the whole length.
    pub cut: bool,
    pub chars: usize,
}

/// A tool call that failed, from what the call and its result carry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failed {
    pub call: String,
    pub outcome: String,
    pub output: String,
}

/// The chat's ledger as the compaction read it. An unreadable ledger is said, never taken as empty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LedgerRead {
    Entries(Vec<String>),
    Unreadable(String),
}

/// P1–P5: the parts of a chat goose keeps itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pillars {
    /// P1 — every message the person wrote, oldest first.
    pub asked: Vec<Asked>,
    /// P2 — every file the chat's `write`/`edit` calls changed.
    pub files: Vec<WrittenFile>,
    /// P3 — the failed tool calls kept, oldest first; `failed_left_out` older ones did not fit.
    pub failed: Vec<Failed>,
    pub failed_left_out: usize,
    /// P4 — the person's note for this compaction and their pins, verbatim.
    pub note: Option<String>,
    pub pins: Vec<String>,
    /// P5 — the ledger, oldest first; `ledger_left_out` older entries did not fit.
    pub ledger: LedgerRead,
    pub ledger_left_out: usize,
}

/// What the kept block is built from besides the conversation.
#[derive(Debug, Clone, Default)]
pub struct KeptSources<'a> {
    pub note: Option<&'a str>,
    pub pins: &'a [String],
    /// The message the compaction appends after the summary word for word; not kept twice.
    pub preserved: Option<&'a str>,
}

impl Pillars {
    pub fn build(messages: &[Message], sources: &KeptSources<'_>, ledger: LedgerRead) -> Self {
        let mut asked = asked_of(messages);
        if let (Some(preserved), Some(last)) = (sources.preserved, asked.last()) {
            if last.text == preserved {
                asked.pop();
            }
        }
        Self {
            asked,
            files: written_files(messages),
            failed: failed_of(messages),
            failed_left_out: 0,
            note: sources
                .note
                .map(str::trim)
                .filter(|note| !note.is_empty())
                .map(str::to_string),
            pins: sources.pins.to_vec(),
            ledger,
            ledger_left_out: 0,
        }
    }

    /// Fits the block into `budget_chars`: the oldest failed calls go first, then the oldest ledger
    /// entries (`ledger_read` still has them), then the person's older messages are cut to
    /// excerpts, oldest first. The newest message, the note and the pins are never cut. Each cut is
    /// said in the block itself. `None`: the window is unknown and nothing is cut.
    pub fn fit(mut self, budget_chars: Option<usize>) -> Self {
        let Some(budget) = budget_chars else {
            return self;
        };
        while self.render().chars().count() > budget {
            if !self.failed.is_empty() {
                self.failed.remove(0);
                self.failed_left_out += 1;
                continue;
            }
            if let LedgerRead::Entries(entries) = &mut self.ledger {
                if !entries.is_empty() {
                    entries.remove(0);
                    self.ledger_left_out += 1;
                    continue;
                }
            }
            let newest = self.asked.len().saturating_sub(1);
            let Some(oldest_whole) = self.asked[..newest]
                .iter()
                .position(|a| !a.cut && a.chars > RECORD_EXCERPT_CHARS)
            else {
                break;
            };
            let entry = &mut self.asked[oldest_whole];
            entry.text = excerpt(&entry.text);
            entry.cut = true;
        }
        self
    }

    /// The `<kept-by-goose>` block stored after the model's summary.
    pub fn render(&self) -> String {
        let mut out = format!(
            "{KEPT_OPEN}\ngoose kept these word for word from the whole conversation — facts, not \
             a paraphrase.\n"
        );
        if !self.asked.is_empty() {
            out.push_str(&format!(
                "\n## The person's messages ({}, oldest first)\n",
                self.asked.len()
            ));
            for (i, asked) in self.asked.iter().enumerate() {
                let mut lines = asked.text.lines();
                out.push_str(&format!("[{}] {}\n", i + 1, lines.next().unwrap_or("")));
                for line in lines {
                    out.push_str(&format!("    {line}\n"));
                }
                if asked.cut {
                    out.push_str(&format!(
                        "    (cut to its start here; {} chars in all)\n",
                        asked.chars
                    ));
                }
            }
        }
        if !self.files.is_empty() {
            let under = common_dir(self.files.iter().map(|f| f.path.as_str()));
            match under {
                Some(dir) => out.push_str(&format!(
                    "\n## Files written ({}, under {dir})\n",
                    self.files.len()
                )),
                None => out.push_str(&format!("\n## Files written ({})\n", self.files.len())),
            }
            for file in &self.files {
                let shown = under
                    .and_then(|dir| file.path.strip_prefix(dir))
                    .map(|rest| rest.trim_start_matches(['/', '\\']))
                    .unwrap_or(&file.path);
                out.push_str(&format!("- {}\n", file_line(shown, file)));
            }
        }
        if !self.failed.is_empty() || self.failed_left_out > 0 {
            out.push_str(&format!(
                "\n## Tool calls that failed ({})\n",
                self.failed.len() + self.failed_left_out
            ));
            if self.failed_left_out > 0 {
                out.push_str(&format!(
                    "({} earlier failed calls are not listed here)\n",
                    self.failed_left_out
                ));
            }
            for failed in &self.failed {
                out.push_str(&format!("- {} {}.", failed.call, failed.outcome));
                if !failed.output.is_empty() {
                    out.push_str(&format!(" {}", failed.output));
                }
                out.push('\n');
            }
        }
        if self.note.is_some() || !self.pins.is_empty() {
            out.push_str("\n## The person's notes\n");
            if let Some(note) = &self.note {
                out.push_str(&format!("- Note for this compaction: {note}\n"));
            }
            for pin in &self.pins {
                out.push_str(&format!("- Pinned: {pin}\n"));
            }
        }
        match &self.ledger {
            LedgerRead::Entries(entries) if !entries.is_empty() || self.ledger_left_out > 0 => {
                out.push_str(&format!(
                    "\n## Ledger ({} entries, oldest first)\n",
                    entries.len() + self.ledger_left_out
                ));
                if self.ledger_left_out > 0 {
                    out.push_str(&format!(
                        "({} older entries are not listed here; ledger_read has them)\n",
                        self.ledger_left_out
                    ));
                }
                for entry in entries {
                    out.push_str(&format!("- {entry}\n"));
                }
            }
            LedgerRead::Entries(_) => {}
            LedgerRead::Unreadable(error) => {
                out.push_str(&format!(
                    "\n## Ledger\nThe ledger could not be read ({error}).\n"
                ));
            }
        }
        out.push_str(KEPT_CLOSE);
        out
    }

    /// What goose keeps, as the summary instruction names it — this chat's counts and names, so
    /// the model knows what it need not repeat.
    pub fn kept_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        if !self.asked.is_empty() {
            lines.push(format!(
                "the person's {}, word for word",
                counted(self.asked.len(), "message", "messages")
            ));
        }
        if !self.files.is_empty() {
            let names: Vec<&str> = self.files.iter().map(|f| file_name(&f.path)).collect();
            lines.push(format!(
                "the {} written, with their line counts: {}",
                counted(self.files.len(), "file", "files"),
                names.join(", ")
            ));
        }
        let failed = self.failed.len() + self.failed_left_out;
        if failed > 0 {
            lines.push(format!(
                "the {} that failed, with the errors",
                counted(failed, "tool call", "tool calls")
            ));
        }
        match (self.note.is_some(), self.pins.len()) {
            (false, 0) => {}
            (true, 0) => lines.push("the person's note for this compaction".to_string()),
            (false, pins) => lines.push(format!(
                "the person's {}",
                counted(pins, "pinned line", "pinned lines")
            )),
            (true, pins) => lines.push(format!(
                "the person's note and {}",
                counted(pins, "pinned line", "pinned lines")
            )),
        }
        if let LedgerRead::Entries(entries) = &self.ledger {
            let total = entries.len() + self.ledger_left_out;
            if total > 0 {
                lines.push(format!(
                    "the {}",
                    counted(total, "ledger entry", "ledger entries")
                ));
            }
        }
        lines
    }
}

fn counted(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}

fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// The folder every path shares, when there are two paths or more and it is not the root.
fn common_dir<'a>(mut paths: impl Iterator<Item = &'a str>) -> Option<&'a str> {
    let first = paths.next()?;
    let mut shared = first.len();
    let mut count = 1;
    for path in paths {
        count += 1;
        shared = first
            .bytes()
            .zip(path.bytes())
            .take(shared)
            .take_while(|(a, b)| a == b)
            .count();
    }
    let dir = first.as_bytes()[..shared]
        .iter()
        .rposition(|b| *b == b'/' || *b == b'\\')?;
    (count > 1 && dir > 0).then(|| &first[..dir])
}

fn file_line(shown: &str, file: &WrittenFile) -> String {
    let how = if file.created { "created" } else { "changed" };
    format!(
        "{shown} — {how}, +{} −{} lines, {}",
        file.added,
        file.removed,
        counted(file.edits, "write", "writes")
    )
}

/// P1: the text of every message the person wrote — user role, shown to the person, text only (a
/// tool result is not the person's). A `/compact` command is not kept (its note is P4). The copy
/// of the newest message a compaction appends after its summary (visible to both) repeats the
/// original that compaction hid from the agent, and is kept once.
fn asked_of(messages: &[Message]) -> Vec<Asked> {
    let mut asked: Vec<Asked> = Vec::new();
    let mut previous_hidden_from_agent: Option<String> = None;
    for message in messages {
        if message.role != Role::User || !message.is_user_visible() {
            continue;
        }
        if message
            .content
            .iter()
            .any(|c| matches!(c, MessageContent::ToolResponse(_)))
        {
            continue;
        }
        let text: Vec<&str> = message
            .content
            .iter()
            .filter_map(|c| match c {
                MessageContent::Text(t) if audience_includes(t.audience(), &Role::Assistant) => {
                    Some(t.text.as_str())
                }
                _ => None,
            })
            .collect();
        let text = text.join("\n");
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        if parse_slash_command(text).is_some_and(|parsed| parsed.command == "compact") {
            continue;
        }
        if message.is_agent_visible() && previous_hidden_from_agent.as_deref() == Some(text) {
            previous_hidden_from_agent = None;
            continue;
        }
        previous_hidden_from_agent = (!message.is_agent_visible()).then(|| text.to_string());
        asked.push(Asked {
            text: text.to_string(),
            cut: false,
            chars: text.chars().count(),
        });
    }
    asked
}

/// P3: every call whose result is an error or says it failed, with its tool, its arguments cut
/// as `record_tool_call` cuts them, and excerpts of its own output.
fn failed_of(messages: &[Message]) -> Vec<Failed> {
    let requests: Vec<&crate::conversation::message::ToolRequest> = messages
        .iter()
        .flat_map(|m| m.content.iter())
        .filter_map(|c| match c {
            MessageContent::ToolRequest(req) => Some(req),
            _ => None,
        })
        .collect();
    let mut failed = Vec::new();
    for response in messages
        .iter()
        .flat_map(|m| m.content.iter())
        .filter_map(|c| match c {
            MessageContent::ToolResponse(res) => Some(res),
            _ => None,
        })
    {
        let (outcome, output) = match &response.tool_result {
            Ok(result) if result.is_error == Some(true) => outcome_and_output(result),
            Ok(_) => continue,
            Err(e) => (
                format!("failed: {}", super::quoted(&excerpt(&e.message))),
                String::new(),
            ),
        };
        let call = match requests.iter().find(|req| req.id == response.id) {
            Some(req) => match &req.tool_call {
                Ok(call) => format!(
                    "{} {}",
                    call.name,
                    arguments_excerpt(call.arguments.as_ref())
                ),
                Err(e) => format!(
                    "A tool call goose could not parse ({})",
                    excerpt(&e.message)
                ),
            },
            None => "A tool call whose request is no longer in the conversation".to_string(),
        };
        failed.push(Failed {
            call,
            outcome,
            output,
        });
    }
    failed
}

/// The model's summary without its scratch: every closed `<analysis>…</analysis>` section, and the
/// NOTE line when it leads. An unclosed `<analysis>` is left as written — cutting to the end of the
/// text would cut the summary with it.
pub fn strip_scratch(summary: &str) -> String {
    let (text, _) = without_analysis(summary);
    let text = text.trim_start();
    let text = match first_line(text) {
        Some((line, rest)) if note_line(line).is_some() => rest.trim_start(),
        _ => text,
    };
    text.trim().to_string()
}

/// How the model read the person's note, from the NOTE line that leads its answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoteVerdict {
    Ok,
    Question(String),
    Concern(String),
    /// The answer does not lead with a NOTE line: the note was followed as written, unassessed.
    Missing,
}

fn first_line(text: &str) -> Option<(&str, &str)> {
    let text = text.trim_start();
    if text.is_empty() {
        return None;
    }
    Some(match text.split_once('\n') {
        Some((line, rest)) => (line, rest),
        None => (text, ""),
    })
}

/// A NOTE line as the instruction asks for it (`NOTE OK`, `NOTE QUESTION: …`, `NOTE CONCERN: …`),
/// tolerating the markdown emphasis or heading marks a model may wrap it in. A question or concern
/// with no words is no verdict.
fn note_line(line: &str) -> Option<NoteVerdict> {
    let line = line
        .trim()
        .trim_matches(|c: char| c == '*' || c == '#' || c == '_' || c.is_whitespace());
    let rest = line.strip_prefix(NOTE_MARKER)?;
    if !rest.starts_with([':', ' ']) {
        return None;
    }
    let rest = rest.trim_start_matches([':', ' ']);
    let (word, said) = rest
        .split_once(|c: char| c == ':' || c.is_whitespace())
        .unwrap_or((rest, ""));
    let said = said
        .trim_start_matches(|c: char| c == ':' || c == '*' || c == '_' || c.is_whitespace())
        .trim_end_matches(|c: char| c == '*' || c == '_' || c.is_whitespace())
        .to_string();
    match word
        .trim_end_matches(['.', '*'])
        .to_ascii_uppercase()
        .as_str()
    {
        "OK" => Some(NoteVerdict::Ok),
        "QUESTION" if !said.is_empty() => Some(NoteVerdict::Question(said)),
        "CONCERN" if !said.is_empty() => Some(NoteVerdict::Concern(said)),
        _ => None,
    }
}

/// `text` without its closed `<analysis>…</analysis>` sections, and whether one is left open.
fn without_analysis(text: &str) -> (String, bool) {
    let mut text = text.to_string();
    while let Some(start) = text.find("<analysis>") {
        let Some(end) = text[start..].find("</analysis>") else {
            return (text, true);
        };
        text.replace_range(start..start + end + "</analysis>".len(), "");
    }
    (text, false)
}

/// The verdict of a complete answer, its scratch aside.
pub fn note_verdict(answer: &str) -> NoteVerdict {
    let (text, _) = without_analysis(answer);
    first_line(&text)
        .and_then(|(line, _)| note_line(line))
        .unwrap_or(NoteVerdict::Missing)
}

/// The verdict of an answer still streaming, once its first line is whole: `None` while the first
/// line may still grow, or while an `<analysis>` section is still open ahead of it.
pub fn streaming_note_verdict(so_far: &str) -> Option<NoteVerdict> {
    let (text, open) = without_analysis(so_far);
    if open {
        return None;
    }
    let (line, _) = text.trim_start().split_once('\n')?;
    Some(note_line(line).unwrap_or(NoteVerdict::Missing))
}

/// The section headings the model has written so far (`## …` lines), in order.
pub fn written_parts(so_far: &str) -> Vec<String> {
    so_far
        .lines()
        .filter_map(|line| line.trim_start().strip_prefix("## "))
        .map(|heading| heading.trim().to_string())
        .filter(|heading| !heading.is_empty())
        .collect()
}

/// The stored summary: the model's text, its scratch gone, then the kept block.
pub fn stored_summary(model_text: &str, kept: &Pillars) -> String {
    let written = strip_scratch(model_text);
    if written.is_empty() {
        kept.render()
    } else {
        format!("{written}\n\n{}", kept.render())
    }
}

/// The model-written part of a stored summary (what goose wrote before its kept block).
pub fn model_part(stored: &str) -> &str {
    match stored.find(KEPT_OPEN) {
        Some(at) => stored[..at].trim_end(),
        None => stored,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::platform_extensions::developer::file_diff::{
        diff_texts, with_file_diff, Before,
    };
    use rmcp::model::{AnnotateAble, CallToolRequestParams, CallToolResult, RawContent};

    fn text_result(text: &str) -> CallToolResult {
        CallToolResult::success(vec![RawContent::text(text).no_annotation()])
    }

    fn call(id: &str, tool: &str, args: serde_json::Value) -> Message {
        let mut params = CallToolRequestParams::new(tool.to_string());
        params.arguments = args.as_object().cloned();
        Message::assistant().with_tool_request(id, Ok(params))
    }

    fn chat() -> Vec<Message> {
        let mut failed =
            CallToolResult::error(vec![
                RawContent::text("SyntaxError: Unexpected token").no_annotation()
            ]);
        failed.structured_content = Some(serde_json::json!({"exit_code": 1}));
        vec![
            Message::user().with_text("inactive = no login in 24 months, THAT is the cutoff"),
            call(
                "w1",
                "write",
                serde_json::json!({"path": "/w/plan.js", "content": "const SEED = 20260928;\n"}),
            ),
            Message::user().with_tool_response(
                "w1",
                Ok(with_file_diff(
                    text_result("Created /w/plan.js"),
                    "/w/plan.js",
                    Before::None,
                    &diff_texts("/w/plan.js", None, "const SEED = 20260928;\n"),
                )),
            ),
            call(
                "s1",
                "shell",
                serde_json::json!({"command": "node plan.js"}),
            ),
            Message::user().with_tool_response("s1", Ok(failed)),
            Message::assistant().with_text("fixed it"),
            Message::user().with_text("/compact keep the lead rule"),
            Message::user().with_text("a project lead is never skipped"),
        ]
    }

    fn entries(lines: &[&str]) -> LedgerRead {
        LedgerRead::Entries(lines.iter().map(|l| l.to_string()).collect())
    }

    /// P1–P5 are what the calls and the person's words actually carry: each message whole, the
    /// file from its diff, the failed call with its own error, the note and pins verbatim, the
    /// ledger entry — and the `/compact` command is no message of the person's.
    #[test]
    fn the_kept_block_carries_the_words_files_errors_notes_and_ledger() {
        let pins = vec!["seed 20260928".to_string()];
        let kept = Pillars::build(
            &chat(),
            &KeptSources {
                note: Some("keep the lead rule"),
                pins: &pins,
                preserved: None,
            },
            entries(&["2026-09-28 12:20 [fact] 10 pass / 0 fail"]),
        );
        let block = kept.render();
        assert!(block.starts_with(KEPT_OPEN) && block.ends_with(KEPT_CLOSE));
        assert!(block.contains(
            "## The person's messages (2, oldest first)\n[1] inactive = no login in 24 months, \
             THAT is the cutoff\n[2] a project lead is never skipped\n"
        ));
        assert!(!block.contains("/compact"));
        assert!(
            block.contains("## Files written (1)\n- /w/plan.js — created, +1 −0 lines, 1 write\n")
        );
        assert!(block.contains(
            "## Tool calls that failed (1)\n- shell {\"command\":\"node plan.js\"} failed (exit 1). \
             Output: \"SyntaxError: Unexpected token\"\n"
        ));
        assert!(block
            .contains("- Note for this compaction: keep the lead rule\n- Pinned: seed 20260928\n"));
        assert!(block.contains(
            "## Ledger (1 entries, oldest first)\n- 2026-09-28 12:20 [fact] 10 pass / 0 fail\n"
        ));
    }

    /// The copy of the newest message a compaction appends after its summary is not kept a second
    /// time, and the copy the PREVIOUS compaction appended (visible to both, after the hidden
    /// original) is one message, not two.
    #[test]
    fn a_message_is_kept_once() {
        let mut messages = vec![
            Message::user()
                .with_text("run the plan")
                .with_visibility(true, false),
            Message::user().with_text("run the plan"),
            Message::user().with_text("now the tests"),
        ];
        let kept = Pillars::build(&messages, &KeptSources::default(), entries(&[]));
        let texts: Vec<&str> = kept.asked.iter().map(|a| a.text.as_str()).collect();
        assert_eq!(texts, ["run the plan", "now the tests"]);

        let kept = Pillars::build(
            &messages,
            &KeptSources {
                preserved: Some("now the tests"),
                ..Default::default()
            },
            entries(&[]),
        );
        assert_eq!(kept.asked.len(), 1);

        messages.push(Message::user().with_text("now the tests").agent_only());
        let kept = Pillars::build(&messages, &KeptSources::default(), entries(&[]));
        assert_eq!(
            kept.asked.len(),
            2,
            "an agent-only message is goose's, not the person's"
        );
    }

    /// Over its budget the block gives up the oldest failed calls first, then the oldest ledger
    /// entries, then cuts the person's OLDER messages — never the newest — and says each cut.
    #[test]
    fn the_block_fits_its_budget_in_order_and_says_what_it_cut() {
        let long = "x".repeat(4 * RECORD_EXCERPT_CHARS);
        let mut messages = vec![Message::user().with_text(&long)];
        messages.extend(chat());
        let kept = Pillars::build(
            &messages,
            &KeptSources::default(),
            entries(&[
                "2026-09-28 11:47 [decision] old",
                "2026-09-28 12:20 [fact] new",
            ]),
        );
        let whole = kept.render().chars().count();

        let without_errors = kept.clone().fit(Some(whole - 1));
        assert_eq!(without_errors.failed_left_out, 1);
        assert_eq!(without_errors.ledger_left_out, 0);
        assert!(without_errors
            .render()
            .contains("(1 earlier failed calls are not listed here)"));

        let tight = kept.clone().fit(Some(whole - 2 * RECORD_EXCERPT_CHARS));
        assert_eq!(tight.ledger_left_out, 2);
        assert!(tight.asked[0].cut && tight.asked[0].chars == long.chars().count());
        assert!(
            !tight.asked.last().unwrap().cut,
            "the newest message stays whole"
        );
        let rendered = tight.render();
        assert!(rendered.contains("(2 older entries are not listed here; ledger_read has them)"));
        assert!(rendered.contains(&format!(
            "(cut to its start here; {} chars in all)",
            long.chars().count()
        )));

        assert_eq!(
            kept.clone().fit(None),
            kept,
            "an unknown window cuts nothing"
        );
    }

    #[test]
    fn an_unreadable_ledger_is_said_never_taken_as_empty() {
        let kept = Pillars::build(
            &chat(),
            &KeptSources::default(),
            LedgerRead::Unreadable("Permission denied (os error 13)".to_string()),
        );
        assert!(kept.render().contains(
            "## Ledger\nThe ledger could not be read (Permission denied (os error 13)).\n"
        ));
    }

    /// W2: #3p's summary was 73% `<analysis>` scratch. Stripped: every closed section and the
    /// leading NOTE line; an unclosed one is left, since cutting it would cut the summary.
    #[test]
    fn the_scratch_and_the_note_line_are_stripped() {
        assert_eq!(
            strip_scratch(
                "<analysis>\nlong\n</analysis>\n\nHere is my summary:\n## Where we are\nx"
            ),
            "Here is my summary:\n## Where we are\nx"
        );
        assert_eq!(
            strip_scratch("**NOTE OK**\n## Where we are\nx"),
            "## Where we are\nx"
        );
        assert_eq!(
            strip_scratch("NOTE CONCERN: 24 not 12\n\n## Where we are\nx"),
            "## Where we are\nx"
        );
        assert_eq!(
            strip_scratch("<analysis>open\n## x"),
            "<analysis>open\n## x"
        );
        assert_eq!(
            strip_scratch("NOTES from the call\nx"),
            "NOTES from the call\nx"
        );
    }

    #[test]
    fn the_note_verdict_is_read_from_the_first_line_or_said_missing() {
        assert_eq!(note_verdict("NOTE OK\n## a"), NoteVerdict::Ok);
        assert_eq!(note_verdict("NOTE OK."), NoteVerdict::Ok);
        assert_eq!(
            note_verdict("<analysis>x</analysis>\nNOTE QUESTION: 12 or 24?\n"),
            NoteVerdict::Question("12 or 24?".to_string())
        );
        assert_eq!(
            note_verdict("**NOTE CONCERN:** the chat says 24.**"),
            NoteVerdict::Concern("the chat says 24.".to_string())
        );
        assert_eq!(note_verdict("## Where we are"), NoteVerdict::Missing);
        assert_eq!(note_verdict("NOTE QUESTION:"), NoteVerdict::Missing);

        assert_eq!(streaming_note_verdict("NOTE QUES"), None);
        assert_eq!(streaming_note_verdict("<analysis>still\n"), None);
        assert_eq!(
            streaming_note_verdict("NOTE QUESTION: 12 or 24?\n"),
            Some(NoteVerdict::Question("12 or 24?".to_string()))
        );
        assert_eq!(
            streaming_note_verdict("## Where we are\n"),
            Some(NoteVerdict::Missing)
        );
    }

    #[test]
    fn the_parts_are_the_headings_written_so_far() {
        assert_eq!(
            written_parts("NOTE OK\n## Where we are\nx\n## Next step\n##\n"),
            ["Where we are", "Next step"]
        );
    }

    #[test]
    fn the_stored_summary_is_the_model_part_then_the_kept_block() {
        let kept = Pillars::build(&chat(), &KeptSources::default(), entries(&[]));
        let stored = stored_summary("<analysis>a</analysis>NOTE OK\n## Where we are\nx", &kept);
        assert_eq!(model_part(&stored), "## Where we are\nx");
        assert!(stored.ends_with(&kept.render()));
    }

    /// E2E #3p (session 20260928_19, 2026-09-28): its compaction ran at the start of the reply to
    /// "Run the plan on the fake data…" (sessions.db 771037), the summary it stored is message
    /// 771038. Replayed from a read-only export of the session and its chat ledger:
    /// `Q357_SESSION_JSON=/tmp/q357/s19.json Q357_LEDGER=~/.goose/ledgers/20260928_19.md`.
    #[test]
    #[ignore = "reads a local export of E2E #3p's session"]
    fn e2e_3p_pillars_carry_the_six_facts() {
        let messages: Vec<Message> = serde_json::from_str(
            &std::fs::read_to_string(std::env::var("Q357_SESSION_JSON").unwrap()).unwrap(),
        )
        .unwrap();
        let ledger = crate::agents::platform_extensions::ledger::parse_entries(
            &std::fs::read_to_string(std::env::var("Q357_LEDGER").unwrap()).unwrap(),
        );
        let newest = "Run the plan on the fake data. Give me totals per decision, and list every \
                      project lead who would have been skipped without the lead rule.";
        let kept = Pillars::build(
            &messages,
            &KeptSources {
                preserved: Some(newest),
                ..Default::default()
            },
            LedgerRead::Entries(ledger),
        )
        .fit(crate::context_mgmt::kept_budget_chars(Some(178_176)));
        let block = kept.render();
        println!("{block}");
        println!(
            "kept block: {} chars; P1 {} messages ({} cut), P2 {} files, P3 {} failed calls \
             ({} left out), P5 {} ledger entries",
            block.chars().count(),
            kept.asked.len(),
            kept.asked.iter().filter(|a| a.cut).count(),
            kept.files.len(),
            kept.failed.len(),
            kept.failed_left_out,
            match &kept.ledger {
                LedgerRead::Entries(e) => e.len(),
                LedgerRead::Unreadable(_) => 0,
            }
        );
        let facts = [
            ("the 24-month rule", "no login in 24 months"),
            ("the lead exception", "project leads must never be dropped"),
            ("ISO dates", "ISO dates (YYYY-MM-DD)"),
            ("British spelling", "British spelling"),
            ("zero npm dependencies", "zero npm dependencies"),
            ("10/10 tests", "10 pass / 0 fail"),
        ];
        let missing: Vec<&str> = facts
            .iter()
            .filter(|(_, needle)| !block.contains(needle))
            .map(|(fact, _)| *fact)
            .collect();
        assert!(missing.is_empty(), "missing from P1–P5: {missing:?}");
        // The other two live only where code does not look: the seed in the generator's source
        // (the write at 770753), the missing row in goose's own prose (770839, 771058). They are
        // the model's part to carry (WRITTEN_PARTS: "the value chosen (a seed …)", results "as they
        // now stand") — said here, so no one mistakes them for kept.
        assert!(!block.contains("20260928"), "the seed is not in P1–P5");
        assert!(
            !block.contains("NO USER ROW") && !block.contains("no row"),
            "svc-edi's missing row is not in P1–P5"
        );
    }
}
