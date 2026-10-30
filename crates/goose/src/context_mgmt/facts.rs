//! Round 4 (2026-10-30): the facts goose builds itself from the conversation and stores beside
//! EVERY model-written summary, so no summary — however thin — is the only memory of the work.
//! Beside the kept block's person's words, folders, files, notes and ledger ([`super::pillars`]):
//! what each written file holds now (its size and its functions, exports and routes), the newest
//! tool calls with their outcome, the errors still open, the commands that worked, and the
//! summaries earlier compactions stored.
//!
//! Why: the round-3 gate tried to tell a real summary from a vacant one by its words — a list of
//! vacant words, a "concrete name" anchor — and an independent review found it fragile both ways
//! ("None." and "Keine." refused, "the work goes on in app/x.py" accepted). The gate is now
//! structural only, and these facts carry what a weak summary leaves out.

use super::{arguments_excerpt, outcome_and_output, COMPACTED_PREFIX};
use crate::conversation::message::{Message, MessageContent};
use rmcp::model::Role;
use std::path::Path;

/// The newest tool calls the facts list.
pub const LAST_CALLS: usize = 10;
/// The commands that worked the facts list, newest kept.
pub const WORKED_COMMANDS: usize = 8;
/// The characters of one listed call or command.
pub const LINE_CHARS: usize = 400;
/// The names one file's outline lists, and its characters.
pub const OUTLINE_NAMES: usize = 40;
pub const OUTLINE_CHARS: usize = 900;
/// The lines and characters of a file's head when no name could be read from it.
const HEAD_LINES: usize = 8;
const HEAD_CHARS: usize = 400;
/// A file larger than this is not read for its outline.
const MAX_OUTLINE_BYTES: u64 = 512 * 1024;

pub fn clip(text: &str, max: usize) -> String {
    let text = text.trim();
    if text.chars().count() <= max {
        text.to_string()
    } else {
        format!("{}…", text.chars().take(max).collect::<String>())
    }
}

/// What a written file holds as goose last saw it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileNow {
    pub lines: usize,
    pub bytes: usize,
    /// Where it was read: on disk at compaction, or the conversation's last whole write of it.
    pub from_disk: bool,
    /// Its functions, classes, exports and routes, in file order.
    pub names: Vec<String>,
    /// Its first lines, when no name could be read from it.
    pub head: Option<String>,
}

impl FileNow {
    pub fn of(text: &str, from_disk: bool) -> Self {
        let names = outline(text);
        let head = names.is_empty().then(|| {
            let lines: Vec<&str> = text
                .lines()
                .filter(|l| !l.trim().is_empty())
                .take(HEAD_LINES)
                .collect();
            clip(&lines.join("\n"), HEAD_CHARS)
        });
        Self {
            lines: text.lines().count(),
            bytes: text.len(),
            from_disk,
            names,
            head: head.filter(|h| !h.is_empty()),
        }
    }

    pub fn line(&self) -> String {
        let place = if self.from_disk {
            "on disk now"
        } else {
            "as last written in the conversation"
        };
        let mut out = format!("{} lines, {} bytes {place}", self.lines, self.bytes);
        if !self.names.is_empty() {
            out.push_str(&format!(
                "; defines: {}",
                clip(&self.names.join(", "), OUTLINE_CHARS)
            ));
        } else if let Some(head) = &self.head {
            out.push_str(&format!("; opens with: {}", head.replace('\n', " ⏎ ")));
        }
        out
    }
}

/// The file as it reads on disk, else as the conversation last left it: its last whole `write`,
/// with the `edit` calls after it applied (each replaces its `before` text with its `after`) —
/// `None` when neither is there.
pub fn file_now(path: &str, messages: &[Message]) -> Option<FileNow> {
    file_text_now(path, messages).map(|(text, from_disk)| FileNow::of(&text, from_disk))
}

/// The text [`file_now`] reads, and whether it came from the disk.
pub fn file_text_now(path: &str, messages: &[Message]) -> Option<(String, bool)> {
    let on_disk = Path::new(path);
    if !super::pillars::is_secret_like_path(on_disk) {
        if let Ok(meta) = std::fs::metadata(on_disk) {
            if meta.is_file() && meta.len() <= MAX_OUTLINE_BYTES {
                if let Ok(text) = std::fs::read_to_string(on_disk) {
                    return Some((text, true));
                }
            }
        }
    }
    let names_it = |named: &str| {
        named == path || path.ends_with(&format!("/{}", named.trim_start_matches("./")))
    };
    let mut text: Option<String> = None;
    for call in messages
        .iter()
        .flat_map(|m| m.content.iter())
        .filter_map(|c| match c {
            MessageContent::ToolRequest(request) => request.tool_call.as_ref().ok(),
            _ => None,
        })
    {
        let Some(args) = call.arguments.as_ref() else {
            continue;
        };
        let arg = |key: &str| args.get(key).and_then(|v| v.as_str());
        if !arg("path").is_some_and(names_it) {
            continue;
        }
        match (call.name.as_ref(), &mut text) {
            ("write", _) => text = arg("content").map(str::to_string),
            ("edit", Some(current)) => {
                if let (Some(before), Some(after)) = (arg("before"), arg("after")) {
                    if !before.is_empty() && current.contains(before) {
                        *current = current.replacen(before, after, 1);
                    }
                }
            }
            _ => {}
        }
    }
    text.map(|text| (text, false))
}

/// The `TODO` / `FIXME` markers one file carries, and the lines the "Still to do" list keeps.
pub const TODO_MARKERS_PER_FILE: usize = 3;
pub const STILL_TO_DO: usize = 12;

/// The lines of `text` that carry a `TODO` or `FIXME` marker (the code convention, as a whole
/// word in capitals), as "path line n: the line", at most [`TODO_MARKERS_PER_FILE`].
pub fn todo_markers(path: &str, text: &str) -> Vec<String> {
    text.lines()
        .enumerate()
        .filter(|(_, line)| {
            line.split(|c: char| !c.is_ascii_alphanumeric())
                .any(|word| word == "TODO" || word == "FIXME")
        })
        .take(TODO_MARKERS_PER_FILE)
        .map(|(i, line)| format!("{path} line {}: {}", i + 1, clip(line, 160)))
        .collect()
}

/// The items the newest scratchpad (`todo_write`) left unchecked (`- [ ] …`), in its order.
pub fn scratchpad_open_items(messages: &[Message]) -> Vec<String> {
    let newest = messages
        .iter()
        .flat_map(|m| m.content.iter())
        .filter_map(|c| match c {
            MessageContent::ToolRequest(request) => request.tool_call.as_ref().ok(),
            _ => None,
        })
        .filter(|call| call.name.ends_with("todo_write"))
        .filter_map(|call| {
            call.arguments
                .as_ref()
                .and_then(|a| a.get("content"))
                .and_then(|v| v.as_str())
                .map(str::to_string)
        })
        .next_back();
    newest
        .iter()
        .flat_map(|content| content.lines())
        .filter_map(|line| {
            let line = line.trim_start();
            ["- [ ]", "* [ ]", "+ [ ]"]
                .iter()
                .find_map(|open| line.strip_prefix(open))
                .map(|item| format!("scratchpad: {}", clip(item, 200)))
        })
        .filter(|item| item.len() > "scratchpad: ".len())
        .collect()
}

/// The names a file defines, read line by line without a parser: functions, classes, structs,
/// exports (Python, JavaScript/TypeScript, Rust, Go, shell), markdown headings, and the routes it
/// serves (a quoted `/path` on a line that names a method or a route).
pub fn outline(text: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    let mut routes: Vec<String> = Vec::new();
    let push = |names: &mut Vec<String>, name: String| {
        if !name.is_empty() && !names.contains(&name) && names.len() < OUTLINE_NAMES {
            names.push(name);
        }
    };
    let ident = |rest: &str| -> String {
        rest.chars()
            .take_while(|c| c.is_alphanumeric() || matches!(c, '_' | '$'))
            .collect()
    };
    for raw in text.lines() {
        let indent = raw.len() - raw.trim_start().len();
        let line = raw.trim();
        if indent <= 4 {
            let line = line
                .trim_start_matches("export ")
                .trim_start_matches("default ")
                .trim_start_matches("pub(crate) ")
                .trim_start_matches("pub ")
                .trim_start_matches("async ");
            let found = [
                ("def ", "def "),
                ("class ", "class "),
                ("function ", "function "),
                ("function* ", "function "),
                ("fn ", "fn "),
                ("struct ", "struct "),
                ("enum ", "enum "),
                ("trait ", "trait "),
                ("interface ", "interface "),
                ("func ", "func "),
            ]
            .iter()
            .find_map(|(lead, shown)| {
                line.strip_prefix(lead)
                    .map(|rest| format!("{shown}{}", ident(rest.trim_start_matches('*').trim())))
            });
            match found {
                Some(name) if name.len() > name.split(' ').next().unwrap_or("").len() + 1 => {
                    let name = if indent > 0 {
                        format!("  {name}")
                    } else {
                        name
                    };
                    push(&mut names, name.trim_end().to_string());
                }
                _ => {
                    if indent == 0 {
                        // `const handler = (…) =>` / `= async function`: a named function value.
                        for lead in ["const ", "let ", "var "] {
                            if let Some(rest) = line.strip_prefix(lead) {
                                let name = ident(rest);
                                let after = rest.get(name.len()..).unwrap_or_default().trim_start();
                                if !name.is_empty()
                                    && after.starts_with('=')
                                    && (after.contains("=>") || after.contains("function"))
                                {
                                    push(&mut names, format!("const {name}"));
                                }
                            }
                        }
                        // Markdown headings, and `name() {` shell functions.
                        if let Some(heading) = line.strip_prefix('#') {
                            let heading = heading.trim_start_matches('#').trim();
                            // `# Title` and `## Section`: the document's own outline.
                            if !heading.is_empty()
                                && (heading.chars().count() <= 60 && raw.starts_with("# ")
                                    || raw.starts_with("## "))
                            {
                                push(&mut names, format!("# {heading}"));
                            }
                        } else if let Some(name) = line.strip_suffix("() {") {
                            if name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                                push(&mut names, format!("{name}()"));
                            }
                        }
                    }
                }
            }
        }
        let lower = line.to_lowercase();
        if ["get", "post", "put", "delete", "patch", "route", "path"]
            .iter()
            .any(|w| lower.contains(w))
        {
            for quote in ['"', '\''] {
                for (i, piece) in line.split(quote).enumerate() {
                    let looks_like_route = i % 2 == 1
                        && piece.starts_with('/')
                        && piece.len() > 1
                        && piece.len() <= 80
                        && !piece.contains(' ')
                        && !piece.starts_with("//");
                    if looks_like_route && !routes.iter().any(|r| r == piece) && routes.len() < 30 {
                        routes.push(piece.to_string());
                    }
                }
            }
        }
    }
    if !routes.is_empty() {
        push(&mut names, format!("routes {}", routes.join(" ")));
    }
    names
}

/// One tool call of the conversation, as the facts read it.
#[derive(Debug, Clone)]
struct Call {
    id: String,
    /// What the call acts on: its command, else its path — what a later call must repeat to
    /// settle an earlier failure.
    key: String,
    tool: String,
    /// The call named no command or path: a later success of the same tool settles it (the
    /// Flash-Next run's `edit` calls that left out `path` failed, and the next ones named it).
    targetless: bool,
    line: String,
    failed: Option<bool>,
    command: Option<String>,
}

fn calls_of(messages: &[Message]) -> Vec<Call> {
    let results: std::collections::HashMap<&str, &crate::conversation::message::ToolResponse> =
        messages
            .iter()
            .flat_map(|m| m.content.iter())
            .filter_map(|c| match c {
                MessageContent::ToolResponse(response) => Some((response.id.as_str(), response)),
                _ => None,
            })
            .collect();
    let mut calls = Vec::new();
    for request in messages
        .iter()
        .flat_map(|m| m.content.iter())
        .filter_map(|c| match c {
            MessageContent::ToolRequest(request) => Some(request),
            _ => None,
        })
    {
        let Ok(call) = &request.tool_call else {
            continue;
        };
        let args = call.arguments.as_ref();
        let arg = |key: &str| {
            args.and_then(|a| a.get(key))
                .and_then(|v| v.as_str())
                .map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "))
        };
        let command = arg("command").filter(|_| call.name.contains("shell"));
        let target = command
            .clone()
            .or_else(|| arg("path"))
            .or_else(|| arg("file_path"))
            .unwrap_or_default();
        let targetless = target.is_empty();
        let key = format!("{} {target}", call.name);
        let (failed, outcome) = match results.get(request.id.as_str()).map(|r| &r.tool_result) {
            Some(Ok(result)) => {
                let exit = result
                    .structured_content
                    .as_ref()
                    .and_then(|s| s.get("exit_code"))
                    .and_then(serde_json::Value::as_i64);
                let failed = result.is_error == Some(true) || exit.is_some_and(|code| code != 0);
                let (outcome, output) = outcome_and_output(result);
                let outcome = if failed && !outcome.starts_with("failed") {
                    format!("failed (exit {})", exit.unwrap_or(-1))
                } else {
                    outcome
                };
                (Some(failed), format!("{outcome}. {output}"))
            }
            Some(Err(e)) => (Some(true), format!("failed: {}", clip(&e.message, 160))),
            None => (None, "no result".to_string()),
        };
        calls.push(Call {
            id: request.id.clone(),
            key,
            tool: call.name.to_string(),
            targetless,
            // The arguments are cut first: the outcome is what the line is for.
            line: format!(
                "{} {} → {}",
                call.name,
                clip(&arguments_excerpt(args), LINE_CHARS / 2),
                clip(&outcome, LINE_CHARS / 2)
            ),
            failed,
            command,
        });
    }
    calls
}

/// The newest [`LAST_CALLS`] tool calls, oldest first, each with its outcome.
pub fn last_calls(messages: &[Message]) -> Vec<String> {
    let calls = calls_of(messages);
    let from = calls.len().saturating_sub(LAST_CALLS);
    calls[from..].iter().map(|c| c.line.clone()).collect()
}

/// The ids of the failed calls a LATER call of the same tool on the same command or path settled
/// by succeeding: what is left of the failures are the errors still open.
pub fn settled_failures(messages: &[Message]) -> std::collections::HashSet<String> {
    let calls = calls_of(messages);
    calls
        .iter()
        .enumerate()
        .filter(|(i, call)| {
            call.failed == Some(true)
                && calls[i + 1..].iter().any(|later| {
                    later.failed == Some(false)
                        && (later.key == call.key || (call.targetless && later.tool == call.tool))
                })
        })
        .map(|(_, call)| call.id.clone())
        .collect()
}

/// Commands that only look (read, list, search, print): they are not the commands that "worked".
const LOOKING_COMMANDS: [&str; 16] = [
    "cat", "ls", "head", "tail", "grep", "rg", "find", "wc", "sed", "echo", "pwd", "which", "tree",
    "stat", "file", "diff",
];

/// The shell commands that ran and exited 0 and did more than look — the tests, runs, builds
/// and checks the work can repeat — newest last, each once, at most [`WORKED_COMMANDS`].
pub fn worked_commands(messages: &[Message]) -> Vec<String> {
    let mut worked: Vec<String> = Vec::new();
    for call in calls_of(messages) {
        let (Some(command), Some(false)) = (&call.command, call.failed) else {
            continue;
        };
        // The first word of the command's last step (`cd x && pytest -q` runs pytest).
        let step = command
            .rsplit("&&")
            .next()
            .unwrap_or(command)
            .split(['|', ';'])
            .next()
            .unwrap_or("")
            .trim();
        let first = step.split_whitespace().next().unwrap_or("");
        if first.is_empty() || LOOKING_COMMANDS.contains(&first) || first == "cd" {
            continue;
        }
        let line = clip(command, LINE_CHARS);
        worked.retain(|w| *w != line);
        worked.push(line);
    }
    let from = worked.len().saturating_sub(WORKED_COMMANDS);
    worked.split_off(from)
}

/// The model-written part of every summary an earlier compaction stored in `messages`, oldest
/// first: the summary row is the one just before a compaction's continuation line, seen by the
/// model when it was stored (a later compaction hides it, and the session store keeps it).
pub fn earlier_summaries(messages: &[Message]) -> Vec<String> {
    messages
        .windows(2)
        .filter(|pair| {
            pair[1].role == Role::Assistant
                && !pair[1].is_user_visible()
                && pair[1].as_concat_text().starts_with(COMPACTED_PREFIX)
                && pair[0].role == Role::User
                && !pair[0].is_user_visible()
        })
        .map(|pair| {
            let stored = pair[0].as_concat_text();
            let written = super::pillars::model_part(&stored);
            // A flagged summary carries the model's answer after goose's flag line.
            let written = match written.strip_prefix(super::pillars::FLAG_LEAD) {
                Some(flagged) => flagged.split_once("]\n\n").map_or("", |(_, answer)| answer),
                None => written.as_str(),
            };
            written.trim().to_string()
        })
        .filter(|text| !text.is_empty())
        .collect()
}
