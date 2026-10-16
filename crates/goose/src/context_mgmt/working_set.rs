//! The working set a compaction gives back: the CURRENT on-disk content of the files the
//! conversation read or wrote, newest first, within a share of the window, placed right after the
//! summary as its own agent-only message.
//!
//! Why (Forge 2.0 runs A and B on Gabee, Qwen3.8-27B, 2026-10-16): the summaries held the file
//! list, the decisions and the next step, yet after every heavy compaction the model re-read 14-26
//! files over 7-16 calls (+28-42k tokens; run A 7.2k -> 34.8k) and then re-planned for 800-1,150 s;
//! in run B the next step was written 2 h 37 min after a compaction. The words of a summary do not
//! replace the files; the files do.

use std::path::{Path, PathBuf};

use rmcp::model::Role;

use super::pillars::resolve_folder;
use crate::config::Config;
use crate::conversation::message::{Message, MessageContent};

pub const WORKING_SET_OPEN: &str = "<current-files-after-compaction>";
pub const WORKING_SET_CLOSE: &str = "</current-files-after-compaction>";

/// `GOOSE_COMPACTION_WORKING_SET_SHARE` (env > config > default): the share of the window the
/// working set may take; 0 turns it off.
pub const WORKING_SET_SHARE_KEY: &str = "GOOSE_COMPACTION_WORKING_SET_SHARE";

// The files re-read after a compaction were 27.6k tokens in run A (7.2k -> 34.8k) and 28-42k in
// run B, 17-26% of the 158,833-token window the engine serves; a quarter (39.7k tokens there)
// holds A's whole re-read set and all but ~2k of B's largest. What the model re-read cost those
// tokens anyway, plus 7-16 calls and the re-plan.
pub const DEFAULT_WORKING_SET_SHARE: f64 = 0.25; // ratio: of the window; measured re-reads 17-26%

/// The working set's budget in chars for a window of `context_limit` tokens; `None` when the window
/// is unknown or the share is 0.
pub fn working_set_budget_chars(context_limit: Option<usize>, share: f64) -> Option<usize> {
    if share <= 0.0 {
        return None;
    }
    context_limit.map(|limit| {
        (limit as f64 * crate::agents::platform_extensions::recall::CHARS_PER_TOKEN * share)
            as usize
    })
}

pub fn working_set_share() -> f64 {
    match Config::global().get_param::<f64>(WORKING_SET_SHARE_KEY) {
        Ok(share) if (0.0..1.0).contains(&share) => share,
        Ok(share) => {
            tracing::warn!(
                "{WORKING_SET_SHARE_KEY} is {share}, not a share in [0, 1); using \
                 {DEFAULT_WORKING_SET_SHARE}"
            );
            DEFAULT_WORKING_SET_SHARE
        }
        Err(crate::config::ConfigError::NotFound(_)) => DEFAULT_WORKING_SET_SHARE,
        Err(e) => {
            tracing::warn!(
                "{WORKING_SET_SHARE_KEY} could not be read ({e}); using {DEFAULT_WORKING_SET_SHARE}"
            );
            DEFAULT_WORKING_SET_SHARE
        }
    }
}

/// Whether `text` is a working-set block goose placed after a summary — goose's, never the
/// person's words.
pub fn is_working_set_text(text: &str) -> bool {
    text.starts_with(WORKING_SET_OPEN)
}

/// Tools that print a file's content to the model: the shell commands the measured runs read with.
const READERS: [&str; 7] = ["cat", "head", "tail", "nl", "sed", "less", "more"];

fn named(name: &str, tool: &str) -> bool {
    name == tool || name.ends_with(&format!("__{tool}"))
}

/// A file the conversation's calls touched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Touched {
    pub path: PathBuf,
    /// Only ever read with `head` or `tail`: the conversation saw its ends, never the file — a
    /// log it peeked at (Forge 2.0 run B: `head -40 .dev/rec1.log`, 1,027 lines) is not worked on.
    pub in_part: bool,
}

/// The files the conversation's calls read (a shell `cat`/`head`/`tail`/`nl`/`sed`/`less`/`more`
/// at the head of a pipeline) or wrote (`write`, `edit`), absolute, newest touch first, each once.
/// Only calls whose result came back and succeeded count: a call declined in approve mode, skipped
/// by the repeat guard or failed read nothing, and goose must not read for it — a declined
/// `cat ~/.aws/credentials` would otherwise be read at the compaction and sent to the provider.
pub fn touched_files(messages: &[Message], working_dir: Option<&Path>) -> Vec<Touched> {
    let mut touched: Vec<Touched> = Vec::new();
    let succeeded: std::collections::HashSet<&str> = messages
        .iter()
        .flat_map(|m| m.content.iter())
        .filter_map(|c| match c {
            MessageContent::ToolResponse(r) => match &r.tool_result {
                Ok(result) if result.is_error != Some(true) => Some(r.id.as_str()),
                _ => None,
            },
            _ => None,
        })
        .collect();
    let calls = messages
        .iter()
        .filter(|m| m.role == Role::Assistant)
        .flat_map(|m| m.content.iter())
        .filter_map(|c| match c {
            MessageContent::ToolRequest(req) if succeeded.contains(req.id.as_str()) => {
                req.tool_call.as_ref().ok()
            }
            _ => None,
        });
    for call in calls {
        let Some(arguments) = call.arguments.as_ref() else {
            continue;
        };
        let name = call.name.as_ref();
        let paths: Vec<Touched> = if named(name, "write") || named(name, "edit") {
            arguments
                .get("path")
                .and_then(|p| p.as_str())
                .filter(|p| names_a_file(p))
                .and_then(|p| resolve_folder(p, working_dir))
                .map(|path| Touched {
                    path,
                    in_part: false,
                })
                .into_iter()
                .collect()
        } else if named(name, "shell") {
            let base = ["cwd", "working_dir", "workdir"]
                .iter()
                .find_map(|key| arguments.get(*key).and_then(|v| v.as_str()))
                .and_then(|dir| resolve_folder(dir, working_dir))
                .or_else(|| working_dir.map(Path::to_path_buf));
            arguments
                .get("command")
                .and_then(|c| c.as_str())
                .map(|command| shell_reads(command, base.as_deref()))
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        for file in paths {
            let in_part = file.in_part
                && touched
                    .iter()
                    .find(|known| known.path == file.path)
                    .is_none_or(|known| known.in_part);
            touched.retain(|known| known.path != file.path);
            touched.push(Touched {
                path: file.path,
                in_part,
            });
        }
    }
    touched.reverse();
    touched
}

/// The command split where a new simple command starts, each part with whether it heads its
/// pipeline (`cat a | head` reads `a`; the `head` reads its input).
fn simple_commands(command: &str) -> Vec<(String, bool)> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut heads = true;
    let mut quote: Option<char> = None;
    let mut chars = command.chars().peekable();
    while let Some(c) = chars.next() {
        if let Some(q) = quote {
            current.push(c);
            if c == q {
                quote = None;
            } else if c == '\\' && q == '"' {
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            }
            continue;
        }
        match c {
            '\'' | '"' => {
                quote = Some(c);
                current.push(c);
            }
            '\\' => {
                current.push(c);
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            }
            '\n' | ';' => {
                parts.push((std::mem::take(&mut current), heads));
                heads = true;
            }
            '&' if chars.peek() == Some(&'&') => {
                chars.next();
                parts.push((std::mem::take(&mut current), heads));
                heads = true;
            }
            '|' if chars.peek() == Some(&'|') => {
                chars.next();
                parts.push((std::mem::take(&mut current), heads));
                heads = true;
            }
            '|' => {
                parts.push((std::mem::take(&mut current), heads));
                heads = false;
            }
            _ => current.push(c),
        }
    }
    parts.push((current, heads));
    parts
}

/// A variable, a substitution, or a token carrying a newline or markup names no file goose can
/// find. Forge 2.0 run B (message 2448): a malformed `edit` call's `path` was
/// "src/jira.js\n</parameter>\n<parameter=background>\nnull".
fn names_a_file(path: &str) -> bool {
    !path.contains(['$', '`', '\n', '<', '>'])
}

fn is_redirection(token: &str) -> bool {
    let rest = token.trim_start_matches(|c: char| c.is_ascii_digit() || c == '&');
    rest.starts_with('>') || rest.starts_with('<')
}

/// A redirection operator standing alone, whose target (a file, a heredoc delimiter, a
/// here-string) is the next token.
fn takes_the_next_token(token: &str) -> bool {
    matches!(
        token.trim_start_matches(|c: char| c.is_ascii_digit()),
        ">" | ">>" | "<" | ">|" | "&>" | "&>>" | "<<" | "<<-" | "<<<"
    )
}

/// The heredoc delimiters a line opens (`<< EOF`, `<<'EOF'`, `<<-"END"`), in order, each with
/// whether its closing line may be indented by tabs (`<<-`). A here-string (`<<<`) opens none.
fn heredoc_delimiters(line: &str) -> Vec<(String, bool)> {
    let mut found = Vec::new();
    let mut rest = line;
    while let Some(at) = rest.find("<<") {
        let after = &rest[at + 2..];
        if let Some(string) = after.strip_prefix('<') {
            rest = string;
            continue;
        }
        let (strip_tabs, after) = match after.strip_prefix('-') {
            Some(after) => (true, after),
            None => (false, after),
        };
        let after = after.trim_start();
        let word: String = match after.chars().next() {
            Some(quote @ ('\'' | '"')) => after[1..].split(quote).next().unwrap_or("").to_string(),
            _ => after
                .chars()
                .take_while(|c| {
                    !c.is_whitespace() && !matches!(c, ';' | '&' | '|' | '<' | '>' | ')')
                })
                .filter(|c| *c != '\\')
                .collect(),
        };
        if !word.is_empty() {
            found.push((word, strip_tabs));
        }
        rest = after;
    }
    found
}

/// The command with every heredoc's body taken out: the lines after a line that opens a heredoc,
/// up to and including its delimiter line, are data, not commands.
fn without_heredoc_bodies(command: &str) -> String {
    let mut out = String::new();
    let mut open: std::collections::VecDeque<(String, bool)> = Default::default();
    for line in command.split('\n') {
        if let Some((delimiter, strip_tabs)) = open.front() {
            let closing = if *strip_tabs {
                line.trim_start_matches('\t')
            } else {
                line
            };
            if closing == delimiter {
                open.pop_front();
            }
            continue;
        }
        open.extend(heredoc_delimiters(line));
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// The files one shell command prints, resolved against `base` and the `cd`s before each part.
fn shell_reads(command: &str, base: Option<&Path>) -> Vec<Touched> {
    let mut folder = base.map(Path::to_path_buf);
    let mut reads = Vec::new();
    for (part, heads) in simple_commands(&without_heredoc_bodies(command)) {
        let Ok(tokens) = shell_words::split(&part) else {
            continue;
        };
        let mut tokens = tokens
            .into_iter()
            .skip_while(|t| t.contains('=') && !t.starts_with('-') && !t.contains('/'));
        let Some(program) = tokens.next() else {
            continue;
        };
        let program = program.rsplit('/').next().unwrap_or(&program).to_string();
        let rest: Vec<String> = tokens.collect();
        if program == "cd" {
            folder = rest
                .first()
                .and_then(|dir| resolve_folder(dir, folder.as_deref()));
            continue;
        }
        if !heads || !READERS.contains(&program.as_str()) {
            continue;
        }
        if program == "sed" && rest.iter().any(|t| t.starts_with("-i")) {
            continue;
        }
        let mut operands = Vec::new();
        let mut skip_next = false;
        let mut script_given = program != "sed";
        for token in &rest {
            if skip_next {
                skip_next = false;
                continue;
            }
            if is_redirection(token) {
                skip_next = takes_the_next_token(token);
                continue;
            }
            if token.starts_with('-') && token.len() > 1 {
                match (program.as_str(), token.as_str()) {
                    ("head" | "tail", "-n" | "-c") => skip_next = true,
                    ("sed", "-e" | "-f") => {
                        skip_next = true;
                        script_given = true;
                    }
                    _ => {}
                }
                continue;
            }
            if !script_given {
                script_given = true;
                continue;
            }
            operands.push(token.clone());
        }
        for operand in operands {
            if !names_a_file(&operand) {
                continue;
            }
            let Some(path) = resolve_folder(&operand, folder.as_deref()) else {
                continue;
            };
            let in_part = matches!(program.as_str(), "head" | "tail");
            let paths = if operand.contains(['*', '?']) {
                expand_glob(&path)
            } else {
                vec![path]
            };
            reads.extend(paths.into_iter().map(|path| Touched { path, in_part }));
        }
    }
    reads
}

/// A glob in the last component only (`src/*.js`), the shell's way: sorted, hidden files only when
/// the pattern starts with a dot. A pattern that matches nothing names no file.
fn expand_glob(pattern: &Path) -> Vec<PathBuf> {
    let (Some(dir), Some(name)) = (pattern.parent(), pattern.file_name()) else {
        return Vec::new();
    };
    let name = name.to_string_lossy();
    if dir.to_string_lossy().contains(['*', '?']) {
        return Vec::new();
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut matched: Vec<PathBuf> = entries
        .flatten()
        .filter(|e| {
            let candidate = e.file_name().to_string_lossy().into_owned();
            (name.starts_with('.') || !candidate.starts_with('.'))
                && wildcard_match(name.as_bytes(), candidate.as_bytes())
        })
        .map(|e| e.path())
        .collect();
    matched.sort();
    matched
}

fn wildcard_match(pattern: &[u8], text: &[u8]) -> bool {
    match (pattern.first(), text.first()) {
        (None, None) => true,
        (Some(b'*'), _) => {
            wildcard_match(&pattern[1..], text)
                || (!text.is_empty() && wildcard_match(pattern, &text[1..]))
        }
        (Some(b'?'), Some(_)) => wildcard_match(&pattern[1..], &text[1..]),
        (Some(p), Some(t)) if p == t => wildcard_match(&pattern[1..], &text[1..]),
        _ => false,
    }
}

/// What the working set says about one file.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Entry {
    Text { path: PathBuf, content: String },
    Gone { path: PathBuf },
    Binary { path: PathBuf, bytes: u64 },
    Unreadable { path: PathBuf, error: String },
    DidNotFit { path: PathBuf, size: String },
    Dependency { path: PathBuf },
    ReadInPart { path: PathBuf },
    NotRegular { path: PathBuf },
    Outside { path: PathBuf },
    Secret { path: PathBuf, why: &'static str },
}

/// Folders whose files hold credentials, by name.
const SECRET_FOLDERS: [&str; 6] = [".ssh", ".aws", ".gnupg", ".kube", ".azure", ".docker"];

/// Whether `path` looks like a credential: inside one of `SECRET_FOLDERS`, or a file named like an
/// environment file, a key, a certificate bundle or a credentials store. A named exclusion — the
/// block says why the file is not shown — never a silent one.
fn secret_kind(path: &Path) -> Option<&'static str> {
    if path.components().any(|c| {
        SECRET_FOLDERS
            .iter()
            .any(|folder| c.as_os_str() == std::ffi::OsStr::new(folder))
    }) {
        return Some("inside a credentials folder");
    }
    let name = path.file_name()?.to_string_lossy().to_ascii_lowercase();
    let key_like = [".pem", ".key", ".p12", ".pfx", ".keystore", ".jks"]
        .iter()
        .any(|ext| name.ends_with(ext))
        || ["id_rsa", "id_ed25519", "id_ecdsa", "id_dsa"]
            .iter()
            .any(|key| name.starts_with(key));
    if key_like {
        return Some("looks like a key or certificate file");
    }
    if name.starts_with(".env") {
        return Some("looks like an environment file");
    }
    if matches!(
        name.as_str(),
        ".netrc" | ".npmrc" | ".pypirc" | ".git-credentials" | "credentials"
    ) || name.contains("secret")
    {
        return Some("looks like a credentials file");
    }
    None
}

/// Whether `path` lies inside a dependency or build folder (`skills::should_skip_dir`, the list the
/// tree tool names without walking), counting only the folders below `working_dir` when the path
/// is under it — a chat whose own folder sits inside a `dist/` is not all dependency.
fn in_dependency_folder(path: &Path, working_dir: Option<&Path>) -> bool {
    let below = working_dir
        .and_then(|dir| path.strip_prefix(dir).ok())
        .unwrap_or(path);
    below
        .parent()
        .is_some_and(|parent| parent.ancestors().any(crate::skills::should_skip_dir))
}

/// The widest a char can be in UTF-8: a file of more bytes than this many times the budget left
/// cannot fit, and is not read to find that out.
const UTF8_MAX_CHAR_BYTES: u64 = 4; // measured: the UTF-8 encoding's widest char (an algorithm constant)

fn read_entry(path: &Path, budget_left: usize) -> Option<Entry> {
    use std::io::Read;
    let path_buf = path.to_path_buf();
    let meta = match std::fs::metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Some(Entry::Gone { path: path_buf })
        }
        Err(e) => {
            return Some(Entry::Unreadable {
                path: path_buf,
                error: e.to_string(),
            })
        }
        Ok(meta) if meta.is_dir() => return None,
        // A device, a FIFO or a socket (`cat /dev/urandom | tr …`): reading one may never end.
        Ok(meta) if !meta.is_file() => return Some(Entry::NotRegular { path: path_buf }),
        Ok(meta) => meta,
    };
    let bound = UTF8_MAX_CHAR_BYTES * budget_left as u64;
    if meta.len() > bound {
        return Some(Entry::DidNotFit {
            path: path_buf,
            size: format!("{} bytes", meta.len()),
        });
    }
    // Bounded even so: the file may have grown since its metadata was read.
    let mut bytes = Vec::new();
    let read =
        std::fs::File::open(path).and_then(|file| file.take(bound + 1).read_to_end(&mut bytes));
    if let Err(e) = read {
        return Some(Entry::Unreadable {
            path: path_buf,
            error: e.to_string(),
        });
    }
    if bytes.len() as u64 > bound {
        return Some(Entry::DidNotFit {
            path: path_buf,
            size: format!("more than {bound} bytes"),
        });
    }
    let content = match String::from_utf8(bytes) {
        Ok(content) if !content.contains('\0') => content,
        _ => {
            return Some(Entry::Binary {
                path: path_buf,
                bytes: meta.len(),
            })
        }
    };
    let chars = content.chars().count();
    if chars > budget_left {
        return Some(Entry::DidNotFit {
            path: path_buf,
            size: format!("{chars} chars"),
        });
    }
    Some(Entry::Text {
        path: path_buf,
        content,
    })
}

/// The working-set block for `files` (newest first) within `budget_chars`: every text file whole
/// while it fits, newest first; a file that does not fit is named with its size, never cut; a
/// file no longer on disk, a binary file, an unreadable one, one only read in part, one inside a
/// dependency folder, a non-regular file, one outside the chat's folder and one that looks like a
/// credential are each said — the last three are never read. `None` when there is no file to speak of.
pub fn working_set_block(
    files: &[Touched],
    budget_chars: usize,
    working_dir: Option<&Path>,
) -> Option<String> {
    let mut entries = Vec::new();
    let mut used = 0usize;
    for Touched { path, in_part } in files {
        let entry = if working_dir.is_some_and(|dir| !path.starts_with(dir)) {
            Entry::Outside { path: path.clone() }
        } else if let Some(why) = secret_kind(path) {
            Entry::Secret {
                path: path.clone(),
                why,
            }
        } else if in_dependency_folder(path, working_dir) {
            Entry::Dependency { path: path.clone() }
        } else if *in_part {
            Entry::ReadInPart { path: path.clone() }
        } else {
            let Some(entry) = read_entry(path, budget_chars - used) else {
                continue;
            };
            entry
        };
        if let Entry::Text { content, .. } = &entry {
            used += content.chars().count();
        }
        entries.push(entry);
    }
    if entries.is_empty() {
        return None;
    }
    let shown = entries
        .iter()
        .filter(|e| matches!(e, Entry::Text { .. }))
        .count();
    let mut out = format!(
        "{WORKING_SET_OPEN}\nCurrent file contents after compaction: the files this conversation \
         read or wrote, as they are on disk NOW, newest first ({shown} of {} shown in full, \
         {used} of a {budget_chars}-char budget). They supersede any earlier view of these files \
         in this conversation; read a file again only if it changes or is listed below as not \
         shown.\n",
        entries.len()
    );
    for entry in &entries {
        if let Entry::Text { path, content } = entry {
            out.push_str(&format!(
                "\n===== {} ({} lines) =====\n{content}",
                path.display(),
                content.lines().count()
            ));
            if !content.ends_with('\n') {
                out.push('\n');
            }
        }
    }
    let notes: Vec<String> = entries
        .iter()
        .filter_map(|entry| match entry {
            Entry::Text { .. } => None,
            Entry::Gone { path } => Some(format!(
                "{} — not on disk now (deleted or moved since this conversation touched it)",
                path.display()
            )),
            Entry::Binary { path, bytes } => Some(format!(
                "{} — a binary file ({bytes} bytes); its content is not shown",
                path.display()
            )),
            Entry::Unreadable { path, error } => Some(format!(
                "{} — could not be read now ({error})",
                path.display()
            )),
            Entry::ReadInPart { path } => Some(format!(
                "{} — only ever read in part (head/tail); not shown",
                path.display()
            )),
            Entry::Dependency { path } => Some(format!(
                "{} — inside a dependency or build folder; not shown",
                path.display()
            )),
            Entry::DidNotFit { path, size } => Some(format!(
                "{} — {size}, more than the budget left; not shown",
                path.display()
            )),
            Entry::NotRegular { path } => Some(format!(
                "{} — not a regular file (a device, pipe or socket); not read",
                path.display()
            )),
            Entry::Outside { path } => Some(format!(
                "{} — outside the chat's folder; not read",
                path.display()
            )),
            Entry::Secret { path, why } => Some(format!("{} — {why}; not read", path.display())),
        })
        .collect();
    if !notes.is_empty() {
        out.push_str("\nNot shown:\n");
        for note in notes {
            out.push_str(&format!("- {note}\n"));
        }
    }
    out.push_str(WORKING_SET_CLOSE);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::{CallToolRequestParams, CallToolResult, Content};
    use std::sync::atomic::{AtomicUsize, Ordering};

    static CALLS: AtomicUsize = AtomicUsize::new(0);

    fn call(tool: &str, args: serde_json::Value) -> Message {
        let mut params = CallToolRequestParams::new(tool.to_string());
        params.arguments = args.as_object().cloned();
        let id = format!("c{}", CALLS.fetch_add(1, Ordering::Relaxed));
        Message::assistant().with_tool_request(id, Ok(params))
    }

    /// Each call followed by its result: succeeded unless `declined` names it.
    fn ran(calls: Vec<Message>, declined: &[usize]) -> Vec<Message> {
        let mut out = Vec::new();
        for (i, request) in calls.into_iter().enumerate() {
            let id = request
                .content
                .iter()
                .find_map(|c| match c {
                    MessageContent::ToolRequest(r) => Some(r.id.clone()),
                    _ => None,
                })
                .unwrap();
            out.push(request);
            let result = if declined.contains(&i) {
                CallToolResult::error(vec![Content::text("The user declined this tool call.")])
            } else {
                CallToolResult::success(vec![Content::text("ok")])
            };
            out.push(Message::user().with_tool_response(id, Ok(result)));
        }
        out
    }

    fn shell(command: &str) -> Message {
        call("shell", serde_json::json!({ "command": command }))
    }

    fn whole(path: PathBuf) -> Touched {
        Touched {
            path,
            in_part: false,
        }
    }

    /// The read shapes Forge 2.0 runs A and B used, word for word but the folder: `cd <app> && cat
    /// …`, several files per `cat`, `; echo ===; cat …`, globs, `2>/dev/null`, `head`/`sed -n`
    /// ranges, pipelines whose later commands read no file, an in-place `sed -i` (an edit, not a
    /// read), and run B's malformed `edit` path (message 2448).
    #[test]
    fn the_runs_read_shapes_name_their_files_newest_first() {
        let app = tempfile::tempdir().unwrap();
        let root = app.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        for f in [
            "src/jira.js",
            "src/ledger.js",
            "src/sync.js",
            "manifest.yml",
        ] {
            std::fs::write(root.join(f), "x\n").unwrap();
        }
        let messages = vec![
            shell(&format!(
                "cd \"{}\" && cat src/jira.js; echo ===LEDGER===; cat src/ledger.js",
                root.display()
            )),
            shell("wc -l FORGE2-CONTRACT.md 2>/dev/null; ls src"),
            shell("cat src/*.js 2>/dev/null | head -80"),
            shell("sed -n '1,80p' manifest.yml"),
            shell("head -n 40 notes.md && node build.mjs"),
            call(
                "write",
                serde_json::json!({"path": "src/new.js", "content": "y"}),
            ),
            shell("sed -i 's/a/b/' src/sync.js"),
            call(
                "edit",
                serde_json::json!({"path": "src/jira.js\n</parameter>\n<parameter=background>\nnull"}),
            ),
        ];
        let names: Vec<(String, bool)> = touched_files(&ran(messages, &[]), Some(root))
            .into_iter()
            .map(|t| {
                (
                    t.path.strip_prefix(root).unwrap().display().to_string(),
                    t.in_part,
                )
            })
            .collect();
        let w = |name: &str| (name.to_string(), false);
        assert_eq!(
            names,
            [
                w("src/new.js"),
                ("notes.md".to_string(), true),
                w("manifest.yml"),
                w("src/sync.js"),
                w("src/ledger.js"),
                w("src/jira.js"),
            ],
            "the glob read sync.js last and ledger.js/jira.js again; notes.md only with head"
        );
    }

    /// A file read whole at any point is whole, whichever read came last.
    #[test]
    fn a_file_read_whole_once_stays_whole_after_a_head() {
        let messages = vec![
            shell("cat /w/a.js"),
            shell("head -5 /w/a.js; tail /w/b.log"),
        ];
        assert_eq!(
            touched_files(&ran(messages, &[]), None),
            [
                Touched {
                    path: PathBuf::from("/w/b.log"),
                    in_part: true
                },
                whole(PathBuf::from("/w/a.js")),
            ]
        );
    }

    #[test]
    fn a_tool_call_with_its_own_folder_and_a_relative_cd_resolve_there() {
        let messages = vec![
            call(
                "developer__shell",
                serde_json::json!({"command": "cd web && cat 'a b.js' | wc -l", "cwd": "/w"}),
            ),
            call("edit", serde_json::json!({"path": "/abs/x.rs"})),
        ];
        let paths: Vec<PathBuf> = touched_files(&ran(messages, &[]), Some(Path::new("/home/me")))
            .into_iter()
            .map(|t| t.path)
            .collect();
        assert_eq!(
            paths,
            [PathBuf::from("/abs/x.rs"), PathBuf::from("/w/web/a b.js")]
        );
    }

    /// Newest first while the budget lasts; a file that does not fit is named, never cut, and an
    /// older one that fits still goes in. A deleted file, a binary file and a file only read in
    /// part are each said.
    #[test]
    fn the_block_fills_newest_first_and_says_what_it_left_out() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        std::fs::write(d.join("new.js"), "a".repeat(40)).unwrap();
        std::fs::write(d.join("big.js"), "b".repeat(100)).unwrap();
        std::fs::write(d.join("old.js"), "c".repeat(50)).unwrap();
        std::fs::write(d.join("run.log"), "line\n".repeat(400)).unwrap();
        std::fs::write(d.join("logo.png"), [0x89, b'P', b'N', b'G', 0, 1, 2]).unwrap();
        let files = [
            whole(d.join("new.js")),
            whole(d.join("gone.js")),
            whole(d.join("big.js")),
            Touched {
                path: d.join("run.log"),
                in_part: true,
            },
            whole(d.join("logo.png")),
            whole(d.join("old.js")),
        ];
        let block = working_set_block(&files, 100, Some(d)).unwrap();
        let said = |path: &str, what: &str| {
            assert!(
                block.contains(&format!("- {} — {what}", d.join(path).display())),
                "{path}: {block}"
            )
        };
        assert!(block.starts_with(WORKING_SET_OPEN), "{block}");
        assert!(block.ends_with(WORKING_SET_CLOSE), "{block}");
        assert!(
            block.contains("(2 of 6 shown in full, 90 of a 100-char budget)"),
            "{block}"
        );
        let shown = |path: &str, content: String| {
            block
                .find(&format!(
                    "===== {} (1 lines) =====\n{content}",
                    d.join(path).display()
                ))
                .unwrap_or_else(|| panic!("{path} whole: {block}"))
        };
        assert!(
            shown("new.js", "a".repeat(40)) < shown("old.js", "c".repeat(50)),
            "newest first"
        );
        assert!(!block.contains(&"b".repeat(100)), "never cut: {block}");
        assert!(!block.contains("line\nline"), "{block}");
        said("big.js", "100 chars, more than the budget left; not shown");
        said("gone.js", "not on disk now");
        said("logo.png", "a binary file (7 bytes)");
        said("run.log", "only ever read in part (head/tail); not shown");
    }

    /// A type file read under node_modules is named, never shown: Forge 2.0 run B read six of
    /// them before its first compaction, newer than most of its own sources.
    #[test]
    fn a_file_inside_a_dependency_folder_is_named_not_shown() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        std::fs::create_dir_all(d.join("node_modules/@forge/llm")).unwrap();
        std::fs::write(
            d.join("node_modules/@forge/llm/types.d.ts"),
            "export type X = 1;",
        )
        .unwrap();
        std::fs::write(d.join("index.js"), "run();").unwrap();
        let files = [
            whole(d.join("node_modules/@forge/llm/types.d.ts")),
            whole(d.join("index.js")),
        ];
        let block = working_set_block(&files, 1_000, Some(d)).unwrap();
        assert!(block.contains("run();"), "{block}");
        assert!(!block.contains("export type X"), "{block}");
        assert!(
            block.contains("types.d.ts — inside a dependency or build folder; not shown"),
            "{block}"
        );

        let inside_dist = d.join("dist/app");
        std::fs::create_dir_all(&inside_dist).unwrap();
        std::fs::write(inside_dist.join("a.js"), "a();").unwrap();
        let block = working_set_block(
            &[whole(inside_dist.join("a.js"))],
            1_000,
            Some(&inside_dist),
        )
        .unwrap();
        assert!(
            block.contains("a();"),
            "the chat's own folder is not a dependency: {block}"
        );
    }

    /// A call that did not succeed read nothing, and goose reads nothing for it: run as given, a
    /// declined `cat ~/.aws/credentials` would be read at the compaction and sent to the provider.
    #[test]
    fn only_calls_that_succeeded_count() {
        let messages = ran(
            vec![
                shell("cat /home/me/.aws/credentials"),
                shell("cat /w/app.js"),
                call("write", serde_json::json!({"path": "/w/denied.js"})),
            ],
            &[0, 2],
        );
        let paths: Vec<PathBuf> = touched_files(&messages, None)
            .into_iter()
            .map(|t| t.path)
            .collect();
        assert_eq!(paths, [PathBuf::from("/w/app.js")]);
        let unanswered = vec![shell("cat /w/never.js")];
        assert!(
            touched_files(&unanswered, None).is_empty(),
            "no result, no read"
        );
    }

    /// Outside the chat's folder, or named like a credential: named, never read.
    #[test]
    fn files_outside_the_folder_or_like_credentials_are_named_never_read() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        std::fs::create_dir_all(d.join(".aws")).unwrap();
        std::fs::write(d.join(".aws/credentials"), "aws_secret_access_key=AKIA").unwrap();
        std::fs::write(d.join(".env.local"), "TOKEN=abc").unwrap();
        std::fs::write(d.join("server.pem"), "-----BEGIN").unwrap();
        std::fs::write(d.join("app.js"), "run();").unwrap();
        let other = tempfile::tempdir().unwrap();
        std::fs::write(other.path().join("notes.md"), "elsewhere").unwrap();
        let files = [
            whole(d.join(".aws/credentials")),
            whole(d.join(".env.local")),
            whole(d.join("server.pem")),
            whole(other.path().join("notes.md")),
            whole(d.join("app.js")),
        ];
        let block = working_set_block(&files, 10_000, Some(d)).unwrap();
        assert!(block.contains("run();"), "{block}");
        for secret in ["AKIA", "TOKEN=abc", "-----BEGIN", "elsewhere"] {
            assert!(!block.contains(secret), "{secret}: {block}");
        }
        assert!(
            block.contains("credentials — inside a credentials folder; not read"),
            "{block}"
        );
        assert!(
            block.contains(".env.local — looks like an environment file; not read"),
            "{block}"
        );
        assert!(
            block.contains("server.pem — looks like a key or certificate file; not read"),
            "{block}"
        );
        assert!(
            block.contains("notes.md — outside the chat's folder; not read"),
            "{block}"
        );
    }

    /// `cat /dev/urandom | tr …` names a device: never opened (a read of it never ends), said.
    #[cfg(unix)]
    #[test]
    fn a_device_is_named_never_read() {
        let messages = ran(
            vec![shell("cat /dev/urandom | tr -dc a-z | head -c 8")],
            &[],
        );
        let files = touched_files(&messages, None);
        assert_eq!(files, [whole(PathBuf::from("/dev/urandom"))]);
        let block = working_set_block(&files, 1_000, None).unwrap();
        assert!(
            block
                .contains("/dev/urandom — not a regular file (a device, pipe or socket); not read"),
            "{block}"
        );
    }

    /// A heredoc's delimiter is not a file and its body is not commands: `cat > a.js << 'EOF'`
    /// writes a.js (the redirection target is skipped) and the `cat b.js` line inside the body is
    /// data. A here-string's word is not a file either.
    #[test]
    fn heredoc_delimiters_and_bodies_are_not_reads() {
        let messages = ran(
            vec![
                shell("cat > a.js << 'EOF'\ncat b.js\nconst x = 1;\nEOF\ncat c.js"),
                shell("cat <<-END > d.js\n\tcat e.js\n\tEND\ncat f.js"),
                shell("cat <<< hello; cat g.js"),
            ],
            &[],
        );
        let names: Vec<String> = touched_files(&messages, Some(Path::new("/w")))
            .into_iter()
            .map(|t| t.path.display().to_string())
            .collect();
        assert_eq!(names, ["/w/g.js", "/w/f.js", "/w/c.js"]);
    }

    #[test]
    fn nothing_touched_places_no_block_and_a_zero_share_no_budget() {
        assert_eq!(working_set_block(&[], 1_000, None), None);
        assert_eq!(working_set_budget_chars(Some(158_833), 0.0), None);
        assert_eq!(
            working_set_budget_chars(None, DEFAULT_WORKING_SET_SHARE),
            None
        );
        assert_eq!(
            working_set_budget_chars(Some(158_833), DEFAULT_WORKING_SET_SHARE),
            Some(158_833)
        );
    }
}

/// Gate 8's trace over a real run: `GOOSE_WORKING_SET_TRACE=<messages.json>` (an export of a
/// session's messages before a compaction, `[{role, created, content, metadata}]`) and
/// `GOOSE_WORKING_SET_TRACE_DIR=<the app folder>`; prints what the working set would hold at
/// the window `GOOSE_WORKING_SET_TRACE_WINDOW` (tokens).
#[cfg(test)]
mod trace {
    use super::*;

    #[test]
    #[ignore = "reads a local export of a run's session"]
    fn trace_a_runs_working_set() {
        let messages: Vec<Message> = serde_json::from_str(
            &std::fs::read_to_string(std::env::var("GOOSE_WORKING_SET_TRACE").unwrap()).unwrap(),
        )
        .unwrap();
        let dir = PathBuf::from(std::env::var("GOOSE_WORKING_SET_TRACE_DIR").unwrap());
        let window: usize = std::env::var("GOOSE_WORKING_SET_TRACE_WINDOW")
            .unwrap()
            .parse()
            .unwrap();
        let files = touched_files(&messages, Some(&dir));
        println!("touched {} files, newest first", files.len());
        let budget = working_set_budget_chars(Some(window), DEFAULT_WORKING_SET_SHARE).unwrap();
        let block = working_set_block(&files, budget, Some(&dir)).unwrap();
        println!(
            "budget {budget} chars; block {} chars; {}",
            block.chars().count(),
            block.lines().nth(1).unwrap()
        );
        let local = format!("{}/", dir.display());
        for line in block.lines().filter(|l| l.starts_with("===== ")) {
            println!("  shown {}", line.replace(&local, ""));
        }
        if let Some((_, not_shown)) = block.rsplit_once("\nNot shown:\n") {
            for line in not_shown.lines() {
                println!("  {}", line.replace(&local, ""));
            }
        }
    }
}
