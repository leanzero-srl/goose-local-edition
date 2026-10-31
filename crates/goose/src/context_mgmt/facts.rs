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
pub fn file_now(path: &str, messages: &[Message], working_dir: Option<&Path>) -> Option<FileNow> {
    file_text_now(path, messages, working_dir)
        .map(|(text, from_disk)| FileNow::of(&text, from_disk))
}

/// The text [`file_now`] reads, and whether it came from the disk. `working_dir` is the chat's
/// folder: only the path below it is judged secret-like (`pillars::is_secret_like_path`).
pub fn file_text_now(
    path: &str,
    messages: &[Message],
    working_dir: Option<&Path>,
) -> Option<(String, bool)> {
    let on_disk = Path::new(path);
    if !super::pillars::is_secret_like_path(on_disk, working_dir) {
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
    /// The call failed because its arguments did not parse: it ran nothing.
    unparsed: bool,
    line: String,
    failed: Option<bool>,
    command: Option<String>,
    /// A shell call's [`exit_step`] (whitespace-collapsed, its `cd` steps aside): the step whose
    /// status the call's exit code is. `None` when it cannot be told from the command's text.
    exit_step: Option<String>,
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
        // The exit step is read from the command as written: collapsing its whitespace first
        // would join its lines, and a newline ends a step just as `;` does.
        let exit_step = args
            .and_then(|a| a.get("command"))
            .and_then(|v| v.as_str())
            .filter(|_| call.name.contains("shell"))
            .and_then(exit_step)
            .map(|step| without_cwd(&step).to_string());
        let target = command
            .clone()
            .or_else(|| arg("path"))
            .or_else(|| arg("file_path"))
            .unwrap_or_default();
        let mut unparsed = false;
        // A command is the same command from any folder: `cd /tmp && curl …` that failed is
        // settled by `cd . && curl …` that worked (the verifier's real run listed it as open).
        let key = format!("{} {}", call.name, without_cwd(&target));
        let (failed, outcome) = match results.get(request.id.as_str()).map(|r| &r.tool_result) {
            Some(Ok(result)) => {
                let exit = result
                    .structured_content
                    .as_ref()
                    .and_then(|s| s.get("exit_code"))
                    .and_then(serde_json::Value::as_i64);
                let failed = result.is_error == Some(true) || exit.is_some_and(|code| code != 0);
                unparsed = failed && arguments_did_not_parse(result);
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
        // A call whose arguments did not parse ran nothing, so its "target" may be any text the
        // model leaked into a field: like a call with no target, the tool's next success settles it.
        let targetless = target.is_empty() || unparsed;
        calls.push(Call {
            id: request.id.clone(),
            key,
            tool: call.name.to_string(),
            targetless,
            unparsed,
            // The arguments are cut first: the outcome is what the line is for.
            line: format!(
                "{} {} → {}",
                call.name,
                clip(&arguments_excerpt(args), LINE_CHARS / 2),
                clip(&outcome, LINE_CHARS / 2)
            ),
            failed,
            command,
            exit_step,
        });
    }
    calls
}

/// Whether a failed tool result is a developer tool's refusal of arguments that did not parse
/// ([`ARGUMENTS_DID_NOT_PARSE`], with or without the shell's `Error: ` lead).
fn arguments_did_not_parse(result: &rmcp::model::CallToolResult) -> bool {
    use crate::agents::platform_extensions::developer::ARGUMENTS_DID_NOT_PARSE;
    result.content.iter().filter_map(|c| c.as_text()).any(|t| {
        let text = t.text.trim_start();
        text.strip_prefix("Error: ")
            .unwrap_or(text)
            .starts_with(ARGUMENTS_DID_NOT_PARSE)
    })
}

/// The newest [`LAST_CALLS`] tool calls, oldest first, each with its outcome; a failed call a
/// later one settled ([`settled_failures`]) is one short line without its arguments. Why (MiMo
/// two-Mac Gauntlet run 8985a86d, compaction at call 55, 2026-10-31): the list carried three failed
/// `edit` calls with their `before` text word for word, though the next edit of the file worked.
pub fn last_calls(messages: &[Message]) -> Vec<String> {
    let calls = calls_of(messages);
    let settled = settled_failures(messages);
    let from = calls.len().saturating_sub(LAST_CALLS);
    calls[from..]
        .iter()
        .map(|c| {
            if !settled.contains(&c.id) {
                return c.line.clone();
            }
            let why = if c.unparsed {
                "its arguments did not parse (nothing ran)"
            } else {
                "failed"
            };
            format!("{} → {why}; a later {} succeeded (settled)", c.tool, c.tool)
        })
        .collect()
}

/// The ids of the failed calls a LATER call of the same tool on the same command (from any folder,
/// [`without_cwd`]) or path settled by succeeding: what is left of the failures, less the
/// [`look_up_failures`], are the errors still open.
pub fn settled_failures(messages: &[Message]) -> std::collections::HashSet<String> {
    let calls = calls_of(messages);
    calls
        .iter()
        .enumerate()
        .filter(|(i, call)| {
            call.failed == Some(true)
                && calls[i + 1..].iter().any(|later| {
                    later.failed == Some(false)
                        && (later.key == call.key
                            || (call.targetless && later.tool == call.tool)
                            || (later.tool == call.tool
                                && call.exit_step.is_some()
                                && later.exit_step == call.exit_step))
                })
        })
        .map(|(_, call)| call.id.clone())
        .collect()
}

/// `command` without the `cd <folder> &&` (or `;`) steps it opens with: the folder it ran in.
fn without_cwd(command: &str) -> &str {
    let mut rest = command.trim();
    while let Some(after) = rest.strip_prefix("cd ") {
        let Some(end) = after.find(['&', ';']) else {
            break;
        };
        rest = after
            .get(end..)
            .unwrap_or("")
            .trim_start_matches(['&', ';'])
            .trim_start();
    }
    rest
}

/// Commands that only look (read, list, search, test, print): they are not the commands that
/// "worked", and when one exits non-zero it found nothing (`ls` of a missing file, `wc` on a glob
/// that matched none, `test -f`, `grep` with no match) — an answer, not an error still open.
const LOOKING_COMMANDS: [&str; 22] = [
    "cat", "ls", "head", "tail", "grep", "rg", "find", "wc", "sed", "echo", "pwd", "which", "tree",
    "stat", "file", "diff", "test", "[", "[[", "type", "readlink", "realpath",
];

/// Whether every step of `command` (split at `&&`, `||`, `;` and `|`; its `cd` steps and
/// `NAME=value` prefixes aside) is one of the [`LOOKING_COMMANDS`].
fn only_looks(command: &str) -> bool {
    let mut steps = 0;
    for step in command.split(['&', '|', ';']) {
        let mut words = step
            .split_whitespace()
            .skip_while(|w| w.contains('=') && !w.starts_with(['-', '=']));
        let Some(first) = words.next() else {
            continue;
        };
        let first = first.rsplit('/').next().unwrap_or(first);
        if first == "cd" {
            continue;
        }
        // `sed -i` writes the file it names, and so does a step that sends its output to a file
        // (`cat > test/x.mjs <<'EOF'`, `echo x >> log`): its failure is an error. A redirect to
        // /dev/null or to another stream (`2>&1`) writes nothing.
        let writes = (first == "sed" && step.split_whitespace().any(|w| w.starts_with("-i")))
            || writes_a_file(step);
        if !LOOKING_COMMANDS.contains(&first) || writes {
            return false;
        }
        steps += 1;
    }
    steps > 0
}

/// Whether `step` sends output to a file: an unquoted `>` or `>>` whose target is not /dev/null
/// and not another stream (`>&2`, `2>&1`).
fn writes_a_file(step: &str) -> bool {
    let chars: Vec<char> = step.chars().collect();
    let (mut single, mut double) = (false, false);
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '\\' if !single => i += 1,
            '\'' if !double => single = !single,
            '"' if !single => double = !double,
            '>' if !single && !double => {
                let mut j = i + 1;
                if chars.get(j) == Some(&'>') {
                    j += 1;
                }
                if chars.get(j) == Some(&'&') {
                    i = j + 1;
                    continue;
                }
                while chars.get(j).is_some_and(|c| c.is_whitespace()) {
                    j += 1;
                }
                let target: String = chars[j..]
                    .iter()
                    .take_while(|c| !c.is_whitespace() && !matches!(c, ';' | '&' | '|'))
                    .collect();
                // An empty target is a step `only_looks` cut at `&` (`2>&1`): no file named.
                if !target.is_empty() && target != "/dev/null" {
                    return true;
                }
                i = j;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    false
}

/// Shell words that open or close a compound command: a list that holds one is not split into
/// steps here (`if …; then …; fi` would end on `fi`).
const COMPOUND_WORDS: [&str; 17] = [
    "if", "then", "else", "elif", "fi", "for", "while", "until", "do", "done", "case", "esac", "{",
    "}", "function", "select", "time",
];

/// The step of a shell command whose status is the command's exit code: the last of its
/// top-level steps, which `;` and newlines separate (outside quotes, `$(…)`, backquotes and
/// here-document bodies), whitespace collapsed. A `&&`/`||` chain or a pipe stays one step: its
/// status may be any part's. `None` when the text cannot say: an empty command, a compound
/// command (`if`/`for`/`{ … }`/a subshell), `set -e` or `exit` in an earlier step (the shell may
/// stop there with that step's status), unbalanced quotes or brackets.
///
/// Why (steward 2026-10-31, the live MiMo run 8985a86d, second compaction): a call that wrote a
/// probe and ran it, `cat > test/perf2.mjs <<'EOF' … EOF` + newline + `node test/perf2.mjs`,
/// failed; the next `cd … && node test/perf2.mjs` worked, but the kept block listed the first as
/// an open error and a to-do, since its whole text never ran again. `node test/fill_probe.mjs;
/// nproc; grep -m1 "model name" /proc/cpuinfo` exited 2 only because grep found no /proc on
/// macOS — a look-up that found nothing, listed as an open error too.
pub(crate) fn exit_step(command: &str) -> Option<String> {
    let chars: Vec<char> = command.chars().collect();
    let mut steps: Vec<String> = Vec::new();
    let mut step = String::new();
    let (mut single, mut double, mut backquote) = (false, false, false);
    let mut depth = 0usize;
    let mut heredocs: Vec<(String, bool)> = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' && !single {
            // A backslash-newline joins two lines into one step.
            if chars.get(i + 1) == Some(&'\n') {
                step.push(' ');
            } else {
                step.push(c);
                if let Some(next) = chars.get(i + 1) {
                    step.push(*next);
                }
            }
            i += 2;
            continue;
        }
        let top = !single && !double && !backquote && depth == 0;
        match c {
            '\'' if !double && !backquote => single = !single,
            '"' if !single && !backquote => double = !double,
            '`' if !single => backquote = !backquote,
            '(' if !single && !double && !backquote => {
                if i == 0 || chars[i - 1] != '$' {
                    // A subshell or a function body at the top: a compound command.
                    if depth == 0 && step.trim().is_empty() {
                        return None;
                    }
                }
                depth += 1;
            }
            ')' if !single && !double && !backquote => depth = depth.checked_sub(1)?,
            '#' if top && step.chars().last().is_none_or(|p| p.is_whitespace()) => {
                // A comment runs to the end of its line.
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
                continue;
            }
            '<' if top && chars.get(i + 1) == Some(&'<') && chars.get(i + 2) != Some(&'<') => {
                let mut j = i + 2;
                let strip_tabs = chars.get(j) == Some(&'-');
                if strip_tabs {
                    j += 1;
                }
                while chars.get(j).is_some_and(|c| *c == ' ' || *c == '\t') {
                    j += 1;
                }
                let word: String = chars[j..]
                    .iter()
                    .take_while(|c| !c.is_whitespace() && !matches!(c, ';' | '&' | '|' | '<' | '>'))
                    .filter(|c| !matches!(c, '\'' | '"' | '\\'))
                    .collect();
                if word.is_empty() {
                    return None;
                }
                heredocs.push((word, strip_tabs));
                step.extend(&chars[i..j]);
                i = j;
                continue;
            }
            '\n' if top => {
                steps.push(std::mem::take(&mut step));
                // The bodies of the here-documents this line opened: skipped, line by line,
                // up to each one's end word.
                for (word, strip_tabs) in std::mem::take(&mut heredocs) {
                    loop {
                        let start = i + 1;
                        if start > chars.len() {
                            return None;
                        }
                        let end = chars[start..]
                            .iter()
                            .position(|c| *c == '\n')
                            .map_or(chars.len(), |n| start + n);
                        let line: String = chars[start..end].iter().collect();
                        i = end;
                        let line = if strip_tabs {
                            line.trim_start_matches('\t')
                        } else {
                            &line
                        };
                        if line == word {
                            break;
                        }
                        if end >= chars.len() {
                            // The body never ended: the shell reads to the end of the text.
                            return None;
                        }
                    }
                }
                i += 1;
                continue;
            }
            ';' if top && chars.get(i + 1) != Some(&';') => {
                steps.push(std::mem::take(&mut step));
                i += 1;
                continue;
            }
            _ => {}
        }
        step.push(c);
        i += 1;
    }
    if single || double || backquote || depth != 0 {
        return None;
    }
    steps.push(step);
    let steps: Vec<String> = steps
        .iter()
        .map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|s| !s.is_empty())
        .collect();
    let (last, earlier) = steps.split_last()?;
    let first_word = |s: &str| s.split_whitespace().next().unwrap_or("").to_string();
    if steps.iter().any(|s| {
        s.split(['&', '|'])
            .any(|part| COMPOUND_WORDS.contains(&first_word(part).as_str()))
    }) {
        return None;
    }
    let stops_early = |s: &String| {
        s.split(['&', '|']).any(|part| {
            let mut words = part.split_whitespace();
            match words.next() {
                Some("exit") | Some("return") => true,
                Some("set") => words.any(|w| {
                    (w.starts_with('-') && !w.starts_with("--") && w.contains('e'))
                        || w == "errexit"
                }),
                _ => false,
            }
        })
    };
    if earlier.iter().any(stops_early) {
        return None;
    }
    Some(last.clone())
}

/// The ids of the failed shell calls that only looked ([`only_looks`]): not errors still open.
/// Round 6 (2026-10-30): of the real Flash-Next run's 3 open errors, 2 were `wc` on a glob that
/// matched nothing and a `cd /tmp && curl …` later run from another folder.
pub fn look_up_failures(messages: &[Message]) -> std::collections::HashSet<String> {
    calls_of(messages)
        .into_iter()
        .filter(|call| {
            call.failed == Some(true)
                && (call.command.as_deref().is_some_and(only_looks)
                    || call.exit_step.as_deref().is_some_and(only_looks))
        })
        .map(|call| call.id)
        .collect()
}

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

#[cfg(test)]
mod tests {
    use super::{exit_step, only_looks};

    /// The real shapes of the MiMo run 8985a86d (second compaction, 2026-10-31) and the cases
    /// where the text cannot say which step set the exit code.
    #[test]
    fn the_exit_step_is_the_last_top_level_step_or_none_when_the_text_cannot_say() {
        let probe = "cd /w && cat > test/perf2.mjs <<'EOF'\nconst a = 1; const b = 2;\nconsole.log(a)\nEOF\nnode test/perf2.mjs";
        assert_eq!(exit_step(probe).as_deref(), Some("node test/perf2.mjs"));
        let fill = "cd /w && cat > t.mjs <<'EOF'\nx;\nEOF\nnode t.mjs; nproc; grep -m1 \"model name\" /proc/cpuinfo";
        assert_eq!(
            exit_step(fill).as_deref(),
            Some("grep -m1 \"model name\" /proc/cpuinfo")
        );
        // A `;` in quotes, `$(…)` or a here-document body is not a step's end.
        assert_eq!(exit_step("echo 'a; b'").as_deref(), Some("echo 'a; b'"));
        assert_eq!(
            exit_step("x=$(ls; pwd) && make").as_deref(),
            Some("x=$(ls; pwd) && make")
        );
        assert_eq!(
            exit_step("python3 - <<EOF\nprint(1); print(2)\nEOF").as_deref(),
            Some("python3 - <<EOF")
        );
        // A chain stays one step; a trailing comment line is not a step.
        assert_eq!(
            exit_step("make && ./run\n# done").as_deref(),
            Some("make && ./run")
        );
        for cannot in [
            "if true; then ls; fi",
            "for f in *; do wc $f; done",
            "set -e; make; grep x log",
            "make || exit 1; grep x log",
            "(cd a; make); ls",
            "echo 'open",
            "cat <<EOF\nnever ends",
            "",
        ] {
            assert_eq!(exit_step(cannot), None, "{cannot}");
        }
    }

    #[test]
    fn a_step_that_writes_a_file_does_not_only_look() {
        assert!(only_looks("ls /w/x 2>/dev/null"));
        assert!(only_looks("grep -n TODO a.py > /dev/null"));
        assert!(!only_looks("cat > test/x.mjs <<'EOF'"));
        assert!(!only_looks("echo done >> /w/log.txt"));
        assert!(!only_looks("echo hi>/ro/file"));
    }
}
