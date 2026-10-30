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
use std::path::{Component, Path, PathBuf};

pub const KEPT_OPEN: &str = "<kept-by-goose>";
pub const KEPT_CLOSE: &str = "</kept-by-goose>";

/// The first line of a summary written under a note: how the model read the note.
pub const NOTE_MARKER: &str = "NOTE";

/// How a stored summary opens when goose could not check the model's part of it (round 4): the
/// facts goose kept below it are its own and stand without it.
pub const FLAG_LEAD: &str = "[goose could not check this summary";

/// The five parts goose keeps, in the block's order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pillar {
    Asked,
    Files,
    Failed,
    Notes,
    Ledger,
}

impl Pillar {
    pub const ALL: [Pillar; 5] = [
        Pillar::Asked,
        Pillar::Files,
        Pillar::Failed,
        Pillar::Notes,
        Pillar::Ledger,
    ];
}

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

/// A file the person's messages name, kept as it reads on disk when goose compacts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document {
    pub path: String,
    pub text: String,
    pub chars: usize,
    /// Not kept whole (no room, or the window is unknown): only the path and the size are kept,
    /// and its binding lines while they fit (`key_lines`).
    pub left_out: bool,
    /// A left-out file's binding lines — its headings and the lines that say must, never, always,
    /// only, required, shall — as many as the block's room holds (round 4: every real-model
    /// summary of the Flash-Next run missed a rule its contract files carried).
    pub key_lines: Vec<String>,
}

/// A folder the conversation's tool calls worked in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkFolder {
    pub path: String,
    /// Calls that ran here: a shell command that opened with `cd` to it, or a call whose `cwd`
    /// argument named it.
    pub calls: usize,
    /// Files the `write`/`edit` calls changed under it.
    pub files: usize,
    /// The newest call that worked here, by its place among the conversation's calls.
    newest: usize,
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
    /// The person's newest message is not in `asked`: the compaction re-sends it word for word
    /// right after the summary (kept once, not twice).
    pub resent_after: bool,
    /// P2 — the folders the work happened in, newest first (Q-394: after #3r's compaction the
    /// model knew `scripts/build_report.js` but not the folder its commands had cd'ed into, and
    /// searched the home folder for it).
    pub work: Vec<WorkFolder>,
    /// The chat's own folder, said beside the work folders when the work happened elsewhere.
    pub chat_folder: Option<String>,
    /// P2 — every file the chat's `write`/`edit` calls changed, by its absolute path.
    pub files: Vec<WrittenFile>,
    /// What each of `files` holds now — its size and what it defines (round 4): after the
    /// Flash-Next compaction the model re-grepped its own modules to learn what it had built.
    /// `None` when neither the disk nor a whole write in the conversation has it.
    pub files_now: Vec<Option<super::facts::FileNow>>,
    /// P2 — the files the person's messages name, as they read now. Why (Flash-Next Gauntlet
    /// sb-7.2 stopped run, 2026-10-29): after the compaction 7 of the next 13 calls re-read
    /// `SB7-CONTRACT.md` (three chunks), `VISUAL-CONTRACT.md`, `STARTER.md` and
    /// `BROWSER-TESTING.md` — the documents the task named, read once each before it.
    pub documents: Vec<Document>,
    /// The newest messages the compaction keeps word for word after the summary.
    pub tail_messages: usize,
    /// P3 — the errors still open: failed calls no later success of the same call settled, oldest
    /// first; `failed_left_out` older ones did not fit; `failed_settled` were settled.
    pub failed: Vec<Failed>,
    pub failed_left_out: usize,
    pub failed_settled: usize,
    /// Failed shell calls that only looked (`ls`, `wc`, `test`, `grep` finding nothing): not
    /// errors, counted on one line (`facts::look_up_failures`).
    pub failed_looked: usize,
    /// What is still to do by the tool calls themselves (round 5): each error still open, the
    /// `TODO`/`FIXME` markers the written files carry, and the items the newest scratchpad left
    /// unchecked. Why: three real Flash-Next summaries (2026-10-30) dropped an open item the
    /// transcript showed (the framing check `fitcheck.py` crashed and never ran clean; run 2 said
    /// it "proved" the framing); the errors were listed, but nowhere as work still to do.
    pub todo: Vec<String>,
    /// The shell commands that ran, exited 0 and did more than look, newest last.
    pub worked: Vec<String>,
    /// The newest tool calls with their outcome, oldest first.
    pub calls: Vec<String>,
    /// The model-written part of each summary an earlier compaction stored, oldest first;
    /// `earlier_left_out` older ones did not fit. They are carried, never re-written.
    pub earlier: Vec<String>,
    pub earlier_left_out: usize,
    /// P4 — the person's note for this compaction and their pins, verbatim.
    pub note: Option<String>,
    pub pins: Vec<String>,
    /// P5 — the ledger, oldest first; `ledger_left_out` older entries did not fit.
    pub ledger: LedgerRead,
    pub ledger_left_out: usize,
}

/// The newest tool calls and the commands that worked a fitted block keeps before it cuts the
/// person's words.
const FIT_FLOOR_LINES: usize = 3;
/// The characters the newest earlier summary is cut to when the block cannot hold it whole.
const EARLIER_EXCERPT_CHARS: usize = 2_000;

/// What the kept block is built from besides the conversation.
#[derive(Debug, Clone, Default)]
pub struct KeptSources<'a> {
    pub note: Option<&'a str>,
    pub pins: &'a [String],
    /// The message the compaction appends after the summary word for word; not kept twice.
    pub preserved: Option<&'a str>,
    /// The session's folder: what the chat's tools resolve a relative path against.
    pub working_dir: Option<&'a Path>,
}

impl Pillars {
    pub fn build(messages: &[Message], sources: &KeptSources<'_>, ledger: LedgerRead) -> Self {
        let mut asked = asked_of(messages);
        let mut resent_after = false;
        if let (Some(preserved), Some(last)) = (sources.preserved, asked.last()) {
            if last.text == preserved.trim() {
                asked.pop();
                resent_after = true;
            }
        }
        let files = absolute_files(written_files(messages), sources.working_dir);
        let documents = named_documents(&asked, sources.preserved, sources.working_dir);
        let settled = super::facts::settled_failures(messages);
        let looked = super::facts::look_up_failures(messages);
        let failed = failed_of(messages);
        let failed_settled = failed.iter().filter(|(id, _)| settled.contains(id)).count();
        let failed_looked = failed
            .iter()
            .filter(|(id, _)| !settled.contains(id) && looked.contains(id))
            .count();
        let failed: Vec<Failed> = failed
            .into_iter()
            .filter(|(id, _)| !settled.contains(id) && !looked.contains(id))
            .map(|(_, failed)| failed)
            .collect();
        let texts: Vec<Option<(String, bool)>> = files
            .iter()
            .map(|file| super::facts::file_text_now(&file.path, messages, sources.working_dir))
            .collect();
        let todo = still_to_do(&failed, &files, &texts, messages);
        Self {
            resent_after,
            files_now: texts
                .iter()
                .map(|text| {
                    text.as_ref()
                        .map(|(text, from_disk)| super::facts::FileNow::of(text, *from_disk))
                })
                .collect(),
            todo,
            failed_settled,
            failed_looked,
            worked: super::facts::worked_commands(messages),
            calls: super::facts::last_calls(messages),
            earlier: super::facts::earlier_summaries(messages),
            earlier_left_out: 0,
            documents,
            tail_messages: 0,
            asked,
            work: work_folders(messages, sources.working_dir, &files),
            chat_folder: sources.working_dir.map(|dir| dir.display().to_string()),
            files,
            failed,
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
    /// said in the block itself. Then the named files' texts fill what room is left. `None`: the
    /// window is unknown; nothing is cut and no file's text is kept (only its path and size).
    pub fn fit(mut self, budget_chars: Option<usize>) -> Self {
        let Some(budget) = budget_chars else {
            return self;
        };
        while self.render().chars().count() > budget {
            if self.earlier.len() > 1 {
                self.earlier.remove(0);
                self.earlier_left_out += 1;
                continue;
            }
            if !self.failed.is_empty() {
                self.failed.remove(0);
                self.failed_left_out += 1;
                continue;
            }
            if self.todo.len() > FIT_FLOOR_LINES {
                self.todo.pop();
                continue;
            }
            if let LedgerRead::Entries(entries) = &mut self.ledger {
                if !entries.is_empty() {
                    entries.remove(0);
                    self.ledger_left_out += 1;
                    continue;
                }
            }
            if self.calls.len() > FIT_FLOOR_LINES {
                self.calls.remove(0);
                continue;
            }
            if self.worked.len() > FIT_FLOOR_LINES {
                self.worked.remove(0);
                continue;
            }
            if let Some(now) = self.files_now.iter_mut().find(|n| n.is_some()) {
                *now = None;
                continue;
            }
            // The person's middle messages are cut first: the first is the task and the newest
            // the request at hand.
            let newest = self.asked.len().saturating_sub(1);
            if let Some(middle) =
                self.asked.iter().enumerate().position(|(i, a)| {
                    i > 0 && i < newest && !a.cut && a.chars > RECORD_EXCERPT_CHARS
                })
            {
                let entry = &mut self.asked[middle];
                entry.text = excerpt(&entry.text);
                entry.cut = true;
                continue;
            }
            if let Some(earlier) = self.earlier.last_mut() {
                if earlier.chars().count() > EARLIER_EXCERPT_CHARS {
                    *earlier = format!(
                        "{}… (cut to fit)",
                        earlier
                            .chars()
                            .take(EARLIER_EXCERPT_CHARS)
                            .collect::<String>()
                    );
                    continue;
                }
            }
            if !self.calls.is_empty() {
                self.calls.remove(0);
                continue;
            }
            if !self.worked.is_empty() {
                self.worked.remove(0);
                continue;
            }
            if !self.earlier.is_empty() {
                self.earlier.remove(0);
                self.earlier_left_out += 1;
                continue;
            }
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
        self.load_documents(budget);
        self
    }

    /// The named files' texts take only the room the rest of the block leaves, in the order the
    /// person named them: a file that can be read again never pushes out a failed call, a ledger
    /// entry or the person's words.
    fn load_documents(&mut self, budget: usize) {
        for i in 0..self.documents.len() {
            let room = budget.saturating_sub(self.render().chars().count());
            if self.documents[i].chars > room.min(MAX_NAMED_DOCUMENT_BYTES) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&self.documents[i].path) else {
                continue;
            };
            let document = &mut self.documents[i];
            document.chars = text.chars().count();
            document.text = text;
            document.left_out = false;
            if self.render().chars().count() > budget {
                let document = &mut self.documents[i];
                document.text.clear();
                document.left_out = true;
            }
        }
        // The files left out keep their binding lines, while they fit.
        for i in 0..self.documents.len() {
            if !self.documents[i].left_out || self.documents[i].chars > MAX_KEY_LINE_SOURCE_BYTES {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&self.documents[i].path) else {
                continue;
            };
            for line in key_lines(&text) {
                self.documents[i].key_lines.push(line);
                if self.render().chars().count() > budget {
                    self.documents[i].key_lines.pop();
                    break;
                }
            }
        }
    }

    /// The `<kept-by-goose>` block stored after the model's summary.
    pub fn render(&self) -> String {
        let mut out = format!(
            "{KEPT_OPEN}\ngoose kept these word for word from the whole conversation — facts, not \
             a paraphrase.\n"
        );
        if !self.earlier.is_empty() || self.earlier_left_out > 0 {
            out.push_str(&format!(
                "\n## The summaries earlier compactions stored ({}, oldest first; the work went on after each)\n",
                self.earlier.len() + self.earlier_left_out
            ));
            if self.earlier_left_out > 0 {
                out.push_str(&format!(
                    "({} older summaries are not listed here)\n",
                    self.earlier_left_out
                ));
            }
            for (i, earlier) in self.earlier.iter().enumerate() {
                out.push_str(&format!(
                    "### Earlier summary {}\n{}\n",
                    i + 1,
                    earlier.trim()
                ));
            }
        }
        for pillar in Pillar::ALL {
            out.push_str(&self.section(pillar));
        }
        out.push_str(KEPT_CLOSE);
        out
    }

    /// One part of the block as it renders — empty when the part holds nothing.
    pub fn section(&self, pillar: Pillar) -> String {
        let mut out = String::new();
        match pillar {
            Pillar::Asked if !self.asked.is_empty() || self.resent_after => {
                out.push_str(&format!(
                    "\n## The person's messages ({}, oldest first)\n",
                    self.asked.len() + usize::from(self.resent_after)
                ));
                if self.resent_after {
                    out.push_str("(the newest is re-sent word for word right after this block)\n");
                }
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
            Pillar::Files
                if !self.files.is_empty()
                    || !self.work.is_empty()
                    || !self.documents.is_empty() =>
            {
                if !self.work.is_empty() {
                    out.push_str("\n## Where the work is (newest first)\n");
                    if let Some(sentence) = self.elsewhere() {
                        out.push_str(&format!("{sentence}\n"));
                    }
                    for folder in &self.work {
                        out.push_str(&format!("- {}\n", folder_line(folder)));
                    }
                }
                if !self.files.is_empty() {
                    out.push_str(&format!("\n## Files written ({})\n", self.files.len()));
                    for (i, file) in self.files.iter().enumerate() {
                        out.push_str(&format!("- {}\n", file_line(&file.path, file)));
                        if let Some(Some(now)) = self.files_now.get(i) {
                            out.push_str(&format!("    {}\n", now.line()));
                        }
                    }
                }
                if !self.documents.is_empty() {
                    out.push_str(&format!(
                        "\n## Files the person named ({}), as they read now\n",
                        self.documents.len()
                    ));
                    for document in &self.documents {
                        if document.left_out {
                            out.push_str(&format!(
                                "### {} (~{} characters, left out to fit; read it again only for the \
                                 lines you need)\n",
                                document.path, document.chars
                            ));
                            if !document.key_lines.is_empty() {
                                out.push_str("Its headings and binding lines, word for word:\n");
                                for line in &document.key_lines {
                                    out.push_str(&format!("  {line}\n"));
                                }
                            }
                        } else {
                            out.push_str(&format!(
                                "### {}\n{}\n",
                                document.path,
                                document.text.trim_end()
                            ));
                        }
                    }
                }
            }
            Pillar::Failed
                if !self.failed.is_empty()
                    || !self.todo.is_empty()
                    || self.failed_left_out > 0
                    || self.failed_settled > 0
                    || self.failed_looked > 0
                    || !self.worked.is_empty()
                    || !self.calls.is_empty() =>
            {
                if !self.todo.is_empty() {
                    out.push_str(&format!(
                        "\n## Still to do, by the tool calls ({}): goose's list; it holds even where \
                         a summary says otherwise\n",
                        self.todo.len()
                    ));
                    for item in &self.todo {
                        out.push_str(&format!("- {item}\n"));
                    }
                }
                if !self.failed.is_empty()
                    || self.failed_left_out > 0
                    || self.failed_settled > 0
                    || self.failed_looked > 0
                {
                    out.push_str(&format!(
                        "\n## Errors still open ({})\n",
                        self.failed.len() + self.failed_left_out
                    ));
                    if self.failed_settled > 0 {
                        out.push_str(&format!(
                            "({} earlier failed calls were settled: the same call succeeded later)\n",
                            self.failed_settled
                        ));
                    }
                    if self.failed_looked > 0 {
                        out.push_str(&format!(
                            "({} failed look-ups - ls, wc, test, grep and the like finding \
                             nothing - are not errors)\n",
                            self.failed_looked
                        ));
                    }
                    if self.failed_left_out > 0 {
                        out.push_str(&format!(
                            "({} earlier open errors are not listed here)\n",
                            self.failed_left_out
                        ));
                    }
                    for failed in &self.failed {
                        out.push_str(&format!("- {}\n", failed_line(failed)));
                    }
                }
                if !self.worked.is_empty() {
                    out.push_str("\n## Commands that worked (exit 0, newest last)\n");
                    for command in &self.worked {
                        out.push_str(&format!("- {command}\n"));
                    }
                }
                if !self.calls.is_empty() {
                    out.push_str(&format!(
                        "\n## The last {} tool calls (oldest first)\n",
                        self.calls.len()
                    ));
                    for call in &self.calls {
                        out.push_str(&format!("- {call}\n"));
                    }
                }
            }
            Pillar::Notes if self.note.is_some() || !self.pins.is_empty() => {
                out.push_str("\n## The person's notes\n");
                if let Some(note) = &self.note {
                    out.push_str(&format!("- Note for this compaction: {note}\n"));
                }
                for pin in &self.pins {
                    out.push_str(&format!("- Pinned: {pin}\n"));
                }
            }
            Pillar::Ledger => match &self.ledger {
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
            },
            _ => {}
        }
        out
    }

    /// One part's items as a person reads them (the Context tab): each message, file, failed call,
    /// note line or ledger entry.
    pub fn items(&self, pillar: Pillar) -> Vec<String> {
        match pillar {
            Pillar::Asked => self.asked.iter().map(|a| a.text.clone()).collect(),
            Pillar::Files => self
                .elsewhere()
                .into_iter()
                .chain(
                    self.work
                        .iter()
                        .map(|folder| format!("Where the work is: {}", folder_line(folder))),
                )
                .chain(self.files.iter().map(|f| file_line(&f.path, f)))
                .chain(self.documents.iter().map(document_line))
                .collect(),
            Pillar::Failed => self.failed.iter().map(failed_line).collect(),
            Pillar::Notes => self
                .note
                .iter()
                .cloned()
                .chain(self.pins.iter().cloned())
                .collect(),
            Pillar::Ledger => match &self.ledger {
                LedgerRead::Entries(entries) => entries.clone(),
                LedgerRead::Unreadable(_) => Vec::new(),
            },
        }
    }

    /// What goose keeps, as the summary instruction names it — this chat's counts and names, so
    /// the model knows what it need not repeat.
    pub fn kept_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        if !self.work.is_empty() {
            let folders: Vec<&str> = self.work.iter().map(|f| f.path.as_str()).collect();
            lines.push(format!(
                "the {} the work happened in, newest first: {}",
                counted(folders.len(), "folder", "folders"),
                folders.join(", ")
            ));
        }
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
        let whole: Vec<&str> = self
            .documents
            .iter()
            .filter(|d| !d.left_out)
            .map(|d| file_name(&d.path))
            .collect();
        if !whole.is_empty() {
            lines.push(format!(
                "the whole text of the {} the person named: {}",
                counted(whole.len(), "file", "files"),
                whole.join(", ")
            ));
        }
        if self.tail_messages > 0 {
            lines.push(format!(
                "the newest {} of this conversation, word for word after your summary",
                counted(self.tail_messages, "message", "messages")
            ));
        }
        if self.files_now.iter().any(Option::is_some) {
            lines.push("what each written file defines now, and its size".to_string());
        }
        let failed = self.failed.len() + self.failed_left_out;
        if failed > 0 {
            lines.push(format!(
                "the {} still failing, with the errors",
                counted(failed, "tool call", "tool calls")
            ));
        }
        if !self.worked.is_empty() {
            lines.push(format!(
                "the {} that worked",
                counted(self.worked.len(), "command", "commands")
            ));
        }
        if !self.calls.is_empty() {
            lines.push(format!(
                "the newest {} with their outcome",
                counted(self.calls.len(), "tool call", "tool calls")
            ));
        }
        if !self.earlier.is_empty() {
            lines.push(format!(
                "the {} earlier compactions stored, word for word",
                counted(self.earlier.len(), "summary", "summaries")
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

    /// Said when a call worked in a folder other than the chat's own: #3r's chat folder was the
    /// home folder and every command cd'ed into the project's `work` folder first, so its relative
    /// paths were that folder's.
    fn elsewhere(&self) -> Option<String> {
        let chat = self.chat_folder.as_deref()?;
        if !self.work.iter().any(|f| f.calls > 0 && f.path != chat) {
            return None;
        }
        Some(format!(
            "The chat's own folder is {chat}, but commands worked in the folders below: a relative \
             path a command used is relative to the folder it worked in, not to the chat's."
        ))
    }
}

fn document_line(document: &Document) -> String {
    if document.left_out {
        format!(
            "{} (~{} characters, left out to fit)",
            document.path, document.chars
        )
    } else {
        format!("{} ({} chars, kept whole)", document.path, document.chars)
    }
}

/// bytes: the largest named file whose binding lines are read when it is not kept whole.
const MAX_KEY_LINE_SOURCE_BYTES: usize = 1024 * 1024;

/// Words that make a line of a brief binding.
const BINDING_WORDS: [&str; 7] = [
    "must", "never", "always", "only", "required", "shall", "exactly",
];

/// A document's headings and binding lines, in order, each once and cut to a line's length.
fn key_lines(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let heading = line.starts_with('#');
        let binding = line
            .split(|c: char| !c.is_alphanumeric())
            .any(|word| BINDING_WORDS.contains(&word.to_lowercase().as_str()));
        if !(heading || binding) {
            continue;
        }
        let line = super::facts::clip(line, super::facts::LINE_CHARS);
        if !lines.contains(&line) {
            lines.push(line);
        }
    }
    lines
}

/// bytes: the largest named file whose text the kept block carries; a bigger one is named by its
/// path and size only. A task's documents run to a few thousand words (the Flash-Next run's
/// `SB7-CONTRACT.md`, `BROWSER-TESTING.md`); a file past 64 KiB is data, not a brief.
pub const MAX_NAMED_DOCUMENT_BYTES: usize = 64 * 1024;

/// Name parts that mark a file as holding secrets: such a file is never read into the block,
/// which is sent to the provider. Matched against the file's and its folders' names split at
/// `.`, `_` and `-`, ignoring case: `prod.env`, `secrets.yaml`, `key.pem`, `terraform.tfstate`,
/// `id_rsa`, `aws-credentials.json`.
const SECRET_NAME_PARTS: [&str; 34] = [
    "env",
    "envrc",
    "secret",
    "secrets",
    "credential",
    "credentials",
    "creds",
    "password",
    "passwords",
    "passwd",
    "token",
    "tokens",
    "apikey",
    "apikeys",
    "private",
    "pem",
    "key",
    "keys",
    "p12",
    "pfx",
    "jks",
    "keystore",
    "tfstate",
    "tfvars",
    "kdbx",
    "gpg",
    "pgp",
    "asc",
    "ovpn",
    "htpasswd",
    "netrc",
    "kubeconfig",
    "rsa",
    "ed25519",
];

/// Whether a written file's path names a secret-holding file: its text is then never read for the
/// facts. The rule is about NAMES, never a location: only the part of the path BELOW the folder
/// the machine or the session chose is judged — the chat's folder, the OS temp folder, `/tmp`,
/// the home folder (each raw and canonical). Round 6 (2026-10-30): the check ran on the whole
/// absolute path and "private" is a secret word, so every file under macOS `/private/tmp` or
/// `/private/var/folders/…/T` — where every benchmark workspace lives (`/private/tmp/gfix`) — was
/// never read from disk, and the facts fell back to the conversation's last write of it.
pub(crate) fn is_secret_like_path(path: &Path, working_dir: Option<&Path>) -> bool {
    is_secret_like(below_chosen_folder(path, working_dir))
}

/// `path` less the longest of the folders [`is_secret_like_path`] does not judge that it sits
/// under; the whole path when it sits under none of them.
fn below_chosen_folder<'a>(path: &'a Path, working_dir: Option<&Path>) -> &'a Path {
    let mut roots: Vec<PathBuf> = working_dir.map(Path::to_path_buf).into_iter().collect();
    roots.push(std::env::temp_dir());
    roots.push(PathBuf::from("/tmp"));
    roots.extend(dirs::home_dir());
    let canonical: Vec<PathBuf> = roots.iter().filter_map(|r| r.canonicalize().ok()).collect();
    roots
        .into_iter()
        .chain(canonical)
        // The root folder itself is never a chosen folder: the whole path is judged then.
        .filter(|root| root.components().count() > 1)
        .filter_map(|root| path.strip_prefix(&root).ok().map(Path::components))
        .min_by_key(|rest| rest.clone().count())
        .map_or(path, |rest| rest.as_path())
}

fn is_secret_like(inside: &Path) -> bool {
    inside.components().any(|component| {
        component
            .as_os_str()
            .to_string_lossy()
            .to_lowercase()
            .split(['.', '_', '-'])
            .any(|part| SECRET_NAME_PARTS.contains(&part))
    })
}

/// Words that, shortly before a file name in the same sentence, make it a file the person does
/// NOT want used ("don't read prod.env", "ignore legacy.md", "without the old SPEC.md").
const NEGATIONS: [&str; 12] = [
    "not",
    "no",
    "never",
    "nor",
    "ignore",
    "skip",
    "without",
    "except",
    "avoid",
    "exclude",
    "excluding",
    "dont",
];

/// Whether the words before a file name (`before`, nearest last) negate it: a negation among the
/// four nearest, in the same sentence.
fn negated(before: &[&str]) -> bool {
    for word in before.iter().rev().take(4) {
        if word.ends_with(['.', '!', '?', ';']) {
            return false;
        }
        let bare: String = word
            .chars()
            .filter(|c| c.is_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect();
        if NEGATIONS.contains(&bare.as_str())
            || (bare.ends_with("nt") && word.contains(['\'', '’']))
        {
            return true;
        }
    }
    false
}

/// The files under the chat's folder that the person's messages name as task inputs — by a path
/// relative to it or an absolute one inside it — in the order first named, with their size on
/// disk; `fit` reads the ones that fit, up to [`MAX_NAMED_DOCUMENT_BYTES`] each. The block is sent
/// to the provider, so a file is never read when it is hidden (`.env`, `.git/…`), outside the
/// folder, secret-like by name ([`is_secret_like`]: `prod.env`, `key.pem`, `terraform.tfstate`),
/// or named in a negation ("don't touch secrets.yaml").
fn named_documents(
    asked: &[Asked],
    preserved: Option<&str>,
    working_dir: Option<&Path>,
) -> Vec<Document> {
    let Some(folder) = working_dir.and_then(|dir| dir.canonicalize().ok()) else {
        return Vec::new();
    };
    let mut seen = std::collections::HashSet::new();
    let mut documents = Vec::new();
    for text in asked.iter().map(|a| a.text.as_str()).chain(preserved) {
        let words: Vec<&str> = text.split_whitespace().collect();
        for (at, word) in words.iter().enumerate() {
            let word = word
                .trim_matches(|c: char| {
                    matches!(
                        c,
                        '`' | '"' | '\'' | '(' | ')' | '[' | ']' | ',' | ';' | ':' | '<' | '>'
                    )
                })
                .trim_end_matches('.');
            if !word.contains('.') || word.contains("://") || word.starts_with('-') {
                continue;
            }
            if negated(&words[..at]) {
                continue;
            }
            let Ok(path) = folder.join(word).canonicalize() else {
                continue;
            };
            let Ok(inside) = path.strip_prefix(&folder) else {
                continue;
            };
            let hidden = inside
                .components()
                .any(|c| c.as_os_str().to_string_lossy().starts_with('.'));
            let Ok(meta) = path.metadata() else {
                continue;
            };
            if hidden || is_secret_like(inside) || !meta.is_file() || !seen.insert(path.clone()) {
                continue;
            }
            documents.push(Document {
                path: path.display().to_string(),
                text: String::new(),
                chars: meta.len() as usize,
                left_out: true,
                key_lines: Vec::new(),
            });
        }
    }
    documents
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

fn folder_line(folder: &WorkFolder) -> String {
    let mut said = Vec::new();
    if folder.calls > 0 {
        said.push(format!(
            "{} ran here",
            counted(folder.calls, "call", "calls")
        ));
    }
    if folder.files > 0 {
        said.push(format!(
            "{} written under it",
            counted(folder.files, "file", "files")
        ));
    }
    format!("{} — {}", folder.path, said.join(", "))
}

/// Absolute to a POSIX shell (`/…`) or to the platform (a drive path on Windows).
fn is_absolute(path: &str) -> bool {
    path.starts_with('/') || Path::new(path).is_absolute()
}

/// A folder as the shell reads it: absolute as written, `~` as the home folder, otherwise joined to
/// `base`; `.` and `..` resolved. `None` when goose cannot name it: relative with no base, a
/// variable or command substitution, `cd -`.
fn resolve_folder(path: &str, base: Option<&Path>) -> Option<PathBuf> {
    if path.is_empty() || path == "-" || path.contains(['$', '`']) {
        return None;
    }
    let joined = if path == "~" {
        dirs::home_dir()?
    } else if let Some(rest) = path.strip_prefix("~/") {
        dirs::home_dir()?.join(rest)
    } else if is_absolute(path) {
        PathBuf::from(path)
    } else {
        base?.join(path)
    };
    let mut normal = PathBuf::new();
    for component in joined.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normal.pop();
            }
            other => normal.push(other),
        }
    }
    Some(normal)
}

/// The quoted shell word `text` opens with, quotes removed, and what follows it.
fn quoted_word(text: &str) -> Option<(&str, &str)> {
    let quote = text.chars().next().filter(|c| *c == '"' || *c == '\'')?;
    text.strip_prefix(quote)?.split_once(quote)
}

/// The folder a shell command works in when it opens with `cd` — `cd A && cd B && …` is B joined
/// to A. `None` when it opens with no `cd` to a folder goose can name.
fn cd_folder(command: &str, base: Option<&Path>) -> Option<PathBuf> {
    let mut rest = command.trim_start();
    let mut folder: Option<PathBuf> = None;
    while let Some(after) = rest
        .strip_prefix("cd")
        .filter(|after| after.starts_with([' ', '\t']))
    {
        let after = after.trim_start();
        let (target, tail) = quoted_word(after).unwrap_or_else(|| {
            after
                .find(|c: char| c.is_whitespace() || matches!(c, ';' | '&' | '|'))
                .map_or((after, ""), |end| after.split_at(end))
        });
        let tail = tail.trim_start_matches([' ', '\t']);
        let next = ["&&", "||", ";", "\n"]
            .iter()
            .find_map(|separator| tail.strip_prefix(separator));
        if next.is_none() && !tail.is_empty() {
            break;
        }
        folder = Some(resolve_folder(target, folder.as_deref().or(base))?);
        rest = next.unwrap_or("").trim_start();
    }
    folder
}

type Arguments = serde_json::Map<String, serde_json::Value>;

/// The folder a call says it runs in: a `cwd`-style argument, and the `cd` its shell command opens
/// with, taken from there.
fn call_folder(arguments: Option<&Arguments>, base: Option<&Path>) -> Option<PathBuf> {
    let arguments = arguments?;
    let named = ["cwd", "working_dir", "workdir"]
        .iter()
        .find_map(|key| arguments.get(*key).and_then(|v| v.as_str()))
        .and_then(|dir| resolve_folder(dir, base));
    let cd = arguments
        .get("command")
        .and_then(|command| command.as_str())
        .and_then(|command| cd_folder(command, named.as_deref().or(base)));
    cd.or(named)
}

/// The written files, every path absolute: a `write`/`edit` result records the path it wrote,
/// relative only when the call carried no folder — and then it is the session's folder's.
fn absolute_files(mut files: Vec<WrittenFile>, working_dir: Option<&Path>) -> Vec<WrittenFile> {
    for file in &mut files {
        if !is_absolute(&file.path) {
            if let Some(path) = resolve_folder(&file.path, working_dir) {
                file.path = path.display().to_string();
            }
        }
    }
    files
}

fn is_under(path: &str, folder: &str) -> bool {
    path.strip_prefix(folder)
        .is_some_and(|rest| rest.starts_with(['/', '\\']))
}

/// The folders the conversation's calls worked in, newest first: each folder a call ran in (`cd`,
/// `cwd`), and — for a written file under none of them — the file's own folder. A file counts
/// under the deepest such folder that holds it.
fn work_folders(
    messages: &[Message],
    working_dir: Option<&Path>,
    files: &[WrittenFile],
) -> Vec<WorkFolder> {
    let calls: Vec<(&str, Option<&Arguments>)> = messages
        .iter()
        .flat_map(|m| m.content.iter())
        .filter_map(|c| match c {
            MessageContent::ToolRequest(req) => req.tool_call.as_ref().ok(),
            _ => None,
        })
        .map(|call| (call.name.as_ref(), call.arguments.as_ref()))
        .collect();
    let mut folders: Vec<WorkFolder> = Vec::new();
    for (position, (_, arguments)) in calls.iter().enumerate() {
        let Some(folder) = call_folder(*arguments, working_dir) else {
            continue;
        };
        let path = folder.display().to_string();
        match folders.iter_mut().find(|f| f.path == path) {
            Some(known) => {
                known.calls += 1;
                known.newest = position;
            }
            None => folders.push(WorkFolder {
                path,
                calls: 1,
                files: 0,
                newest: position,
            }),
        }
    }
    let written_at = |file: &WrittenFile| {
        calls.iter().rposition(|(name, arguments)| {
            matches!(*name, "write" | "edit")
                && arguments
                    .and_then(|a| a.get("path"))
                    .and_then(|p| p.as_str())
                    .and_then(|p| resolve_folder(p, working_dir))
                    .is_some_and(|p| p.display().to_string() == file.path)
        })
    };
    for file in files {
        let holder = folders
            .iter()
            .enumerate()
            .filter(|(_, f)| is_under(&file.path, &f.path))
            .max_by_key(|(_, f)| f.path.len())
            .map(|(i, _)| i);
        let index = match holder {
            Some(i) => i,
            None => {
                let Some(parent) = Path::new(&file.path).parent() else {
                    continue;
                };
                let path = parent.display().to_string();
                match folders.iter().position(|f| f.path == path) {
                    Some(i) => i,
                    None => {
                        folders.push(WorkFolder {
                            path,
                            calls: 0,
                            files: 0,
                            newest: 0,
                        });
                        folders.len() - 1
                    }
                }
            }
        };
        let folder = &mut folders[index];
        folder.files += 1;
        if let Some(at) = written_at(file) {
            folder.newest = folder.newest.max(at);
        }
    }
    folders.sort_by_key(|f| std::cmp::Reverse(f.newest));
    folders
}

/// The "Still to do" list ([`Pillars::todo`]): open errors first (each a call to make succeed),
/// then the written files' markers, then the scratchpad's unchecked items; at most
/// [`super::facts::STILL_TO_DO`].
fn still_to_do(
    failed: &[Failed],
    files: &[WrittenFile],
    texts: &[Option<(String, bool)>],
    messages: &[Message],
) -> Vec<String> {
    let errors = failed.iter().map(|failed| {
        format!(
            "make `{}` succeed: it {} and no later run of it succeeded",
            super::facts::clip(&failed.call, 200),
            failed.outcome
        )
    });
    let markers = files
        .iter()
        .zip(texts)
        .filter_map(|(file, text)| text.as_ref().map(|(text, _)| (file, text)))
        .flat_map(|(file, text)| super::facts::todo_markers(&file.path, text));
    errors
        .chain(markers)
        .chain(super::facts::scratchpad_open_items(messages))
        .take(super::facts::STILL_TO_DO)
        .collect()
}

fn failed_line(failed: &Failed) -> String {
    if failed.output.is_empty() {
        format!("{} {}.", failed.call, failed.outcome)
    } else {
        format!("{} {}. {}", failed.call, failed.outcome, failed.output)
    }
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
fn failed_of(messages: &[Message]) -> Vec<(String, Failed)> {
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
            Ok(result)
                if result.is_error == Some(true)
                    || result
                        .structured_content
                        .as_ref()
                        .and_then(|s| s.get("exit_code"))
                        .and_then(serde_json::Value::as_i64)
                        .is_some_and(|code| code != 0) =>
            {
                outcome_and_output(result)
            }
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
        failed.push((
            response.id.clone(),
            Failed {
                call,
                outcome,
                output,
            },
        ));
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
        let Some(end) = text.get(start..).and_then(|rest| rest.find("</analysis>")) else {
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

/// The summary sections ([`super::WRITTEN_PARTS`]) the model has written so far, in order, by
/// their canonical headings.
pub fn written_parts(so_far: &str) -> Vec<String> {
    written_sections(so_far)
        .into_iter()
        .map(|(part, _)| super::WRITTEN_PARTS[part].0.to_string())
        .collect()
}

/// Which summary section a line heads, and the words after the heading on the same line. Read
/// leniently (2026-10-30: real summaries head their sections `## Where we are:`, `### Where we
/// are`, `**Where we are**`, `**Where we are:** sync done`, `1. Where we are:`): a markdown heading
/// of any level whose title is the section's, with a closing colon or emphasis; a line in emphasis
/// that opens with the title; a plain line that opens with the title and a colon. Case is ignored.
fn part_heading(line: &str) -> Option<(usize, &str)> {
    let line = line.trim();
    let hashes = line.chars().take_while(|c| *c == '#').count();
    let rest = line.get(hashes..).unwrap_or_default().trim_start();
    let rest = match rest.split_once(['.', ')']) {
        Some((number, after))
            if !number.is_empty() && number.chars().all(|c| c.is_ascii_digit()) =>
        {
            after.trim_start()
        }
        _ => rest,
    };
    let marks = rest.chars().take_while(|c| matches!(c, '*' | '_')).count();
    let rest = rest.get(marks..).unwrap_or_default();
    WRITTEN_PARTS_TITLES
        .iter()
        .enumerate()
        .find_map(|(part, title)| {
            let head = rest.get(..title.len())?;
            if !head.eq_ignore_ascii_case(title) {
                return None;
            }
            let after = rest.get(title.len()..).unwrap_or_default();
            let closing = after
                .chars()
                .take_while(|c| matches!(c, '*' | '_' | ':') || c.is_whitespace())
                .map(char::len_utf8)
                .sum::<usize>();
            let (closed, inline) = after.split_at(closing);
            let colon = closed.contains(':');
            let emphasis = marks > 0 && closed.trim_start().starts_with(['*', '_']);
            let accepted = if hashes > 0 {
                inline.is_empty()
            } else {
                emphasis || colon
            };
            // `Where we are now …` is a sentence, not the heading: the title ends at a separator.
            let separated = !closed.is_empty() || inline.is_empty();
            (accepted && separated).then_some((part, inline.trim()))
        })
}

const WRITTEN_PARTS_TITLES: [&str; super::WRITTEN_PARTS.len()] = {
    let mut titles = [""; super::WRITTEN_PARTS.len()];
    let mut i = 0;
    while i < titles.len() {
        titles[i] = super::WRITTEN_PARTS[i].0;
        i += 1;
    }
    titles
};

/// The summary sections in `text` (its scratch stripped), in order: each section's index in
/// [`super::WRITTEN_PARTS`] and the words written under it up to the next heading. A heading inside
/// a code fence is the fence's text, not a section; any other markdown heading ends a section.
pub fn written_sections(text: &str) -> Vec<(usize, String)> {
    let text = strip_scratch(text);
    let mut sections: Vec<(usize, String)> = Vec::new();
    let mut open = false;
    let mut fence: Option<&str> = None;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if let Some(marker) = fence {
            if trimmed.starts_with(marker) {
                fence = None;
            } else if let (true, Some((_, body))) = (open, sections.last_mut()) {
                body.push('\n');
                body.push_str(line);
            }
            continue;
        }
        if let Some(marker) = ["```", "~~~"].into_iter().find(|m| trimmed.starts_with(m)) {
            fence = Some(marker);
            continue;
        }
        if let Some((part, inline)) = part_heading(trimmed) {
            sections.push((part, inline.to_string()));
            open = true;
            continue;
        }
        if trimmed.starts_with('#') {
            open = false;
            continue;
        }
        if let (true, Some((_, body))) = (open, sections.last_mut()) {
            body.push('\n');
            body.push_str(line);
        }
    }
    sections
}

/// The stored summary: the model's text, its scratch gone, then the kept block. A FLAGGED summary
/// (round 5) is stored the other way round — goose's flag line, the kept block, then the model's
/// answer — so what the model reads first after the compaction is what goose knows, not what a
/// summary that failed a check says.
pub fn stored_summary(model_text: &str, kept: &Pillars, flag: Option<&str>) -> String {
    let written = strip_scratch(model_text);
    let written = written.trim();
    match flag {
        Some(why) => {
            let lead = format!(
                "{FLAG_LEAD}: {why}. goose's facts come first and are authoritative; the model's \
                 answer follows them as written — trust the facts over it, and read a file again \
                 before relying on what it holds.]"
            );
            if written.is_empty() {
                format!("{lead}\n\n{}", kept.render())
            } else {
                format!("{lead}\n\n{}\n\n{FLAGGED_ANSWER}\n{written}", kept.render())
            }
        }
        None if written.is_empty() => kept.render(),
        None => format!("{written}\n\n{}", kept.render()),
    }
}

/// What heads the model's answer after the kept block in a flagged summary.
pub const FLAGGED_ANSWER: &str = "The model's answer (flagged; goose's facts above win):";

/// The model-written part of a stored summary: what goose wrote before its kept block, or, in a
/// flagged one, goose's flag line and the model's answer after the block.
pub fn model_part(stored: &str) -> String {
    let Some((before, rest)) = stored.split_once(KEPT_OPEN) else {
        return stored.to_string();
    };
    let before = before.trim_end();
    if !before.starts_with(FLAG_LEAD) {
        return before.to_string();
    }
    let after = rest
        .split_once(KEPT_CLOSE)
        .map_or("", |(_, after)| after)
        .trim();
    let after = after.strip_prefix(FLAGGED_ANSWER).unwrap_or(after).trim();
    if after.is_empty() {
        before.to_string()
    } else {
        format!("{before}\n\n{after}")
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
                working_dir: None,
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
            "## Errors still open (1)\n- shell {\"command\":\"node plan.js\"} failed (exit 1). \
             Output: \"SyntaxError: Unexpected token\"\n"
        ));
        assert!(block
            .contains("- Note for this compaction: keep the lead rule\n- Pinned: seed 20260928\n"));
        assert!(block.contains(
            "## Ledger (1 entries, oldest first)\n- 2026-09-28 12:20 [fact] 10 pass / 0 fail\n"
        ));
    }

    const WORK: &str = "/Users/someone/goose-builds/quality/RU-3r/work";

    fn written(id: &str, path: &str, content: &str) -> [Message; 2] {
        [
            call(
                id,
                "write",
                serde_json::json!({"path": path, "content": content}),
            ),
            Message::user().with_tool_response(
                id,
                Ok(with_file_diff(
                    text_result("Created"),
                    path,
                    Before::None,
                    &diff_texts(path, None, content),
                )),
            ),
        ]
    }

    fn ran(id: &str, command: &str) -> [Message; 2] {
        [
            call(id, "shell", serde_json::json!({"command": command})),
            Message::user().with_tool_response(id, Ok(text_result("ok"))),
        ]
    }

    /// E2E #3r (Q-394): the chat's folder was the home folder, every command opened with
    /// `cd <run>/work && …` and named its files relative to it, the writes landed under it. After
    /// the compaction the model knew `scripts/build_report.js` but not where, and searched the home
    /// folder three levels deep for a file six levels down. The kept block names the folder, says
    /// it is not the chat's, and gives every written file whole.
    #[test]
    fn a_3r_shaped_chat_keeps_the_folder_the_work_is_in() {
        let mut messages = vec![Message::user().with_text("build the readiness report")];
        messages.extend(ran("s1", &format!("cd {WORK} && ls -la")));
        messages.extend(written(
            "w1",
            &format!("{WORK}/scripts/build_report.js"),
            "const out = 'report/readiness-assessment.docx';\n",
        ));
        messages.extend(written(
            "w2",
            &format!("{WORK}/lib/identityRules.js"),
            "module.exports = {};\n",
        ));
        messages.extend(ran(
            "s2",
            &format!("cd {WORK} && node scripts/build_report.js > report/run.log"),
        ));
        messages.extend(ran(
            "s3",
            "cd /Users/someone/goose-builds/quality/RU-3r && ls work/",
        ));
        messages.extend(ran("s4", "curl -s https://support.atlassian.com/"));
        messages.extend(ran(
            "s5",
            &format!("cd \"{WORK}\" && python3 - <<'PY'\nprint(1 > 0)\nPY"),
        ));
        messages.push(Message::user().with_text("make a PDF too: report/readiness-assessment.pdf"));

        let home = Path::new("/Users/someone");
        let kept = Pillars::build(
            &messages,
            &KeptSources {
                working_dir: Some(home),
                ..Default::default()
            },
            entries(&[]),
        );
        let block = kept.render();
        assert!(
            block.contains(&format!(
                "## Where the work is (newest first)\nThe chat's own folder is /Users/someone, but \
                 commands worked in the folders below: a relative path a command used is relative \
                 to the folder it worked in, not to the chat's.\n- {WORK} — 3 calls ran here, 2 \
                 files written under it\n- /Users/someone/goose-builds/quality/RU-3r — 1 call ran \
                 here\n"
            )),
            "{block}"
        );
        assert!(
            block.contains(&format!(
                "## Files written (2)\n- {WORK}/scripts/build_report.js — created, +1 −0 lines, 1 \
                 write\n"
            )),
            "{block}"
        );
        assert!(kept.kept_lines()[0].starts_with(&format!(
            "the 2 folders the work happened in, newest first: {WORK}, "
        )));
        assert_eq!(
            kept.items(Pillar::Files)[1],
            format!("Where the work is: {WORK} — 3 calls ran here, 2 files written under it")
        );
    }

    /// A folder is said only when goose can name it: `cd` chains resolve, a relative `cd` is the
    /// session folder's, a `cwd` argument counts; a variable, `cd -` or a `cd` that is not the
    /// command's first step name nothing. A write recorded with a relative path (a call that
    /// carried no folder) is the session folder's.
    #[test]
    fn the_folders_are_the_ones_the_calls_name() {
        let base = Some(Path::new("/home/me"));
        let cd = |command: &str| cd_folder(command, base).map(|p| p.display().to_string());
        assert_eq!(cd("cd /a && cd b && make").as_deref(), Some("/a/b"));
        assert_eq!(cd("cd proj; ls").as_deref(), Some("/home/me/proj"));
        assert_eq!(cd("cd '/x y/z' || exit 1").as_deref(), Some("/x y/z"));
        assert_eq!(cd("cd /a/b/../c\nls").as_deref(), Some("/a/c"));
        assert_eq!(cd("cd /a").as_deref(), Some("/a"));
        assert_eq!(cd("cd $DIR && ls"), None);
        assert_eq!(cd("cd - && ls"), None);
        assert_eq!(cd("ls && cd /a"), None);
        assert_eq!(cd("cdx /a && ls"), None);
        assert_eq!(cd("cd /a b && ls"), None);

        let args = serde_json::json!({"command": "cd sub && ls", "cwd": "/srv/app"});
        assert_eq!(
            call_folder(args.as_object(), base),
            Some(PathBuf::from("/srv/app/sub"))
        );

        let mut messages = vec![Message::user().with_text("write it")];
        messages.extend(written("w1", "notes/plan.md", "x\n"));
        let kept = Pillars::build(
            &messages,
            &KeptSources {
                working_dir: base,
                ..Default::default()
            },
            entries(&[]),
        );
        assert_eq!(kept.files[0].path, "/home/me/notes/plan.md");
        assert_eq!(kept.work[0].path, "/home/me/notes");
        assert!(
            !kept.render().contains("The chat's own folder"),
            "no command worked outside the chat's folder: its relative paths are the chat's"
        );
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
        let mut messages = vec![
            Message::user().with_text("the task"),
            Message::user().with_text(&long),
        ];
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
            .contains("(1 earlier open errors are not listed here)"));

        let tight = kept.clone().fit(Some(whole - 2 * RECORD_EXCERPT_CHARS));
        assert_eq!(tight.ledger_left_out, 2);
        assert!(!tight.asked[0].cut, "the task stays whole");
        assert!(tight.asked[1].cut && tight.asked[1].chars == long.chars().count());
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
        let stored = stored_summary(
            "<analysis>a</analysis>NOTE OK\n## Where we are\nx",
            &kept,
            None,
        );
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

    /// E2E #3r (session 20260928_21, working_dir /Users/mihaiperdum): the compaction that stored
    /// message 772237 read messages 771786..772236. Replayed from a read-only export of those
    /// messages: `Q394_SESSION_JSON=/tmp/q394/s21.json`. The block must name the run's `work`
    /// folder first — the folder the model searched the home folder for after that compaction.
    #[test]
    #[ignore = "reads a local export of E2E #3r's session"]
    fn e2e_3r_the_kept_block_names_the_work_folder() {
        let messages: Vec<Message> = serde_json::from_str(
            &std::fs::read_to_string(std::env::var("Q394_SESSION_JSON").unwrap()).unwrap(),
        )
        .unwrap();
        let newest = "Aoife reads everything on her iPad, so make a PDF too: \
                      report/readiness-assessment.pdf, from the docx, using whatever works on this \
                      Mac. Then open the PDF and tell me how many pages it has and what's on the \
                      last page.";
        let kept = Pillars::build(
            &messages,
            &KeptSources {
                preserved: Some(newest),
                working_dir: Some(Path::new("/Users/mihaiperdum")),
                ..Default::default()
            },
            LedgerRead::Entries(Vec::new()),
        )
        .fit(crate::context_mgmt::kept_budget_chars(Some(178_176)));
        println!("{}", kept.section(Pillar::Files));
        println!("kept lines: {:#?}", kept.kept_lines());
        let work = "/Users/mihaiperdum/goose-builds/quality/RU-2026-09-28-3r-split-tensor/work";
        assert_eq!(kept.work[0].path, work);
        let block = kept.render();
        assert!(block.contains("The chat's own folder is /Users/mihaiperdum, but"));
        assert!(block.contains(&format!("- {work}/scripts/build_report.js — ")));
        assert!(kept.kept_lines()[0].contains(work));
    }

    /// The files the task names are kept whole from disk when they fit the room left; a hidden
    /// file or one outside the chat's folder is never read; an unknown window keeps paths only.
    #[test]
    fn the_files_the_person_named_are_kept_whole_when_they_fit() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("SB7-CONTRACT.md"),
            "# Contract\nsync 12,288",
        )
        .unwrap();
        std::fs::write(dir.path().join(".env"), "SECRET=1").unwrap();
        let asked = Message::user().with_text(
            "`SB7-CONTRACT.md` is the contract; never print .env or /etc/hosts, see https://x.io/a.md",
        );
        let build = |budget| {
            Pillars::build(
                std::slice::from_ref(&asked),
                &KeptSources {
                    working_dir: Some(dir.path()),
                    ..Default::default()
                },
                LedgerRead::Entries(Vec::new()),
            )
            .fit(budget)
        };
        let kept = build(Some(100_000));
        assert_eq!(kept.documents.len(), 1, "{:?}", kept.documents);
        let rendered = kept.render();
        assert!(rendered.contains("# Contract\nsync 12,288"), "{rendered}");
        assert!(!rendered.contains("SECRET") && !rendered.contains("localhost"));
        assert!(kept
            .kept_lines()
            .iter()
            .any(|l| l.contains("SB7-CONTRACT.md")));
        let tight = build(Some(kept.render().chars().count() - 5));
        assert!(tight.documents[0].left_out && !tight.render().contains("sync 12,288"));
        assert!(build(None).documents[0].left_out);
    }

    /// Round 4: a named contract too big to keep whole keeps its headings and binding lines while
    /// they fit — every real-model summary of the Flash-Next run missed that framing "must go
    /// through the camera", a rule its contract carried.
    #[test]
    fn a_named_file_left_out_keeps_its_binding_lines() {
        let dir = tempfile::tempdir().unwrap();
        let filler: String = (0..400)
            .map(|i| format!("Line {i} describes the field in some detail.\n"))
            .collect();
        std::fs::write(
            dir.path().join("VISUAL-CONTRACT.md"),
            format!("# Visual contract\n{filler}Framing MUST go through the camera, never the layout.\n{filler}"),
        )
        .unwrap();
        let asked = Message::user().with_text("VISUAL-CONTRACT.md amends the 3D field");
        let kept = Pillars::build(
            std::slice::from_ref(&asked),
            &KeptSources {
                working_dir: Some(dir.path()),
                ..Default::default()
            },
            LedgerRead::Entries(Vec::new()),
        )
        .fit(Some(2_000));
        assert!(kept.documents[0].left_out);
        let rendered = kept.render();
        assert!(
            rendered.contains("  # Visual contract\n")
                && rendered.contains("  Framing MUST go through the camera, never the layout.\n"),
            "{rendered}"
        );
        assert!(!rendered.contains("Line 7 describes"), "{rendered}");
        assert!(rendered.chars().count() <= 2_000);
    }

    /// Defect 7 (review 2026-10-30): only dot-paths were skipped, so a named `prod.env`,
    /// `secrets.yaml`, `key.pem` or `terraform.tfstate` was read into the block sent to the
    /// provider. Secret-like names, files named in a negation, and files over the size cap are
    /// never read.
    #[test]
    fn a_secret_like_or_negated_file_is_never_read() {
        let dir = tempfile::tempdir().unwrap();
        for name in [
            "prod.env",
            "secrets.yaml",
            "key.pem",
            "terraform.tfstate",
            "id_rsa.txt",
            "aws-credentials.json",
            "legacy.md",
        ] {
            std::fs::write(dir.path().join(name), format!("LEAKED {name}")).unwrap();
        }
        std::fs::write(dir.path().join("notes.md"), "# Notes\nthe brief").unwrap();
        std::fs::write(
            dir.path().join("big.md"),
            "y".repeat(MAX_NAMED_DOCUMENT_BYTES + 1),
        )
        .unwrap();
        let asked = Message::user().with_text(
            "use prod.env, secrets.yaml, key.pem, terraform.tfstate, id_rsa.txt, \
             aws-credentials.json, notes.md and big.md. Don't read legacy.md",
        );
        let kept = Pillars::build(
            std::slice::from_ref(&asked),
            &KeptSources {
                working_dir: Some(dir.path()),
                ..Default::default()
            },
            LedgerRead::Entries(Vec::new()),
        )
        .fit(Some(1_000_000));
        let named: Vec<&str> = kept.documents.iter().map(|d| file_name(&d.path)).collect();
        assert_eq!(named, ["notes.md", "big.md"]);
        let rendered = kept.render();
        assert!(!rendered.contains("LEAKED"), "{rendered}");
        assert!(rendered.contains("# Notes\nthe brief"));
        assert!(kept.documents[1].left_out, "over the size cap");
        assert!(!negated(&["please", "read"]) && negated(&["don't", "touch"]));
        assert!(!negated(&["never.", "Then", "read"]), "a sentence ends");
    }

    /// Round 6 (2026-10-30, verifier's FIX4 defect 1): the secret check ran on the WHOLE absolute
    /// path, and "private" is a secret word, so every file of a benchmark workspace — macOS keeps
    /// them under `/private/tmp` (the Gauntlet's `/private/tmp/gfix`) — was never read from disk.
    /// A real benchmark workdir: under `/tmp`, its own name holding secret words. The contract the
    /// task names is kept whole, a written file is read from disk (a shell changed it after the
    /// write), and secret NAMES below the folder are still refused.
    #[test]
    fn a_benchmark_workdir_under_private_tmp_is_read_but_secret_names_are_not() {
        let dir = tempfile::Builder::new()
            .prefix("gfix-private-token-")
            .tempdir_in("/tmp")
            .unwrap();
        let work = dir.path().canonicalize().unwrap();
        assert!(work.starts_with("/private/tmp"), "{}", work.display());
        std::fs::write(
            work.join("SB7-CONTRACT.md"),
            "# Contract\nframing must go through the camera",
        )
        .unwrap();
        std::fs::write(work.join("prod.env"), "KEY=1").unwrap();
        std::fs::create_dir(work.join("config")).unwrap();
        let app = work.join("app.js");
        let creds = work.join("config/credentials.json");
        std::fs::write(
            &app,
            "export function start() {}\nexport function stop() {}\n",
        )
        .unwrap();
        std::fs::write(&creds, "{\"token\": \"abc\"}").unwrap();
        let mut messages =
            vec![Message::user().with_text("Read SB7-CONTRACT.md and prod.env, then build app.js")];
        messages.extend(written(
            "w1",
            &app.display().to_string(),
            "export function start() {}\n",
        ));
        messages.extend(written("w2", &creds.display().to_string(), "{}"));
        for working_dir in [dir.path(), work.as_path()] {
            let kept = Pillars::build(
                &messages,
                &KeptSources {
                    working_dir: Some(working_dir),
                    ..Default::default()
                },
                LedgerRead::Entries(Vec::new()),
            )
            .fit(Some(100_000));
            let rendered = kept.render();
            assert!(
                rendered.contains("framing must go through the camera"),
                "{rendered}"
            );
            assert!(!rendered.contains("KEY=1"), "{rendered}");
            assert!(
                kept.documents.iter().all(|d| !d.path.ends_with("prod.env")),
                "{:?}",
                kept.documents
            );
            let app_now = kept.files_now[0].as_ref().expect("app.js");
            assert!(app_now.from_disk, "read from disk: {app_now:?}");
            assert_eq!(app_now.lines, 2, "the shell's change on disk: {app_now:?}");
            let creds_now = kept.files_now[1].as_ref().expect("credentials.json");
            assert!(
                !creds_now.from_disk,
                "a secret name is never read: {creds_now:?}"
            );
        }
        // Outside the chat's folder, under /tmp: judged by the names below /tmp only.
        assert!(!is_secret_like_path(
            Path::new("/private/tmp/gfix/app.js"),
            None
        ));
        assert!(!is_secret_like_path(
            Path::new("/tmp/gfix/web/index.html"),
            None
        ));
        assert!(is_secret_like_path(
            Path::new("/private/tmp/gfix/config/credentials.json"),
            None
        ));
        assert!(is_secret_like_path(&creds, Some(dir.path())));
        assert!(
            is_secret_like_path(Path::new("/private/etc/app.conf"), None),
            "no chosen folder: the whole path is judged"
        );
    }
}
